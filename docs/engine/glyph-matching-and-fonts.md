# Glyph matching, fonts, and character sets

## Glyph table

Build an immutable glyph table from a font fingerprint, raster size, cell dimensions, baseline, glyph repertoire, and rendering mode. Rasterize masks with the chosen font library, then store coverage, bounding box, centroid, edge-orientation features, and the mask itself. Rasterization support is not equivalent to text shaping support. [S26](../research/source-register.md#s26)

### Executable font-table contract

`GlyphRepertoire::from_preset(GlyphRepertoirePreset::{PrintableAscii, BlockElements, BraillePatterns, AllSupported})` creates the named printable ASCII (U+0020–U+007E, including space), Unicode blocks (U+2580–U+259F), Braille (U+2800–U+28FF), or combined set. `GlyphRepertoire::new(&str)` accepts validated subsets, sorts them canonically, and reports deterministic duplicate/unsupported failures as `Error::Glyph { character, reason }`. The public `is_supported_single_cell_scalar` predicate is the same scalar contract intended for future consumers; `build_glyph_table` accepts only `&GlyphRepertoire`, so raw strings cannot bypass validation.

Font lookup is direct and substitution is disabled. A missing cmap entry returns a scalar-specific `GlyphUnavailable` error; a glyph whose measured horizontal advance exceeds the configured fixed `cell_width` returns `MultiCellMetrics`. `GlyphMetrics::cell_classification(cell_width)` exposes the measured single-cell/multi-cell classification for retained records.

`build_glyph_preview` rasterizes one supported scalar through the exact glyph-table mask path and returns row-major 8-bit coverage plus its mean coverage. Its status is `Available`, `Unavailable`, `Invisible`, or `MultiCell`; space remains `Available` with its actual blank mask, and missing/multi-cell glyphs have no mask. No tofu/fallback mask is synthesized.

The current executable workspace contains only `crates/ascii-core/`, `crates/ascii-convert/`, and `crates/ascii-media/`. There is no application/editor tree (`apps/`), `crates/ascii-clip/` reader/validator, or `crates/ascii-export/` text exporter; `docs/ui/` is design documentation, not an editor implementation. Consequently, the predicate and preview are library contracts only: clip/text-export reuse and editor integration remain unmet FNT-002 acceptance and this change must stay open/draft.

Each glyph is rasterized with pinned `fontdue 0.9.3` (default features disabled; `hashbrown` enabled, SIMD disabled), centered by advance width and positioned at the configured baseline, then clipped to the cell. Space remains a real measured blank mask. Bounds, coverage-weighted centroid, normalized mean coverage, and four gradient-orientation energy bins come from the resulting mask. The immutable table sorts by measured coverage, then font glyph index, then Unicode scalar.

The SHA-256 table fingerprint includes a descriptor/rasterizer mode version, font bytes, raster size's IEEE-754 bits, cell width/height, baseline, and the canonical sorted scalar repertoire. Reordering the same repertoire does not change the fingerprint. Exact dependency versions and enabled features are recorded in `crates/ascii-convert/Cargo.toml` and `Cargo.lock`.

Before raster-mask allocation, the builder checks encoded input length, glyph count, cell-mask/table bytes, rasterizer scratch, checked work, and cancellation. It returns only a complete table or a typed error; no partial glyphs are published. `GlyphRecord` and `GlyphTable` are intentionally non-`Clone`, so `Clone` cannot implicitly duplicate masks outside the construction budget. Cancellation is polled every 256 cmap/outline work units and mask pixels, plus each raster row. `ttf-parser`/`fontdue` parse and rasterize calls and final sorting are not internally interruptible; checks around them ensure cancellation observed before return yields `Error::Cancelled` rather than a table.

Work accounting separately includes parser preflight and rasterization. The preflight follows the pinned parser's outline expansion and curve subdivision, including composite components, and records a checked edge-traversal estimate per glyph at the requested raster size. For each retained line, it bounds the scalar rasterizer's vertical pixel steps, or horizontal plus vertical steps for a diagonal, with allowances for coordinate rounding and the final pixel update. Each requested scalar is charged, including multiple scalars mapping to the same glyph; bitmap-area and cell work are charged separately. Valid legacy cmap format-2 subtables are validated and budgeted even beside Unicode maps; malformed subtables still fail with typed errors. Estimates that overflow, or total work above the supplied limit, produce typed errors before rasterization. These are conservative work units for the modeled loops, not a wall-clock guarantee.

The raster-byte budget is a checked logical-allocation envelope, not a process-RSS or allocator-metadata limit. Encoded input is charged once alongside masks, table records, rasterizer scratch, and parser/preflight storage. For pinned `fontdue 0.9.3`, the preflight conservatively models 128 bytes per glyph record, 32 bytes per hash-map entry (including capacity slack), 128 bytes per retained outline line, and up to three name-capacity bytes per encoded font byte; its glyph/edge vectors and curve stack are also charged. Checked multipliers, per-input caps, and the total budget reject estimates before `fontdue` parsing/rasterization allocations. The allocation envelope follows the pinned parser/rasterizer layouts and must be revisited when those dependency implementations change; physical allocator overhead is outside the modeled limit.

Version one enforces that repertoire at construction: C0/C1 controls, bidi formatting, combining marks and variation selectors, emoji, CJK, and all other unlisted scalars are rejected before font lookup. Full shaped text, grapheme clusters, CJK width rules, and color emoji require a separate design rather than silently breaking the grid.

### FNT-002 verification evidence

- Focused command: `cargo test --locked -p ascii-convert --test repertoire` — 9 tests passed, including all 256 Braille dot patterns at 8×12 and 10×15, complete preset font availability, block alignment/coverage, strict rejections, preview-mask identity, and missing/invisible states.
- Workspace command: `cargo test --locked --workspace --all-targets` — 156 tests passed across 11 suites in `ascii-core`, `ascii-convert`, and `ascii-media`.
- Quality commands: `cargo fmt --all -- --check` and `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed.
- Executable smoke: temporary offline path-dependent binary ran `cargo run --locked --offline` and asserted actual `A` preview-mask equality, blank space preservation, and `C` unavailable status. Output: `preview smoke passed: A=Available coverage=Some(0.21764706), space=Available, C=Unavailable`.
- Fixture/config evidence (source-generated, rights-cleared, no downloaded assets): `crates/ascii-convert/tests/support/repertoire_font.rs` SHA-256 `c48600adeffe69204ec1352a4bbaa2df7609d3b22530408722d9c44df922a764`; `crates/ascii-convert/tests/repertoire.rs` (raster sizes, cells, baseline, budgets, assertions) SHA-256 `3476a20ee52113fd1efb2852eaa0dbeca4db951d30a4286cfb9bce02dac16ab8`.
- Environment: Windows 10 x64, `x86_64-pc-windows-msvc`, Cargo/rustc 1.100.0; `CARGO_HOME=D:/.worktrees/.cargo-cache`, `CARGO_TARGET_DIR=D:/.worktrees/.targets/fnt-002`.

## Density matching

For mask `m_g(p)` in [0,1], coverage is its mean over the cell. Compare the target's normalized luminance against the reconstructed cell luminance, taking foreground and background polarity into account. A hard-coded string such as ` .:-=+*#%@` is only a starter UI preset; actual glyph darkness depends on font and size.

Coverage ties are resolved deterministically. Space is a valid glyph. Sparse masks can become visually invisible at small sizes; `build_glyph_preview` exposes the measured mask and `Invisible` status, but no editor currently consumes that API.

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
