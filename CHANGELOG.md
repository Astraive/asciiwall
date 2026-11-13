# Changelog

All notable changes to this repository will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and future releases will follow [VERSIONING.md](VERSIONING.md) and [docs/release-process.md](docs/release-process.md).

## [Unreleased]

**Repository status:** Design specification, repository-infrastructure baseline, and full `356`-issue planning backlog published on GitHub Project 4 (`278` in `Astraive/asciiwall`, `78` in `Astraive/asciiwall-library`). A dependency-free Rust Cargo workspace contains the portable `ascii-core` frame contract. **No application binary or published release tag exists yet.**

### Added

- Initial repository MIT license ([LICENSE](LICENSE), `Copyright (c) 2026 Astraive`).
- Frozen 100-document research and engineering specification pack (`docs/`, merged in PR `#1` at `3801d37`), plus documentation verification report ([docs/quality/documentation-verification.md](docs/quality/documentation-verification.md)).
- External local-backlog validation and dry-run publication tooling (`scripts/validate-backlog.py`, `scripts/publish-issues.ps1`, [docs/backlog-validation.md](docs/backlog-validation.md), [docs/backlog-publication.md](docs/backlog-publication.md)); live issue creation requires explicit authorization for the requested scope.
- Repository policies, governance, versioning policy, and community standards ([README.md](README.md), [CONTRIBUTING.md](CONTRIBUTING.md), [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), [SECURITY.md](SECURITY.md), [SUPPORT.md](SUPPORT.md), [GOVERNANCE.md](GOVERNANCE.md), [VERSIONING.md](VERSIONING.md), `.editorconfig`, `.gitattributes`) (`[PRJ-004]`).
- GitHub templates, ownership, release categories, Dependabot configuration, and CI/convention checks (`.github/`, `.githooks/`, `scripts/check-repository.py`, `scripts/check-conventions.py`, `scripts/check-powershell.ps1`, `scripts/configure-github.ps1`) (`[PRJ-004]`).
- Central planning and architecture entrypoints (`docs/plan.md`, `docs/roadmap.md`, `docs/architecture.md`, `docs/development.md`, `docs/issue-conventions.md`, `docs/dependency-graph.md`, `docs/quality-gates.md`, `docs/requirements-traceability.md`, `docs/platform-support.md`, `docs/release-process.md`, `docs/references.md`, `docs/github-setup.md`) (`[PRJ-004]`).
- Milestone-scoped publication with required ancestor containers, fail-closed hard-dependency boundaries, explicit publisher-account checks, deferred out-of-scope references, and scope-isolated ledger verification (`[PRJ-004]`).
- Append-only `-CreateMissing` issue-publication mode in `scripts/publish-issues.ps1` with durable `creation_batch` pinning, read-only preservation of existing issues and ledger records, milestone ownership isolation, per-item non-`Status` project custom-field GraphQL batching, offline preview basis reporting, and an isolated scope/preservation regression (`scripts/test-publisher-scope.ps1`) documented in [docs/backlog-publication.md](docs/backlog-publication.md).
- Full `356`-issue planning publication across `Astraive/asciiwall` (`278`) and `Astraive/asciiwall-library` (`78`) (`39` protected foundation issues plus `317` newly verified `-CreateMissing` issues including `asciiwall/M1`), confirmed by an independent REST/GraphQL audit (`354` native parent edges, `576` native hard blockers including `38` cross-repo edges, `356` Project 4 issue items with zero drafts, and unchanged protected issue/project metadata) documented in [docs/setup-audit.md](docs/setup-audit.md).
- Dependency-free `crates/ascii-core` COR-001 contracts for bounded ASCII frames, allocator-safe grid preflight, reusable owned cell storage, cancellation, typed displayable errors, and zero-copy validated raster views, with behavior-focused integration tests; updated the [data model contract](docs/architecture/data-model-and-contracts.md) and [E01 implementation plan](docs/plans/engine-implementation-plan.md).

### Changed

- Repaired relative Markdown navigation across existing original root documentation entrypoints (`c321475`, committed after PR `#1` and prior to `[PRJ-004]`).
- Updated Project 4 registries and audit semantics for layout-specific grouping (`BOARD_LAYOUT` `verticalGroupByFields` columns / `groupByFields` swimlanes; 17 `TABLE_LAYOUT` views with one `group_by`, UI-only `slice_by`, and two ordered sorts). Configured and saved all table views in the authenticated Orca browser, reloaded all 18 views, verified native axes/sorts against GraphQL, and recorded UI slice reads plus screenshots under external `../.issues/reports/project-view-ui-verification.json`.
- Documented the PowerShell 7.5+ minimum requirement (`ConvertFrom-Json -DateKind String`) for `scripts/publish-issues.ps1` in [docs/backlog-publication.md](docs/backlog-publication.md).

### Fixed

- Durable publication-ledger replacement uses a same-directory overwrite rename instead of the Windows PowerShell `File.Replace` null-backup binding; a real filesystem regression runs locally and in CI.
- Project-field schema preflight preserves singleton GraphQL argument collections instead of mistaking a valid input object for multiple arguments.
- Preserve ISO-looking JSON text and exact protected ledger timestamp strings through repeated reads and atomic writes; the real filesystem regression now exercises lossless reloads.
