# Extensionalities agent guide

## Isolation contract (downstream mini-repo seam)

Treat `extensionalities/` as a standalone downstream mini-repo inside `liteparse-extended` so upstream `liteparse` syncs and merges stay clean:

- Keep all fork-specific source, manifests, lockfile, fixtures, development records (`dev/` and `dev/PRDs/`), and test/audit scratch (`sandbox/` and `target/`) exclusively inside `extensionalities/`.
- Never place `dev/`, `PRDs/`, `sandbox/`, or downstream artifacts in the upstream repository root or upstream crate directories.
- This is an independent Cargo workspace. Do not register its crates in the upstream root workspace.
- Use the upstream `liteparse` public library through the path dependency. Do not patch its CLI or other upstream-owned files.
- Build and check with the explicit `extensionalities/Cargo.toml` manifest; commit this workspace's `Cargo.lock`.
- Keep built-in Tesseract enabled. Do not disable it to make a build pass.
- `glassbox` provides native inspection and extraction. `litx` remains a compatibility alias for the same implementation.
- The Rust package names remain `litx` and `litx-inspect`. Neither command replaces upstream `lit`.
- Further features and consumer delivery require separate approval.
- Preserve the root workspace and lockfile during dependency updates. Public API changes can break this workspace even when Git merges cleanly.
- Keep build products, sandbox files, credentials, and private documents out of commits. Review fixtures before adding them.
- Do not push, publish, activate delivery bundles, or modify consumer recipes without explicit authorization.

## Documentation, sandbox, and entrypoints

- Keep non-code development documentation under [`dev/`](dev/README.md), not beside the Cargo workspace files. This `AGENTS.md` remains at the downstream root as the agent entrypoint.
- [`README.md`](README.md) is the public Glassbox guide. Keep `../README.md` unchanged as upstream documentation.
- Group each feature's phases in its own `dev/PRDs/YYYYMMDD_<feature-slug>/` folder. The inspection/extraction work is [`20261117_litx-inspect-extract`](dev/PRDs/20261117_litx-inspect-extract/README.md) and XML profiling is [`20261118_xml-structure-profile`](dev/PRDs/20261118_xml-structure-profile/README.md).
- Keep temporary audit harnesses, dogfood probes, and scratch test fixtures inside [`sandbox/`](sandbox/) (gitignored by `extensionalities/.gitignore`), not in the upstream root or `/tmp`. Clean temporary scratch inputs after runs.
- Keep code, Cargo manifests/lockfile, scripts, and executable test fixtures in their existing locations. Text fixtures such as `fixtures/hello.txt` are test data, not development documentation.
- Preserve historical reports and their approvals, limitations, verification claims, and receipts. Moving a report does not make its historical checks current or grant audit clearance.
- Before editing this guide, preserve a dated recoverable original under `dev/instruction-history/` and record its SHA-256. Prior versions: [`20261118-AGENTS.before-prd-routing.md`](dev/instruction-history/20261118-AGENTS.before-prd-routing.md) (`7aec3d6677b48b1ce0e00eea2deb7b3ccd19d5e46fe448f05b0f42f61a89dc56`) and [`20261118-AGENTS.before-downstream-seam.md`](dev/instruction-history/20261118-AGENTS.before-downstream-seam.md) (`8adb95c42350b17cf232029b377d8b1cf7f901c15f04dd603f92f83c0d310e27`).

## Shared PRD management

The pre-branding guide is preserved in [`20261119-AGENTS.before-glassbox-branding.md`](dev/instruction-history/20261119-AGENTS.before-glassbox-branding.md).
Its SHA-256 is `b0bafe0f8712a4e648c7cb6ba5bd9805344754c74403e012d6cfd6449c2a9867`.

The guide before README relocation is preserved in [`20261120-AGENTS.before-readme-isolation.md`](dev/instruction-history/20261120-AGENTS.before-readme-isolation.md).
Its SHA-256 is `86f35f33e121e364f53651a50a988c1ac7c53fd86f40553cd29a0d7ca8d60fa9`.

Use the same shared lifecycle as `~/chai/agents/coding/`; do not copy the manager or agent definitions into this repository.

Read these shared contracts when producing or receiving feature records:

- `~/chai/agents/meta/contracts/handoff-contract.md`
- `~/chai/agents/meta/contracts/prd-notes.md`
- `~/chai/agents/meta/contracts/prd-vocabulary.md`
- For audits, also `~/chai/agents/meta/contracts/audit-return-contract.md`.

Resolve `~/chai` on the current host. Run `python3 -B "$HOME/chai/agents/scripts/manage_PRDs.py" --folder <absolute-feature-folder> --route` at intake and before handoff. Follow the returned paths instead of guessing the active stage from phase numbers or timestamps.

| Owner | Lifecycle record and gate |
| --- | --- |
| `master-explore` | Agree the feature folder name, then `scaffold intent`. Keep unpromoted ideas as folderless entries in `dev/PRDs/IDEAS.md` when needed. |
| `master-refine` | `scaffold design`; own numbered ADRs under `decisions/`. Cite the source intent and decisions. |
| `master-plan` | `scaffold spec` and `scaffold milestones`; own the verification contract, task DAG, and routing clarity. |
| `master-drive` / implementation workers | Follow the approved spec and `milestones.json`; require both recorded approval and `execution_ready: true` before new planned implementation. |
| `master-audit` | Independently audit in another thread. Use milestone-scoped `scaffold audit`, preserve review waves/remediation and snapshot evidence, then `record-audit` with reconciliation/verdict outputs. |
| `worker-record` | Reconcile documentation and evidence; archive the entire feature under `dev/documentary/` only after the required audit and release gates. |

Use the manager's `scaffold`, `link`, and `record-audit` commands to maintain managed frontmatter and relationships. New stage records remain `draft` until approval is recorded in their Handoff; do not infer approval from implementation, a passing test run, or a phase report.

After changing managed records, run `--write-index --route` for that feature. The manager owns only its marked section in the feature's generated `AGENTS.md`; preserve surrounding instructions. Atlas Indexer is required by the shared manager. Report missing tools rather than inventing an index or bypassing checks.

The migrated inspection/extraction reports are historical evidence linked from a retrospective **draft** intent. No historical design/spec approval, task completion, or independent audit verdict has been fabricated. The pending read-only audit may proceed under its explicit user brief; the migration does not authorize new implementation.

## Upstream-sync ignore guard

User-authorized upstream-file exceptions are the ignore rule and these GitHub-required entry files:

- `../.gitignore`: preserve `/extensionalities/dev/`.
- `../.github/workflows/glassbox-ci.yml`: downstream checks.
- `../.github/workflows/glassbox-upstream-sync.yml`: checked upstream synchronization.

Keep workflow implementation and tests under `scripts/`; keep the root README unchanged.
The sync preserves exact downstream and workflow trees and mirrors upstream-owned paths elsewhere.
Automatic merging requires every reported check to succeed and enforced, strict required checks on `main`.
Publication, live activation, credential installation, and billing changes require separate authorization.

The pre-automation guide is preserved in [`20261121-AGENTS.before-ci-automation.md`](dev/instruction-history/20261121-AGENTS.before-ci-automation.md).
Its SHA-256 is `7e7b23fea299cb8df936dfd96f66a87efc3755c0af8e96b74f2a9a646f0d005a`.

- Check that exact rule after upstream pulls, merges, rebases, or other syncs, and before staging development records.
- If a sync removes it, restore only that rule while preserving the other upstream ignore entries.
- From the repository root, verify with `git check-ignore --no-index -v extensionalities/dev/README.md`. The matching rule must be `/extensionalities/dev/` in the root `.gitignore`.
- Ignoring a directory does not untrack existing files. Do not use `git rm --cached`, force-add ignored records, commit, or alter the index without explicit authorization.
- Keep private source documents, raw extraction reports, binaries, caches, and generated test evidence under ignored `target/`, not in PRDs.

## Branch, PR, and merge approval

Work on feature branches and complete local audits before opening a PR to Glassbox's `main`.
GitHub checks are a final external check, not independent audit clearance.
Run downstream formatting, Clippy, tests, release smoke checks, and Glassbox workflow validation.
Do not run the inherited upstream test matrices.

Feature PRs, including the CI setup PR, require Sri's explicit merge instruction after checks pass.
Green checks never supply that permission.
Weekly upstream-sync PRs are the approved all-green automatic exception.
Never bypass protection, enable auto-merge for feature PRs, or push directly to `main`.
After an approved GitHub merge, fetch and fast-forward local `main` to the exact remote commit.
Preserve the current branch and dirty work; stop on divergence instead of resetting or creating another merge.
Do not delete branches without separate permission.

The guide before PR activation is preserved in [`20261121-AGENTS.before-pr-activation.md`](dev/instruction-history/20261121-AGENTS.before-pr-activation.md).
Its SHA-256 is `603911a38e2cb8cebe49b4274f4b73eb126777cd9f87b0f2b3364f0442f97ebf`.
