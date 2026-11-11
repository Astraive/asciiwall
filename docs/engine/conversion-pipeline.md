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

Let the fitted source aspect ratio be `W/H` and the actual character cell aspect be `cw/ch`. For a chosen column count `C`, select approximately `R = round(C * H/W * cw/ch)` rows, then apply the documented fit rule. For a 1920x1080 source, 240 columns and 8x16 cells produce about 68 rows. For a fixed target surface, derive both dimensions from the fitted cell size and explicitly handle leftover pixels.

Measure actual glyph advance and line height; do not assume characters are square. Font changes can alter grid geometry and invalidate descriptor/cache data. Crop and fit transformations used by conversion must be the same transforms used for pointer mapping and previews.

## Analysis stages

Normalize orientation, alpha interpretation, color transfer, and range. Filter source samples over the destination cell area, rather than sampling one pixel and amplifying aliasing. For quality modes, retain a small sub-cell patch, for example 8x16 luminance samples, plus gradient descriptors. Patch dimensions are algorithm parameters and part of the cache key.

A fast mode uses calibrated glyph coverage. A quality mode shortlists masks by coverage and edge orientation, then minimizes bounded reconstruction error. A contour mode emphasizes structure and intentionally discards low-contrast texture; it must not be sold as universally more faithful.

## Parallelism

Independent cell descriptors and candidate evaluation can be split across rows/tiles using a bounded pool. Error-diffusion dithering introduces order dependencies; do not parallelize it as though each pixel/cell were independent. Ordered dithering is a simpler deterministic parallel alternative.

Reuse scratch arrays, glyph descriptors, and output buffers. Check cancellation at tile boundaries. Results include generation ids so an expensive conversion started before a slider change cannot overwrite the latest preview.

## Numerical behavior

Define rounding, tie-breaking, NaN handling, and clipping. Use stable glyph-index tie-breaking for deterministic CPU output. Normalize feature weights to documented ranges. Reject nonfinite settings before they enter the pipeline. The initial implementation can use floating-point computation with tolerance-based GPU comparison; do not promise bit-identical output across all hardware.

## Required tests

A uniform patch produces the expected coverage order for both light-on-dark and dark-on-light styles. A circle stays circular under cell-aspect correction. Thin horizontal/vertical/diagonal lines favor appropriate masks in structural mode. Transparent edges do not produce dark halos. Hard scene cuts reset temporal history. Changing only a post-render tint must not trigger image decoding.

Structure-aware ASCII has published prior art; these stages are the project's proposed practical algorithm, not a claim to reproduce a particular paper exactly. [S39](../research/source-register.md#s39)
