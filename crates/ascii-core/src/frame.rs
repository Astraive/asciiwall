//! Owned ASCII frames and borrowed raster-frame validation.

use alloc::vec::Vec;

use crate::GridSize;
use crate::budget::{Budget, CancellationToken, validate_grid_budget};
use crate::error::{BudgetKind, Error, MalformedReason, UnsupportedReason};

/// Stable fixed-width identity for an immutable glyph table.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GlyphTableId([u8; 32]);

impl GlyphTableId {
    /// Creates a glyph-table identity from its fixed-width digest bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the identity bytes without exposing mutable storage.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// One immutable ASCII output cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsciiCell {
    glyph: u32,
    fg_srgba8: [u8; 4],
    bg_srgba8: [u8; 4],
}

impl AsciiCell {
    /// Creates a cell; its glyph bound is checked when the containing frame is validated.
    #[must_use]
    pub const fn new(glyph: u32, fg_srgba8: [u8; 4], bg_srgba8: [u8; 4]) -> Self {
        Self {
            glyph,
            fg_srgba8,
            bg_srgba8,
        }
    }

    /// Returns this cell's glyph index.
    #[must_use]
    pub const fn glyph(self) -> u32 {
        self.glyph
    }

    /// Returns the foreground color in encoded sRGBA8.
    #[must_use]
    pub const fn fg_srgba8(self) -> [u8; 4] {
        self.fg_srgba8
    }

    /// Returns the background color in encoded sRGBA8.
    #[must_use]
    pub const fn bg_srgba8(self) -> [u8; 4] {
        self.bg_srgba8
    }
}

/// Immutable identity and media timing attached to one ASCII frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameMetadata {
    glyph_table_id: GlyphTableId,
    frame_id: u64,
    generation: u64,
    pts_us: i64,
    duration_us: u64,
}

impl FrameMetadata {
    /// Creates metadata with its glyph-table identity and media timing.
    #[must_use]
    pub const fn new(
        glyph_table_id: GlyphTableId,
        frame_id: u64,
        generation: u64,
        pts_us: i64,
        duration_us: u64,
    ) -> Self {
        Self {
            glyph_table_id,
            frame_id,
            generation,
            pts_us,
            duration_us,
        }
    }

    /// Returns the immutable glyph-table identity bound to this frame.
    #[must_use]
    pub const fn glyph_table_id(self) -> GlyphTableId {
        self.glyph_table_id
    }

    /// Returns the stable frame identifier.
    #[must_use]
    pub const fn frame_id(self) -> u64 {
        self.frame_id
    }

    /// Returns the source generation for stale-result rejection.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }

    /// Returns the signed media presentation timestamp in microseconds.
    #[must_use]
    pub const fn pts_us(self) -> i64 {
        self.pts_us
    }

    /// Returns the media-frame duration in microseconds.
    #[must_use]
    pub const fn duration_us(self) -> u64 {
        self.duration_us
    }
}

/// An owned, immutable ASCII frame after validation against its declared glyph table.
#[derive(Eq, PartialEq)]
pub struct AsciiFrame {
    size: GridSize,
    metadata: FrameMetadata,
    cells: Vec<AsciiCell>,
}

impl core::fmt::Debug for AsciiFrame {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AsciiFrame")
            .field("size", &self.size)
            .field("metadata", &self.metadata)
            .field("cell_count", &self.cells.len())
            .finish()
    }
}

impl AsciiFrame {
    /// Creates a frame only after validating its shape, glyph bounds, cancellation state,
    /// and explicit byte/work budget.
    ///
    /// The glyph-table identity comes from `metadata`. Consumers that require a particular
    /// table can call [`validate_frame`] with that expected identity.
    /// This function does not allocate. Call [`validate_grid_budget`] before allocating the
    /// supplied cell vector when constructing a frame from dimensions.
    pub fn try_new(
        size: GridSize,
        metadata: FrameMetadata,
        cells: Vec<AsciiCell>,
        glyph_count: u32,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<Self, Error> {
        let frame = Self {
            size,
            metadata,
            cells,
        };
        validate_frame(
            &frame,
            metadata.glyph_table_id(),
            glyph_count,
            budget,
            cancellation,
        )?;
        Ok(frame)
    }

    /// Returns the dimensions of this frame.
    #[must_use]
    pub const fn size(&self) -> GridSize {
        self.size
    }

    /// Returns the immutable metadata attached to this frame.
    #[must_use]
    pub const fn metadata(&self) -> FrameMetadata {
        self.metadata
    }

    /// Returns the immutable glyph-table identity bound to this frame.
    #[must_use]
    pub const fn glyph_table_id(&self) -> GlyphTableId {
        self.metadata.glyph_table_id()
    }

    /// Returns the stable frame identifier.
    #[must_use]
    pub const fn frame_id(&self) -> u64 {
        self.metadata.frame_id()
    }

    /// Returns the source generation for stale-result rejection.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.metadata.generation()
    }

    /// Returns the signed media presentation timestamp in microseconds.
    #[must_use]
    pub const fn pts_us(&self) -> i64 {
        self.metadata.pts_us()
    }

    /// Returns the media-frame duration in microseconds.
    #[must_use]
    pub const fn duration_us(&self) -> u64 {
        self.metadata.duration_us()
    }

    /// Returns the immutable cell slice in row-major order: `row * columns + column`.
    #[must_use]
    pub fn cells(&self) -> &[AsciiCell] {
        &self.cells
    }

    /// Consumes the frame and returns its owned parts so callers can reuse the cell allocation.
    #[must_use]
    pub fn into_parts(self) -> (GridSize, FrameMetadata, Vec<AsciiCell>) {
        (self.size, self.metadata, self.cells)
    }
}

/// Validates a frame's exact dimensions, glyph-table binding, glyph bounds, capacity, and budgets.
pub fn validate_frame(
    frame: &AsciiFrame,
    expected_glyph_table_id: GlyphTableId,
    glyph_count: u32,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<(), Error> {
    let grid_budget = validate_grid_budget(frame.size, budget, cancellation)?;

    if frame.metadata.glyph_table_id() != expected_glyph_table_id {
        return Err(Error::Malformed(MalformedReason::GlyphTableMismatch));
    }
    if frame.cells.len() != grid_budget.cell_count_usize() {
        return Err(Error::Malformed(MalformedReason::CellCountMismatch));
    }
    let cell_capacity = u64::try_from(frame.cells.capacity()).map_err(|_| Error::SizeOverflow)?;
    let cell_size =
        u64::try_from(core::mem::size_of::<AsciiCell>()).map_err(|_| Error::SizeOverflow)?;
    let allocated_frame_bytes = cell_capacity
        .checked_mul(cell_size)
        .ok_or(Error::SizeOverflow)?;
    if allocated_frame_bytes > budget.max_frame_bytes() {
        return Err(Error::BudgetExceeded(BudgetKind::FrameBytes));
    }

    // Poll at bounded intervals so long glyph scans can respond to concurrent cancellation
    // without paying for an atomic read on every cell.
    const CANCELLATION_POLL_INTERVAL: usize = 1024;
    for (index, cell) in frame.cells.iter().enumerate() {
        if index % CANCELLATION_POLL_INTERVAL == 0 && cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if cell.glyph >= glyph_count {
            return Err(Error::Malformed(MalformedReason::GlyphIndexOutOfRange));
        }
    }
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }

    Ok(())
}

/// Channel layouts represented by a raster input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelChannels {
    /// One grayscale sample per pixel.
    Gray,
    /// Red, green, and blue samples per pixel.
    Rgb,
    /// Red, green, blue, and alpha samples per pixel.
    Rgba,
    /// Any other channel layout, retained so it can be rejected explicitly.
    Other(u8),
}

/// Pixel component layout and sample depth for a raster input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelFormat {
    channels: PixelChannels,
    sample_depth_bits: u8,
}

impl PixelFormat {
    /// Describes a channel layout and bits per sample.
    #[must_use]
    pub const fn new(channels: PixelChannels, sample_depth_bits: u8) -> Self {
        Self {
            channels,
            sample_depth_bits,
        }
    }

    /// Describes 8-bit grayscale pixels.
    #[must_use]
    pub const fn gray8() -> Self {
        Self::new(PixelChannels::Gray, 8)
    }

    /// Describes 8-bit RGB pixels.
    #[must_use]
    pub const fn rgb8() -> Self {
        Self::new(PixelChannels::Rgb, 8)
    }

    /// Describes 8-bit RGBA pixels.
    #[must_use]
    pub const fn rgba8() -> Self {
        Self::new(PixelChannels::Rgba, 8)
    }

    /// Returns the channel layout.
    #[must_use]
    pub const fn channels(self) -> PixelChannels {
        self.channels
    }

    /// Returns the number of bits per sample.
    #[must_use]
    pub const fn sample_depth_bits(self) -> u8 {
        self.sample_depth_bits
    }

    fn bytes_per_pixel(self) -> Result<usize, Error> {
        if self.sample_depth_bits != 8 {
            return Err(Error::Unsupported(UnsupportedReason::SampleDepth));
        }
        match self.channels {
            PixelChannels::Gray => Ok(1),
            PixelChannels::Rgb => Ok(3),
            PixelChannels::Rgba => Ok(4),
            PixelChannels::Other(_) => Err(Error::Unsupported(UnsupportedReason::PixelFormat)),
        }
    }
}

/// Known color-primary descriptions accepted or rejected by the portable raster contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorPrimaries {
    /// Standard sRGB/Rec. 709 primaries.
    Srgb,
    /// Display P3 primaries.
    DisplayP3,
    /// Rec. 2020 primaries.
    Rec2020,
    /// An unrecognized or implementation-specific primary set.
    Other(u8),
}

/// Transfer function attached to raster color samples.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferFunction {
    /// The sRGB transfer curve.
    Srgb,
    /// Linear-light samples.
    Linear,
    /// Perceptual Quantizer transfer curve.
    Pq,
    /// Hybrid Log-Gamma transfer curve.
    Hlg,
    /// An unrecognized or implementation-specific transfer curve.
    Other(u8),
}

/// Alpha interpretation for a raster's channel layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlphaMode {
    /// No alpha channel; pixels are treated as opaque.
    Opaque,
    /// Alpha is an independent, straight (unassociated) value.
    Straight,
    /// Color channels are premultiplied by alpha.
    Premultiplied,
    /// An unrecognized alpha interpretation.
    Other(u8),
}

/// Explicit color primaries, transfer, and alpha metadata for a raster.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColorDescription {
    primaries: ColorPrimaries,
    transfer: TransferFunction,
    alpha_mode: AlphaMode,
}

impl ColorDescription {
    /// Creates a color description without silently normalizing metadata.
    #[must_use]
    pub const fn new(
        primaries: ColorPrimaries,
        transfer: TransferFunction,
        alpha_mode: AlphaMode,
    ) -> Self {
        Self {
            primaries,
            transfer,
            alpha_mode,
        }
    }

    /// Creates the supported sRGB primaries/transfer description with explicit alpha semantics.
    #[must_use]
    pub const fn srgb(alpha_mode: AlphaMode) -> Self {
        Self::new(ColorPrimaries::Srgb, TransferFunction::Srgb, alpha_mode)
    }

    /// Returns the declared color primaries.
    #[must_use]
    pub const fn primaries(self) -> ColorPrimaries {
        self.primaries
    }

    /// Returns the declared transfer function.
    #[must_use]
    pub const fn transfer(self) -> TransferFunction {
        self.transfer
    }

    /// Returns the declared alpha interpretation.
    #[must_use]
    pub const fn alpha_mode(self) -> AlphaMode {
        self.alpha_mode
    }
}

/// Row orientation of a raster's first stored pixel row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RasterOrientation {
    /// Row zero is the top row and subsequent rows proceed downward.
    TopDown,
    /// Row zero is the bottom row.
    BottomUp,
    /// Any rotated, mirrored, or unrecognized orientation.
    Other(u8),
}

/// A zero-copy view over caller-owned raster bytes.
#[derive(Clone, Copy)]
pub struct RasterFrameView<'a> {
    width: u32,
    height: u32,
    row_stride: usize,
    format: PixelFormat,
    color: ColorDescription,
    orientation: RasterOrientation,
    pixels: &'a [u8],
}

impl core::fmt::Debug for RasterFrameView<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("RasterFrameView")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("row_stride", &self.row_stride)
            .field("format", &self.format)
            .field("color", &self.color)
            .field("orientation", &self.orientation)
            .field("pixel_byte_len", &self.pixels.len())
            .finish()
    }
}

impl<'a> RasterFrameView<'a> {
    /// Creates a borrowed view. Metadata and backing extent are checked by
    /// [`validate_raster_frame`].
    #[must_use]
    pub const fn new(
        width: u32,
        height: u32,
        row_stride: usize,
        format: PixelFormat,
        color: ColorDescription,
        orientation: RasterOrientation,
        pixels: &'a [u8],
    ) -> Self {
        Self {
            width,
            height,
            row_stride,
            format,
            color,
            orientation,
            pixels,
        }
    }

    /// Returns the raster width in pixels.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Returns the raster height in pixels.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }

    /// Returns the byte distance between starts of successive rows.
    #[must_use]
    pub const fn row_stride(self) -> usize {
        self.row_stride
    }

    /// Returns the pixel format.
    #[must_use]
    pub const fn format(self) -> PixelFormat {
        self.format
    }

    /// Returns the explicit color description.
    #[must_use]
    pub const fn color(self) -> ColorDescription {
        self.color
    }

    /// Returns the row orientation.
    #[must_use]
    pub const fn orientation(self) -> RasterOrientation {
        self.orientation
    }

    /// Returns the original borrowed pixel slice.
    #[must_use]
    pub const fn pixels(self) -> &'a [u8] {
        self.pixels
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ValidatedRasterFrameView<'a> {
    view: RasterFrameView<'a>,
    row_bytes: usize,
    required_extent: usize,
    pixel_count: u64,
}

impl<'a> ValidatedRasterFrameView<'a> {
    /// Returns the validated source view, retaining its backing-slice lifetime.
    #[must_use]
    pub const fn view(&self) -> RasterFrameView<'a> {
        self.view
    }

    /// Returns bytes occupied by pixels in one row, excluding any row padding.
    #[must_use]
    pub const fn row_bytes(&self) -> usize {
        self.row_bytes
    }

    /// Returns the exact minimum slice extent needed to read all pixels.
    #[must_use]
    pub const fn required_extent(&self) -> usize {
        self.required_extent
    }

    /// Returns the number of pixels represented by the view.
    #[must_use]
    pub const fn pixel_count(&self) -> u64 {
        self.pixel_count
    }

    /// Returns the original slice, which may contain trailing bytes beyond the image extent.
    #[must_use]
    pub const fn pixels(&self) -> &'a [u8] {
        self.view.pixels
    }

    /// Returns only the minimum readable extent, excluding trailing bytes.
    #[must_use]
    pub fn readable_pixels(&self) -> &'a [u8] {
        &self.view.pixels[..self.required_extent]
    }
}

/// Validates raster metadata, stride, accessible extent, budgets, and cancellation without copying.
///
/// This portable contract currently accepts top-down, 8-bit Gray/RGB/RGBA data with sRGB
/// primaries and transfer. The exact required extent is `(height - 1) * row_stride + row_bytes`;
/// unused padding after the last row is not required, and any trailing backing bytes are allowed.
pub fn validate_raster_frame<'a>(
    view: RasterFrameView<'a>,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<ValidatedRasterFrameView<'a>, Error> {
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if view.width == 0 || view.height == 0 {
        return Err(Error::Malformed(MalformedReason::EmptyDimensions));
    }
    if view.orientation != RasterOrientation::TopDown {
        return Err(Error::Unsupported(UnsupportedReason::Orientation));
    }

    let bytes_per_pixel = view.format.bytes_per_pixel()?;
    match (view.format.channels, view.color.alpha_mode) {
        (PixelChannels::Gray | PixelChannels::Rgb, AlphaMode::Opaque)
        | (PixelChannels::Rgba, AlphaMode::Straight | AlphaMode::Premultiplied) => {}
        _ => return Err(Error::Unsupported(UnsupportedReason::AlphaMode)),
    }
    if view.color.primaries != ColorPrimaries::Srgb || view.color.transfer != TransferFunction::Srgb
    {
        return Err(Error::Unsupported(UnsupportedReason::ColorDescription));
    }

    let width = usize::try_from(u64::from(view.width)).map_err(|_| Error::SizeOverflow)?;
    let row_bytes = width
        .checked_mul(bytes_per_pixel)
        .ok_or(Error::SizeOverflow)?;
    if view.row_stride < row_bytes {
        return Err(Error::Malformed(MalformedReason::RowStrideTooSmall));
    }

    let height_minus_one =
        usize::try_from(u64::from(view.height - 1)).map_err(|_| Error::SizeOverflow)?;
    let required_extent = height_minus_one
        .checked_mul(view.row_stride)
        .and_then(|prior_rows| prior_rows.checked_add(row_bytes))
        .ok_or(Error::SizeOverflow)?;
    let raster_bytes = u64::try_from(required_extent).map_err(|_| Error::SizeOverflow)?;
    let pixel_count = u64::from(view.width) * u64::from(view.height);

    if raster_bytes > budget.max_raster_bytes() {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
    }
    if pixel_count > budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }
    if view.pixels.len() < required_extent {
        return Err(Error::Malformed(MalformedReason::RasterBackingTooShort));
    }
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }

    Ok(ValidatedRasterFrameView {
        view,
        row_bytes,
        required_extent,
        pixel_count,
    })
}
