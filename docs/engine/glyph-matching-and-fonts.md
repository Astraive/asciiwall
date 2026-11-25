# Glyph matching, fonts, and character sets

## Glyph table

Build an immutable glyph table from a font fingerprint, raster size, cell dimensions, baseline, glyph repertoire, and rendering mode. Rasterize masks with the chosen font library, then store coverage, bounding box, centroid, edge-orientation features, and the mask itself. Rasterization support is not equivalent to text shaping support. [S26](../research/source-register.md#s26)

### Executable font-table contract

`ascii-convert::build_glyph_table(font_bytes, &repertoire, config, budget, cancellation)` builds one row-major, 8-bit coverage mask per requested Unicode scalar. `GlyphRepertoire::new(&str)` validates a nonempty, duplicate-free repertoire and stores it privately; its only accepted scalars are printable ASCII (including space), Unicode block elements U+2580–U+259F, and Braille patterns U+2800–U+28FF. The public `is_supported_single_cell_scalar` predicate is the shared contract for future consumers. The builder accepts only `&GlyphRepertoire`, so raw strings cannot bypass validation. Missing font glyphs remain a separate typed error. `GlyphConfig` requires finite positive raster size, nonzero cell dimensions, and a baseline no lower than the cell's bottom edge.

Each glyph is rasterized with pinned `fontdue 0.9.3` (default features disabled; `hashbrown` enabled, SIMD disabled), centered by advance width and positioned at the configured baseline, then clipped to the cell. Space remains a real measured blank mask. Bounds, coverage-weighted centroid, normalized mean coverage, dimensionless X/Y total variation, and four gradient-orientation energy bins come from the resulting mask. The immutable table sorts by measured coverage, then font glyph index, then Unicode scalar.

The SHA-256 table fingerprint includes descriptor version 2, rasterizer mode, font bytes, raster size's IEEE-754 bits, cell width/height, baseline, and the canonical sorted scalar repertoire. Version 2 includes normalized X/Y total-variation features; changing descriptor semantics invalidates the table fingerprint. Reordering the same repertoire does not change the fingerprint. Exact dependency versions and enabled features are recorded in `crates/ascii-convert/Cargo.toml` and `Cargo.lock`.

Before raster-mask allocation, the builder checks encoded input length, glyph count, cell-mask/table bytes, rasterizer scratch, checked work, and cancellation. It returns only a complete table or a typed error; no partial glyphs are published. `GlyphRecord` and `GlyphTable` are intentionally non-`Clone`, so `Clone` cannot implicitly duplicate masks outside the construction budget. Cancellation is polled every 256 cmap/outline work units and mask pixels, plus each raster row. `ttf-parser`/`fontdue` parse and rasterize calls and final sorting are not internally interruptible; checks around them ensure cancellation observed before return yields `Error::Cancelled` rather than a table.

Work accounting separately includes parser preflight and rasterization. The preflight follows the pinned parser's outline expansion and curve subdivision, including composite components, and records a checked edge-traversal estimate per glyph at the requested raster size. For each retained line, it bounds the scalar rasterizer's vertical pixel steps, or horizontal plus vertical steps for a diagonal, with allowances for coordinate rounding and the final pixel update. Each requested scalar is charged, including multiple scalars mapping to the same glyph; bitmap-area and cell work are charged separately. Valid legacy cmap format-2 subtables are validated and budgeted even beside Unicode maps; malformed subtables still fail with typed errors. Estimates that overflow, or total work above the supplied limit, produce typed errors before rasterization. These are conservative work units for the modeled loops, not a wall-clock guarantee.

The raster-byte budget is a checked logical-allocation envelope, not a process-RSS or allocator-metadata limit. Encoded input is charged once alongside masks, table records, rasterizer scratch, and parser/preflight storage. For pinned `fontdue 0.9.3`, the preflight conservatively models 128 bytes per glyph record, 32 bytes per hash-map entry (including capacity slack), 128 bytes per retained outline line, and up to three name-capacity bytes per encoded font byte; its glyph/edge vectors and curve stack are also charged. Checked multipliers, per-input caps, and the total budget reject estimates before `fontdue` parsing/rasterization allocations. The allocation envelope follows the pinned parser/rasterizer layouts and must be revisited when those dependency implementations change; physical allocator overhead is outside the modeled limit.

Version one enforces that repertoire at construction: C0/C1 controls, bidi formatting, combining marks and variation selectors, emoji, CJK, and all other unlisted scalars are rejected before font lookup. Full shaped text, grapheme clusters, CJK width rules, and color emoji require a separate design rather than silently breaking the grid.

## Density matching

For mask `m_g(p)` in [0,1], coverage is its mean over the cell. Compare the target's normalized luminance against the reconstructed cell luminance, taking foreground and background polarity into account. A hard-coded string such as ` .:-=+*#%@` is only a starter UI preset; actual glyph darkness depends on font and size.

Coverage ties are resolved deterministically. Space is a valid glyph. Sparse masks can become visually invisible at small sizes, so the repertoire editor should show actual rasterized previews rather than only Unicode labels.

## Structure-aware matching

CNV-007 implements patch description and candidate shortlisting; detailed pixel reconstruction scoring, color fitting, and temporal penalties are separate work. `StructuralDescriptor::from_luminance(samples, dimensions, foreground_luminance, background_luminance, budget, cancellation)` consumes the retained linear-light patch and maps each luminance sample to density coverage:

```text
coverage = clamp((luminance - background_luminance)
                 / (foreground_luminance - background_luminance), 0, 1)
```

This definition supports either foreground/background polarity and makes tone, shape, and edge features dimensionless. Tone is mean coverage. X and Y shape features are the mean per-row/per-column total variations, clamped to `[0, 1]`. The four edge-orientation fractions use the same finite differences, zero-boundary convention, bins, and energy normalization as immutable glyph masks. A zero-energy patch remains finite and has zero orientation bins.

For shortlist comparison, normalized samples are deterministically resampled to the glyph table's cell width and height before calculating candidate features. Each target cell-center coordinate maps to the source index `floor(((2 * i + 1) * source_extent) / (2 * target_extent))`; coverage, gradients, and orientation therefore use the same raster geometry as glyph descriptors even when input patch dimensions differ. Source descriptor getters continue to report native patch features, while candidate scores use the common cell-grid features.

The shortlist uses these normalized distances:

```text
tone_distance  = abs(target_coverage - glyph_coverage)
shape_distance = 0.5 * L1(target_xy_variation, glyph_xy_variation)
edge_distance  = 0.5 * L1(target_orientation_bins, glyph_orientation_bins)
score          = w_tone * tone_distance + w_shape * shape_distance + w_edge * edge_distance
```

`StructuralWeights::new` validates each nonnegative weight in `[0, 1]` and a sum of one; therefore every distance and combined score remains in `[0, 1]`. `ShortlistOptions` validates a maximum candidate count of 1–4096, and a call also rejects a count larger than its glyph repertoire. `StructuralScratch` reuses a caller-owned top-K heap bounded by that count. Candidates are returned in score order with stable ties by font glyph index, Unicode scalar, then table position; no detailed reconstruction is performed by this API.

`StructuralIdentity` is SHA-256 over structural descriptor version 2, the resolved `SamplingIdentity`, source patch dimensions and native/common feature values, the glyph-table mask fingerprint, foreground/background luminance, maximum candidates, and normalized weights. The common cell-grid features are part of the ranking semantics, while source dimensions still invalidate identity when sampling geometry changes. The scratch byte envelope and checked patch-feature, scoring, and heap work are validated against the supplied `Budget`; cancellation is polled at bounded work checkpoints. Malformed controls, budget exhaustion, arithmetic overflow, allocation failure, and cancellation retain their distinct `ascii_core::Error` variants, and failure returns no partial shortlist.

`convert_structural` runs a bounded shortlist and scores each shortlisted mask using normalized linear-light RGB reconstruction `R = B + (F - B) M`, descriptor distance, tone mean error, and an optional temporal glyph-change penalty. The finite weights are supplied with `ConversionSettings::with_reconstruction_weights`; their sum is one, the default is `(0.70, 0.20, 0.05, 0.05)`, and zero temporal weight makes the result independent of `previous`. The default candidate cap is 64. Equal scores resolve by font glyph index and then table position. Sampling patches are at measured glyph-cell dimensions, and patch, shortlist, rerank, output, and cancellation work are bounded by the call's budget.

Candidate-specific palette fitting uses CNV-005's configured strategy for each shortlisted mask, retaining the fitted foreground/background pair and scoring that candidate's reconstructed linear-light RGB error alongside the same descriptor, tone, and temporal terms. All candidates in one call use the same configured fixed-color or palette-fit path; color-fit work and palette storage are included in the budget preflight. The structural phase's checked aggregate work bound is reserved before density matching, so both phase preflights share one operation-wide work limit.

The synthetic vertical-edge regression uses two same-density masks. It checks that explicit per-pixel reconstruction picks the vertical mask for a vertical edge, repeats deterministically, and ignores a previous frame when temporal weight is zero. This demonstrates a shape-sensitive fixture only; it is not a reproduction of the cited research paper and does not establish universally better fidelity or a performance improvement over density mode.

## Contour matching (CNV-009)

`convert_contour` is an explicit structure/aesthetic option alongside density and structural reconstruction. It reuses linear-light normalized patches and calibrated immutable glyph masks, takes a bounded structural top-K shortlist, then reranks those candidates by thresholded tone, oriented contour, and gradient-shape loss. `ContourOptions::new` validates the candidate cap, normalized contrast/edge thresholds, and finite normalized objective weights. Stable ties use score, font glyph index, then scalar. `ContourIdentity` hashes the resolved sampling/crop identity, glyph fingerprint, all thresholds/weights/candidate controls, calibrated colors, patch values, and algorithm version.

Configured foreground/background luminance calibrates both polarities. Target coverage below the contrast threshold is treated as background; edge gradients below the edge threshold are ignored. A patch without any surviving edge uses the explicit empty-background rule: select the least-coverage glyph (then lowest font glyph index), even for a uniform foreground patch. This removes texture by design and can discard tone. The mode emits the configured fixed color pair; candidate-specific palette fitting is not supported. Its higher per-patch cost and deliberate tone loss make it a structure/aesthetic tradeoff, never a universal fidelity upgrade.

The source-generated regressions exercise horizontal and vertical masks (`equal_area_orientation_font()`) and both diagonal orientations (`diagonal_orientation_font()`), plus deterministic selection, endpoint thresholds, identity changes, and the empty-background policy. Matched low/high-threshold runs use identical source/table, sampling and candidate cap; they separately assert silhouette selection, measured tone error, and equal reserved work. Faint texture and uniform light/dark fields verify threshold suppression and background behavior. No external image or runtime asset is used.

Matched comparisons should hold the generated font/table, raster crop and sampling filter, grid/cell size, fixed colors, and candidate limit constant. Report silhouette/orientation agreement, tone error against the unthresholded patch (so suppression is visible), and elapsed or checked work cost as separate measures. `ContourConversion::work_units_bound()` exposes the deterministic shortlist, rerank, and frame-validation bound for matched cost reporting; it does not include shared normalization/sampling. The fixture demonstrates only this controlled synthetic case, not general image quality or speed.

### Contour evidence record

Executed on Windows x64 MSVC with rustc 1.100.0/Cargo 1.100.0: `CARGO_HOME='D:/worktrees/cnv-008-cargo-home-isolated' CARGO_TARGET_DIR='D:/worktrees/cnv-009-contour-target' cargo test --offline --locked -p ascii-convert --test contour_matcher` (6 passed); `cargo fmt --all -- --check`; the same environment prefix with `cargo clippy --offline --locked --workspace --all-targets -- -D warnings`; `python scripts/check-repository.py` (127 Markdown and 3 JSON files). The workflow runs `cargo test --locked --package ascii-convert --test contour_matcher` on its Ubuntu, Windows, and macOS matrix. Matched fixtures use one generated table, 8×12 cell, one-cell contain/box sampling, linear gray input, fixed white-on-black colors, K=2. Tests report threshold tone loss separately from silhouette choice; the returned contour work bound includes shortlist, reranking, and frame-validation ceilings, excluding shared normalization/sampling.

Input fingerprints (SHA-256): `tests/support/synthetic_font.rs` = `b052dfbf80e227d85b0f0a3ba0dfceaed2bf0bc47b1e653661b5c31cc398d270`; `tests/contour_matcher.rs` = `24e420404cffc1a7d20081e783f06eb650cb2d61b3441dfbf8cb8576a02367d2`; `.github/workflows/repository-checks.yml` = `f67db0c4c725788f37822d8bc79bd416641119bb7ba37b7e0b071a656c86de2f`; `src/contour.rs` = `c24c115aed8a66375396faae5d387f831c0e6e27d7453f338f64161b09d1ba7a`; `src/density.rs` = `575c1a59c645a69a60b6d5b87f90e79227b17e1491360a0df6dcd83eb37b124f`; `src/lib.rs` = `b095f8ce8c0fb453df0936a46d31dd814602da22402f32ebbf3b92bb8a20a382`; converter manifest = `41396d81d09ba717454bc8a76c1c35b634136b6352b408850669093512214bd4`; workspace manifest = `5ff5c32ec18a9806226a3445e38afc072eb65ecb4b3b65488c32a7ca06516630`; `Cargo.lock` = `a2c482e2f1b033b44d2a9166610a88046dc2084201678df206ae175e2c77b8f4`; toolchain file = `c3dcaa2e4c0d9b494b33f2d1539f78166c78b3f1355bb68934405d559af4db43`.

Observed toolchain environment: Windows x64 MSVC target `x86_64-pc-windows-msvc`, rustc 1.100.0 (`0ef1bc386`, 2026-11-09), Cargo 1.100.0 (`3acc79c0c`, 2026-11-05), LLVM 23.1.5. Results for this updated branch: contour matcher 6 passed; strict locked workspace Clippy passed; `cargo fmt --all -- --check` passed; repository checker passed (127 Markdown and 3 JSON files).

## Structural matcher evidence

The deterministic synthetic vertical-edge fixture is generated in-process by `equal_area_orientation_font()` in `crates/ascii-convert/tests/support/synthetic_font.rs`; raster samples are copied from the generated vertical glyph mask. No external image, network fetch, or runtime asset is used. The fixture compares equal-density horizontal and vertical masks and asserts stable selection, repeated output equality, no change with a zero temporal weight, and successful CNV-005 two-color least-squares fitting on the same candidate set. A separate deterministic two-cell case succeeds for one cell under a 7,500-unit cap but returns `BudgetExceeded(WorkUnits)` for the two-cell operation; the same call then succeeds under the full budget.

Targeted commands (with the isolated offline dependency cache):
`CARGO_HOME='D:/worktrees/cnv-008-cargo-home-isolated' CARGO_TARGET_DIR='D:/worktrees/cnv-008-target' cargo test --offline --locked -p ascii-convert --test structural_matcher`
`CARGO_HOME='D:/worktrees/cnv-008-cargo-home-isolated' CARGO_TARGET_DIR='D:/worktrees/cnv-008-target' cargo test --offline --locked -p ascii-convert --test density_matcher`

Supported environment verified: Windows 10 x64, `x86_64-pc-windows-msvc`, Rust/Cargo 1.100.0.

Results: structural matcher **12 passed**, density matcher **29 passed**. `CARGO_HOME='D:/worktrees/cnv-008-cargo-home-isolated' CARGO_TARGET_DIR='D:/worktrees/cnv-008-target' cargo clippy --offline --locked --workspace --all-targets -- -D warnings` passed; `cargo fmt --all -- --check` passed; `python scripts/check-repository.py` passed.

Fingerprint basis (SHA-256):
- Fixture generator: `b052dfbf80e227d85b0f0a3ba0dfceaed2bf0bc47b1e653661b5c31cc398d270` (`tests/support/synthetic_font.rs`).
- Test and fixture parameters: `bdfa1d2a58c3644d5ddfc4c70351223f08aa4b8ea752f4534dfeb5e4e1fa402b` (`tests/structural_matcher.rs`).
- Locked dependency/config inputs: `a2c482e2f1b033b44d2a9166610a88046dc2084201678df206ae175e2c77b8f4` (`Cargo.lock`), `5ff5c32ec18a9806226a3445e38afc072eb65ecb4b3b65488c32a7ca06516630` (`Cargo.toml`), and `c3dcaa2e4c0d9b494b33f2d1539f78166c78b3f1355bb68934405d559af4db43` (`rust-toolchain.toml`).

The scores are a static quality objective only. No throughput, broad fidelity, or paper-reproduction claim is made. Full CNV-008 acceptance remains blocked pending independent exact-head review.

## Temporal stabilization

A prior glyph can be retained when the new candidate's improvement is smaller than a hysteresis threshold. Make the threshold sensitive to time and local motion so noise is reduced without freezing legitimate changes. Clear history on scene cuts, seek, source change, font/grid change, and decode discontinuity. A global high penalty may look stable while destroying moving detail; measure both stability and reconstruction quality.

## Font portability

A package refers to a bundled engine font id or a licensed package font asset. A creator's system-font path is not portable and must never appear in a public package. Export detects missing or nonredistributable fonts and offers replacement or a baked raster output. Retain font licenses and reserved-name constraints where applicable. [S53](../research/source-register.md#s53)

Changing a font changes glyph matching, not just presentation. Reusing a grid with a different font is an explicit stylistic operation; recomputation is necessary when the user asks for conversion-equivalent output.
