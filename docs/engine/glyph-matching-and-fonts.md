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

`convert_structural` runs a bounded shortlist and then scores each shortlisted mask using normalized linear-light luminance reconstruction `R = B + (F - B) M`, descriptor distance, mean-coverage error, and an optional temporal glyph-change penalty. The finite weights are supplied with `ConversionSettings::with_reconstruction_weights`; their sum is one, the default is `(0.70, 0.20, 0.05, 0.05)`, and zero temporal weight makes the result independent of `previous`. The default candidate cap is 64. Equal scores resolve by font glyph index and then table position. Sampling patches are at measured glyph-cell dimensions, and patch, shortlist, rerank, output, and cancellation work are bounded by the call's budget.

The initial `convert_structural` contract supports the fixed configured foreground/background reconstruction baseline. It does not yet integrate CNV-005 candidate-specific palette/color fitting into this objective; `with_color_fit` settings are therefore rejected instead of silently comparing candidates under a different palette-fit objective. This draft must not be treated as complete CNV-008 acceptance until that integration is implemented.

The synthetic vertical-edge regression uses two same-density masks. It checks that explicit per-pixel reconstruction picks the vertical mask for a vertical edge, repeats deterministically, and ignores a previous frame when temporal weight is zero. This demonstrates a shape-sensitive fixture only; it is not a reproduction of the cited research paper and does not establish universally better fidelity or a performance improvement over density mode.

## Temporal stabilization

A prior glyph can be retained when the new candidate's improvement is smaller than a hysteresis threshold. Make the threshold sensitive to time and local motion so noise is reduced without freezing legitimate changes. Clear history on scene cuts, seek, source change, font/grid change, and decode discontinuity. A global high penalty may look stable while destroying moving detail; measure both stability and reconstruction quality.

## Font portability

A package refers to a bundled engine font id or a licensed package font asset. A creator's system-font path is not portable and must never appear in a public package. Export detects missing or nonredistributable fonts and offers replacement or a baked raster output. Retain font licenses and reserved-name constraints where applicable. [S53](../research/source-register.md#s53)

Changing a font changes glyph matching, not just presentation. Reusing a grid with a different font is an explicit stylistic operation; recomputation is necessary when the user asks for conversion-equivalent output.
