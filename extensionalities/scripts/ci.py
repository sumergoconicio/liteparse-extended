#!/usr/bin/env python3
"""Downstream CI commands. Requires Python 3.10+, Git, and the native build tools.

Run from the repository root. Checks write only Cargo output and isolated
fixtures under extensionalities/target. No upstream tests or formatters run.
Command failures propagate; missing diff objects conservatively enable checks.
The workflows command additionally requires Go 1.24+ and network access.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = "extensionalities/Cargo.toml"


def relevant(path):
    return (
        path.startswith(("extensionalities/", "crates/", ".cargo/"))
        or path in {"Cargo.toml", "Cargo.lock"}
        or path.startswith("rust-toolchain")
        or path.startswith(".github/workflows/glassbox-")
    )


def needs_checks(root, base, head, force=False):
    if force or not base or not head or set(base) == {"0"}:
        return True
    result = subprocess.run(
        ["git", "-C", str(root), "diff", "--no-renames", "--name-only", "-z", base, head, "--"],
        capture_output=True,
        timeout=30,
    )
    if result.returncode:
        return True
    return any(relevant(os.fsdecode(p)) for p in result.stdout.split(b"\0") if p)


def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)


def workflows():
    # Pin actionlint; isolate Go's module/build caches from application sources.
    cache = ROOT / "extensionalities/target/workflow-lint"
    env = {
        **os.environ, "GOPATH": str(cache / "go"),
        "GOMODCACHE": str(cache / "modules"), "GOCACHE": str(cache / "build"),
    }
    subprocess.run(
        ["go", "run", "github.com/rhysd/actionlint/cmd/actionlint@v1.7.12",
         "-shellcheck=", "-pyflakes=",
         ".github/workflows/glassbox-ci.yml",
         ".github/workflows/glassbox-upstream-sync.yml"],
        cwd=ROOT, env=env, check=True, timeout=240,
    )


def smoke():
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "extensionalities/target"))
    if not target.is_absolute():
        target = ROOT / target
    fixture = ROOT / "extensionalities/fixtures/hello.txt"
    reports = []
    for name in ("glassbox", "litx"):
        executable = target / "release" / name
        for command in ("inspect", "extract"):
            output = subprocess.check_output(
                [str(executable), command, str(fixture)], cwd=ROOT, timeout=30
            )
            report = json.loads(output)
            assert not report["errors"], report["errors"]
            assert report["text"]["content"].strip() == fixture.read_text().strip()
            reports.append(report)
        output = subprocess.check_output(
            [str(executable), "inspect", "--batch"],
            input=json.dumps({"id": "smoke", "path": str(fixture)}).encode() + b"\n",
            cwd=ROOT,
            timeout=30,
        )
        response = json.loads(output)
        assert response["id"] == "smoke" and not response["report"]["errors"]
    assert reports[0] == reports[2] and reports[1] == reports[3]
    with tempfile.TemporaryDirectory(
        prefix="ci-ocr-", dir=ROOT / "extensionalities/target"
    ) as directory:
        # An explicitly missing model must never cause a runtime download.
        env = {**os.environ, "TESSDATA_PREFIX": str(Path(directory) / "missing")}
        output = subprocess.check_output(
            [str(target / "release/glassbox"), "inspect", "--identify",
             str(ROOT / "extensionalities/fixtures/native.pdf")],
            cwd=ROOT, env=env, timeout=30,
        )
        report = json.loads(output)
        assert report["errors"] and report["text"]["truncated"]
    print("PASS: release commands, batch responses, and offline OCR failure")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("changes", "format", "check", "smoke", "workflows"))
    args = parser.parse_args()
    if args.command == "changes":
        required = needs_checks(
            ROOT, os.environ.get("BASE_SHA"), os.environ.get("HEAD_SHA"),
            os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch"
            or os.environ.get("HEAD_BRANCH", "").startswith("automation/upstream-sync/"),
        )
        value = str(required).lower()
        output = os.environ.get("GITHUB_OUTPUT")
        if output:
            with open(output, "a", encoding="utf-8") as stream:
                stream.write(f"run={value}\n")
        print(f"Downstream checks required: {value}")
    elif args.command == "format":
        # --all also follows local path dependencies; name only our packages.
        run("cargo", "fmt", "--manifest-path", MANIFEST,
            "-p", "litx", "-p", "litx-inspect", "--check")
    elif args.command == "workflows":
        workflows()
    elif args.command == "check":
        run("python3", "-B", "-m", "unittest", "discover",
            "-s", "extensionalities/scripts/tests", "-v")
        run("cargo", "clippy", "--locked", "--manifest-path", MANIFEST,
            "--workspace", "--all-targets", "--no-deps", "--", "-D", "warnings")
        run("cargo", "test", "--locked", "--manifest-path", MANIFEST, "--workspace")
        run("cargo", "build", "--locked", "--release", "--manifest-path", MANIFEST,
            "--workspace", "--bins")
        smoke()
    else:
        smoke()


if __name__ == "__main__":
    main()
