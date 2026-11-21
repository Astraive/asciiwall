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

A fast mode uses calibrated glyph coverage. A quality mode shortlists masks by coverage and edge orientation, then minimizes bounded reconstruction error. A contour mode emphasizes structure and intentionally discards low-contrast texture; it must not be sold as universally more faithful.

## Parallelism

Independent cell descriptors and candidate evaluation can be split across rows/tiles using a bounded pool. Error-diffusion dithering introduces order dependencies; do not parallelize it as though each pixel/cell were independent. Ordered dithering is a simpler deterministic parallel alternative.

The density path checks cancellation every 32 output cells, every 64 glyph candidates, and every 256 sampling, patch-initialization, or descriptor operations. It polls again immediately before output-cell allocation, and its aggregate work ceiling includes final frame-cell validation. A cancelled operation returns no partial frame or patch result. Results carry the caller's original generation, frame and timing identity so stale conversions can be discarded after settings change.

## Numerical behavior

Density matching clamps normalized coverage to `[0, 1]` for either foreground/background polarity and resolves equal-distance glyphs by font glyph index, Unicode scalar, then stable table order. Reject empty/overflowing geometry and foreground/background colors that are indistinguishable by linear luminance or output sRGB8 encoding; check cancellation at bounded work intervals. Transform-edge checks account for local f64 coordinate precision while capping tolerance to a conservative fraction of the mapped extent, so narrow representable crops at large origins remain valid without accepting collapsed geometry. The initial implementation can use floating-point computation with tolerance-based GPU comparison; do not promise bit-identical output across all hardware.

For deterministic area-sampling fixtures, the arithmetic mean of retained subcell luminance agrees with the directly integrated box coverage within `1e-6` absolute linear-luminance error; this is a test tolerance, not a cross-device bit-identity claim.

## Required tests

A uniform patch produces the expected coverage order for both light-on-dark and dark-on-light styles. A circle stays circular under cell-aspect correction. Thin horizontal/vertical/diagonal lines favor appropriate masks in structural mode. Transparent edges do not produce dark halos. Hard scene cuts reset temporal history. Changing only a post-render tint must not trigger image decoding.

Structure-aware ASCII has published prior art; these stages are the project's proposed practical algorithm, not a claim to reproduce a particular paper exactly. [S39](../research/source-register.md#s39)
