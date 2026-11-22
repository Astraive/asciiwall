# Changelog

All notable changes to this repository will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and future releases will follow [VERSIONING.md](VERSIONING.md) and [docs/release-process.md](docs/release-process.md).

## [Unreleased]

**Repository status:** Design specification, repository-infrastructure baseline, and full `356`-issue planning backlog published on GitHub Project 4 (`278` in `Astraive/asciiwall`, `78` in `Astraive/asciiwall-library`). The Rust workspace contains portable core/conversion contracts, bounded PNG/JPEG decoding, bounded text/ANSI export, and a headless CLI that now converts local images to `.txt`/`.ans`; PNG output, structural/source-color matching, and settings loading remain unavailable. **No published release tag exists yet.**

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
- Headless `asciiwall convert` argument, format-conflict, overwrite, stdout/JSON validation, terminal-safe diagnostics, bounded explicit-font loading with no implicit font selection, and no-clobber file publication. Density-mode `.txt`/`.ans` requests use the shared PNG/JPEG decoder, density matcher, and terminal-safe exporter; PNG rendering, structural matching, source-color conversion, and settings loading remain explicit capability errors (`[CLI-001]`).
- Added bounded, headless `.txt`/`.ans` serialization in `ascii-render`, with strict shared glyph checks, LF-terminated rows, ANSI16/256/true-color-only SGR, nearest-palette selection, all-or-error output, target-loss notices, and CLI integration for monochrome density conversions.
- Bounded `ascii-convert` glyph tables with measured cell-clipped masks, descriptors, deterministic fingerprints, and typed font-validation failures ([FNT-001](docs/engine/glyph-matching-and-fonts.md)).
- Restricted FNT-001 glyph-table construction to the privately stored `GlyphRepertoire` validated against printable ASCII, block elements, and Braille, with regression coverage for unsupported Unicode scalars.
- Added `ascii-media` PNG/JPEG decoding with content sniffing, strict PNG/JPEG validation, normalized owned RGB/RGBA frames, EXIF orientation, explicit color-capability errors, cancellation, and operation/shared-resource preflight budgets (`[IMG-001]`).
- Explicit `ascii-convert` sRGB/linear-light normalization, opaque-background compositing, alpha-preserving transparent-layer source-over, profile warnings, and typed HDR rejection ([CNV-002](docs/engine/color-and-image-processing.md)).
- Added named strict-cell presets for printable ASCII/space, Unicode blocks, Braille, and their union, plus deterministic scalar-specific validation and fixed-cell metric classification ([font contract](docs/engine/glyph-matching-and-fonts.md)).
- Added matcher-identical single-glyph previews with available, unavailable, invisible, and multi-cell states; source-generated font fixtures cover the complete supported repertoire and every Braille dot pattern at two raster sizes. The `ascii-render` text/ANSI exporter shares the strict scalar predicate and uses tables produced by the measured-cell builder; editor and clip-reader integrations remain absent, so FNT-002 is incomplete ([FNT-002]).
- Added `ascii-render` text-export regression coverage proving the 8px synthetic glyph reaches serialization and a 4px over-advance returns `MultiCellMetrics` before export ([VID-010]).

### Changed

- Tie CLI normalized-raster retention to its configured raster-byte ceiling through one explicit shared owner per conversion request ([CLI-001]).
- Repaired relative Markdown navigation across existing original root documentation entrypoints (`c321475`, committed after PR `#1` and prior to `[PRJ-004]`).
- Updated Project 4 registries and audit semantics for layout-specific grouping (`BOARD_LAYOUT` `verticalGroupByFields` columns / `groupByFields` swimlanes; 17 `TABLE_LAYOUT` views with one `group_by`, UI-only `slice_by`, and two ordered sorts). Configured and saved all table views in the authenticated Orca browser, reloaded all 18 views, verified native axes/sorts against GraphQL, and recorded UI slice reads plus screenshots under external `../.issues/reports/project-view-ui-verification.json`.
- Documented the PowerShell 7.5+ minimum requirement (`ConvertFrom-Json -DateKind String`) for `scripts/publish-issues.ps1` in [docs/backlog-publication.md](docs/backlog-publication.md).

### Fixed

- Durable publication-ledger replacement uses a same-directory overwrite rename instead of the Windows PowerShell `File.Replace` null-backup binding; a real filesystem regression runs locally and in CI.
- Project-field schema preflight preserves singleton GraphQL argument collections instead of mistaking a valid input object for multiple arguments.
- Preserve ISO-looking JSON text and exact protected ledger timestamp strings through repeated reads and atomic writes; the real filesystem regression now exercises lossless reloads.
- CLI-001 gives existing-output conflicts precedence over charset validation, identifies the parsed command, avoids echoing supplied parser values/paths, opens Unix font paths nonblocking and validates opened handles, and rejects Windows pipe handles and device/volume namespaces using requested and resolved paths while preserving regular-file symlinks and extended-length filesystem paths.
- CLI-001 prioritizes existing-output conflicts over format and charset validation and constrains CLI diagnostics to static messages, preventing request paths, hosts, and option values from being disclosed. Missing font input never selects a default; structural/source-color matching and PNG export fail closed.
- CLI-001 validates charsets through the shared strict-cell repertoire, loads only an explicitly selected bounded font, and uses shared bounded decoder, density, and text/ANSI export providers. Static diagnostics do not disclose input paths or glyph metadata, and failed or cancelled conversions preserve an existing output.
- CLI-001 normalizes invalid strict-cell repertoire errors to malformed configuration before font I/O while preserving typed glyph diagnostics from font measurement ([CLI-001]).
- Map only final no-clobber destination collisions to `cli.usage` (exit `2`); staging and artifact I/O failures remain `cli.io`, including `AlreadyExists` errors (`[CLI-001]`).
- Check cancellation before publishing stdout artwork, and propagate success-status JSON serialization, newline-writing, and flush failures (`[CLI-001]`).
- Avoid appending an error JSON response when CLI success-status output fails ([CLI-001]).
- Strengthen CLI-001's status-write regression to verify the published artifact survives a partially written success response without a duplicate error response (`[CLI-001]`).
- Clarify that `--config` settings loading is unavailable in this build and valid conversion requests using it fail with capability error `4` (`[CLI-001]`).
- Map FNT-002 typed missing-requested-glyph and multi-cell failures to stable, glyph-safe CLI validation diagnostics, with a process-level JSON regression ([CLI-001]).
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
- Import `ascii-render`'s synthetic-font test fixture from `CARGO_MANIFEST_DIR` for portable rustfmt resolution, keeping its description as ordinary comments so the shared source remains valid when included in a nested test module ([VID-010]).
