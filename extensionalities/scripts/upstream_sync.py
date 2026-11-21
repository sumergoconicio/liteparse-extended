#!/usr/bin/env python3
"""Plan or automate the Glassbox upstream overlay. Python 3.10+, Git, gh.

plan writes Git objects and an isolated ignored index, never working files or
refs. automate additionally creates a GitHub branch/PR and conditionally merges
it. Automation requires GitHub Actions, an explicit token, strict required CI,
and unchanged base/head SHAs. Network calls time out after 60 seconds; checks
have a 40-minute deadline. Failures leave published PRs open, without force
pushes, deletions, credential changes, or downstream lockfile updates.
"""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
from urllib.parse import urlencode

ROOT = Path(__file__).resolve().parents[2]
REPOSITORY = "sumergoconicio/glassbox"
GATE = "Glassbox downstream gate"
WORKFLOW = "Glassbox downstream CI"
PROTECTED = ("extensionalities", ".github/workflows")
IGNORE = b"/extensionalities/dev/"
COAUTHOR = "Co-authored-by: factory-droid[bot] <138933559+factory-droid[bot]@users.noreply.github.com>"


class SyncError(RuntimeError):
    pass


def git(root, *args, data=None, env=None):
    result = subprocess.run(
        ["git", "-C", str(root), *args], input=data, capture_output=True,
        timeout=60, env=env,
    )
    if result.returncode:
        raise SyncError(f"Git command failed: {args[0]}")
    return result.stdout


def oid(root, ref):
    value = git(root, "rev-parse", "--verify", ref).decode().strip()
    if not re.fullmatch(r"[0-9a-f]{40}", value):
        raise SyncError("Expected a SHA-1 Git object")
    return value


def tree_entries(root, ref):
    result = {}
    for record in git(root, "ls-tree", "-rz", "--full-tree", ref).split(b"\0"):
        if record:
            metadata, path = record.split(b"\t", 1)
            result[path] = metadata
    return result


def protected(path):
    return any(path == p.encode() or path.startswith(p.encode() + b"/") for p in PROTECTED)


def plan(root, base, upstream):
    """Mirror upstream-owned paths; overlay exact downstream and workflow trees."""
    base = oid(root, base + "^{commit}")
    upstream = oid(root, upstream + "^{commit}")
    overlays = []
    for path in PROTECTED:
        tree = oid(root, f"{base}:{path}")
        if git(root, "cat-file", "-t", tree).strip() != b"tree":
            raise SyncError(f"Protected path must be a directory: {path}")
        overlays.append({"path": path, "mode": "040000", "type": "tree", "sha": tree})
    incoming = tree_entries(root, upstream)
    if b".github" in incoming:
        raise SyncError("Upstream .github is not a directory")
    ignore = incoming.get(b".gitignore", b"100644 blob").split()
    if ignore[0] not in (b"100644", b"100755"):
        raise SyncError("Upstream .gitignore must be a regular file")
    content = git(root, "cat-file", "blob", ignore[2].decode()) if len(ignore) == 3 else b""
    if not content.splitlines() or content.splitlines()[-1] != IGNORE:
        content += (b"\n" if content and not content.endswith(b"\n") else b"") + IGNORE + b"\n"
    try:
        ignore_text = content.decode("utf-8")
    except UnicodeDecodeError as error:
        raise SyncError("Upstream .gitignore is not UTF-8") from error
    blob = git(root, "hash-object", "-w", "--stdin", data=content).strip()
    selected = {p: m for p, m in incoming.items() if not protected(p)}
    selected.update({p: m for p, m in tree_entries(root, base).items() if protected(p)})
    selected[b".gitignore"] = ignore[0] + b" blob " + blob
    scratch = root / "extensionalities/target"
    scratch.mkdir(parents=True, exist_ok=True)
    index = Path(tempfile.mkdtemp(prefix="sync-plan-", dir=scratch)) / "index"
    env = {**os.environ, "GIT_INDEX_FILE": str(index)}
    records = b"".join(m + b"\t" + p + b"\0" for p, m in sorted(selected.items()))
    git(root, "update-index", "-z", "--index-info", data=records, env=env)
    tree = git(root, "write-tree", env=env).decode().strip()
    for overlay in overlays:
        if oid(root, f"{tree}:{overlay['path']}") != overlay["sha"]:
            raise SyncError("Protected tree changed during planning")
    overlays.append({
        "path": ".gitignore", "mode": ignore[0].decode(),
        "type": "blob", "content": ignore_text,
    })
    return {
        "base": base, "upstream": upstream, "tree": tree,
        "upstream_tree": oid(root, upstream + "^{tree}"),
        "overlays": overlays,
        "changed": tree != oid(root, base + "^{tree}"),
    }


def api(repository, path="", payload=None, method="GET", paginate=False):
    args = ["gh", "api", f"repos/{repository}/{path}".rstrip("/"),
            "--method", method]
    if paginate:
        args.extend(["--paginate", "--slurp"])
    if payload is not None:
        args.extend(["--input", "-"])
    result = subprocess.run(
        args, input=json.dumps(payload) if payload is not None else None,
        capture_output=True, text=True, timeout=60,
    )
    if result.returncode:
        raise SyncError(f"GitHub request failed ({method} {path}); no retry attempted")
    return json.loads(result.stdout)


def require_protection(settings):
    checks = settings.get("required_status_checks") or {}
    contexts = set(checks.get("contexts", []))
    contexts.update(c.get("context") for c in checks.get("checks", []))
    if (
        not checks.get("strict")
        or GATE not in contexts
        or not (settings.get("enforce_admins") or {}).get("enabled")
    ):
        raise SyncError("main must require the downstream gate, strict updates, and administrator enforcement")


def all_green(checks):
    """Missing/pending checks wait; failed, skipped, or neutral checks block."""
    if not checks:
        return False
    pending = {"PENDING", "QUEUED", "IN_PROGRESS", "WAITING", "REQUESTED", "EXPECTED"}
    ready = any(c.get("name") == GATE and c.get("workflow") == WORKFLOW for c in checks)
    for check in checks:
        state = check.get("state")
        if state in pending:
            ready = False
        elif state != "SUCCESS":
            raise SyncError("A PR check failed, was skipped/cancelled, or has an unknown result")
    return ready


def pr_checks(repository, number):
    result = subprocess.run(
        ["gh", "pr", "checks", str(number), "--repo", repository,
         "--json", "name,state,workflow"],
        capture_output=True, text=True, timeout=60,
    )
    if result.returncode not in (0, 1, 8):
        raise SyncError("Could not read PR checks")
    if not result.stdout.strip():
        if "no checks reported" in result.stderr.lower():
            return []
        raise SyncError("PR checks returned no readable result")
    checks = json.loads(result.stdout)
    if not isinstance(checks, list):
        raise SyncError("Unexpected PR check response")
    return checks


def verify_refs(repository, number, branch, head, base):
    pr = api(repository, f"pulls/{number}")
    current = api(repository, "branches/main")
    if (
        pr.get("state") != "open" or pr.get("draft") or pr.get("mergeable") is not True
        or pr["base"]["ref"] != "main" or pr["head"]["ref"] != branch
        or pr["base"]["repo"]["full_name"] != repository
        or pr["head"]["repo"]["full_name"] != repository
        or pr["head"]["sha"] != head or current["commit"]["sha"] != base
    ):
        raise SyncError("PR state, mergeability, or base/head changed; manual review required")


def merge_when_green(repository, number, branch, head, base, wait_seconds=2400):
    deadline = time.monotonic() + wait_seconds
    while time.monotonic() < deadline:
        if all_green(pr_checks(repository, number)):
            verify_refs(repository, number, branch, head, base)
            require_protection(api(repository, "branches/main/protection"))
            # Re-read all checks after state/protection verification.
            if all_green(pr_checks(repository, number)):
                result = api(repository, f"pulls/{number}/merge", {
                    "sha": head, "merge_method": "merge",
                    "commit_message": COAUTHOR,
                }, method="PUT")
                if not result.get("merged"):
                    raise SyncError("GitHub refused the merge; PR remains open")
                return result
        time.sleep(15)
    raise SyncError("Checks did not all pass within 40 minutes; PR remains open")


def automate(root, base, upstream):
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("GITHUB_REPOSITORY") != REPOSITORY:
        raise SyncError("Live automation is restricted to Glassbox GitHub Actions")
    if not os.environ.get("GH_TOKEN"):
        raise SyncError("Set the separately approved UPSTREAM_SYNC_TOKEN repository secret")
    repository = REPOSITORY
    require_protection(api(repository, "branches/main/protection"))
    metadata = api(repository)
    if metadata.get("default_branch") != "main" or not metadata.get("allow_merge_commit"):
        raise SyncError("Expected main and merge-commit support")
    query = urlencode({"state": "open", "base": "main", "per_page": 100})
    pages = api(repository, f"pulls?{query}", paginate=True)
    if any(pr["head"]["ref"].startswith("automation/upstream-sync/")
           for page in pages for pr in page):
        raise SyncError("An upstream-sync PR is already open; resolve it before another sync")
    candidate = plan(root, base, upstream)
    if api(repository, "branches/main")["commit"]["sha"] != candidate["base"]:
        raise SyncError("main moved during planning")
    if not candidate["changed"]:
        print("No upstream-owned file changes")
        return
    # Confirm the fork network can resolve the upstream parent before any write.
    api(repository, f"git/commits/{candidate['upstream']}")
    tree = api(repository, "git/trees", {
        "base_tree": candidate["upstream_tree"], "tree": candidate["overlays"],
    }, method="POST")
    if tree["sha"] != candidate["tree"]:
        raise SyncError("GitHub tree differs from the verified local tree")
    commit = api(repository, "git/commits", {
        "message": f"chore: sync upstream LiteParse {candidate['upstream'][:12]}\n\n{COAUTHOR}",
        "tree": tree["sha"], "parents": [candidate["base"], candidate["upstream"]],
    }, method="POST")
    branch = f"automation/upstream-sync/{candidate['upstream'][:12]}-{candidate['base'][:12]}"
    api(repository, "git/refs", {
        "ref": f"refs/heads/{branch}", "sha": commit["sha"],
    }, method="POST")
    pr = api(repository, "pulls", {
        "title": f"Sync upstream LiteParse {candidate['upstream'][:12]}",
        "base": "main", "head": branch,
        "body": (
            f"Upstream: `run-llama/liteparse@{candidate['upstream']}`.\n\n"
            "Upstream-owned paths mirror that snapshot. extensionalities and "
            ".github/workflows retain their exact base trees. The downstream "
            "ignore rule is preserved. Automatic merge requires every check "
            "to succeed, strict required CI, and unchanged base/head commits."
        ),
    }, method="POST")
    print(f"Created {pr['html_url']}", flush=True)
    merge_when_green(repository, pr["number"], branch, commit["sha"], candidate["base"])
    print(f"Merged {pr['html_url']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("plan", "automate"))
    parser.add_argument("--base", required=True)
    parser.add_argument("--upstream", required=True)
    args = parser.parse_args()
    try:
        if args.command == "plan":
            print(json.dumps(plan(ROOT, args.base, args.upstream), indent=2))
        else:
            automate(ROOT, args.base, args.upstream)
    except (SyncError, subprocess.TimeoutExpired, json.JSONDecodeError) as error:
        parser.exit(1, f"Upstream sync stopped: {error}\n")


if __name__ == "__main__":
    main()
