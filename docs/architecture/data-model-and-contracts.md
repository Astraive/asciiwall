# Data model and engine contracts

## Core types

The following is conceptual Rust, not a ready-to-compile public API.

```rust
pub struct AsciiCell {
    pub glyph: u32,
    pub fg_srgba8: [u8; 4],
    pub bg_srgba8: [u8; 4],
}

pub struct AsciiFrame {
    pub columns: u32,
    pub rows: u32,
    pub glyph_table_id: GlyphTableId,
    pub frame_id: u64,
    pub generation: u64,
    pub pts_us: i64,
    pub duration_us: u64,
    pub cells: Vec<AsciiCell>,
}

pub struct RasterFrameView<'a> {
    pub width: u32,
    pub height: u32,
    pub row_stride: usize,
    pub format: PixelFormat,
    pub color: ColorDescription,
    pub pixels: &'a [u8],
}
```

A Rust struct is not a wire format. Explicit encoders define byte order, padding, field sizes, bounds, and versioning. Do not serialize native memory layouts, pointers, `usize`, or compiler-dependent enum representations.

## Invariants

`columns * rows` uses checked arithmetic and must equal the cell count. Every glyph index must exist in the referenced immutable glyph table. A glyph table contains normalized, supported display symbols and a font fingerprint; it is not a set of arbitrary terminal escape sequences. Colors identify encoded sRGB plus alpha; the renderer converts to linear-light values for blending as required.

A raster view validates stride, dimensions, sample depth, orientation, transfer characteristics, and available bytes. Stride is not assumed to equal width times channels. A frame keeps owned/shared backing storage alive while any consumer uses the view.

## Identity and versioning

`PackageId` identifies a published artwork namespace; `package_version` identifies content release; `content_digest` identifies exact bytes. A local fork gets a new identity or explicit fork lineage, never silently overwrites an upstream immutable version.

`AssignmentId` binds an installed version and override set to monitor selectors. `MonitorId` is a local best-effort stable identifier with a fallback match record, not a portable hardware serial to embed in exported packages. `Generation` invalidates results after a settings change, seek, source replacement, or topology reset.

## Frame and event contracts

Media presentation timestamps and monotonic runtime clocks are distinct. Wall-clock time is an optional scene signal, not a pacing clock. `InputSnapshot` contains only permitted pointer/button information and no text entry. `WindowSnapshot` contains ephemeral geometry/state with quality and freshness markers, no mandatory title or screenshot.

Every engine operation receives a cancellation token and resource budget. Errors distinguish malformed data, unsupported features, denied permissions, missing assets, exhausted budgets, and temporarily unavailable devices. Do not collapse these into a generic black wallpaper.
