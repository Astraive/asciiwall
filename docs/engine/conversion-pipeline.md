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

Let the source aspect ratio be `W/H` and the measured character-cell aspect be `cw/ch`. For a chosen column count `C`, select `R = round-half-up(C * H/W * cw/ch)` rows using exact integer arithmetic; reject a zero-row result. For a fixed pixel surface, floor width and height by the measured cell dimensions; unused right/bottom pixels stay outside the output grid. A 1920x1080 source, 240 columns and 8x16 cells produce exactly 68 rows.

Measure actual glyph advance and line height; do not assume characters are square. Font changes can alter grid geometry and invalidate descriptor/cache data. Crop and fit transformations used by conversion must be the same transforms used for pointer mapping and previews.

## Analysis stages

Normalize orientation, alpha interpretation, color transfer, and range first. Box sampling integrates pixel-square overlap weights over each destination-cell footprint in linear light; transparent pixels and contain letterboxes composite against the configured background. Nearest sampling chooses the pixel containing the inverse-mapped sample center using floor boundaries, with exact source right/bottom edges assigned to the final pixel. Quality modes may retain bounded sub-cell luminance patches and gradient descriptors; those dimensions belong in the cache identity.

A fast mode uses calibrated glyph coverage. A quality mode shortlists masks by coverage and edge orientation, then minimizes bounded reconstruction error. A contour mode emphasizes structure and intentionally discards low-contrast texture; it must not be sold as universally more faithful.

## Parallelism

Independent cell descriptors and candidate evaluation can be split across rows/tiles using a bounded pool. Error-diffusion dithering introduces order dependencies; do not parallelize it as though each pixel/cell were independent. Ordered dithering is a simpler deterministic parallel alternative.

The density path checks cancellation every 32 output cells, every 64 glyph candidates, and every 256 sampling/patch operations. Results carry the caller's original generation, frame and timing identity so stale conversions can be discarded after settings change.

## Numerical behavior

Density matching clamps normalized coverage to `[0, 1]` for either foreground/background polarity and resolves equal-distance glyphs by font glyph index, Unicode scalar, then stable table order. Reject empty/overflowing geometry and indistinguishable foreground/background luminance; check cancellation at bounded work intervals. The initial implementation can use floating-point computation with tolerance-based GPU comparison; do not promise bit-identical output across all hardware.

## Required tests

A uniform patch produces the expected coverage order for both light-on-dark and dark-on-light styles. A circle stays circular under cell-aspect correction. Thin horizontal/vertical/diagonal lines favor appropriate masks in structural mode. Transparent edges do not produce dark halos. Hard scene cuts reset temporal history. Changing only a post-render tint must not trigger image decoding.

Structure-aware ASCII has published prior art; these stages are the project's proposed practical algorithm, not a claim to reproduce a particular paper exactly. [S39](../research/source-register.md#s39)
