//! Bounded linear-light reconstruction of one sampled cell by a glyph coverage mask.
//!
//! The reconstruction is `R(p) = B + (F - B) M(p)` with byte coverage interpreted as
//! `M = coverage / 255`. Scores are mean squared error across all linear-light RGB channels.
//! Candidate glyphs must each be fit and scored with their own returned colors before reranking.

use core::fmt;

use ascii_core::{Budget, BudgetKind, CancellationToken, Error};

use crate::color::LinearRgb;
use crate::palette::{
    GradientStop, Palette, PaletteColor, PaletteError, PaletteStorage, gradient_color,
    rgb_channels, rgb_from_channels,
};

/// Version of glyph color reconstruction and candidate-pair semantics.
pub const COLOR_FIT_ALGORITHM_VERSION: u32 = 1;
/// Maximum samples accepted for one glyph-cell reconstruction.
pub const MAX_COLOR_FIT_SAMPLES: usize = 65_536;
/// Hard ceiling on channel operations for one glyph-cell fit.
pub const MAX_COLOR_FIT_WORK_UNITS: u64 = 64_000_000;
const MASK_FULL_SCALE: f64 = 255.0;
const DEGENERATE_RELATIVE_DETERMINANT: f64 = 1.0e-12;
const CANCELLATION_SAMPLE_INTERVAL: usize = 256;

/// Color-fitting strategy used for one candidate glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorFitStrategy {
    /// Hold this background fixed and solve the best foreground allowed by the palette.
    FixedBackground(LinearRgb),
    /// Solve the foreground/background pair jointly for the candidate's mask.
    TwoColorLeastSquares,
}

/// The palette-resolved colors and reconstruction score for one glyph candidate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorFit {
    foreground: PaletteColor,
    background: PaletteColor,
    mean_squared_error: f64,
    work_units: u64,
}

impl ColorFit {
    /// Returns the candidate-specific resolved foreground, including an optional palette label.
    #[must_use]
    pub const fn foreground(self) -> PaletteColor {
        self.foreground
    }

    /// Returns the candidate-specific resolved background, including an optional palette label.
    #[must_use]
    pub const fn background(self) -> PaletteColor {
        self.background
    }

    /// Returns linear-light RGB mean squared reconstruction error in `[0, 1]`.
    #[must_use]
    pub const fn mean_squared_error(self) -> f64 {
        self.mean_squared_error
    }

    /// Returns the checked conservative work estimate charged for this candidate fit.
    #[must_use]
    pub const fn work_units(self) -> u64 {
        self.work_units
    }
}

/// Color-fit input, palette, cancellation, or resource-limit failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorFitError {
    /// The target patch is empty, too large, or does not match the coverage-mask length.
    InvalidPatch,
    /// Palette validation or resolution failed.
    Palette(PaletteError),
    /// A core cancellation, arithmetic, or resource-budget failure.
    Core(Error),
}

impl fmt::Display for ColorFitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPatch => formatter.write_str("invalid color-fit patch"),
            Self::Palette(error) => write!(formatter, "color-fit palette failed: {error}"),
            Self::Core(error) => write!(formatter, "color fit failed: {error}"),
        }
    }
}

impl std::error::Error for ColorFitError {}

impl From<Error> for ColorFitError {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}

impl From<PaletteError> for ColorFitError {
    fn from(error: PaletteError) -> Self {
        Self::Palette(error)
    }
}

/// Fits and scores one candidate glyph using its row-major target RGB samples and coverage mask.
///
/// `target` contains the cell's per-pixel colors in linear-light RGB, and `coverage` contains the
/// same-sized row-major glyph mask in `[0, 255]`. The function returns the exact resolved colors
/// used by its score. Original mode solves unconstrained colors; fixed and ANSI palettes enumerate
/// the palette colors to minimize reconstruction error; gradients search each pair of interpolated
/// segments. Monochrome mode uses its configured pair. No unrelated post-fit recoloring is done.
pub fn fit_color_patch(
    target: &[LinearRgb],
    coverage: &[u8],
    palette: &Palette,
    strategy: ColorFitStrategy,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<ColorFit, ColorFitError> {
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled.into());
    }
    if target.is_empty() || target.len() != coverage.len() || target.len() > MAX_COLOR_FIT_SAMPLES {
        return Err(ColorFitError::InvalidPatch);
    }

    let work_units = estimate_work(target.len(), palette, strategy)?;
    check_work(work_units, budget, cancellation)?;

    if let Some((foreground, background)) = palette.monochrome_pair() {
        let score = mean_squared_error(target, coverage, foreground, background, cancellation)?;
        return Ok(ColorFit {
            foreground: PaletteColor::from_fit(foreground, None, None),
            background: PaletteColor::from_fit(background, None, None),
            mean_squared_error: score,
            work_units,
        });
    }

    let (foreground, background, score) = match (strategy, palette.storage()) {
        (ColorFitStrategy::FixedBackground(requested_background), PaletteStorage::Original) => {
            let foreground =
                fit_fixed_background(target, coverage, requested_background, cancellation)?;
            let score = mean_squared_error(
                target,
                coverage,
                foreground,
                requested_background,
                cancellation,
            )?;
            (
                PaletteColor::from_fit(foreground, None, None),
                PaletteColor::from_fit(requested_background, None, None),
                score,
            )
        }
        (ColorFitStrategy::TwoColorLeastSquares, PaletteStorage::Original) => {
            let (foreground, background) =
                fit_two_color_least_squares(target, coverage, cancellation)?;
            let score = mean_squared_error(target, coverage, foreground, background, cancellation)?;
            (
                PaletteColor::from_fit(foreground, None, None),
                PaletteColor::from_fit(background, None, None),
                score,
            )
        }
        (
            ColorFitStrategy::FixedBackground(requested_background),
            PaletteStorage::Fixed(_) | PaletteStorage::Ansi16 | PaletteStorage::Ansi256,
        ) => {
            let background = palette.resolve_unbudgeted(requested_background, cancellation)?;
            fit_fixed_palette_foreground(target, coverage, palette, background, cancellation)?
        }
        (
            ColorFitStrategy::TwoColorLeastSquares,
            PaletteStorage::Fixed(_) | PaletteStorage::Ansi16 | PaletteStorage::Ansi256,
        ) => fit_discrete_palette_pair(target, coverage, palette, cancellation)?,
        (
            ColorFitStrategy::FixedBackground(requested_background),
            PaletteStorage::Gradient(stops),
        ) => {
            let background = palette.resolve_unbudgeted(requested_background, cancellation)?;
            fit_gradient_foreground(target, coverage, stops, background, cancellation)?
        }
        (ColorFitStrategy::TwoColorLeastSquares, PaletteStorage::Gradient(stops)) => {
            fit_gradient_pair(target, coverage, stops, cancellation)?
        }
        (
            _,
            PaletteStorage::Monochrome {
                foreground,
                background,
            },
        ) => {
            let score =
                mean_squared_error(target, coverage, *foreground, *background, cancellation)?;
            (
                PaletteColor::from_fit(*foreground, None, None),
                PaletteColor::from_fit(*background, None, None),
                score,
            )
        }
    };

    Ok(ColorFit {
        foreground,
        background,
        mean_squared_error: score,
        work_units,
    })
}

/// Returns a conservative channel-operation count before performing a candidate fit.
///
/// Callers reranking multiple glyph candidates should checked-add each estimate and preflight the
/// total against their job budget before publishing a winner.
pub fn estimate_color_fit_work(
    sample_count: usize,
    palette: &Palette,
    strategy: ColorFitStrategy,
) -> Result<u64, ColorFitError> {
    if sample_count == 0 || sample_count > MAX_COLOR_FIT_SAMPLES {
        return Err(ColorFitError::InvalidPatch);
    }
    estimate_work(sample_count, palette, strategy)
}

fn estimate_work(
    sample_count: usize,
    palette: &Palette,
    strategy: ColorFitStrategy,
) -> Result<u64, ColorFitError> {
    let n = u64::try_from(sample_count).map_err(|_| Error::SizeOverflow)?;
    let candidates = match (strategy, palette.storage()) {
        (_, PaletteStorage::Monochrome { .. }) => 1,
        (ColorFitStrategy::FixedBackground(_), PaletteStorage::Original) => 3,
        (ColorFitStrategy::TwoColorLeastSquares, PaletteStorage::Original) => 4,
        (ColorFitStrategy::FixedBackground(_), PaletteStorage::Fixed(colors)) => {
            u64::try_from(colors.len()).map_err(|_| Error::SizeOverflow)? + 1
        }
        (ColorFitStrategy::FixedBackground(_), PaletteStorage::Ansi16) => 17,
        (ColorFitStrategy::FixedBackground(_), PaletteStorage::Ansi256) => 257,
        (ColorFitStrategy::TwoColorLeastSquares, PaletteStorage::Fixed(colors)) => {
            let count = u64::try_from(colors.len()).map_err(|_| Error::SizeOverflow)?;
            count
                .checked_mul(count)
                .ok_or(Error::SizeOverflow)?
                .checked_add(1)
                .ok_or(Error::SizeOverflow)?
        }
        (ColorFitStrategy::TwoColorLeastSquares, PaletteStorage::Ansi16) => 257,
        (ColorFitStrategy::TwoColorLeastSquares, PaletteStorage::Ansi256) => 65_537,
        (ColorFitStrategy::FixedBackground(_), PaletteStorage::Gradient(stops)) => {
            let segments =
                u64::try_from(stops.len().saturating_sub(1)).map_err(|_| Error::SizeOverflow)?;
            segments
                .checked_mul(2)
                .ok_or(Error::SizeOverflow)?
                .checked_add(1)
                .ok_or(Error::SizeOverflow)?
        }
        (ColorFitStrategy::TwoColorLeastSquares, PaletteStorage::Gradient(stops)) => {
            let segments =
                u64::try_from(stops.len().saturating_sub(1)).map_err(|_| Error::SizeOverflow)?;
            segments
                .checked_mul(segments)
                .and_then(|pairs| pairs.checked_mul(2))
                .ok_or(Error::SizeOverflow)?
                .checked_add(1)
                .ok_or(Error::SizeOverflow)?
        }
    };
    let mut work = n.checked_mul(3).ok_or(Error::SizeOverflow)?;
    work = work.checked_mul(candidates).ok_or(Error::SizeOverflow)?;
    if matches!(strategy, ColorFitStrategy::FixedBackground(_))
        && !palette.is_original()
        && palette.monochrome_pair().is_none()
    {
        work = work
            .checked_add(palette.resolution_work_units())
            .ok_or(Error::SizeOverflow)?;
    }
    Ok(work)
}

fn check_work(
    work_units: u64,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<(), ColorFitError> {
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled.into());
    }
    if work_units > MAX_COLOR_FIT_WORK_UNITS || work_units > budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits).into());
    }
    Ok(())
}

fn fit_fixed_background(
    target: &[LinearRgb],
    coverage: &[u8],
    background: LinearRgb,
    cancellation: &CancellationToken,
) -> Result<LinearRgb, ColorFitError> {
    let bg = rgb_channels(background);
    let mut numerator = [0.0; 3];
    let mut denominator = 0.0;
    for index in 0..target.len() {
        check_cancel_periodically(index, cancellation)?;
        let mask = f64::from(coverage[index]) / MASK_FULL_SCALE;
        let color = rgb_channels(target[index]);
        for channel in 0..3 {
            numerator[channel] += mask * (color[channel] - bg[channel]);
        }
        denominator += mask * mask;
    }
    let foreground = if denominator == 0.0 {
        bg
    } else {
        [
            (bg[0] + numerator[0] / denominator).clamp(0.0, 1.0),
            (bg[1] + numerator[1] / denominator).clamp(0.0, 1.0),
            (bg[2] + numerator[2] / denominator).clamp(0.0, 1.0),
        ]
    };
    Ok(rgb_from_channels(foreground)?)
}

fn fit_two_color_least_squares(
    target: &[LinearRgb],
    coverage: &[u8],
    cancellation: &CancellationToken,
) -> Result<(LinearRgb, LinearRgb), ColorFitError> {
    let mut fg = [0.0; 3];
    let mut bg = [0.0; 3];
    for channel in 0..3 {
        let mut a_sum = 0.0;
        let mut c_sum = 0.0;
        let mut cross_sum = 0.0;
        let mut rhs_bg = 0.0;
        let mut rhs_fg = 0.0;
        let mut pixel_square_sum = 0.0;
        let mut pixel_sum = 0.0;

        for index in 0..target.len() {
            check_cancel_periodically(index, cancellation)?;
            let mask = f64::from(coverage[index]) / MASK_FULL_SCALE;
            let value = rgb_channels(target[index])[channel];
            let a = 1.0 - mask;
            a_sum += a * a;
            c_sum += mask * mask;
            cross_sum += a * mask;
            rhs_bg += a * value;
            rhs_fg += mask * value;
            pixel_square_sum += value * value;
            pixel_sum += value;
        }

        let determinant = a_sum * c_sum - cross_sum * cross_sum;
        let scale = a_sum * c_sum;
        if scale == 0.0 || determinant <= DEGENERATE_RELATIVE_DETERMINANT * scale {
            let mean = (pixel_sum / target.len() as f64).clamp(0.0, 1.0);
            bg[channel] = mean;
            fg[channel] = mean;
            continue;
        }

        let mut best: Option<(f64, f64, f64)> = None;
        let unconstrained_bg = (rhs_bg * c_sum - rhs_fg * cross_sum) / determinant;
        let unconstrained_fg = (rhs_fg * a_sum - rhs_bg * cross_sum) / determinant;
        if (0.0..=1.0).contains(&unconstrained_bg) && (0.0..=1.0).contains(&unconstrained_fg) {
            consider_channel_pair(
                unconstrained_fg,
                unconstrained_bg,
                a_sum,
                c_sum,
                cross_sum,
                rhs_bg,
                rhs_fg,
                pixel_square_sum,
                &mut best,
            );
        }

        for fixed_bg in [0.0, 1.0] {
            let fitted_fg = if c_sum == 0.0 {
                0.0
            } else {
                ((rhs_fg - cross_sum * fixed_bg) / c_sum).clamp(0.0, 1.0)
            };
            consider_channel_pair(
                fitted_fg,
                fixed_bg,
                a_sum,
                c_sum,
                cross_sum,
                rhs_bg,
                rhs_fg,
                pixel_square_sum,
                &mut best,
            );
        }
        for fixed_fg in [0.0, 1.0] {
            let fitted_bg = if a_sum == 0.0 {
                0.0
            } else {
                ((rhs_bg - cross_sum * fixed_fg) / a_sum).clamp(0.0, 1.0)
            };
            consider_channel_pair(
                fixed_fg,
                fitted_bg,
                a_sum,
                c_sum,
                cross_sum,
                rhs_bg,
                rhs_fg,
                pixel_square_sum,
                &mut best,
            );
        }
        let (best_fg, best_bg, _) = best.ok_or(ColorFitError::InvalidPatch)?;
        fg[channel] = best_fg;
        bg[channel] = best_bg;
    }
    Ok((rgb_from_channels(fg)?, rgb_from_channels(bg)?))
}

#[allow(clippy::too_many_arguments)]
fn consider_channel_pair(
    foreground: f64,
    background: f64,
    a_sum: f64,
    c_sum: f64,
    cross_sum: f64,
    rhs_bg: f64,
    rhs_fg: f64,
    pixel_square_sum: f64,
    best: &mut Option<(f64, f64, f64)>,
) {
    let error = (pixel_square_sum - 2.0 * background * rhs_bg - 2.0 * foreground * rhs_fg
        + background * background * a_sum
        + 2.0 * background * foreground * cross_sum
        + foreground * foreground * c_sum)
        .max(0.0);
    if best.is_none_or(|(_, _, best_error)| error < best_error) {
        *best = Some((foreground, background, error));
    }
}

fn fit_fixed_palette_foreground(
    target: &[LinearRgb],
    coverage: &[u8],
    palette: &Palette,
    background: PaletteColor,
    cancellation: &CancellationToken,
) -> Result<(PaletteColor, PaletteColor, f64), ColorFitError> {
    let candidate_count = palette
        .candidate_count()
        .ok_or(ColorFitError::InvalidPatch)?;
    let mut best: Option<(PaletteColor, f64)> = None;
    for candidate_index in 0..candidate_count {
        if candidate_index % 32 == 0 && cancellation.is_cancelled() {
            return Err(Error::Cancelled.into());
        }
        let foreground = palette
            .indexed_color(candidate_index)
            .ok_or(ColorFitError::InvalidPatch)?;
        let score = mean_squared_error(
            target,
            coverage,
            foreground.color(),
            background.color(),
            cancellation,
        )?;
        if best.is_none_or(|(_, best_score)| score < best_score) {
            best = Some((foreground, score));
        }
    }
    let (foreground, score) = best.ok_or(ColorFitError::InvalidPatch)?;
    Ok((foreground, background, score))
}

fn fit_discrete_palette_pair(
    target: &[LinearRgb],
    coverage: &[u8],
    palette: &Palette,
    cancellation: &CancellationToken,
) -> Result<(PaletteColor, PaletteColor, f64), ColorFitError> {
    let candidate_count = palette
        .candidate_count()
        .ok_or(ColorFitError::InvalidPatch)?;
    let mut best: Option<(PaletteColor, PaletteColor, f64)> = None;
    let mut candidates_seen = 0_usize;
    for foreground_index in 0..candidate_count {
        let foreground = palette
            .indexed_color(foreground_index)
            .ok_or(ColorFitError::InvalidPatch)?;
        for background_index in 0..candidate_count {
            if candidates_seen.is_multiple_of(32) && cancellation.is_cancelled() {
                return Err(Error::Cancelled.into());
            }
            candidates_seen += 1;
            let background = palette
                .indexed_color(background_index)
                .ok_or(ColorFitError::InvalidPatch)?;
            let score = mean_squared_error(
                target,
                coverage,
                foreground.color(),
                background.color(),
                cancellation,
            )?;
            if best.is_none_or(|(_, _, best_score)| score < best_score) {
                best = Some((foreground, background, score));
            }
        }
    }
    best.ok_or(ColorFitError::InvalidPatch)
}

fn fit_gradient_foreground(
    target: &[LinearRgb],
    coverage: &[u8],
    stops: &[GradientStop],
    background: PaletteColor,
    cancellation: &CancellationToken,
) -> Result<(PaletteColor, PaletteColor, f64), ColorFitError> {
    let background_channels = rgb_channels(background.color());
    let mut best: Option<(PaletteColor, f64)> = None;
    for segment in 0..stops.len() - 1 {
        if segment % 16 == 0 && cancellation.is_cancelled() {
            return Err(Error::Cancelled.into());
        }
        let (first, _) = gradient_color(stops[segment], stops[segment + 1], 0.0)?;
        let (last, _) = gradient_color(stops[segment], stops[segment + 1], 1.0)?;
        let first_channels = rgb_channels(first);
        let direction = [
            rgb_channels(last)[0] - first_channels[0],
            rgb_channels(last)[1] - first_channels[1],
            rgb_channels(last)[2] - first_channels[2],
        ];
        let mut numerator = 0.0;
        let mut denominator = 0.0;
        for index in 0..target.len() {
            check_cancel_periodically(index, cancellation)?;
            let mask = f64::from(coverage[index]) / MASK_FULL_SCALE;
            let observed = rgb_channels(target[index]);
            let first_reconstruction = [
                background_channels[0] + mask * (first_channels[0] - background_channels[0]),
                background_channels[1] + mask * (first_channels[1] - background_channels[1]),
                background_channels[2] + mask * (first_channels[2] - background_channels[2]),
            ];
            for channel in 0..3 {
                let feature = mask * direction[channel];
                numerator += feature * (observed[channel] - first_reconstruction[channel]);
                denominator += feature * feature;
            }
        }
        let parameter = if denominator == 0.0 {
            0.0
        } else {
            (numerator / denominator).clamp(0.0, 1.0)
        };
        let (color, position) = gradient_color(stops[segment], stops[segment + 1], parameter)?;
        let candidate = PaletteColor::from_fit(color, None, Some(position));
        let score = mean_squared_error(target, coverage, color, background.color(), cancellation)?;
        if best.is_none_or(|(_, best_score)| score < best_score) {
            best = Some((candidate, score));
        }
    }
    let (foreground, score) = best.ok_or(ColorFitError::InvalidPatch)?;
    Ok((foreground, background, score))
}

fn fit_gradient_pair(
    target: &[LinearRgb],
    coverage: &[u8],
    stops: &[GradientStop],
    cancellation: &CancellationToken,
) -> Result<(PaletteColor, PaletteColor, f64), ColorFitError> {
    let segment_count = stops
        .len()
        .checked_sub(1)
        .ok_or(ColorFitError::InvalidPatch)?;
    let mut best: Option<(PaletteColor, PaletteColor, f64)> = None;
    let mut pairs_seen = 0_usize;
    for foreground_segment in 0..segment_count {
        for background_segment in 0..segment_count {
            if pairs_seen.is_multiple_of(8) && cancellation.is_cancelled() {
                return Err(Error::Cancelled.into());
            }
            pairs_seen += 1;
            let fitted = fit_gradient_segment_pair(
                target,
                coverage,
                stops[foreground_segment],
                stops[foreground_segment + 1],
                stops[background_segment],
                stops[background_segment + 1],
                cancellation,
            )?;
            if best.is_none_or(|(_, _, best_score)| fitted.2 < best_score) {
                best = Some(fitted);
            }
        }
    }
    best.ok_or(ColorFitError::InvalidPatch)
}

fn fit_gradient_segment_pair(
    target: &[LinearRgb],
    coverage: &[u8],
    foreground_first: GradientStop,
    foreground_last: GradientStop,
    background_first: GradientStop,
    background_last: GradientStop,
    cancellation: &CancellationToken,
) -> Result<(PaletteColor, PaletteColor, f64), ColorFitError> {
    let foreground_start = rgb_channels(foreground_first.color());
    let foreground_end = rgb_channels(foreground_last.color());
    let background_start = rgb_channels(background_first.color());
    let background_end = rgb_channels(background_last.color());
    let foreground_direction = subtract(foreground_end, foreground_start);
    let background_direction = subtract(background_end, background_start);
    let mut feature_f_square = 0.0;
    let mut feature_b_square = 0.0;
    let mut feature_cross = 0.0;
    let mut rhs_f = 0.0;
    let mut rhs_b = 0.0;
    let mut residual_square = 0.0;

    for index in 0..target.len() {
        check_cancel_periodically(index, cancellation)?;
        let mask = f64::from(coverage[index]) / MASK_FULL_SCALE;
        let target_channels = rgb_channels(target[index]);
        for channel in 0..3 {
            let baseline = background_start[channel]
                + mask * (foreground_start[channel] - background_start[channel]);
            let residual = target_channels[channel] - baseline;
            let feature_f = mask * foreground_direction[channel];
            let feature_b = (1.0 - mask) * background_direction[channel];
            feature_f_square += feature_f * feature_f;
            feature_b_square += feature_b * feature_b;
            feature_cross += feature_f * feature_b;
            rhs_f += feature_f * residual;
            rhs_b += feature_b * residual;
            residual_square += residual * residual;
        }
    }

    let (foreground_parameter, background_parameter) = solve_bounded_pair(
        feature_f_square,
        feature_b_square,
        feature_cross,
        rhs_f,
        rhs_b,
        residual_square,
    )?;
    let (foreground, foreground_position) =
        gradient_color(foreground_first, foreground_last, foreground_parameter)?;
    let (background, background_position) =
        gradient_color(background_first, background_last, background_parameter)?;
    let score = mean_squared_error(target, coverage, foreground, background, cancellation)?;
    Ok((
        PaletteColor::from_fit(foreground, None, Some(foreground_position)),
        PaletteColor::from_fit(background, None, Some(background_position)),
        score,
    ))
}

fn solve_bounded_pair(
    feature_f_square: f64,
    feature_b_square: f64,
    feature_cross: f64,
    rhs_f: f64,
    rhs_b: f64,
    residual_square: f64,
) -> Result<(f64, f64), ColorFitError> {
    let determinant = feature_f_square * feature_b_square - feature_cross * feature_cross;
    let scale = feature_f_square * feature_b_square;
    let mut best: Option<(f64, f64, f64)> = None;
    if scale > 0.0 && determinant > DEGENERATE_RELATIVE_DETERMINANT * scale {
        let foreground = (rhs_f * feature_b_square - rhs_b * feature_cross) / determinant;
        let background = (rhs_b * feature_f_square - rhs_f * feature_cross) / determinant;
        if (0.0..=1.0).contains(&foreground) && (0.0..=1.0).contains(&background) {
            consider_bounded_pair(
                foreground,
                background,
                feature_f_square,
                feature_b_square,
                feature_cross,
                rhs_f,
                rhs_b,
                residual_square,
                &mut best,
            );
        }
    }
    for foreground in [0.0, 1.0] {
        let background = if feature_b_square == 0.0 {
            0.0
        } else {
            ((rhs_b - feature_cross * foreground) / feature_b_square).clamp(0.0, 1.0)
        };
        consider_bounded_pair(
            foreground,
            background,
            feature_f_square,
            feature_b_square,
            feature_cross,
            rhs_f,
            rhs_b,
            residual_square,
            &mut best,
        );
    }
    for background in [0.0, 1.0] {
        let foreground = if feature_f_square == 0.0 {
            0.0
        } else {
            ((rhs_f - feature_cross * background) / feature_f_square).clamp(0.0, 1.0)
        };
        consider_bounded_pair(
            foreground,
            background,
            feature_f_square,
            feature_b_square,
            feature_cross,
            rhs_f,
            rhs_b,
            residual_square,
            &mut best,
        );
    }
    best.map(|(foreground, background, _)| (foreground, background))
        .ok_or(ColorFitError::InvalidPatch)
}

#[allow(clippy::too_many_arguments)]
fn consider_bounded_pair(
    foreground: f64,
    background: f64,
    feature_f_square: f64,
    feature_b_square: f64,
    feature_cross: f64,
    rhs_f: f64,
    rhs_b: f64,
    residual_square: f64,
    best: &mut Option<(f64, f64, f64)>,
) {
    let error = (residual_square - 2.0 * foreground * rhs_f - 2.0 * background * rhs_b
        + foreground * foreground * feature_f_square
        + 2.0 * foreground * background * feature_cross
        + background * background * feature_b_square)
        .max(0.0);
    if best.is_none_or(|(_, _, best_error)| error < best_error) {
        *best = Some((foreground, background, error));
    }
}

fn mean_squared_error(
    target: &[LinearRgb],
    coverage: &[u8],
    foreground: LinearRgb,
    background: LinearRgb,
    cancellation: &CancellationToken,
) -> Result<f64, ColorFitError> {
    let foreground = rgb_channels(foreground);
    let background = rgb_channels(background);
    let mut squared_error = 0.0;
    for index in 0..target.len() {
        check_cancel_periodically(index, cancellation)?;
        let mask = f64::from(coverage[index]) / MASK_FULL_SCALE;
        let target = rgb_channels(target[index]);
        for channel in 0..3 {
            let reconstructed =
                background[channel] + (foreground[channel] - background[channel]) * mask;
            let difference = target[channel] - reconstructed;
            squared_error += difference * difference;
        }
    }
    let score = squared_error / (target_len_as_f64(target.len()) * 3.0);
    if !score.is_finite() {
        return Err(ColorFitError::InvalidPatch);
    }
    Ok(score)
}

fn target_len_as_f64(length: usize) -> f64 {
    length as f64
}

fn check_cancel_periodically(
    sample_index: usize,
    cancellation: &CancellationToken,
) -> Result<(), ColorFitError> {
    if sample_index.is_multiple_of(CANCELLATION_SAMPLE_INTERVAL) && cancellation.is_cancelled() {
        return Err(Error::Cancelled.into());
    }
    Ok(())
}

fn subtract(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}
