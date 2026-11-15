# Changelog

All notable changes to this repository will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and future releases will follow [VERSIONING.md](VERSIONING.md) and [docs/release-process.md](docs/release-process.md).

## [Unreleased]

**Repository status:** Design specification, repository-infrastructure baseline, and full `356`-issue planning backlog published on GitHub Project 4 (`278` in `Astraive/asciiwall`, `78` in `Astraive/asciiwall-library`). The Rust workspace includes portable `ascii-core` contracts, `ascii-convert` glyph-table and image-processing implementations, PNG/JPEG decoding, and the bounded `ascii-scene` runtime. **No application binary or published release tag exists yet.**

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
- Added `ascii-core` package, assignment, and local monitor identities; immutable package-release and assignment-binding conflicts; generation-stale-result guards; and a fixed v1 package-release wire descriptor with exact golden and rejection vectors ([COR-002]).
- Bounded `ascii-convert` glyph tables with measured cell-clipped masks, descriptors, deterministic fingerprints, and typed font-validation failures ([FNT-001](docs/engine/glyph-matching-and-fonts.md)).
- Restricted FNT-001 glyph-table construction to the privately stored `GlyphRepertoire` validated against printable ASCII, block elements, and Braille, with regression coverage for unsupported Unicode scalars.
- Added `ascii-media` PNG/JPEG decoding with content sniffing, strict PNG/JPEG validation, normalized owned RGB/RGBA frames, EXIF orientation, explicit color-capability errors, cancellation, and operation/shared-resource preflight budgets (`[IMG-001]`).
- Explicit `ascii-convert` sRGB/linear-light normalization, opaque-background compositing, alpha-preserving transparent-layer source-over, profile warnings, and typed HDR rejection ([CNV-002](docs/engine/color-and-image-processing.md)).
- Bounded `ascii-runtime` handoff queues, buffer leases, completion acknowledgements, and shared concurrency/memory budgets ([COR-005](docs/architecture/resource-ownership-and-shutdown.md)).

### Changed

- Hardened portable handoff ownership: live completion tokens own a strong pool handle without slot-to-pool self-pinning; unacknowledged slots stay quarantined while any pool handle remains and until generation invalidation, while dropping the last handle releases pool memory and output permits. Dropped leases reclaim only after acknowledgement; one-slot freshness replacement validates before eviction; Stop now takes priority over repeated revocation; existing transition charges transfer without double counting. CPU handoff, slot writes, raster validation, and pixel callbacks now hold shared work permits, while active output leases retain output permits until acknowledgement, generation invalidation, or pool destruction. Focused regressions also race concurrent lease drop with completion acknowledgement and sustain concurrent revocation traffic while Stop is pending. Native OS/GPU transport remains unimplemented and is not claimed.

- Recorded the COR-005 integration blocker: this workspace has no application, decoder, scene-transition manager, or transport-owner interface to wire decode/transition permits or validate native lifecycle behavior; production consumers and native transport/device evidence remain prerequisites, so issue `#73` stays open.

- Repaired relative Markdown navigation across existing original root documentation entrypoints (`c321475`, committed after PR `#1` and prior to `[PRJ-004]`).
- Updated Project 4 registries and audit semantics for layout-specific grouping (`BOARD_LAYOUT` `verticalGroupByFields` columns / `groupByFields` swimlanes; 17 `TABLE_LAYOUT` views with one `group_by`, UI-only `slice_by`, and two ordered sorts). Configured and saved all table views in the authenticated Orca browser, reloaded all 18 views, verified native axes/sorts against GraphQL, and recorded UI slice reads plus screenshots under external `../.issues/reports/project-view-ui-verification.json`.
- Documented the PowerShell 7.5+ minimum requirement (`ConvertFrom-Json -DateKind String`) for `scripts/publish-issues.ps1` in [docs/backlog-publication.md](docs/backlog-publication.md).

### Fixed

- Durable publication-ledger replacement uses a same-directory overwrite rename instead of the Windows PowerShell `File.Replace` null-backup binding; a real filesystem regression runs locally and in CI.
- Project-field schema preflight preserves singleton GraphQL argument collections instead of mistaking a valid input object for multiple arguments.
- Preserve ISO-looking JSON text and exact protected ledger timestamp strings through repeated reads and atomic writes; the real filesystem regression now exercises lossless reloads.
- Hardened FNT-001 cancellation through parser preflight, glyph description, rasterization, and final sorting; prevented implicit deep clones of budgeted tables, validated legacy cmap format 2 beside Unicode maps, fixed one-pixel edge descriptors, expanded fingerprint invalidation coverage, and documented the checked parser-allocation envelope.
- Bounded in-flight PNG/JPEG cancellation to each 128-byte compressed-input refill, preserving codec errors and releasing reservations without publishing partial frames. Work on the currently buffered compressed input is not preempted (`[IMG-001]`).
- Bound IMG-001 snapshot source reads to 128-byte requests, polling cancellation between completed calls without claiming to interrupt a blocked caller `Read` (`[IMG-001]`).
- Poll IMG-001 cancellation at row boundaries and every 128 pixels during normalization; deterministic wide one-row PNG coverage asserts no frame is published and decoder reservations are released (`[IMG-001]`).
- Poll cancellation inside JPEG marker/entropy fill-byte runs and stop charging the owned PNG snapshot again as codec scratch; bound PNG scratch by row/inflater buffers and the largest non-IDAT chunk buffer, with large ancillary and IDAT regressions (`[IMG-001]`).
- Preflight premultiplied raster channels before allocating normalized output, charge the cancellable validation pass to work budgets, and test cancellation within that pass ([CNV-002]).
- Made `NormalizedRaster` intentionally non-`Clone` to prevent unbudgeted pixel-buffer copies, and clarified that `ascii-media` decoding is outside this PR's scope ([CNV-002]).
- Reserve normalized output under a caller-shared RAII owner until each raster drops; keep `Budget` allocation-free and limited to per-operation input-plus-output peak accounting ([CNV-002]).
- Size density grids from the selected custom-crop geometry, retain normalized profile warnings in detailed conversion results, and preflight and cancellably initialize bounded quality-patch storage ([CNV-001]).
- Charge premultiplied validation and readable input extent to density-conversion work and simultaneous-memory budgets; cancel before quality-patch buffer reservations and reject sRGB8-indistinguishable foreground/background colors ([CNV-001]).
- Include final frame cell validation in density work ceilings, preserve typed unsupported pixel-format errors through preflight, and poll cancellation before reserving output cells ([CNV-001]).
- Preserve orientation error precedence for unsupported-depth density inputs while rejecting validly oriented unsupported sample depths before stride calculations ([CNV-001]).
- Preflight unsupported alpha layouts and HDR/wide-gamut metadata before density sampling geometry, work, and stride checks ([CNV-001]).
- Explain unsupported raster color profiles in the core error display so the convenience density API directs callers to supply SDR sRGB instead of reporting only generic unsupported input ([CNV-001]).
- Account for local f64 coordinate precision when checking mapped crop edges, while rejecting extents that are collapsed beyond a conservative fraction of their size (`[CNV-001]`).
- Reject contain transforms when f64 rounding cannot preserve centered letterbox margins at extreme destination sizes (`#294`).
- Normalize box-sampling overlap per axis before combining weights, allowing valid subnormal footprints without requiring representable absolute 2D area (`[CNV-001]`).
- Keep Box subinterval starts and positive spans separate while measuring overlaps relative to integer source-pixel origins, preserving narrow quality patches at large or fractional source origins ([CNV-001]).
- Collect COR-005 output permits under the buffer-pool lock, fallibly reserve for only live permits outside it, then release after unlocking; document the non-nested pool/budget lock order.
- Keep the active old-scene COR-005 memory permit borrowed while a transition is staged; abort releases only the candidate charge, while completion releases the old charge once and deactivates the original permit.
- Recheck urgent Stop/revocation flags under the ordinary control-queue lock before popping a FIFO event.
- Correct repository-state documentation to list the merged `ascii-convert` workspace crate without claiming a production application or consumer.
