"""Isolated automation tests. Retain synthetic repositories under ignored target."""

import importlib.util
import itertools
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
TARGET = SCRIPTS.parent / "target"
TARGET.mkdir(exist_ok=True)


def load(name):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


ci = load("ci")
sync = load("upstream_sync")


def protection():
    return {
        "required_status_checks": {"strict": True, "contexts": [sync.GATE]},
        "enforce_admins": {"enabled": True},
    }


def green():
    return [
        {"name": sync.GATE, "workflow": sync.WORKFLOW, "state": "SUCCESS"},
        {"name": "checks (ubuntu-24.04)", "state": "SUCCESS"},
    ]


class TreeTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="test-sync-", dir=TARGET))
        subprocess.run(["git", "init", "--quiet", str(self.root)], check=True)
        self.counter = 0
        self.files = {
            ".gitignore": b"target/\n/extensionalities/dev/\n",
            ".github/workflows/glassbox-ci.yml": b"ours\n",
            "extensionalities/Cargo.toml": b"[workspace]\n",
            "extensionalities/README.md": b"downstream\n",
            "README.md": b"upstream old\n",
            "old-only.txt": b"remove with upstream\n",
        }
        self.base = self.commit(self.files)

    def commit(self, files, parent=None):
        records = []
        for path, value in sorted(files.items()):
            mode, content = value if isinstance(value, tuple) else ("100644", value)
            blob = sync.git(self.root, "hash-object", "-w", "--stdin", data=content).strip()
            records.append(mode.encode() + b" blob " + blob + b"\t" + path.encode() + b"\0")
        self.counter += 1
        env = {**os.environ, "GIT_INDEX_FILE": str(self.root / f"fixture-index-{self.counter}")}
        sync.git(self.root, "update-index", "-z", "--index-info", data=b"".join(records), env=env)
        tree = sync.git(self.root, "write-tree", env=env).strip().decode()
        # Synthetic Git fixture data, not a configured author identity.
        body = f"tree {tree}\n"
        if parent:
            body += f"parent {parent}\n"
        body += (
            "author Fixture <fixture@example.invalid> 1 +0000\n"
            "committer Fixture <fixture@example.invalid> 1 +0000\n\nfixture\n"
        )
        return sync.git(self.root, "hash-object", "-t", "commit", "-w", "--stdin",
                        data=body.encode()).strip().decode()

    def upstream(self, **changes):
        files = dict(self.files)
        files.pop("old-only.txt")
        files.update({"README.md": b"new upstream\n", ".gitignore": b"new-ignore/\n"})
        files.update(changes)
        return self.commit(files, self.base)

    def test_exact_overlay_including_hostile_protected_changes_and_deletions(self):
        upstream = self.upstream(**{
            "extensionalities/README.md": b"must not win\n",
            "extensionalities/new.rs": b"must not be imported\n",
            ".github/workflows/new-upstream.yml": b"must not run\n",
            "new.rs": b"upstream code\n",
        })
        result = sync.plan(self.root, self.base, upstream)
        for path in sync.PROTECTED:
            self.assertEqual(sync.oid(self.root, f"{result['tree']}:{path}"),
                             sync.oid(self.root, f"{self.base}:{path}"))
        entries = sync.tree_entries(self.root, result["tree"])
        self.assertNotIn(b"old-only.txt", entries)
        self.assertIn(b"new.rs", entries)
        self.assertEqual(sync.git(self.root, "show", result["tree"] + ":README.md"), b"new upstream\n")
        self.assertEqual(sync.git(self.root, "show", result["tree"] + ":.gitignore"),
                         b"new-ignore/\n/extensionalities/dev/\n")
        self.assertFalse((self.root / ".git/index").exists())
        self.assertFalse((self.root / "README.md").exists())

    def test_idempotent_and_no_downstream_lockfile_rewrite(self):
        result = sync.plan(self.root, self.base, self.base)
        self.assertFalse(result["changed"])
        self.assertEqual(result["tree"], sync.oid(self.root, self.base + "^{tree}"))

    def test_change_detection_handles_renames_out_of_the_downstream_tree(self):
        files = dict(self.files)
        files["docs/moved.md"] = files.pop("extensionalities/README.md")
        head = self.commit(files, self.base)
        self.assertTrue(ci.needs_checks(self.root, self.base, head))
        files = {**self.files, "README.md": b"unrelated documentation\n"}
        head = self.commit(files, self.base)
        self.assertFalse(ci.needs_checks(self.root, self.base, head))

    def test_upstream_protected_root_symlink_cannot_replace_downstream(self):
        files = {p: c for p, c in self.files.items() if not p.startswith("extensionalities/")}
        files["extensionalities"] = ("120000", b"/outside")
        upstream = self.commit(files, self.base)
        result = sync.plan(self.root, self.base, upstream)
        self.assertEqual(sync.oid(self.root, result["tree"] + ":extensionalities"),
                         sync.oid(self.root, self.base + ":extensionalities"))

    def test_unsafe_ignore_or_github_shape_is_rejected(self):
        for path in [".gitignore", ".github"]:
            with self.subTest(path=path):
                files = {p: c for p, c in self.files.items()
                         if p != path and not p.startswith(path + "/")}
                files[path] = ("120000", b"/outside")
                upstream = self.commit(files, self.base)
                with self.assertRaises(sync.SyncError):
                    sync.plan(self.root, self.base, upstream)

    def test_upstream_deletions_of_protected_trees_are_ignored(self):
        upstream = self.commit({"README.md": b"upstream only\n"}, self.base)
        result = sync.plan(self.root, self.base, upstream)
        self.assertEqual(sync.oid(self.root, result["tree"] + ":extensionalities"),
                         sync.oid(self.root, self.base + ":extensionalities"))
        self.assertEqual(result["overlays"][-1]["content"], "/extensionalities/dev/\n")

    def test_ignore_guard_wins_over_later_upstream_negation(self):
        upstream = self.upstream(**{
            ".gitignore": b"/extensionalities/dev/\n!/extensionalities/dev/\n",
        })
        result = sync.plan(self.root, self.base, upstream)
        (self.root / ".gitignore").write_text(result["overlays"][-1]["content"])
        output = sync.git(self.root, "check-ignore", "--no-index", "-v",
                          "extensionalities/dev/README.md")
        self.assertTrue(output.split(b"\t")[0].endswith(b":/extensionalities/dev/"))

    def test_publisher_uses_verified_tree_and_does_not_override_identity(self):
        upstream = self.upstream()
        candidate = sync.plan(self.root, self.base, upstream)
        calls = []

        def api(repository, path="", payload=None, method="GET", paginate=False):
            calls.append((path, payload, method))
            if path == "branches/main/protection":
                return protection()
            if path == "":
                return {"default_branch": "main", "allow_merge_commit": True}
            if path.startswith("pulls?"):
                return [[]]
            if path == "branches/main":
                return {"commit": {"sha": self.base}}
            if path == "git/trees":
                self.assertEqual(payload["tree"], candidate["overlays"])
                return {"sha": candidate["tree"]}
            if path == "git/commits":
                self.assertEqual(payload["parents"], [self.base, upstream])
                self.assertNotIn("author", payload)
                self.assertNotIn("committer", payload)
                return {"sha": "c" * 40}
            if path == "pulls":
                return {"number": 7, "html_url": "https://github.com/example/repository/pull/7"}
            return {}

        with patch.dict(os.environ, {
            "GITHUB_ACTIONS": "true", "GITHUB_REPOSITORY": sync.REPOSITORY,
            "GH_TOKEN": "synthetic-test-credential",
        }), patch.object(sync, "api", side_effect=api), patch.object(sync, "merge_when_green") as merge:
            sync.automate(self.root, self.base, upstream)
            merge.assert_called_once()
        self.assertEqual(sum(method == "POST" for _, _, method in calls), 4)


class GateTests(unittest.TestCase):
    def test_real_workflow_gate_accepts_only_the_two_valid_job_combinations(self):
        source = (ci.ROOT / ".github/workflows/glassbox-ci.yml").read_text()
        code = source.split("          python3 - <<'PY'\n", 1)[1].rsplit("          PY", 1)[0]
        program = compile(textwrap.dedent(code), "<workflow-gate>", "exec")
        valid = {
            ("success", "true", "success", "success", "success"),
            ("success", "false", "skipped", "skipped", "skipped"),
        }
        states = ("success", "failure", "cancelled", "skipped")
        for values in itertools.product(states, ("true", "false", "", "unknown"),
                                        states, states, states):
            with patch.dict(os.environ, dict(zip(
                ("CHANGES", "REQUIRED", "FORMAT", "WORKFLOWS", "CHECKS"), values,
            ))):
                try:
                    exec(program, {})
                    accepted = True
                except AssertionError:
                    accepted = False
            self.assertEqual(accepted, values in valid, values)

    def test_requires_actual_named_gate_and_every_check_successful(self):
        self.assertFalse(sync.all_green([]))
        self.assertFalse(sync.all_green([{"name": "other", "state": "SUCCESS"}]))
        self.assertTrue(sync.all_green(green()))
        for state in ["PENDING", "QUEUED", "IN_PROGRESS", "WAITING"]:
            self.assertFalse(sync.all_green(green() + [{"state": state}]))
        for state in ["FAILURE", "CANCELLED", "SKIPPED", "NEUTRAL", "TIMED_OUT", None]:
            with self.subTest(state=state), self.assertRaises(sync.SyncError):
                sync.all_green(green() + [{"state": state}])

    def test_protection_requires_gate_strictness_and_admin_enforcement(self):
        sync.require_protection(protection())
        for settings in [
            {}, {"required_status_checks": {"strict": True}},
            {**protection(), "enforce_admins": {"enabled": False}},
            {**protection(), "required_status_checks": {"contexts": [sync.GATE]}},
        ]:
            with self.subTest(settings=settings), self.assertRaises(sync.SyncError):
                sync.require_protection(settings)

    def test_merge_rechecks_green_and_pins_head(self):
        calls = []

        def api(repository, path, payload=None, method="GET"):
            calls.append((path, payload, method))
            return protection() if path.endswith("protection") else {"merged": True}

        with patch.object(sync, "pr_checks", return_value=green()), \
             patch.object(sync, "verify_refs") as refs, \
             patch.object(sync, "api", side_effect=api):
            sync.merge_when_green("owner/repo", 7, "branch", "head", "base")
            refs.assert_called_once_with("owner/repo", 7, "branch", "head", "base")
        self.assertEqual(calls[-1][1]["sha"], "head")
        self.assertEqual(calls[-1][2], "PUT")

    def test_failed_or_missing_checks_never_merge(self):
        with patch.object(sync, "pr_checks", return_value=[{"state": "FAILURE"}]), \
             patch.object(sync, "api") as api, self.assertRaises(sync.SyncError):
            sync.merge_when_green("owner/repo", 7, "branch", "head", "base")
        api.assert_not_called()
        with patch.object(sync, "api") as api, self.assertRaises(sync.SyncError):
            sync.merge_when_green("owner/repo", 7, "branch", "head", "base", wait_seconds=0)
        api.assert_not_called()

    def test_base_or_head_movement_blocks_merge(self):
        with patch.object(sync, "pr_checks", return_value=green()), \
             patch.object(sync, "verify_refs", side_effect=sync.SyncError("moved")), \
             patch.object(sync, "api") as api, self.assertRaises(sync.SyncError):
            sync.merge_when_green("owner/repo", 7, "branch", "head", "base")
        api.assert_not_called()

    def test_ref_verification_rejects_conflicts_unknown_mergeability_and_changed_refs(self):
        repository = "owner/repo"
        valid = {
            "state": "open", "draft": False, "mergeable": True,
            "base": {"ref": "main", "repo": {"full_name": repository}},
            "head": {"ref": "branch", "sha": "head", "repo": {"full_name": repository}},
        }
        with patch.object(sync, "api", side_effect=[valid, {"commit": {"sha": "base"}}]):
            sync.verify_refs(repository, 7, "branch", "head", "base")
        bad_cases = [
            {**valid, "mergeable": False}, {**valid, "mergeable": None},
            {**valid, "draft": True}, {**valid, "state": "closed"},
            {**valid, "head": {**valid["head"], "sha": "moved"}},
            {**valid, "head": {**valid["head"], "repo": {"full_name": "outsider/repo"}}},
        ]
        for pr in bad_cases:
            with self.subTest(pr=pr), \
                 patch.object(sync, "api", side_effect=[pr, {"commit": {"sha": "base"}}]), \
                 self.assertRaises(sync.SyncError):
                sync.verify_refs(repository, 7, "branch", "head", "base")
        with patch.object(sync, "api", side_effect=[valid, {"commit": {"sha": "moved"}}]), \
             self.assertRaises(sync.SyncError):
            sync.verify_refs(repository, 7, "branch", "head", "base")

    def test_pending_checks_can_complete_and_are_rechecked_before_merging(self):
        pending = green() + [{"name": "late-check", "state": "PENDING"}]
        with patch.object(sync, "pr_checks", side_effect=[pending, green(), green()]), \
             patch.object(sync, "verify_refs"), patch.object(sync.time, "sleep") as sleep, \
             patch.object(sync, "api", side_effect=[protection(), {"merged": True}]) as api:
            sync.merge_when_green("owner/repo", 7, "branch", "head", "base")
        sleep.assert_called_once_with(15)
        self.assertEqual(api.call_args.kwargs["method"], "PUT")

    def test_checks_regression_between_observations_never_merges(self):
        with patch.object(sync, "pr_checks",
                          side_effect=[green(), [{"state": "FAILURE"}]]), \
             patch.object(sync, "verify_refs"), \
             patch.object(sync, "api", return_value=protection()) as api, \
             self.assertRaises(sync.SyncError):
            sync.merge_when_green("owner/repo", 7, "branch", "head", "base")
        self.assertEqual(api.call_count, 1)  # Protection read only.

    def test_local_automation_never_writes_github(self):
        with patch.dict(os.environ, {"GITHUB_ACTIONS": "false"}), \
             patch.object(sync, "api") as api, self.assertRaises(sync.SyncError):
            sync.automate(Path("."), "HEAD", "HEAD")
        api.assert_not_called()


class ChangesTests(unittest.TestCase):
    def test_workflow_linter_is_pinned_scoped_and_cache_isolated(self):
        with patch.object(ci.subprocess, "run") as run:
            ci.workflows()
        args = run.call_args.args[0]
        self.assertIn("github.com/rhysd/actionlint/cmd/actionlint@v1.7.12", args)
        self.assertEqual(args[-2:], [
            ".github/workflows/glassbox-ci.yml",
            ".github/workflows/glassbox-upstream-sync.yml",
        ])
        self.assertTrue(run.call_args.kwargs["check"])
        for key in ("GOPATH", "GOMODCACHE", "GOCACHE"):
            self.assertTrue(Path(run.call_args.kwargs["env"][key]).is_relative_to(TARGET))

    def test_downstream_and_dependency_paths(self):
        for path in ["extensionalities/README.md", "extensionalities/Cargo.lock",
                     "crates/liteparse/src/lib.rs", "crates/pdfium-sys/build.rs",
                     "Cargo.toml", "Cargo.lock", ".cargo/config.toml",
                     ".github/workflows/glassbox-ci.yml", "rust-toolchain.toml"]:
            self.assertTrue(ci.relevant(path), path)
        for path in ["README.md", "packages/node/README.md", "docs/index.md"]:
            self.assertFalse(ci.relevant(path), path)

    def test_missing_diff_is_fail_safe_and_sync_is_always_checked(self):
        self.assertTrue(ci.needs_checks(Path("."), None, "head"))
        self.assertTrue(ci.needs_checks(Path("."), "0" * 40, "head"))
        self.assertTrue(ci.needs_checks(Path("."), "base", "head", force=True))
        result = subprocess.CompletedProcess([], 1, b"", b"missing")
        with patch.object(ci.subprocess, "run", return_value=result):
            self.assertTrue(ci.needs_checks(Path("."), "base", "head"))


if __name__ == "__main__":
    unittest.main()
