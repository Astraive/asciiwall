# Original Rust ASCII conversion pipeline

## End-to-end contract

```text
bounded decode -> orientation/color normalization -> fit/crop
  -> sampling grid -> cell descriptors -> candidate glyph matching
  -> foreground/background solution -> optional dithering
  -> temporal stabilization -> AsciiFrame -> renderer/exporter
```

Image codecs supply pixels; our code owns sampling, descriptors, matching, color selection, and stabilization. The converter is callable without a desktop, GPU, tray, network connection, or UI. The CPU implementation is the initial correctness reference for later acceleration.

## Grid geometry

For full-raster column sizing, let the source aspect ratio be `W/H` and the measured character-cell aspect be `cw/ch`; for a chosen column count `C`, select `R = round-half-up(C * H/W * cw/ch)` rows using exact integer arithmetic and reject a zero-row result. Custom-crop column sizing uses the selected crop's aspect instead of the full raster. For a fixed pixel surface, floor width and height by the measured cell dimensions; unused right/bottom pixels stay outside the output grid and transform. A 1920x1080 source, 240 columns and 8x16 cells produce exactly 68 rows.

Measure actual glyph advance and line height; do not assume characters are square. Font changes can alter grid geometry and invalidate descriptor/cache data. One published fit transform maps source and display pixel-edge coordinates for conversion, preview consumers, and pointer consumers; contain letterboxes and source points outside cover/custom-crop regions are explicitly rejected by the inverse mapping.

## Analysis stages

Normalize orientation, alpha interpretation, color transfer, and range first; apply the shared contain/cover/custom-crop transform next, before any artistic adjustment. Box sampling is the explainable baseline and integrates normalized pixel-square overlap weights over each actual destination-cell or subcell footprint in linear light. To preserve narrow quality-patch subdivisions at large or fractional crop origins, keep subinterval starts and positive spans separate and measure pixel overlap relative to the crop's integer source-pixel origin. Normalize overlap independently per axis before multiplication so subnormal footprints do not require representable absolute 2D area. Fractional source-pixel edges contribute proportionally; transparent pixels and areas outside a contain content rectangle contribute the configured background, and crop right/bottom edges are exclusive. No sample reads outside the normalized source. Nearest sampling chooses the pixel containing the inverse-mapped sample center using floor boundaries, with exact source right/bottom edges assigned to the final pixel; CNV-003 proposes no alternate reconstruction filter.

Optional quality patches contain validated row-major per-cell linear-luminance samples (each dimension 1–64, at most 4096 values per cell) and mean-luminance/mean-absolute finite-difference descriptors. The stable sampling identity includes resolved source/display geometry, grid and cell dimensions, filter, and patch dimensions. The aggregate operation preflights normalized/input/patch byte bounds and total normalization, matching, sampling, patch-initialization/descriptor, and frame-validation work before allocating patch or output storage.

Density matching is the fast coverage baseline. Structural matching shortlists masks by coverage, normalized gradients, and edge orientation, then reranks by reconstruction. Contour matching reuses the same normalized quality patches, calibrated glyph masks, and bounded structural shortlist before contour-weighted reranking. It emphasizes oriented silhouette edges and intentionally discards low-contrast texture; it is a structure/aesthetic tradeoff, not a universally more faithful mode.

### Bounded structural shortlist (CNV-007)

`StructuralDescriptor::from_luminance` derives normalized patch features from retained linear-light samples and the conversion's foreground/background luminances. It uses the same polarity-normalized `[0, 1]` coverage as density matching and shares the immutable glyph-mask orientation-bin implementation. The native descriptor is computed without temporary allocation.

Before ranking, the shortlist deterministically resamples source samples to the glyph table's cell width and height with center-aligned nearest-neighbor mapping: `source_index = floor(((2 * target_index + 1) * source_extent) / (2 * target_extent))` for each axis. Candidate coverage, gradient, and orientation features are therefore measured in the same geometry as glyph masks, making equivalent geometry at different patch resolutions rank identically. The reusable `StructuralScratch` retains only a validated top-K candidate heap (K is 1–4096 and cannot exceed the repertoire); stable score ties use font glyph index, Unicode scalar, then table position.

Tone, shape-gradient, and edge-orientation distances and each configured weight are in `[0, 1]`. The three nonnegative weights are validated to sum to one, so the combined shortlist score stays in `[0, 1]`. `StructuralIdentity` binds the descriptor version, sampling identity, source patch dimensions and native/common feature values, glyph-table mask fingerprint, foreground/background range, candidate limit, and normalized weights. Scratch bytes plus common feature extraction and scoring/heap work are checked against the caller's `Budget`; cancellation is polled in bounded work tiles and an error returns no partial shortlist.

The deterministic matcher fixtures build their small font masks from source-defined synthetic outlines; no downloaded font or runtime network access is involved.

### Contour mode (CNV-009)

`convert_contour` takes validated `ContourOptions`: a 1–4096 candidate cap no larger than the glyph repertoire; normalized `contrast_threshold` and `edge_threshold`; and tone, oriented-contour, and shape weights in `[0,1]` summing to one. Samples are normalized against the configured foreground/background luminance. Samples below the contrast threshold are treated as background, and gradients weaker than the edge threshold do not contribute to the contour descriptor. The thresholded contour orientation and calibrated coverage are compared against the top-K glyph shortlist; stable ties use score, font glyph index, and scalar.

The empty-background policy is explicit: a cell with no surviving thresholded edge emits the least-coverage glyph (then lowest glyph index), including uniform light, uniform dark, and suppressed faint-texture cells. This preserves flat-field backgrounds instead of replacing them with arbitrary repertoire characters. Output colors remain the caller's configured fixed foreground/background pair; palette fitting is not part of this mode.

`ContourIdentity` includes algorithm version, resolved sampling identity (fit/crop/grid/filter/patch), glyph-mask fingerprint, every threshold and weight, shortlist limit, foreground/background luminance and RGB, output dimensions, patch samples, and structural patch identities. Sampling work is given only the budget left after aggregate shortlist and reranking work is reserved. Scratch, output, cancellation and patch storage stay bounded by the supplied budget. Contour matching costs more than density because it samples retained per-cell patches, ranks the structural shortlist, and evaluates oriented edges; lower tone fidelity is expected where configured suppression removes texture.

For a matched comparison, use one synthetic source, font fingerprint, grid/cell geometry, crop/filter, fixed color pair and candidate budget in each mode. Report oriented-edge/silhouette agreement, raw tone reconstruction loss before and after thresholding, and measured runtime or checked work units separately; do not collapse these into a universal quality ranking. CNV-009's source-generated tests use the same synthetic mask, raster, grid and fixed color pair without external images or network access.

## Parallelism

Conversion matching uses a bounded shared worker admission owner and stable contiguous four-row tiles. Workers reuse their color-sample and structural scratch buffers, share immutable glyph descriptors, and write disjoint row ranges; stable per-cell candidate tie-breaking and serial error-diffusion dependencies preserve reproducibility. No work queue is created. When conversions share a `ConversionResources` owner, aggregate reservations include the whole preflighted input/output, glyph table, normalized raster, patch, scratch, and worker footprint across active previews.

Every work range carries the frame generation. Workers poll cancellation and generation freshness at tile boundaries, and stale jobs release their RAII reservations rather than completing a publishable frame. A caller publishes completed preview/runtime state only through `GenerationController::publish_if_current`, which serializes publication against generation advancement. Image decoding and post-render tint are outside this conversion API; callers keep tint-only changes from re-enqueuing decode or matching.

The converter polls cancellation during sampling, descriptor creation, and matching as well as at tile boundaries. A cancelled or failed job publishes no partial frame and releases its worker/memory reservations. Error-diffusion remains dependency-aware and is not scheduled in the independent-cell pool.

## Numerical behavior

Density matching clamps normalized coverage to `[0, 1]` for either foreground/background polarity and resolves equal-distance glyphs by font glyph index, Unicode scalar, then stable table order. Reject empty/overflowing geometry and foreground/background colors that are indistinguishable by linear luminance or output sRGB8 encoding; check cancellation at bounded work intervals. Transform-edge checks account for local f64 coordinate precision while capping tolerance to a conservative fraction of the mapped extent, so narrow representable crops at large origins remain valid without accepting collapsed geometry. The initial implementation can use floating-point computation with tolerance-based GPU comparison; do not promise bit-identical output across all hardware.

For deterministic area-sampling fixtures, the arithmetic mean of retained subcell luminance agrees with the directly integrated box coverage within `1e-6` absolute linear-luminance error; this is a test tolerance, not a cross-device bit-identity claim.

## Required tests

A uniform patch produces the expected coverage order for both light-on-dark and dark-on-light styles. A circle stays circular under cell-aspect correction. Thin horizontal/vertical/diagonal lines favor appropriate masks in structural mode. Transparent edges do not produce dark halos. Hard scene cuts reset temporal history. Changing only a post-render tint must not trigger image decoding.

Structure-aware ASCII has published prior art; these stages are the project's proposed practical algorithm, not a claim to reproduce a particular paper exactly. [S39](../research/source-register.md#s39)
