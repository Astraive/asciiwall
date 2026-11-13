# Data model and engine contracts

## Core types

`ascii-core` provides these portable models with private fields and read-only accessors. It is `no_std`, forbids unsafe code, and uses `alloc` only for the owned cells of `AsciiFrame`.
The signatures below are contract summaries rather than a standalone Rust module; constructors and accessor details are defined by `crates/ascii-core`.

```rust
pub struct GlyphTableId([u8; 32]);

pub struct AsciiCell {
    glyph: u32,
    fg_srgba8: [u8; 4],
    bg_srgba8: [u8; 4],
}

pub struct FrameMetadata {
    glyph_table_id: GlyphTableId,
    frame_id: u64,
    generation: u64,
    pts_us: i64,
    duration_us: u64,
}

pub struct AsciiFrame {
    size: GridSize,
    metadata: FrameMetadata,
    cells: alloc::vec::Vec<AsciiCell>,
}

pub struct RasterFrameView<'a> {
    width: u32,
    height: u32,
    row_stride: usize,
    format: PixelFormat,
    color: ColorDescription,
    orientation: RasterOrientation,
    pixels: &'a [u8],
}

pub fn validate_grid_budget(
    size: GridSize,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<GridBudget, Error>;

pub fn validate_frame(
    frame: &AsciiFrame,
    expected_glyph_table_id: GlyphTableId,
    glyph_count: u32,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<(), Error>;

pub fn validate_raster_frame<'a>(
    view: RasterFrameView<'a>,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<ValidatedRasterFrameView<'a>, Error>;
```

`AsciiFrame::try_new` takes `FrameMetadata` and an already-owned cell vector. It validates shape,
glyph bounds, actual vector capacity, cancellation, and explicit byte/work budgets without
allocating. Consumers use `validate_frame` to compare the declared table identity with the expected
table. Producers call the allocation-free `validate_grid_budget` before allocation; it checks the
minimum storage and Rust's `isize::MAX` allocation-layout bound. `into_parts` returns the original
cell vector for reuse without copying. Grid work is one unit per cell; raster work is one unit per
pixel. Budgets are explicit; there is no implicit unlimited default.

A frame's `pts_us` is signed media presentation time and `duration_us` is media duration. `AsciiFrame` contains no monotonic runtime clock; pacing time belongs to the runtime contract and must not be encoded as media PTS.

A Rust struct is not a wire format. Explicit encoders define byte order, padding, field sizes, bounds, and versioning. Do not serialize native memory layouts, pointers, `usize`, or compiler-dependent enum representations.

## Invariants

The exact `u32 × u32` cell count fits in `u64`; do not treat that product as overflow-prone. Convert it to platform `usize`, compute cell bytes with checked arithmetic, and reject storage beyond `isize::MAX` before allocation. Every frame has a nonzero grid, exactly one row-major cell per grid position, the expected fixed-width glyph-table identity, and glyph indices below the supplied table count. Colors identify encoded sRGBA8; the renderer converts to linear-light values for blending as required.

The raster validator accepts top-down 8-bit Gray, RGB, or RGBA with sRGB primaries and the sRGB transfer function. It rejects unsupported sample depth, channel layouts, orientation, color/transfer metadata, and alpha semantics instead of silently transforming them. It checks `row_stride >= row_bytes` and the exact readable extent `(height - 1) * row_stride + row_bytes` with checked arithmetic. Padding after the last row is not required; trailing backing bytes are allowed. The validated view borrows the original byte slice, exposes only its required readable extent when requested, and cannot outlive that backing storage. No pixel copy or I/O occurs.

## Identity and versioning

`PackageId` identifies a published artwork namespace; `package_version` identifies content release; `content_digest` identifies exact bytes. A local fork gets a new identity or explicit fork lineage, never silently overwrites an upstream immutable version.

`AssignmentId` binds an installed version and override set to monitor selectors. `MonitorId` is a local best-effort stable identifier with a fallback match record, not a portable hardware serial to embed in exported packages. `Generation` invalidates results after a settings change, seek, source replacement, or topology reset.

## Frame and event contracts

Media presentation timestamps and monotonic runtime clocks are distinct. Wall-clock time is an optional scene signal, not a pacing clock. `InputSnapshot` contains only permitted pointer/button information and no text entry. `WindowSnapshot` contains ephemeral geometry/state with quality and freshness markers, no mandatory title or screenshot.

Every engine operation receives a cancellation token and resource budget. Errors distinguish malformed data, unsupported features, denied permissions, missing assets, exhausted budgets, and temporarily unavailable devices. Do not collapse these into a generic black wallpaper.
