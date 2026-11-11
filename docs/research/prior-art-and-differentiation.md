# Prior art and differentiation

## Relevant precedents

**Lively** demonstrates that a user-facing live wallpaper application needs lifecycle, pausing, distribution, and desktop integration in addition to media playback. Its shell compatibility issues are valuable engineering evidence. Treat its implementation and license as separate from our original implementation; do not copy code under an incompatible licensing assumption. [S40](source-register.md#s40) [S03](source-register.md#s03)

**Chafa** demonstrates strong existing image-to-symbol rendering, selectable symbol sets, palettes, and terminal output. It is an external quality/performance reference, not a runtime dependency for AsciiWall's conversion engine. Comparative tests must align cell counts, allowed glyphs, colors, and fonts before drawing conclusions. [S38](source-register.md#s38)

**Structure-based ASCII Art** by Xu, Zhang, and Wong is prior research on matching image structure with character shapes. The proposed bounded per-cell matcher in this pack is a practical independent design, not a claim to implement the paper's full optimization or to reproduce its results. [S39](source-register.md#s39)

## The product distinction

The proposal combines an editable character representation, procedural and converted scenes, trustworthy per-platform interaction behavior, an original Rust conversion library, portable declarative packages, and an account-free community catalog. These are intended design differences, not claims that no other project has ever combined similar features.

The core remains useful without the wallpaper app: a creator can batch-convert images, benchmark matching, validate packages, or pre-render a sequence using the CLI. That modularity also permits contributors to improve image quality without working on native window integration.

## Benchmark discipline

Compare a fixed grayscale ramp, a geometric edge chart, a color chart, a portrait with known redistribution rights, and a motion clip with a hard scene cut. For each tool, publish glyph repertoire, font, cells, input resizing, palette, temporal settings, build version, and hardware. Report quality preferences separately from objective reconstruction error and speed.

Do not advertise novelty from screenshots alone. Good conversion quality depends on glyph masks, fit, color, and viewing scale; a larger grid can make almost any approach look better. Do not claim lower power than a video wallpaper player without total-system measurements under matched content and display conditions.
