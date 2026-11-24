# Changelog

All notable changes to this repository will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and future releases will follow [VERSIONING.md](VERSIONING.md) and [docs/release-process.md](docs/release-process.md).

## [Unreleased]

**Repository status:** Design specification, repository-infrastructure baseline, and full `356`-issue planning backlog published on GitHub Project 4 (`278` in `Astraive/asciiwall`, `78` in `Astraive/asciiwall-library`). The Rust workspace includes portable `ascii-core` contracts, the `ascii-convert` glyph-table implementation, and PNG/JPEG image decoding. **No application binary or published release tag exists yet.**

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
- Bounded `ascii-convert` glyph tables with measured cell-clipped masks, descriptors, deterministic fingerprints, and typed font-validation failures ([FNT-001](docs/engine/glyph-matching-and-fonts.md)).
- Restricted FNT-001 glyph-table construction to the privately stored `GlyphRepertoire` validated against printable ASCII, block elements, and Braille, with regression coverage for unsupported Unicode scalars.
- Added `ascii-media` PNG/JPEG decoding with content sniffing, strict PNG/JPEG validation, normalized owned RGB/RGBA frames, EXIF orientation, explicit color-capability errors, cancellation, and operation/shared-resource preflight budgets (`[IMG-001]`).
- Explicit `ascii-convert` sRGB/linear-light normalization, opaque-background compositing, alpha-preserving transparent-layer source-over, profile warnings, and typed HDR rejection ([CNV-002](docs/engine/color-and-image-processing.md)).
- Added offline evidence recording and synthetic decoder/host controls (`scripts/evidence.py`, `scripts/test_evidence.py`, `tests/evidence/`): seven distinct evidence levels with required hashes and environment/permission/reviewer metadata, absent native environments enforced as `not-run`, exact CPU-grid diffs, sanitized logs, and bounded per-type retention. The new evidence tests run on Linux, macOS, and Windows, including Windows NTFS handle tests. Documented usage, evidence limits, and outstanding native procedures in [test strategy](docs/quality/test-strategy.md).

### Changed

- Repaired relative Markdown navigation across existing original root documentation entrypoints (`c321475`, committed after PR `#1` and prior to `[PRJ-004]`).
- Updated Project 4 registries and audit semantics for layout-specific grouping (`BOARD_LAYOUT` `verticalGroupByFields` columns / `groupByFields` swimlanes; 17 `TABLE_LAYOUT` views with one `group_by`, UI-only `slice_by`, and two ordered sorts). Configured and saved all table views in the authenticated Orca browser, reloaded all 18 views, verified native axes/sorts against GraphQL, and recorded UI slice reads plus screenshots under external `../.issues/reports/project-view-ui-verification.json`.
- Documented the PowerShell 7.5+ minimum requirement (`ConvertFrom-Json -DateKind String`) for `scripts/publish-issues.ps1` in [docs/backlog-publication.md](docs/backlog-publication.md).

### Fixed

- Validate automated evidence references relative to an explicit artifact root, with no-follow regular-file checks for QA-101 package locators ([QA-004]).

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
- Validate synthetic-control reports against scenario-specific fault/output expectations and configured bounds; reject invalid Unicode during canonical serialization and C1 controls in evidence metadata or sanitized-log reports.
- Harden native/manual evidence inspection and bounded retention with descriptor-relative POSIX access and handle-anchored local NTFS access on Windows; typed manifests hash-check binary evidence without claiming to authenticate execution, and unsupported Windows volumes fail closed. Native handles are closed if conversion to a Python value fails. Documented privacy and concurrent-writer limitations in the [test strategy](docs/quality/test-strategy.md).
- Redact dotted private-field labels and Windows drive-relative paths from sanitized logs and free-text record metadata; document that artifact content still requires human privacy review.
- Reject recognized synthetic reports in native-manual manifest references, including oversized JSON disguised by leading whitespace; matching hashes do not authenticate native execution.
- Reject prefixed synthetic evidence with Unicode-escaped markers regardless of prefix quoting in both manifest and direct-artifact inspection ([QA-004]).
- Visibly escape bidi formatting controls in sanitized evidence metadata and logs before canonical JSON output ([QA-004]).
- Classify streamed JSON primitives across chunk boundaries, rejecting oversized UTF-16 and long-number JSON while preserving numeric-leading and NUL-padded binary artifacts ([QA-004]).
- Reject oversized UTF-16/UTF-32 JSON native evidence, including primitive scalars, before it can bypass the 1 MiB inspection limit ([QA-004]).
- Exercise UTF-32 scalar carry-over across a short 64 KiB stream read, BOM-less LE/BE synthetic JSON detection, and invalid scalar rejection in both byte orders while retaining ordinary-binary controls ([QA-004]).
- Detect UTF-16 LE/BE synthetic markers in oversized native manifest artifacts after non-JSON prefixes, without narrowing the binary artifact size contract ([QA-004]).
- Detect Unicode-escaped synthetic marker keys in oversized UTF-16LE/BE native artifacts after non-JSON prefixes and across 64 KiB stream boundaries ([QA-004]).
- Prune expired POSIX artifacts through a same-filesystem mode-0700 quarantine, verify the moved inode before deletion, and restore replacements with a no-clobber link; serialize cooperating recorder writes and pruning with a persistent advisory lock, while documenting the uncooperative-writer limit ([QA-004]).
- Inject same-directory replacements after the final POSIX pre-move snapshot and verify the replacement is restored, the prune reports incomplete, and normal quiescent archives still meet their retention bound ([QA-004]).
- Route `record --archive` through the persistent POSIX lock used by other archive writes, and assert its inode remains stable across duplicate writer processes ([QA-004]).
- Include completed POSIX archive removals in descriptor-close errors and inject the failure after deletion ([QA-004]).
- Surface Windows archive file-handle close failures during pruning even alongside scan, type, or read errors; preserve the primary failure in the cause chain instead of silently skipping the candidate ([QA-004]).
- Propagate Windows native manifest/artifact handle-close failures through record validation and abort pruning instead of skipping candidates ([QA-004]).
- Preserve completed Windows archive deletions and the primary partial-prune error if directory cleanup also fails, keeping cleanup failures secondary and observable ([QA-004]).
- Include a Windows archive entry deleted before its handle-close failure in partial-prune reports ([QA-004]).
- Surface Windows archive directory-close failures after a successful prune, even inside an unrelated active exception; preserve prune errors as primary when cleanup fails ([QA-004]).
- Sort archive retention by signed modification times so pre-1970 artifacts retain the chronologically newer file ([QA-004]).
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
- Reject uninspectable long-integer JSON in direct native evidence instead of treating it as non-JSON text ([QA-004]).
- Bound aggregate QA-004 prune verification reads to 256 MiB across candidate records, native manifests, referenced artifacts, and Windows rereads; report over-budget scans as errors before deletion or successful counts ([QA-004]).
- Scan fixed synthetic markers across UTF-8, UTF-16LE/BE, and UTF-32LE/BE independently of JSON classification, including escaped markers in oversized non-ASCII-prefixed artifacts ([QA-004]).
- Preflight Windows candidate snapshots and bound each delete/retained-entry reread to its precharged scan-time file size; fail closed on changed entries and report later partial pruning as incomplete within the 256 MiB aggregate cap ([QA-004]).
- Prefer the 1 MiB error for oversized JSON that also contains synthetic markers while continuing to reject markers in non-JSON evidence; assert Windows preflight mutations as incomplete pruning rather than stale retention ([QA-004]).
- Reject oversized BOM-less UTF-16BE JSON strings whose first content character is non-ASCII, including supplementary-plane scalars, before native-manifest binary acceptance ([QA-004]).
- Scan synthetic markers at every byte alignment for UTF-16/32 content and recheck POSIX native evidence directory-entry identity after streaming ([QA-004]).
