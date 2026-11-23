//! Bounded, deterministic CPU density matching over a shared fitted raster transform.
//!
//! Column sizing preserves source aspect after correcting for glyph-cell geometry and rounds
//! positive half rows upward. Fixed pixel surfaces use floor division; right and bottom leftovers
//! remain outside the output grid. Sampling uses the shared box/nearest policy after orientation,
//! transfer, and alpha normalization and before any artistic adjustment.

use core::fmt;

use ascii_core::{
    AsciiCell, AsciiFrame, Budget, BudgetKind, CancellationToken, Error, FrameMetadata,
    GlyphTableId, GridSize, MalformedReason, RasterFrameView, UnsupportedReason,
    validate_grid_budget,
};

use crate::color::{
    ColorNormalizationError, ColorNormalizationWarning, LinearRgb, NormalizedRasterResources,
    OutputEncoding, encode_output, normalize_raster,
};
use crate::color_fit::{ColorFitError, ColorFitStrategy, estimate_color_fit_work, fit_color_patch};
use crate::font::GlyphTable;
use crate::palette::{Palette, PaletteError, PaletteKind};
use crate::sampling::{
    FitMode, FitTransform, QualityPatches, SamplingError, SamplingIdentity, SamplingOptions,
    preflight_sampling_source, prepare_sampling,
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
    fn resolve_for_sampling(
        self,
        source_width: u32,
        source_height: u32,
        cell_width: u32,
        cell_height: u32,
        sampling: SamplingOptions,
    ) -> Result<GridSize, Error> {
        let (Self::Columns(columns), FitMode::CustomCrop(crop)) = (self, sampling.fit()) else {
            return self.resolve(source_width, source_height, cell_width, cell_height);
        };
        if source_width == 0
            || source_height == 0
            || cell_width == 0
            || cell_height == 0
            || columns == 0
        {
            return Err(Error::Malformed(MalformedReason::EmptyDimensions));
        }
        let rows = f64::from(columns)
            * f64::from(source_height)
            * (crop.bottom() - crop.top())
            * f64::from(cell_width)
            / (f64::from(source_width) * (crop.right() - crop.left()) * f64::from(cell_height));
        if !rows.is_finite() {
            return Err(Error::SizeOverflow);
        }
        let rounded_rows = (rows + 0.5).floor();
        if rounded_rows > f64::from(u32::MAX) {
            return Err(Error::SizeOverflow);
        }
        let rows = u32::try_from(rounded_rows as u64).map_err(|_| Error::SizeOverflow)?;
        let size = GridSize::new(columns, rows);
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
            Self::FixedPixelSurface { width, height } if cell_width != 0 && cell_height != 0 => {
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
    color_fit: Option<(&'a Palette, ColorFitStrategy)>,
    sampling: SamplingOptions,
}

impl<'a> ConversionSettings<'a> {
    /// Creates settings after validating nonempty sizing and foreground/background colors that
    /// remain distinguishable after sRGB8 encoding.
    pub fn new(
        sizing: GridSizing,
        metadata: FrameMetadata,
        cancellation: &'a CancellationToken,
        foreground: LinearRgb,
        background: LinearRgb,
    ) -> Result<Self, Error> {
        let foreground_srgb8 = encode_output(foreground, OutputEncoding::Srgb8).channels();
        let background_srgb8 = encode_output(background, OutputEncoding::Srgb8).channels();
        if !sizing.is_nonempty()
            || foreground.luminance().value() == background.luminance().value()
            || foreground_srgb8 == background_srgb8
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }

        Ok(Self {
            sizing,
            metadata,
            cancellation,
            foreground,
            background,
            color_fit: None,
            sampling: SamplingOptions::default(),
        })
    }

    /// Enables candidate-specific reconstruction against this validated palette.
    ///
    /// Each glyph candidate is fit and scored with its own palette-resolved foreground/background
    /// colors; the selected explicit colors are written directly into output cells.
    #[must_use]
    pub const fn with_color_fit(
        mut self,
        palette: &'a Palette,
        strategy: ColorFitStrategy,
    ) -> Self {
        self.color_fit = Some((palette, strategy));
        self
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
    /// Candidate-specific palette validation, fit, or budget failure.
    ColorFit(ColorFitError),
}

impl fmt::Display for DensityConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => write!(formatter, "density conversion failed: {error}"),
            Self::ColorNormalization(error) => {
                write!(
                    formatter,
                    "density conversion color normalization failed: {error}"
                )
            }
            Self::ColorFit(error) => {
                write!(formatter, "density conversion color fit failed: {error}")
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
impl From<ColorFitError> for DensityConversionError {
    fn from(error: ColorFitError) -> Self {
        Self::ColorFit(error)
    }
}

impl From<SamplingError> for DensityConversionError {
    fn from(error: SamplingError) -> Self {
        match error {
            SamplingError::Core(error) => Self::Core(error),
            SamplingError::ColorNormalization(error) => Self::ColorNormalization(error),
        }
    }
}

impl DensityConversionError {
    fn into_core(self) -> Error {
        match self {
            Self::Core(error)
            | Self::ColorNormalization(ColorNormalizationError::Raster(error)) => error,
            Self::ColorNormalization(ColorNormalizationError::UnsupportedColor(_)) => {
                Error::Unsupported(UnsupportedReason::ColorDescription)
            }
            Self::ColorNormalization(ColorNormalizationError::InvalidSample(_)) => {
                Error::Malformed(MalformedReason::InvalidConfiguration)
            }
            Self::ColorFit(ColorFitError::Core(error))
            | Self::ColorFit(ColorFitError::Palette(PaletteError::Core(error))) => error,
            Self::ColorFit(ColorFitError::InvalidPatch)
            | Self::ColorFit(ColorFitError::Palette(PaletteError::InvalidDefinition)) => {
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
    palette_kind: Option<PaletteKind>,
    palette_cache_identity: Option<[u8; 32]>,
    color_fit_strategy: Option<ColorFitStrategy>,
    terminal_appearance_notice: Option<&'static str>,
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

    /// Returns the palette mode used for fitting, if enabled.
    #[must_use]
    pub const fn palette_kind(&self) -> Option<PaletteKind> {
        self.palette_kind
    }

    /// Returns the stable identity of the palette contents and matching semantics.
    #[must_use]
    pub const fn palette_cache_identity(&self) -> Option<[u8; 32]> {
        self.palette_cache_identity
    }

    /// Returns the candidate-specific color-fitting strategy, if enabled.
    #[must_use]
    pub const fn color_fit_strategy(&self) -> Option<ColorFitStrategy> {
        self.color_fit_strategy
    }

    /// Returns the ANSI terminal-appearance caveat for ANSI palette output.
    #[must_use]
    pub const fn terminal_appearance_notice(&self) -> Option<&'static str> {
        self.terminal_appearance_notice
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
/// Pass a shared [`NormalizedRasterResources`] owner to cap normalized output retained by concurrent
/// conversions; the per-operation [`Budget`] remains a separate ceiling.
pub fn convert_density(
    input: RasterFrameView<'_>,
    glyphs: &GlyphTable,
    settings: &ConversionSettings<'_>,
    budget: &Budget,
    resources: &NormalizedRasterResources,
) -> Result<AsciiFrame, Error> {
    convert_density_detailed(input, glyphs, settings, budget, resources)
        .map(DensityConversion::into_frame)
        .map_err(DensityConversionError::into_core)
}

/// Converts a raster into a bounded frame plus the shared geometry and optional quality patches.
///
/// Raster orientation, transfer decoding, and alpha normalization precede sampling. In the default
/// mode, linear-light cell luminance selects a glyph and the configured colors are retained. With
/// color fitting enabled, the converter samples one cell-sized linear-RGB patch, fits each glyph
/// mask against its own resolved foreground/background pair, and emits the winning colors directly.
/// The detailed result carries palette identity and the ANSI appearance caveat.
///
/// `budget` caps per-operation peak bytes and work; its raster-byte ceiling includes the borrowed
/// input extent, normalized output, quality/color sample buffers, and retained palette data.
/// `resources` separately caps normalized output retained by concurrent operations, with each
/// reservation released when its temporary normalized raster drops. Candidate fitting, sampling,
/// and frame validation work is checked before allocations. Fixed-surface pixels not covered by
/// the floor-sized grid remain explicit right/bottom leftovers outside the returned transform.
pub fn convert_density_detailed(
    input: RasterFrameView<'_>,
    glyphs: &GlyphTable,
    settings: &ConversionSettings<'_>,
    budget: &Budget,
    resources: &NormalizedRasterResources,
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
    let crop_derived_sizing = matches!(
        (settings.sizing, settings.sampling.fit()),
        (GridSizing::Columns(_), FitMode::CustomCrop(_))
    );
    let preflight_bytes_per_pixel = if crop_derived_sizing {
        if input.width() == 0 || input.height() == 0 {
            return Err(Error::Malformed(MalformedReason::EmptyDimensions).into());
        }
        Some(preflight_sampling_source(&input)?)
    } else {
        None
    };
    let size = settings.sizing.resolve_for_sampling(
        input.width(),
        input.height(),
        glyphs.cell_width(),
        glyphs.cell_height(),
        settings.sampling,
    )?;
    let grid_budget = validate_grid_budget(size, budget, settings.cancellation)?;
    let glyph_count_u64 = u64::try_from(glyph_records.len()).map_err(|_| Error::SizeOverflow)?;
    let (matching_work, color_samples_per_cell, palette_storage_bytes) =
        if let Some((palette, strategy)) = settings.color_fit {
            let sample_count = u64::from(glyphs.cell_width())
                .checked_mul(u64::from(glyphs.cell_height()))
                .ok_or(Error::SizeOverflow)?;
            let sample_count_usize =
                usize::try_from(sample_count).map_err(|_| Error::SizeOverflow)?;
            if sample_count_usize > crate::color_fit::MAX_COLOR_FIT_SAMPLES {
                return Err(ColorFitError::InvalidPatch.into());
            }
            let fit_work = estimate_color_fit_work(sample_count_usize, palette, strategy)?;
            if fit_work > crate::color_fit::MAX_COLOR_FIT_WORK_UNITS {
                return Err(
                    ColorFitError::Core(Error::BudgetExceeded(BudgetKind::WorkUnits)).into(),
                );
            }
            let total_fit_work = fit_work
                .checked_mul(glyph_count_u64)
                .and_then(|work| work.checked_mul(grid_budget.cell_count()))
                .ok_or(Error::SizeOverflow)?;
            (
                total_fit_work,
                sample_count,
                palette.retained_storage_bytes()?,
            )
        } else {
            (
                grid_budget
                    .cell_count()
                    .checked_mul(glyph_count_u64)
                    .ok_or(Error::SizeOverflow)?,
                0,
                0,
            )
        };
    let frame_validation_work = grid_budget.work_units();
    let bytes_per_pixel = match preflight_bytes_per_pixel {
        Some(bytes_per_pixel) => bytes_per_pixel,
        None => preflight_sampling_source(&input)?,
    };
    let sampling = prepare_sampling(
        &input,
        bytes_per_pixel,
        size,
        glyphs.cell_width(),
        glyphs.cell_height(),
        settings.sampling,
        matching_work,
        frame_validation_work,
        color_samples_per_cell,
        palette_storage_bytes,
        budget,
    )?;

    let normalized = normalize_raster(input, budget, settings.cancellation, resources)?;
    let normalization_warning = normalized.warning();
    let mut patches =
        sampling.allocate_patches(grid_budget.cell_count_usize(), settings.cancellation)?;
    let mut color_samples = if settings.color_fit.is_some() {
        if settings.cancellation.is_cancelled() {
            return Err(Error::Cancelled.into());
        }
        let sample_count =
            usize::try_from(color_samples_per_cell).map_err(|_| Error::SizeOverflow)?;
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(sample_count)
            .map_err(|_| Error::AllocationFailed)?;
        if samples.capacity() != sample_count {
            return Err(Error::BudgetExceeded(BudgetKind::RasterBytes).into());
        }
        Some(samples)
    } else {
        None
    };
    let foreground_luminance = settings.foreground.luminance().value();
    let background_luminance = settings.background.luminance().value();
    let luminance_span = foreground_luminance - background_luminance;
    let foreground = output_color(settings.foreground);
    let background = output_color(settings.background);

    let mut cells = reserve_output_cells(grid_budget.cell_count_usize(), settings.cancellation)?;

    for row in 0..size.rows() {
        for column in 0..size.columns() {
            let index = cells.len();
            if index.is_multiple_of(CANCELLATION_CELL_INTERVAL)
                && settings.cancellation.is_cancelled()
            {
                return Err(Error::Cancelled.into());
            }
            let (target_luminance, descriptors) =
                if settings.color_fit.is_some() && patches.is_none() {
                    (0.0, None)
                } else if let Some(patches) = patches.as_mut() {
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
            if let Some((palette, strategy)) = settings.color_fit {
                let samples = color_samples
                    .as_mut()
                    .ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
                sampling.sample_color_patch(
                    &normalized,
                    column,
                    row,
                    glyphs.cell_width(),
                    glyphs.cell_height(),
                    settings.background,
                    settings.cancellation,
                    samples,
                )?;
                let mut best: Option<(usize, f64, u16, u32, crate::color_fit::ColorFit)> = None;
                for (table_position, glyph) in glyph_records.iter().enumerate() {
                    if table_position.is_multiple_of(CANCELLATION_GLYPH_INTERVAL)
                        && settings.cancellation.is_cancelled()
                    {
                        return Err(Error::Cancelled.into());
                    }
                    let fit = fit_color_patch(
                        samples,
                        glyph.mask(),
                        palette,
                        strategy,
                        budget,
                        settings.cancellation,
                    )?;
                    let score = fit.mean_squared_error();
                    let glyph_key = (glyph.glyph_index(), glyph.character() as u32);
                    let replace = match best {
                        None => true,
                        Some((_, best_score, best_glyph_index, best_scalar, _)) => {
                            score < best_score
                                || (score == best_score
                                    && glyph_key < (best_glyph_index, best_scalar))
                        }
                    };
                    if replace {
                        best = Some((
                            table_position,
                            score,
                            glyph.glyph_index(),
                            glyph.character() as u32,
                            fit,
                        ));
                    }
                }
                let (table_position, _, _, _, fit) =
                    best.ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
                let glyph = u32::try_from(table_position).map_err(|_| Error::SizeOverflow)?;
                cells.push(AsciiCell::new(
                    glyph,
                    output_color(fit.foreground().color()),
                    output_color(fit.background().color()),
                ));
            } else {
                let target_coverage =
                    ((target_luminance - background_luminance) / luminance_span).clamp(0.0, 1.0);
                let glyph = best_glyph(glyph_records.iter(), target_coverage, || {
                    settings.cancellation.is_cancelled()
                })?;
                cells.push(AsciiCell::new(glyph, foreground, background));
            }
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
    let palette_kind = settings.color_fit.map(|(palette, _)| palette.kind());
    let palette_cache_identity = settings
        .color_fit
        .map(|(palette, _)| *palette.cache_identity());
    let color_fit_strategy = settings.color_fit.map(|(_, strategy)| strategy);
    let terminal_appearance_notice = settings
        .color_fit
        .and_then(|(palette, _)| palette.terminal_appearance_notice());
    Ok(DensityConversion {
        frame,
        transform: sampling.transform(),
        patches,
        sampling_identity: sampling.identity(),
        normalization_warning,
        palette_kind,
        palette_cache_identity,
        color_fit_strategy,
        terminal_appearance_notice,
    })
}

fn reserve_output_cells(
    cell_count: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<AsciiCell>, Error> {
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let mut cells = Vec::new();
    cells
        .try_reserve_exact(cell_count)
        .map_err(|_| Error::AllocationFailed)?;
    Ok(cells)
}

fn output_color(color: LinearRgb) -> [u8; 4] {
    let channels = encode_output(color, OutputEncoding::Srgb8).channels();
    [channels[0], channels[1], channels[2], u8::MAX]
}

fn best_glyph<'a>(
    glyphs: impl Iterator<Item = &'a crate::font::GlyphRecord>,
    target_coverage: f64,
    mut is_cancelled: impl FnMut() -> bool,
) -> Result<u32, Error> {
    let mut best: Option<(usize, f64, u16, u32)> = None;
    for (table_position, glyph) in glyphs.enumerate() {
        if table_position.is_multiple_of(CANCELLATION_GLYPH_INTERVAL) && is_cancelled() {
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
    let (table_position, _, _, _) =
        best.ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
    u32::try_from(table_position).map_err(|_| Error::SizeOverflow)
}

#[cfg(test)]
mod tests {
    use super::{CANCELLATION_GLYPH_INTERVAL, best_glyph, reserve_output_cells};
    use crate::font::{GlyphConfig, GlyphRepertoire, build_glyph_table};
    use ascii_core::{Budget, CancellationToken, Error};

    #[allow(dead_code)]
    mod synthetic_font {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/synthetic_font.rs"
        ));
    }

    #[test]
    fn cancellation_interrupts_a_large_glyph_scan() {
        let font = synthetic_font::synthetic_font(false);
        let table = build_glyph_table(
            &font,
            &GlyphRepertoire::new("BA ").unwrap(),
            &GlyphConfig::new(8.0, 8, 12, 9).unwrap(),
            &Budget::new(1_000_000, 2_000_000, 2_000_000),
            &CancellationToken::new(),
        )
        .unwrap();
        let glyphs = table
            .glyphs()
            .iter()
            .cycle()
            .take(CANCELLATION_GLYPH_INTERVAL * 4);
        let mut cancellation_checks = 0;

        let result = best_glyph(glyphs, 0.5, || {
            cancellation_checks += 1;
            cancellation_checks == 2
        });

        assert_eq!(result, Err(Error::Cancelled));
        assert_eq!(cancellation_checks, 2);
    }

    #[test]
    fn output_cell_reservation_checks_cancellation_before_impossible_reserve() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        assert_eq!(
            reserve_output_cells(usize::MAX, &cancellation).err(),
            Some(Error::Cancelled)
        );
    }
}
