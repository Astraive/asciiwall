//! Bounded, deterministic CPU density matching over a shared fitted raster transform.
//!
//! Column sizing preserves source aspect after correcting for glyph-cell geometry and rounds
//! positive half rows upward. Fixed pixel surfaces use floor division; right and bottom leftovers
//! remain outside the output grid. Sampling uses the shared box/nearest policy after orientation,
//! transfer, and alpha normalization and before any artistic adjustment.

use core::fmt;

use ascii_core::{
    AsciiCell, AsciiFrame, Budget, CancellationToken, Error, FrameMetadata, GridSize, GlyphTableId,
    MalformedReason, RasterFrameView, UnsupportedReason, validate_grid_budget,
};

use crate::color::{
    ColorNormalizationError, ColorNormalizationWarning, LinearRgb, OutputEncoding, encode_output,
    normalize_raster,
};
use crate::font::GlyphTable;
use crate::sampling::{
    FitTransform, QualityPatches, SamplingIdentity, SamplingOptions, prepare_sampling,
};

const CANCELLATION_CELL_INTERVAL: usize = 32;
const CANCELLATION_GLYPH_INTERVAL: usize = 64;

/// How an output cell grid is sized before density matching.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GridSizing {
    /// Preserve the complete input raster's aspect at the requested number of columns.
    Columns(u32),
    /// Bound the grid to a pixel surface; leftover pixels at the right and bottom are unused.
    FixedPixelSurface { width: u32, height: u32 },
}

impl GridSizing {
    /// Creates a column-count sizing request. A zero count is rejected by
    /// [`ConversionSettings::new`] and [`Self::resolve`].
    #[must_use]
    pub const fn columns(columns: u32) -> Self {
        Self::Columns(columns)
    }

    /// Creates a fixed pixel-surface sizing request. Zero dimensions are rejected by
    /// [`ConversionSettings::new`] and [`Self::resolve`].
    #[must_use]
    pub const fn fixed_pixel_surface(width: u32, height: u32) -> Self {
        Self::FixedPixelSurface { width, height }
    }

    /// Returns the requested column count for column sizing, if applicable.
    #[must_use]
    pub const fn column_count(self) -> Option<u32> {
        match self {
            Self::Columns(columns) => Some(columns),
            Self::FixedPixelSurface { .. } => None,
        }
    }

    /// Returns the fixed pixel-surface dimensions for fixed-surface sizing, if applicable.
    #[must_use]
    pub const fn pixel_surface(self) -> Option<(u32, u32)> {
        match self {
            Self::Columns(_) => None,
            Self::FixedPixelSurface { width, height } => Some((width, height)),
        }
    }

    /// Resolves the output grid using raster dimensions and measured glyph-cell dimensions.
    ///
    /// Column mode computes
    /// `round(columns * source_height / source_width * cell_width / cell_height)` using exact
    /// integer arithmetic and nearest-integer rounding, with half rows rounded upward. A rounded
    /// zero-row grid is rejected rather than silently expanded. Fixed-surface mode floors each
    /// surface dimension by the corresponding cell dimension.
    pub fn resolve(
        self,
        source_width: u32,
        source_height: u32,
        cell_width: u32,
        cell_height: u32,
    ) -> Result<GridSize, Error> {
        if source_width == 0 || source_height == 0 || cell_width == 0 || cell_height == 0 {
            return Err(Error::Malformed(MalformedReason::EmptyDimensions));
        }

        let size = match self {
            Self::Columns(columns) => {
                if columns == 0 {
                    return Err(Error::Malformed(MalformedReason::EmptyDimensions));
                }
                let numerator = u128::from(columns)
                    .checked_mul(u128::from(source_height))
                    .and_then(|value| value.checked_mul(u128::from(cell_width)))
                    .ok_or(Error::SizeOverflow)?;
                let denominator = u128::from(source_width)
                    .checked_mul(u128::from(cell_height))
                    .ok_or(Error::SizeOverflow)?;
                let whole_rows = numerator / denominator;
                let remainder = numerator % denominator;
                let half_up_threshold = denominator / 2 + denominator % 2;
                let rounded_rows = whole_rows
                    .checked_add(if remainder >= half_up_threshold { 1 } else { 0 })
                    .ok_or(Error::SizeOverflow)?;
                let rows = u32::try_from(rounded_rows).map_err(|_| Error::SizeOverflow)?;
                GridSize::new(columns, rows)
            }
            Self::FixedPixelSurface { width, height } => {
                GridSize::new(width / cell_width, height / cell_height)
            }
        };

        if size.is_empty() {
            return Err(Error::Malformed(MalformedReason::EmptyDimensions));
        }
        Ok(size)
    }

    /// Returns right and bottom leftover pixels for a fixed surface, if cell dimensions are valid.
    #[must_use]
    pub const fn fixed_surface_leftovers(
        self,
        cell_width: u32,
        cell_height: u32,
    ) -> Option<(u32, u32)> {
        match self {
            Self::Columns(_) => None,
            Self::FixedPixelSurface { width, height }
                if cell_width != 0 && cell_height != 0 =>
            {
                Some((width % cell_width, height % cell_height))
            }
            Self::FixedPixelSurface { .. } => None,
        }
    }

    fn is_nonempty(self) -> bool {
        match self {
            Self::Columns(columns) => columns != 0,
            Self::FixedPixelSurface { width, height } => width != 0 && height != 0,
        }
    }
}

/// Validated identity, sizing, sampling, cancellation, and luminance-polarity settings.
#[derive(Clone, Copy, Debug)]
pub struct ConversionSettings<'a> {
    sizing: GridSizing,
    metadata: FrameMetadata,
    cancellation: &'a CancellationToken,
    foreground: LinearRgb,
    background: LinearRgb,
    sampling: SamplingOptions,
}

impl<'a> ConversionSettings<'a> {
    /// Creates settings after validating nonempty sizing and distinct foreground/background
    /// luminance. Colors are linear-light RGB; output cells store their sRGB8 encoding.
    pub fn new(
        sizing: GridSizing,
        metadata: FrameMetadata,
        cancellation: &'a CancellationToken,
        foreground: LinearRgb,
        background: LinearRgb,
    ) -> Result<Self, Error> {
        if !sizing.is_nonempty()
            || foreground.luminance().value() == background.luminance().value()
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }

        Ok(Self {
            sizing,
            metadata,
            cancellation,
            foreground,
            background,
            sampling: SamplingOptions::default(),
        })
    }
    /// Selects the shared fit, filter, and optional retained patch policy.
    #[must_use]
    pub const fn with_sampling(mut self, sampling: SamplingOptions) -> Self {
        self.sampling = sampling;
        self
    }

    /// Returns the fit, filter, and optional retained patch policy.
    #[must_use]
    pub const fn sampling(self) -> SamplingOptions {
        self.sampling
    }

    /// Returns the configured grid-sizing policy.
    #[must_use]
    pub const fn sizing(self) -> GridSizing {
        self.sizing
    }

    /// Returns the exact caller-supplied source metadata.
    #[must_use]
    pub const fn metadata(self) -> FrameMetadata {
        self.metadata
    }

    /// Returns the cancellation token shared with this operation.
    #[must_use]
    pub const fn cancellation(self) -> &'a CancellationToken {
        self.cancellation
    }

    /// Returns the opaque linear-light foreground color.
    #[must_use]
    pub const fn foreground(self) -> LinearRgb {
        self.foreground
    }

    /// Returns the opaque linear-light background color.
    #[must_use]
    pub const fn background(self) -> LinearRgb {
        self.background
    }
}

/// A density conversion failure, retaining the original core or color-normalization error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DensityConversionError {
    /// Grid, budget, allocation, cancellation, identity, or frame validation failed.
    Core(Error),
    /// Raster normalization failed; nested raster, HDR, and invalid-sample distinctions remain
    /// available in the original normalization error.
    ColorNormalization(ColorNormalizationError),
}

impl fmt::Display for DensityConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => write!(formatter, "density conversion failed: {error}"),
            Self::ColorNormalization(error) => {
                write!(formatter, "density conversion color normalization failed: {error}")
            }
        }
    }
}

impl std::error::Error for DensityConversionError {}

impl From<Error> for DensityConversionError {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}

impl From<ColorNormalizationError> for DensityConversionError {

    fn from(error: ColorNormalizationError) -> Self {
        Self::ColorNormalization(error)
    }
}
impl DensityConversionError {
    fn into_core(self) -> Error {
        match self {
            Self::Core(error) | Self::ColorNormalization(ColorNormalizationError::Raster(error)) => {
                error
            }
            Self::ColorNormalization(ColorNormalizationError::UnsupportedColor(_)) => {
                Error::Unsupported(UnsupportedReason::ColorDescription)
            }
            Self::ColorNormalization(ColorNormalizationError::InvalidSample(_)) => {
                Error::Malformed(MalformedReason::InvalidConfiguration)
            }
        }
    }
}
/// The complete result of one density conversion.
///
/// The transform is the exact mapping used to produce `frame`; optional quality patches contain
/// row-major per-cell linear-luminance samples and finite-difference descriptors.
#[derive(Debug, PartialEq)]
pub struct DensityConversion {
    frame: AsciiFrame,
    transform: FitTransform,
    patches: Option<QualityPatches>,
    sampling_identity: SamplingIdentity,
    normalization_warning: Option<ColorNormalizationWarning>,
}

impl DensityConversion {
    /// Returns the converted ASCII frame with the caller's original metadata.
    #[must_use]
    pub const fn frame(&self) -> &AsciiFrame {
        &self.frame
    }

    /// Returns the shared source/display transform used for sampling this frame.
    #[must_use]
    pub const fn transform(&self) -> FitTransform {
        self.transform
    }

    /// Returns optional bounded quality patches in output-cell row-major order.
    #[must_use]
    pub const fn patches(&self) -> Option<&QualityPatches> {
        self.patches.as_ref()
    }

    /// Returns the stable identity of the resolved sampling geometry and policy.
    #[must_use]
    pub const fn sampling_identity(&self) -> SamplingIdentity {
        self.sampling_identity
    }

    /// Returns any warning emitted when unknown input color metadata was assumed to be sRGB.
    #[must_use]
    pub const fn normalization_warning(&self) -> Option<ColorNormalizationWarning> {
        self.normalization_warning
    }

    /// Consumes the result and returns its ASCII frame.
    #[must_use]
    pub fn into_frame(self) -> AsciiFrame {
        self.frame
    }
}


/// Converts a raster into the caller-owned ASCII frame without desktop or GPU dependencies.
///
/// Use [`convert_density_detailed`] when the caller also needs the exact sampling transform,
/// retained quality patches, a normalization warning, or a more specific color-normalization failure.
pub fn convert_density(
    input: RasterFrameView<'_>,
    glyphs: &GlyphTable,
    settings: &ConversionSettings<'_>,
    budget: &Budget,
) -> Result<AsciiFrame, Error> {
    convert_density_detailed(input, glyphs, settings, budget)
        .map(DensityConversion::into_frame)
        .map_err(DensityConversionError::into_core)
}

/// Converts a raster into a bounded frame plus the shared geometry and optional quality patches.
///
/// Raster orientation, transfer decoding, and alpha normalization happen before fitting. Sampling
/// then integrates each destination cell's actual pixel footprint in linear light, composites
/// alpha over the configured background, and matches the result to measured glyph mean coverage.
/// Exact coverage-distance ties use font glyph index, Unicode scalar, then table position.
///
/// Grid, work, normalized-raster, and optional whole-grid patch budgets are checked before their
/// corresponding allocations. Fixed-surface pixels not covered by the floor-sized grid remain
/// explicit right/bottom leftovers and are not part of the returned display transform.
pub fn convert_density_detailed(
    input: RasterFrameView<'_>,
    glyphs: &GlyphTable,
    settings: &ConversionSettings<'_>,
    budget: &Budget,
) -> Result<DensityConversion, DensityConversionError> {
    let settings = *settings;
    if settings.cancellation.is_cancelled() {
        return Err(Error::Cancelled.into());
    }
    if settings.metadata.glyph_table_id() != GlyphTableId::new(*glyphs.fingerprint()) {
        return Err(Error::Malformed(MalformedReason::GlyphTableMismatch).into());
    }

    let glyph_records = glyphs.glyphs();
    if glyph_records.is_empty() {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration).into());
    }
    let glyph_count = u32::try_from(glyph_records.len()).map_err(|_| Error::SizeOverflow)?;
    let size = settings.sizing.resolve(
        input.width(),
        input.height(),
        glyphs.cell_width(),
        glyphs.cell_height(),
    )?;
    let grid_budget = validate_grid_budget(size, budget, settings.cancellation)?;
    let glyph_count_u64 = u64::try_from(glyph_records.len()).map_err(|_| Error::SizeOverflow)?;
    let matching_work = grid_budget
        .cell_count()
        .checked_mul(glyph_count_u64)
        .ok_or(Error::SizeOverflow)?;
    let sampling = prepare_sampling(
        input.width(),
        input.height(),
        size,
        glyphs.cell_width(),
        glyphs.cell_height(),
        settings.sampling,
        matching_work,
        budget,
    )?;

    let normalized = normalize_raster(input, budget, settings.cancellation)?;
    let normalization_warning = normalized.warning();
    let mut patches = sampling.allocate_patches(grid_budget.cell_count_usize())?;
    let foreground_luminance = settings.foreground.luminance().value();
    let background_luminance = settings.background.luminance().value();
    let luminance_span = foreground_luminance - background_luminance;
    let foreground = output_color(settings.foreground);
    let background = output_color(settings.background);

    let mut cells = Vec::new();
    cells
        .try_reserve_exact(grid_budget.cell_count_usize())
        .map_err(|_| Error::AllocationFailed)?;

    for row in 0..size.rows() {
        for column in 0..size.columns() {
            let index = cells.len();
            if index % CANCELLATION_CELL_INTERVAL == 0
                && settings.cancellation.is_cancelled()
            {
                return Err(Error::Cancelled.into());
            }
            let (target_luminance, descriptors) = if let Some(patches) = patches.as_mut() {
                let patch_output = patches.luminance_for_cell_mut(index)?;
                sampling.sample_cell(
                    &normalized,
                    column,
                    row,
                    glyphs.cell_width(),
                    glyphs.cell_height(),
                    settings.background,
                    settings.cancellation,
                    Some(patch_output),
                )?
            } else {
                sampling.sample_cell(
                    &normalized,
                    column,
                    row,
                    glyphs.cell_width(),
                    glyphs.cell_height(),
                    settings.background,
                    settings.cancellation,
                    None,
                )?
            };
            if let (Some(patches), Some(descriptor)) = (patches.as_mut(), descriptors) {
                patches.push_descriptor(descriptor);
            }
            let target_coverage =
                ((target_luminance - background_luminance) / luminance_span).clamp(0.0, 1.0);
            let glyph = best_glyph(glyph_records, target_coverage, || {
                settings.cancellation.is_cancelled()
            })?;
            cells.push(AsciiCell::new(glyph, foreground, background));
        }
    }

    if settings.cancellation.is_cancelled() {
        return Err(Error::Cancelled.into());
    }
    let frame = AsciiFrame::try_new(
        size,
        settings.metadata,
        cells,
        glyph_count,
        budget,
        settings.cancellation,
    )?;
    Ok(DensityConversion {
        frame,
        transform: sampling.transform(),
        patches,
        sampling_identity: sampling.identity(),
        normalization_warning,
    })
}

fn output_color(color: LinearRgb) -> [u8; 4] {
    let channels = encode_output(color, OutputEncoding::Srgb8).channels();
    [channels[0], channels[1], channels[2], u8::MAX]
}

fn best_glyph(
    glyphs: &[crate::font::GlyphRecord],
    target_coverage: f64,
    mut is_cancelled: impl FnMut() -> bool,
) -> Result<u32, Error> {
    let mut best: Option<(usize, f64, u16, u32)> = None;
    for (table_position, glyph) in glyphs.iter().enumerate() {
        if table_position % CANCELLATION_GLYPH_INTERVAL == 0 && is_cancelled() {
            return Err(Error::Cancelled);
        }
        let coverage = f64::from(glyph.mean_coverage());
        if !coverage.is_finite() || !(0.0..=1.0).contains(&coverage) {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let distance = (coverage - target_coverage).abs();
        let key = (glyph.glyph_index(), glyph.character() as u32);
        let replace = match best {
            None => true,
            Some((_, best_distance, best_glyph_index, best_scalar)) => {
                distance < best_distance
                    || (distance == best_distance && key < (best_glyph_index, best_scalar))
            }
        };
        if replace {
            best = Some((
                table_position,
                distance,
                glyph.glyph_index(),
                glyph.character() as u32,
            ));
        }
    }
    let (table_position, _, _, _) = best.ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
    u32::try_from(table_position).map_err(|_| Error::SizeOverflow)
}

#[cfg(test)]
mod tests {
    use super::{CANCELLATION_GLYPH_INTERVAL, best_glyph};
    use crate::font::{GlyphConfig, build_glyph_table};
    use ascii_core::{Budget, CancellationToken, Error};

    #[path = "../tests/support/synthetic_font.rs"]
    mod synthetic_font;

    #[test]
    fn cancellation_interrupts_a_large_glyph_scan() {
        let font = synthetic_font::synthetic_font(false);
        let table = build_glyph_table(
            &font,
            "BA ",
            &GlyphConfig::new(8.0, 8, 12, 9).unwrap(),
            &Budget::new(1_000_000, 2_000_000, 2_000_000),
            &CancellationToken::new(),
        )
        .unwrap();
        let glyphs = table
            .glyphs()
            .iter()
            .cycle()
            .take(CANCELLATION_GLYPH_INTERVAL * 4)
            .cloned()
            .collect::<Vec<_>>();
        let mut cancellation_checks = 0;

        let result = best_glyph(&glyphs, 0.5, || {
            cancellation_checks += 1;
            cancellation_checks == 2
        });

        assert_eq!(result, Err(Error::Cancelled));
        assert_eq!(cancellation_checks, 2);
    }
}
