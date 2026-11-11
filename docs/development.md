# Development workflow and local tooling

> **Current repository state:** Today's `asciiwall` repository contains project governance files, `.github/` configuration/templates, `.githooks/`, documentation, and Python/PowerShell tooling. **There is no Rust `Cargo.toml`, application binary, or buildable installer yet.** Do not interpret future `cargo` or `asciiwall` CLI examples as currently runnable commands.

## Current repository tooling and prerequisites

To inspect, validate, and work on the current repository and local external backlog (`../.issues/`), you need:

- **Python 3.12+** (standard library only; no third-party pip packages required for repository or backlog checkers).
- **PowerShell 7+** (`pwsh`) for `scripts/publish-issues.ps1`, `scripts/configure-github.ps1`, `scripts/check-powershell.ps1`, and `.githooks/install.ps1`.
- **Git** (configured for LF normalization via [`.gitattributes`](../.gitattributes) and [`.editorconfig`](../.editorconfig)).
- **GitHub CLI (`gh`)** — optional for local development; required only for read-only API inspection or explicitly authorized remote configuration/publication runs.

## Local repository and convention checks

These scripts inspect repository files (excluding Git metadata and build/cache directories) and work on a **fresh clone without requiring `../.issues/`**:

| Check | Exact command | Scope |
|---|---|---|
| Repository integrity & link check | `python scripts/check-repository.py` | Validates JSON syntax, balanced Markdown code fences, relative Markdown links and heading fragments inside the repo, and ensures no forbidden external backlog/ledger files were committed |
| Commit / PR title conventions | `python scripts/check-conventions.py --message-file <path>` | Validates Conventional Commit subject format `type(scope): subject [TICKET-ID]` and rejects `Co-authored-by` trailers case-insensitively |
| PR title check | `python scripts/check-conventions.py --title "type(scope): subject [TICKET-ID]"` | Mutually exclusive alternative to `--message-file` for checking a proposed pull-request title |
| PowerShell validation | `pwsh -NoProfile -File scripts/check-powershell.ps1` | Parses repository `.ps1` files with PowerShell's AST parser without executing them |

CI runs these checks through [repository-checks.yml](../.github/workflows/repository-checks.yml) using SHA-pinned actions, read-only contents permissions, and no `pull_request_target` execution. It also parses workflow/issue-form YAML with the runner's Ruby standard library and checks PR titles, bodies, and commit trailers. [security-analysis.yml](../.github/workflows/security-analysis.yml) scans the actual Python tooling with Bandit, audits its installed workflow dependencies with pip-audit, and reports their licenses. CI-only tools are pinned in [requirements-security.txt](../.github/requirements-security.txt), including an audited pip bootstrap; Dependabot covers both that manifest and GitHub Actions. These are not application runtime dependencies or native-platform verification. Static repository/project configuration is documented in [GitHub setup](github-setup.md).

## External backlog regeneration and dry-run commands

The 356-ticket backlog, coverage slices, and derived reports stay strictly outside both Git repositories in `../.issues/` (excluded defensively by [`.gitignore`](../.gitignore)). Full tool behavior is documented in [backlog validation](backlog-validation.md) and [backlog publication](backlog-publication.md).

```bash
# Read-only external backlog validation (zero writes, network-free)
python scripts/validate-backlog.py
python scripts/validate-backlog.py --backlog-root ../.issues --source-root . --json

# Regenerate the 5 derived local reports under ../.issues/reports/ (never committed)
python scripts/validate-backlog.py --write-reports
python scripts/validate-backlog.py --backlog-root ../.issues --source-root . --write-reports --json
```

`validate-backlog.py` refuses any `--backlog-root` located inside a Git repository or containing symlinks, and `--write-reports` writes atomically only to:
- `../.issues/reports/backlog-audit.json`
- `../.issues/reports/backlog-creation-ledger.json`
- `../.issues/reports/backlog-dependency-graph.md`
- `../.issues/reports/backlog-inventory.md`
- `../.issues/reports/backlog-requirement-traceability.md`

To preview the offline publication plan from `../.issues/` without touching GitHub or creating any local ledger/lock file:

```powershell
pwsh -NoProfile -File ./scripts/publish-issues.ps1 -DryRun
pwsh -NoProfile -File ./scripts/publish-issues.ps1 -BacklogPath ../.issues -DryRun -SourceRef main
```

Live issue publication (`-Apply`) requires explicit authorization for its scope. For the authorized application foundation batch, use `-Milestone asciiwall/M0 -ExpectedLogin NeerajCodz`; omit no scope flag accidentally, because unscoped Apply targets the full backlog. Inspect the external ledger and use scoped `-VerifyOnly` for actual remote results. Never copy local reports or `../.issues/` contents into Git.

## Opt-in Git hooks and commit rules

Repository hooks live under `.githooks/` (`pre-commit`, `commit-msg`, and `.githooks/install.ps1`) and are **opt-in** rather than silently forced on clone:

- `.githooks/pre-commit` requires a working Python 3.12+ and PowerShell 7, then runs repository hygiene and PowerShell parsing. It probes `python` and `python3` rather than accepting an unusable Windows Store alias. Installation refuses to replace another custom local hook path.
- `.githooks/commit-msg` invokes `scripts/check-conventions.py --message-file` to enforce subject format and forbid `Co-authored-by` trailers case-insensitively.
- **Contributor identity rule:** The maintainer identity `Neeraj Sathish Kumar <neerajcodz@gmail.com>` applies **only** to this initial infrastructure setup operation. Future contributors must use their own standard Git author/committer identity (`user.name` / `user.email`); repository hooks never force or rewrite contributor identities, and no `Co-authored-by` trailers are permitted by project convention.

## Future Rust workspace workflows

Once `PRJ-001` establishes the Cargo workspace and crates described in [architecture](architecture.md), implementation follows the test-first loop in [contributor workflow](delivery/contributor-workflow.md) and [build environment and toolchain](tech-stack/build-environment-and-toolchain.md):

1. Write a failing contract test for the target work unit (`E01`–`E12`, `D01`–`D12`, `P01`–`P08`, `H01`–`H08`) and verify the intended failure.
2. Implement the minimal complete behavior inside the owning crate without leaking platform `cfg` branches into `ascii-core` or duplicating conversion/validation logic.
3. Run scoped `cargo fmt`, `cargo clippy`, and `cargo test` commands for the touched feature subset (avoiding invalid cross-platform `--all-features` assumptions) plus manual native host verification where desktop integration is affected.
4. Keep an emergency Stop path (`CLI` / relaunch) reachable without depending on a system tray when testing debug desktop hosts.

## Contributor safety and workflow rules

- Never place `../.issues/` files, `publication-ledger.json`, `publication-ledger.lock`, derived `backlog-*.md`/`json` reports, personal media, proprietary fonts, or unredacted host paths inside either repository.
- Never commit unverified "placeholder" Cargo manifests, mock wallpaper packages, or fake CI passes for unimplemented Rust/native checks.
- Keep documentation paths in `docs/` lowercase kebab-case and verify internal relative links with `python scripts/check-repository.py` before opening a pull request.
