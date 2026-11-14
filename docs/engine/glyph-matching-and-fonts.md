# Glyph matching, fonts, and character sets

## Glyph table

Build an immutable glyph table from a font fingerprint, raster size, cell dimensions, baseline, glyph repertoire, and rendering mode. Rasterize masks with the chosen font library, then store coverage, bounding box, centroid, edge-orientation features, and the mask itself. Rasterization support is not equivalent to text shaping support. [S26](../research/source-register.md#s26)

### Executable font-table contract

`ascii-convert::build_glyph_table(font_bytes, charset, config, budget, cancellation)` builds one row-major, 8-bit coverage mask per requested Unicode scalar. It maps scalars through the font's character map without shaping; duplicate/control scalars and missing glyphs are typed errors. `GlyphConfig` requires finite positive raster size, nonzero cell dimensions, and a baseline no lower than the cell's bottom edge.

Each glyph is rasterized with pinned `fontdue 0.9.3` (default features disabled; `hashbrown` enabled, SIMD disabled), centered by advance width and positioned at the configured baseline, then clipped to the cell. Space remains a real measured blank mask. Bounds, coverage-weighted centroid, normalized mean coverage, and four gradient-orientation energy bins come from the resulting mask. The immutable table sorts by measured coverage, then font glyph index, then Unicode scalar.

The SHA-256 table fingerprint includes a descriptor/rasterizer mode version, font bytes, raster size's IEEE-754 bits, cell width/height, baseline, and the canonical sorted scalar repertoire. Reordering the same repertoire does not change the fingerprint. Exact dependency versions and enabled features are recorded in `crates/ascii-convert/Cargo.toml` and `Cargo.lock`.

Before raster allocation, the builder checks encoded font length, glyph count, cell-mask/table bytes, rasterizer scratch, total work, and cancellation. It returns a complete immutable table or a typed error; it never publishes partial glyphs. The current renderer is a single-scalar mask rasterizer, not a shaping engine, and it does not infer support for grapheme clusters or variable-width text.

Work accounting separately includes parser preflight and rasterization. The preflight follows the pinned parser's outline expansion and curve subdivision, including composite components, and records a checked edge-traversal estimate per glyph at the requested raster size. For each retained line, it bounds the scalar rasterizer's vertical pixel steps, or horizontal plus vertical steps for a diagonal, with allowances for coordinate rounding and the final pixel update. Each requested scalar is charged, including multiple scalars mapping to the same glyph; bitmap-area and cell work are charged separately. Estimates that overflow, or total work above the supplied limit, produce typed errors before rasterization. These are conservative work units for the modeled loops, not a wall-clock guarantee.

Version one supports printable ASCII and curated single-cell Unicode sets such as blocks and braille. Reject control characters, combining sequences, bidirectional controls, and unsupported variable-width/emoji glyphs from strict single-cell packages. Full shaped text, grapheme clusters, CJK width rules, and color emoji require a separate design rather than silently breaking the grid.

## Density matching

For mask `m_g(p)` in [0,1], coverage is its mean over the cell. Compare the target's normalized luminance against the reconstructed cell luminance, taking foreground and background polarity into account. A hard-coded string such as ` .:-=+*#%@` is only a starter UI preset; actual glyph darkness depends on font and size.

Coverage ties are resolved deterministically. Space is a valid glyph. Sparse masks can become visually invisible at small sizes, so the repertoire editor should show actual rasterized previews rather than only Unicode labels.

## Structure-aware matching

For target patch `P`, glyph mask `M_g`, foreground `F`, and background `B`, reconstruct `R_g(p)=B+(F-B)*M_g(p)`. A proposed score is:

```text
score(g) = w_pixel * mean_squared_error(P, R_g)
         + w_edge  * edge_descriptor_distance(P, M_g)
         + w_tone  * abs(mean(P) - mean(R_g))
         + temporal_penalty(g, previous_glyph)
```

First shortlist candidates by coverage and rough orientation; then evaluate the detailed score. Solve or choose foreground/background before comparing candidates consistently. Weights are normalized and benchmarked, not arbitrary magic quality constants. Per-channel least-squares color fitting is a later quality option, with clamping and degenerate-mask handling.

## Temporal stabilization

A prior glyph can be retained when the new candidate's improvement is smaller than a hysteresis threshold. Make the threshold sensitive to time and local motion so noise is reduced without freezing legitimate changes. Clear history on scene cuts, seek, source change, font/grid change, and decode discontinuity. A global high penalty may look stable while destroying moving detail; measure both stability and reconstruction quality.

## Font portability

A package refers to a bundled engine font id or a licensed package font asset. A creator's system-font path is not portable and must never appear in a public package. Export detects missing or nonredistributable fonts and offers replacement or a baked raster output. Retain font licenses and reserved-name constraints where applicable. [S53](../research/source-register.md#s53)

Changing a font changes glyph matching, not just presentation. Reusing a grid with a different font is an explicit stylistic operation; recomputation is necessary when the user asks for conversion-equivalent output.
