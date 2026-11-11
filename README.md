# AsciiWall

> **Project status:** **Research, architecture specification, and repository-infrastructure baseline (pre-implementation).**
> There are **no compiled application binaries, Rust crates (`Cargo.toml`), installers, or published wallpaper packages** in this repository yet. All application features, CLI commands, and native platform integrations described in `docs/` are planned engineering specifications tracked for implementation.

AsciiWall is a planned open-source, local-first, offline-capable desktop wallpaper application and CLI powered by an original Rust ASCII conversion, procedural scene, and `wgpu`/`egui` rendering engine. It is designed around **capability-aware native desktop adapters** (targeting Windows first, followed by independently gated macOS and named Linux X11, layer-shell Wayland, KDE Plasma, and GNOME adapters) and **declarative `.asciiwall` packages** without accounts, telemetry, hosted servers, or executable package code.

---

## Documentation Entrypoints

Start with the specification orientation, then inspect the planning and architecture guides:

- **Start here:** [docs/start-here.md](docs/start-here.md) · [docs/readme.md](docs/readme.md) · [docs/documentation-index.md](docs/documentation-index.md) (100 original specification & research documents)
- **Planning & delivery synthesis:**
  - [docs/plan.md](docs/plan.md) — master engineering plan and critical-path overview
  - [docs/roadmap.md](docs/roadmap.md) — expanded `M0`–`M10` milestones and `L0`–`L4` library cadence
  - [docs/architecture.md](docs/architecture.md) — crate boundaries, process/worker isolation, and native host model
  - [docs/development.md](docs/development.md) — current local verification tooling, opt-in hooks, and future workspace workflow
  - [docs/issue-conventions.md](docs/issue-conventions.md) — stable ticket IDs, label taxonomy, PR/commit rules, and external backlog separation
  - [docs/dependency-graph.md](docs/dependency-graph.md) — dependency-order summary and critical-chain analysis
  - [docs/quality-gates.md](docs/quality-gates.md) — `QA-M0`–`QA-M10` / `LQA-L0`–`LQA-L4` acceptance policy and evidence tiers
  - [docs/requirements-traceability.md](docs/requirements-traceability.md) — source pack traceability across 172 settings, 120 edge cases, and 40 work units
  - [docs/platform-support.md](docs/platform-support.md) — tuple-based support policy and current unverified native status
  - [docs/release-process.md](docs/release-process.md) — SemVer, signing custody, codec/font compliance, and release gates
  - [docs/references.md](docs/references.md) — 62 primary vendor/protocol sources and reference conventions
- **Operations & tooling:** [docs/github-setup.md](docs/github-setup.md) · [docs/backlog-validation.md](docs/backlog-validation.md) · [docs/backlog-publication.md](docs/backlog-publication.md) · [docs/quality/documentation-verification.md](docs/quality/documentation-verification.md)

---

## Current Repository Contents

Only actual tracked documentation, policies, templates, hooks, and validation/configuration scripts live in this repository today:

| Path | Current Contents |
| --- | --- |
| `LICENSE` | MIT License (`Copyright (c) 2026 Astraive`) |
| `docs/` | 100 original specification documents, 11 planning entrypoints, GitHub setup guide, and backlog validation/publication guides |
| `scripts/` | `check-repository.py`, `check-conventions.py`, `check-powershell.ps1`, `validate-backlog.py`, `publish-issues.ps1`, `configure-github.ps1` |
| `.github/` | Issue forms (`ISSUE_TEMPLATE/`), PR templates, `CODEOWNERS`, `release.yml`, `dependabot.yml`, `project-management/` static registries, CI workflows |
| `.githooks/` | Read-only `pre-commit` / `commit-msg` hooks and opt-in `install.ps1` ([.githooks/README.md](.githooks/README.md)) |
| Root files | `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `SUPPORT.md`, `GOVERNANCE.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`, `VERSIONING.md`, `.editorconfig`, `.gitattributes`, `.gitignore` |

> **No fake scaffolds:** Rust crates, `Cargo.toml`, desktop binaries, bundled fonts, and media fixtures are not yet created; planned module layouts in [docs/architecture/repository-structure.md](docs/architecture/repository-structure.md) are introduced only as real implementation milestones execute.

---

## Current Verification & Local Checks

On a fresh clone with **Python 3.12+** and **PowerShell 7 (`pwsh`)**, you can run the repository's read-only verification tools directly:

```sh
python scripts/check-repository.py
python scripts/check-conventions.py --title "docs(repo): verify convention checker [DOC-001]"
pwsh -NoProfile -File scripts/check-powershell.ps1
```

Contributors can also opt in to local Git hooks (configuring **only** repository-local `core.hooksPath`, without altering global Git settings or author identity):

```powershell
pwsh -NoProfile -File .githooks/install.ps1
```

See [.githooks/README.md](.githooks/README.md) and [CONTRIBUTING.md](CONTRIBUTING.md) for details.

---

## Planning Backlog, Project 4, and Companion Library

- **External local backlog (not pushed to Git):** Detailed planning records (`356` local tickets across `Astraive/asciiwall` (`278`) and `Astraive/asciiwall-library` (`78`), `38` epics, `2` roadmaps, `300` tasks, `16` milestone gates, `2,915` requirement mappings, and `576` reduced hard-dependency edges) are stored **outside** both Git repositories in `` `../.issues` `` so unpublished backlog records and derived reports (`../.issues/reports/`) are never committed.
  - Read-only local backlog validation: `python scripts/validate-backlog.py` ([docs/backlog-validation.md](docs/backlog-validation.md))
  - Offline publication plan (`-DryRun`, zero remote writes): `pwsh -NoProfile -File scripts/publish-issues.ps1 -DryRun` ([docs/backlog-publication.md](docs/backlog-publication.md))
  - **Remote publication:** Explicitly authorized batches use `publish-issues.ps1 -Milestone <repository/milestone> -ExpectedLogin <account>`. The external publication ledger records actual remote issues, project items, and verification; local `issue.json` files remain authoring records. Stable ticket IDs such as `PRJ-001` are not GitHub issue numbers. See [publication scope and recovery](docs/backlog-publication.md).
- **Coordination board:** Organization [**GitHub Project 4**](https://github.com/orgs/Astraive/projects/4) (private Astraive coordination board covering `Astraive/asciiwall` and `Astraive/asciiwall-library`).
- **Companion artwork repository:** [**`Astraive/asciiwall-library`**](https://github.com/Astraive/asciiwall-library) — holds curated wallpaper recipe definitions, attribution records, and static catalog workflows (no runtime backend; consumes pinned engine releases once implemented).

---

## Key Architectural Invariants

1. **Offline-first & private by design:** Core conversion, import, preview, playback, and export must operate with network access denied; no user accounts, telemetry, or covert screenshots ([docs/security/privacy-and-network-policy.md](docs/security/privacy-and-network-policy.md)).
2. **Truthful platform tuples:** Support means a tested `(OS build, session, compositor/shell, GPU backend, packaging, capabilities)` tuple — never a blanket OS promise ([docs/platform/support-policy.md](docs/platform/support-policy.md)).
3. **Declarative v1 packages:** `.asciiwall` v1 packages contain data and licensed assets only, never executable plugins, scripts, or arbitrary user shaders ([docs/security/plugin-and-shader-policy.md](docs/security/plugin-and-shader-policy.md)).
4. **Separated rights layers:** Repository source and specs are MIT-licensed ([LICENSE](LICENSE)), while future wallpaper art, fonts, and native codec builds carry independent licenses and third-party notices ([docs/library/licensing-and-attribution.md](docs/library/licensing-and-attribution.md)).

---

## Policies & Governance

- [CONTRIBUTING.md](CONTRIBUTING.md) — workflow, PR/commit rules (`type(scope): subject [TICKET-ID]`, no `Co-authored-by` trailers), and review checklist
- [SECURITY.md](SECURITY.md) — private vulnerability reporting via [GitHub Security Advisories](https://github.com/Astraive/asciiwall/security/advisories/new) and security boundaries
- [SUPPORT.md](SUPPORT.md) — current pre-release boundaries and diagnostic expectations
- [GOVERNANCE.md](GOVERNANCE.md) — `@NeerajCodz` single-maintainer bootstrap governance and review model
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) — community participation standards
- [VERSIONING.md](VERSIONING.md) — independent application SemVer and draft format versioning
- [CHANGELOG.md](CHANGELOG.md) — `[Unreleased]` baseline summary
