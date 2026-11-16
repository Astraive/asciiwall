//! Explicit, bounded palette definitions and deterministic color resolution.
//!
//! Colors in this module are linear-light RGB. Palette distance is independently selectable as
//! linear RGB Euclidean distance or encoded-sRGB Euclidean distance. ANSI modes use the canonical
//! xterm color table as explicit sRGB values; their numeric labels do not promise terminal output.

use core::fmt;
use core::mem::size_of;

use ascii_core::{Budget, BudgetKind, CancellationToken, Error};
use sha2::{Digest, Sha256};

use crate::color::{EncodedSrgb, LinearRgb, decode_srgb, encode_srgb};

/// Version of palette matching and identity semantics.
pub const PALETTE_ALGORITHM_VERSION: u32 = 2;
/// Maximum number of colors retained by a user-defined fixed palette.
pub const MAX_PALETTE_COLORS: usize = 4096;
/// Maximum number of ordered stops in a user gradient.
pub const MAX_GRADIENT_STOPS: usize = 128;
/// Hard ceiling on channel-distance work units in a single palette resolution.
pub const MAX_PALETTE_RESOLUTION_WORK_UNITS: u64 = 65_536;
/// Hard ceiling on channel-distance work units for resolving both colors in one pair.
pub const MAX_PALETTE_PAIR_RESOLUTION_WORK_UNITS: u64 = 2 * MAX_PALETTE_RESOLUTION_WORK_UNITS;
// Engineering margin to reduce lower-bound overestimation from floating-point rounding.
const ENCODED_GRADIENT_BOUND_ROUNDING_GUARD: f64 = 1.0e-14;
const ENCODED_GRADIENT_DISTANCE_TOLERANCE: f64 = 1.0e-6;
const ENCODED_GRADIENT_SEARCH_TOLERANCE: f64 = ENCODED_GRADIENT_DISTANCE_TOLERANCE * 0.5;
const ENCODED_GRADIENT_SEGMENT_TIE_TOLERANCE: f64 = ENCODED_GRADIENT_DISTANCE_TOLERANCE * 0.5;
const ENCODED_GRADIENT_MAX_SUBDIVISIONS: usize = 4_096;
const ENCODED_GRADIENT_INITIAL_SAMPLES: u32 = 256;

/// The metric used to compare a requested color with palette colors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteDistanceSpace {
    /// Squared Euclidean distance between linear-light R, G, and B channels.
    LinearRgb,
    /// Squared Euclidean distance between transfer-encoded sRGB R, G, and B channels.
    EncodedSrgb,
}

/// One gradient stop with a normalized position and linear-light RGB value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    position: f64,
    color: LinearRgb,
}

impl GradientStop {
    /// Creates a stop at a finite position in `[0, 1]`.
    pub fn new(position: f64, color: LinearRgb) -> Result<Self, PaletteError> {
        if !position.is_finite() || !(0.0..=1.0).contains(&position) {
            return Err(PaletteError::InvalidDefinition);
        }
        Ok(Self {
            position: if position == 0.0 { 0.0 } else { position },
            color,
        })
    }

    /// Returns the normalized stop position.
    #[must_use]
    pub const fn position(self) -> f64 {
        self.position
    }

    /// Returns the stop color in linear-light RGB.
    #[must_use]
    pub const fn color(self) -> LinearRgb {
        self.color
    }
}

/// A palette definition supplied to [`Palette::new`].
#[derive(Clone, Copy, Debug)]
pub enum PaletteDefinition<'a> {
    /// Preserve the caller's already sampled colors without quantization.
    Original,
    /// Always use this explicit foreground/background pair.
    Monochrome {
        foreground: LinearRgb,
        background: LinearRgb,
    },
    /// Quantize to a caller-ordered list; exact ties use the earliest entry.
    Fixed(&'a [LinearRgb]),
    /// Quantize to the canonical xterm ANSI-16 colors.
    Ansi16,
    /// Quantize to the canonical xterm ANSI-256 colors.
    Ansi256,
    /// Interpolate between ordered linear-light stops; endpoints are required at 0 and 1.
    Gradient(&'a [GradientStop]),
}

/// The semantic kind of a validated palette.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteKind {
    Original,
    Monochrome,
    Fixed,
    Ansi16,
    Ansi256,
    Gradient,
}

/// Palette validation, allocation, cancellation, or resource-limit failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteError {
    /// The definition is empty, malformed, unordered, or exceeds a hard palette limit.
    InvalidDefinition,
    /// A core allocation, cancellation, arithmetic, or resource-budget failure.
    Core(Error),
}

impl fmt::Display for PaletteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDefinition => formatter.write_str("invalid palette definition"),
            Self::Core(error) => write!(formatter, "palette construction failed: {error}"),
        }
    }
}

impl std::error::Error for PaletteError {}

impl From<Error> for PaletteError {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}

/// One explicit resolved palette color, with its palette index when it has one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaletteColor {
    color: LinearRgb,
    index: Option<u16>,
    gradient_position: Option<f64>,
}

impl PaletteColor {
    pub(crate) const fn from_fit(
        color: LinearRgb,
        index: Option<u16>,
        gradient_position: Option<f64>,
    ) -> Self {
        Self {
            color,
            index,
            gradient_position,
        }
    }

    /// Returns the explicit linear-light color to encode for PNG or other resolved-color outputs.
    #[must_use]
    pub const fn color(self) -> LinearRgb {
        self.color
    }

    /// Returns the fixed-palette or ANSI index, if this color has one.
    #[must_use]
    pub const fn index(self) -> Option<u16> {
        self.index
    }

    /// Returns the interpolated normalized gradient position, when this is a gradient color.
    #[must_use]
    pub const fn gradient_position(self) -> Option<f64> {
        self.gradient_position
    }
}

/// Resolved foreground and background colors for one output cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaletteColorPair {
    foreground: PaletteColor,
    background: PaletteColor,
}

impl PaletteColorPair {
    /// Returns the resolved foreground color and its optional label.
    #[must_use]
    pub const fn foreground(self) -> PaletteColor {
        self.foreground
    }

    /// Returns the resolved background color and its optional label.
    #[must_use]
    pub const fn background(self) -> PaletteColor {
        self.background
    }
}

#[derive(Debug)]
pub(crate) enum PaletteStorage {
    Original,
    Monochrome {
        foreground: LinearRgb,
        background: LinearRgb,
    },
    Fixed(Vec<LinearRgb>),
    Ansi16,
    Ansi256,
    Gradient(Vec<GradientStop>),
}

/// A validated immutable palette with a stable matching/cache identity.
#[derive(Debug)]
pub struct Palette {
    storage: PaletteStorage,
    distance_space: PaletteDistanceSpace,
    identity: [u8; 32],
}

impl Palette {
    /// Validates and owns a palette using only bounded storage and work.
    ///
    /// User fixed colors and gradient stops are copied into owned immutable storage. ANSI and
    /// original/monochrome modes retain no palette array. The copied palette storage is charged to
    /// `budget.max_raster_bytes`; definition validation is charged to `budget.max_work_units`.
    pub fn new(
        definition: PaletteDefinition<'_>,
        distance_space: PaletteDistanceSpace,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<Self, PaletteError> {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled.into());
        }

        let storage = match definition {
            PaletteDefinition::Original => PaletteStorage::Original,
            PaletteDefinition::Monochrome {
                foreground,
                background,
            } => PaletteStorage::Monochrome {
                foreground,
                background,
            },
            PaletteDefinition::Fixed(colors) => {
                if colors.is_empty() || colors.len() > MAX_PALETTE_COLORS {
                    return Err(PaletteError::InvalidDefinition);
                }
                validate_storage(colors.len(), core::mem::size_of::<LinearRgb>(), budget)?;
                let mut owned = Vec::new();
                owned
                    .try_reserve_exact(colors.len())
                    .map_err(|_| Error::AllocationFailed)?;
                validate_allocation_capacity::<LinearRgb>(owned.capacity(), budget)?;
                for (index, color) in colors.iter().copied().enumerate() {
                    if index % 64 == 0 && cancellation.is_cancelled() {
                        return Err(Error::Cancelled.into());
                    }
                    owned.push(color);
                }
                PaletteStorage::Fixed(owned)
            }
            PaletteDefinition::Ansi16 => PaletteStorage::Ansi16,
            PaletteDefinition::Ansi256 => PaletteStorage::Ansi256,
            PaletteDefinition::Gradient(stops) => {
                if stops.len() < 2 || stops.len() > MAX_GRADIENT_STOPS {
                    return Err(PaletteError::InvalidDefinition);
                }
                validate_storage(stops.len(), core::mem::size_of::<GradientStop>(), budget)?;
                if stops.first().map(|stop| stop.position) != Some(0.0)
                    || stops.last().map(|stop| stop.position) != Some(1.0)
                {
                    return Err(PaletteError::InvalidDefinition);
                }
                for (index, pair) in stops.windows(2).enumerate() {
                    if index % 64 == 0 && cancellation.is_cancelled() {
                        return Err(Error::Cancelled.into());
                    }
                    if !pair[0].position.is_finite()
                        || !pair[1].position.is_finite()
                        || pair[0].position >= pair[1].position
                    {
                        return Err(PaletteError::InvalidDefinition);
                    }
                }
                let mut owned = Vec::new();
                owned
                    .try_reserve_exact(stops.len())
                    .map_err(|_| Error::AllocationFailed)?;
                validate_allocation_capacity::<GradientStop>(owned.capacity(), budget)?;
                for (index, stop) in stops.iter().copied().enumerate() {
                    if index % 64 == 0 && cancellation.is_cancelled() {
                        return Err(Error::Cancelled.into());
                    }
                    owned.push(stop);
                }
                PaletteStorage::Gradient(owned)
            }
        };

        let identity = identity_for(&storage, distance_space);
        Ok(Self {
            storage,
            distance_space,
            identity,
        })
    }

    /// Returns this palette's explicit matching behavior.
    #[must_use]
    pub const fn kind(&self) -> PaletteKind {
        match &self.storage {
            PaletteStorage::Original => PaletteKind::Original,
            PaletteStorage::Monochrome { .. } => PaletteKind::Monochrome,
            PaletteStorage::Fixed(_) => PaletteKind::Fixed,
            PaletteStorage::Ansi16 => PaletteKind::Ansi16,
            PaletteStorage::Ansi256 => PaletteKind::Ansi256,
            PaletteStorage::Gradient(_) => PaletteKind::Gradient,
        }
    }

    /// Returns the selected distance metric.
    #[must_use]
    pub const fn distance_space(&self) -> PaletteDistanceSpace {
        self.distance_space
    }

    /// Returns the stable SHA-256 identity of palette contents and matching semantics.
    #[must_use]
    pub const fn cache_identity(&self) -> &[u8; 32] {
        &self.identity
    }

    /// Returns a notice for ANSI modes, whose numbered colors may be customized by the terminal.
    #[must_use]
    pub const fn terminal_appearance_notice(&self) -> Option<&'static str> {
        match &self.storage {
            PaletteStorage::Ansi16 | PaletteStorage::Ansi256 => Some(
                "ANSI indices use canonical xterm colors; a terminal may display customized colors.",
            ),
            _ => None,
        }
    }

    /// Resolves a requested color to its nearest allowed color.
    ///
    /// Fixed palette ties select the earliest entry. Gradient interpolation is linear-light RGB;
    /// linear-RGB distance projects exactly onto each segment. Encoded-sRGB distance uses bounded
    /// global branch-and-bound search per segment, detects exact gradient matches, and returns a
    /// result within `1e-6` of the global minimum. Encoded segment ties within the search tolerance
    /// retain the earliest segment. If the tolerance cannot be established within bounded work,
    /// resolution fails with a work-budget error.
    pub fn resolve(
        &self,
        requested: LinearRgb,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<PaletteColor, PaletteError> {
        let work = self.resolution_work_units();
        check_work(work, budget, cancellation)?;
        self.resolve_unbudgeted(requested, cancellation)
    }

    /// Resolves requested foreground/background values using this palette's mode semantics.
    ///
    /// `Original` preserves both sampled values. `Monochrome` always returns its configured pair;
    /// every other mode independently resolves the requested pair.
    /// Pair resolution preflights both searches against the caller budget and a pair-specific
    /// hard cap of twice [`MAX_PALETTE_RESOLUTION_WORK_UNITS`].
    pub fn resolve_pair(
        &self,
        requested_foreground: LinearRgb,
        requested_background: LinearRgb,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<PaletteColorPair, PaletteError> {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled.into());
        }
        if let PaletteStorage::Monochrome {
            foreground,
            background,
        } = &self.storage
        {
            return Ok(PaletteColorPair {
                foreground: PaletteColor {
                    color: *foreground,
                    index: None,
                    gradient_position: None,
                },
                background: PaletteColor {
                    color: *background,
                    index: None,
                    gradient_position: None,
                },
            });
        }
        if matches!(&self.storage, PaletteStorage::Original) {
            return Ok(PaletteColorPair {
                foreground: PaletteColor {
                    color: requested_foreground,
                    index: None,
                    gradient_position: None,
                },
                background: PaletteColor {
                    color: requested_background,
                    index: None,
                    gradient_position: None,
                },
            });
        }
        let work = self
            .resolution_work_units()
            .checked_mul(2)
            .ok_or(Error::SizeOverflow)?;
        check_pair_work(work, budget, cancellation)?;
        let foreground = self.resolve_unbudgeted(requested_foreground, cancellation)?;
        let background = self.resolve_unbudgeted(requested_background, cancellation)?;
        Ok(PaletteColorPair {
            foreground,
            background,
        })
    }

    /// Returns the maximum channel-distance work for resolving one color under this palette.
    #[must_use]
    pub fn resolution_work_units(&self) -> u64 {
        let comparisons = match &self.storage {
            PaletteStorage::Original => return 0,
            PaletteStorage::Monochrome { .. } => 2,
            PaletteStorage::Fixed(colors) => colors.len() as u64,
            PaletteStorage::Ansi16 => 16,
            PaletteStorage::Ansi256 => 256,
            PaletteStorage::Gradient(stops) => {
                let segment_count = stops.len().saturating_sub(1);
                let comparisons_per_segment = match self.distance_space {
                    PaletteDistanceSpace::LinearRgb => 1,
                    PaletteDistanceSpace::EncodedSrgb => {
                        let (initial_samples, subdivisions) =
                            encoded_gradient_search_limits(segment_count);
                        3 + 3
                            * (u64::from(initial_samples)
                                + u64::try_from(subdivisions).unwrap_or(u64::MAX))
                    }
                };
                u64::try_from(segment_count)
                    .unwrap_or(u64::MAX)
                    .saturating_mul(comparisons_per_segment)
            }
        };
        comparisons.saturating_mul(3)
    }
    pub(crate) fn retained_storage_bytes(&self) -> Result<u64, Error> {
        let bytes = match &self.storage {
            PaletteStorage::Fixed(colors) => colors
                .capacity()
                .checked_mul(size_of::<LinearRgb>())
                .ok_or(Error::SizeOverflow)?,
            PaletteStorage::Gradient(stops) => stops
                .capacity()
                .checked_mul(size_of::<GradientStop>())
                .ok_or(Error::SizeOverflow)?,
            PaletteStorage::Original
            | PaletteStorage::Monochrome { .. }
            | PaletteStorage::Ansi16
            | PaletteStorage::Ansi256 => 0,
        };
        u64::try_from(bytes).map_err(|_| Error::SizeOverflow)
    }

    pub(crate) fn storage(&self) -> &PaletteStorage {
        &self.storage
    }

    pub(crate) fn resolve_unbudgeted(
        &self,
        requested: LinearRgb,
        cancellation: &CancellationToken,
    ) -> Result<PaletteColor, PaletteError> {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled.into());
        }
        match &self.storage {
            PaletteStorage::Original => Ok(PaletteColor {
                color: requested,
                index: None,
                gradient_position: None,
            }),
            PaletteStorage::Monochrome {
                foreground,
                background,
            } => {
                let foreground_distance =
                    color_distance(requested, *foreground, self.distance_space);
                let background_distance =
                    color_distance(requested, *background, self.distance_space);
                if foreground_distance <= background_distance {
                    Ok(PaletteColor {
                        color: *foreground,
                        index: None,
                        gradient_position: None,
                    })
                } else {
                    Ok(PaletteColor {
                        color: *background,
                        index: None,
                        gradient_position: None,
                    })
                }
            }
            PaletteStorage::Fixed(colors) => {
                let mut best: Option<(usize, f64)> = None;
                for (index, color) in colors.iter().copied().enumerate() {
                    if index % 64 == 0 && cancellation.is_cancelled() {
                        return Err(Error::Cancelled.into());
                    }
                    let distance = color_distance(requested, color, self.distance_space);
                    if best.is_none_or(|(_, best_distance)| distance < best_distance) {
                        best = Some((index, distance));
                    }
                }
                let (index, _) = best.ok_or(PaletteError::InvalidDefinition)?;
                Ok(PaletteColor {
                    color: colors[index],
                    index: Some(u16::try_from(index).map_err(|_| Error::SizeOverflow)?),
                    gradient_position: None,
                })
            }
            PaletteStorage::Ansi16 | PaletteStorage::Ansi256 => {
                let palette_len = if matches!(&self.storage, PaletteStorage::Ansi16) {
                    16
                } else {
                    256
                };
                let mut best: Option<(usize, f64, LinearRgb)> = None;
                for index in 0..palette_len {
                    if index % 64 == 0 && cancellation.is_cancelled() {
                        return Err(Error::Cancelled.into());
                    }
                    let color = ansi_color(index)?;
                    let distance = color_distance(requested, color, self.distance_space);
                    if best.is_none_or(|(_, best_distance, _)| distance < best_distance) {
                        best = Some((index, distance, color));
                    }
                }
                let (index, _, color) = best.ok_or(PaletteError::InvalidDefinition)?;
                Ok(PaletteColor {
                    color,
                    index: Some(u16::try_from(index).map_err(|_| Error::SizeOverflow)?),
                    gradient_position: None,
                })
            }
            PaletteStorage::Gradient(stops) => {
                let mut best: Option<(usize, f64, f64, LinearRgb)> = None;
                let (initial_samples, max_subdivisions) =
                    if self.distance_space == PaletteDistanceSpace::EncodedSrgb {
                        encoded_gradient_search_limits(stops.len() - 1)
                    } else {
                        (0, 0)
                    };
                for segment in 0..stops.len() - 1 {
                    if cancellation.is_cancelled() {
                        return Err(Error::Cancelled.into());
                    }
                    let (parameter, color, distance) = closest_gradient_segment(
                        requested,
                        stops[segment],
                        stops[segment + 1],
                        self.distance_space,
                        cancellation,
                        initial_samples,
                        max_subdivisions,
                    )?;
                    let replace =
                        best.is_none_or(|(_, best_distance, _, _)| match self.distance_space {
                            PaletteDistanceSpace::LinearRgb => distance < best_distance,
                            PaletteDistanceSpace::EncodedSrgb => {
                                distance + ENCODED_GRADIENT_SEGMENT_TIE_TOLERANCE < best_distance
                            }
                        });
                    if replace {
                        let position = stops[segment].position
                            + (stops[segment + 1].position - stops[segment].position) * parameter;
                        best = Some((segment, distance, position, color));
                    }
                }
                let (_, _, position, color) = best.ok_or(PaletteError::InvalidDefinition)?;
                Ok(PaletteColor {
                    color,
                    index: None,
                    gradient_position: Some(position),
                })
            }
        }
    }

    pub(crate) fn indexed_color(&self, index: usize) -> Option<PaletteColor> {
        let color = match &self.storage {
            PaletteStorage::Fixed(colors) => *colors.get(index)?,
            PaletteStorage::Ansi16 if index < 16 => ansi_color(index).ok()?,
            PaletteStorage::Ansi256 if index < 256 => ansi_color(index).ok()?,
            _ => return None,
        };
        Some(PaletteColor {
            color,
            index: Some(u16::try_from(index).ok()?),
            gradient_position: None,
        })
    }

    pub(crate) fn candidate_count(&self) -> Option<usize> {
        match &self.storage {
            PaletteStorage::Fixed(colors) => Some(colors.len()),
            PaletteStorage::Ansi16 => Some(16),
            PaletteStorage::Ansi256 => Some(256),
            _ => None,
        }
    }

    pub(crate) fn monochrome_pair(&self) -> Option<(LinearRgb, LinearRgb)> {
        match &self.storage {
            PaletteStorage::Monochrome {
                foreground,
                background,
            } => Some((*foreground, *background)),
            _ => None,
        }
    }

    pub(crate) fn is_original(&self) -> bool {
        matches!(&self.storage, PaletteStorage::Original)
    }
}

pub(crate) fn gradient_color(
    first: GradientStop,
    second: GradientStop,
    parameter: f64,
) -> Result<(LinearRgb, f64), PaletteError> {
    let t = parameter.clamp(0.0, 1.0);
    let first_channels = rgb_channels(first.color);
    let second_channels = rgb_channels(second.color);
    let channels = [
        first_channels[0] + (second_channels[0] - first_channels[0]) * t,
        first_channels[1] + (second_channels[1] - first_channels[1]) * t,
        first_channels[2] + (second_channels[2] - first_channels[2]) * t,
    ];
    let color = rgb_from_channels(channels)?;
    let position = first.position + (second.position - first.position) * t;
    Ok((color, position))
}

fn validate_storage(
    count: usize,
    element_size: usize,
    budget: &Budget,
) -> Result<(), PaletteError> {
    let bytes = count.checked_mul(element_size).ok_or(Error::SizeOverflow)?;
    let bytes_u64 = u64::try_from(bytes).map_err(|_| Error::SizeOverflow)?;
    let _layout = isize::try_from(bytes).map_err(|_| Error::SizeOverflow)?;
    if bytes_u64 > budget.max_raster_bytes() {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes).into());
    }
    if u64::try_from(count).map_err(|_| Error::SizeOverflow)? > budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits).into());
    }
    Ok(())
}

fn validate_allocation_capacity<T>(capacity: usize, budget: &Budget) -> Result<(), PaletteError> {
    let bytes = capacity
        .checked_mul(core::mem::size_of::<T>())
        .ok_or(Error::SizeOverflow)?;
    let bytes_u64 = u64::try_from(bytes).map_err(|_| Error::SizeOverflow)?;
    let _layout = isize::try_from(bytes).map_err(|_| Error::SizeOverflow)?;
    let hard_storage_limit = (MAX_PALETTE_COLORS as u64)
        .checked_mul(core::mem::size_of::<LinearRgb>() as u64)
        .ok_or(Error::SizeOverflow)?;
    if bytes_u64 > budget.max_raster_bytes() || bytes_u64 > hard_storage_limit {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes).into());
    }
    Ok(())
}

fn check_work(
    work: u64,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<(), PaletteError> {
    check_work_limit(
        work,
        budget,
        cancellation,
        MAX_PALETTE_RESOLUTION_WORK_UNITS,
    )
}

fn check_pair_work(
    work: u64,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<(), PaletteError> {
    check_work_limit(
        work,
        budget,
        cancellation,
        MAX_PALETTE_PAIR_RESOLUTION_WORK_UNITS,
    )
}

fn check_work_limit(
    work: u64,
    budget: &Budget,
    cancellation: &CancellationToken,
    hard_limit: u64,
) -> Result<(), PaletteError> {
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled.into());
    }
    if work > hard_limit || work > budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits).into());
    }
    Ok(())
}

fn color_distance(left: LinearRgb, right: LinearRgb, space: PaletteDistanceSpace) -> f64 {
    let left = coordinates(left, space);
    let right = coordinates(right, space);
    (left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2) + (left[2] - right[2]).powi(2)
}

fn coordinates(color: LinearRgb, space: PaletteDistanceSpace) -> [f64; 3] {
    match space {
        PaletteDistanceSpace::LinearRgb => rgb_channels(color),
        PaletteDistanceSpace::EncodedSrgb => {
            let encoded = encode_srgb(color);
            [encoded.red(), encoded.green(), encoded.blue()]
        }
    }
}

fn encoded_gradient_search_limits(segment_count: usize) -> (u32, usize) {
    let segment_count = u64::try_from(segment_count).unwrap_or(u64::MAX);
    if segment_count == 0 {
        return (0, 0);
    }
    let comparisons_per_segment = MAX_PALETTE_RESOLUTION_WORK_UNITS / segment_count / 3;
    let available_subdivisions = comparisons_per_segment.saturating_sub(3) / 3;
    let initial_samples = u32::try_from(available_subdivisions / 2)
        .unwrap_or(u32::MAX)
        .min(ENCODED_GRADIENT_INITIAL_SAMPLES);
    let subdivisions = usize::try_from(available_subdivisions - u64::from(initial_samples))
        .unwrap_or(usize::MAX)
        .min(ENCODED_GRADIENT_MAX_SUBDIVISIONS);
    (initial_samples, subdivisions)
}

#[derive(Clone, Copy)]
struct EncodedGradientInterval {
    start: f64,
    end: f64,
    lower_bound: f64,
}

const EMPTY_ENCODED_GRADIENT_INTERVAL: EncodedGradientInterval = EncodedGradientInterval {
    start: 0.0,
    end: 0.0,
    lower_bound: 0.0,
};

fn closest_gradient_segment(
    requested: LinearRgb,
    first: GradientStop,
    second: GradientStop,
    space: PaletteDistanceSpace,
    cancellation: &CancellationToken,
    initial_samples: u32,
    max_subdivisions: usize,
) -> Result<(f64, LinearRgb, f64), PaletteError> {
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled.into());
    }
    if space == PaletteDistanceSpace::LinearRgb {
        let start = rgb_channels(first.color);
        let end = rgb_channels(second.color);
        let target = rgb_channels(requested);
        let direction = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
        let denominator = dot(direction, direction);
        let parameter = if denominator == 0.0 {
            0.0
        } else {
            dot(
                [
                    target[0] - start[0],
                    target[1] - start[1],
                    target[2] - start[2],
                ],
                direction,
            ) / denominator
        }
        .clamp(0.0, 1.0);
        let (color, _) = gradient_color(first, second, parameter)?;
        return Ok((parameter, color, color_distance(requested, color, space)));
    }

    if let Some((parameter, color)) = exact_gradient_match(requested, first, second)? {
        return Ok((parameter, color, 0.0));
    }

    let target = coordinates(requested, PaletteDistanceSpace::EncodedSrgb);
    let mut best = (0.0, first.color, encoded_distance(target, first.color));
    let final_distance = encoded_distance(target, second.color);
    if final_distance < best.2 {
        best = (1.0, second.color, final_distance);
    }

    for sample_index in 1..=initial_samples {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled.into());
        }
        let parameter = f64::from(sample_index) / (f64::from(initial_samples) + 1.0);
        let (color, _) = gradient_color(first, second, parameter)?;
        let distance = encoded_distance(target, color);
        if distance < best.2 || (distance == best.2 && parameter < best.0) {
            best = (parameter, color, distance);
        }
    }

    let root = EncodedGradientInterval {
        start: 0.0,
        end: 1.0,
        lower_bound: encoded_interval_lower_bound(target, first.color, second.color),
    };
    let mut intervals = [EMPTY_ENCODED_GRADIENT_INTERVAL; ENCODED_GRADIENT_MAX_SUBDIVISIONS + 1];
    let mut interval_count = 0;
    if root.lower_bound < best.2 - ENCODED_GRADIENT_SEARCH_TOLERANCE {
        push_encoded_interval(&mut intervals, &mut interval_count, root)?;
    }

    let mut subdivisions = 0;
    while let Some(interval) = pop_encoded_interval(&mut intervals, &mut interval_count) {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled.into());
        }
        if interval.lower_bound >= best.2 - ENCODED_GRADIENT_SEARCH_TOLERANCE {
            break;
        }
        if subdivisions == max_subdivisions {
            return Err(Error::BudgetExceeded(BudgetKind::WorkUnits).into());
        }

        let (start_color, _) = gradient_color(first, second, interval.start)?;
        let (end_color, _) = gradient_color(first, second, interval.end)?;
        let midpoint = (interval.start + interval.end) * 0.5;
        let (color, _) = gradient_color(first, second, midpoint)?;
        let distance = encoded_distance(target, color);
        if distance < best.2 || (distance == best.2 && midpoint < best.0) {
            best = (midpoint, color, distance);
        }

        let left = EncodedGradientInterval {
            start: interval.start,
            end: midpoint,
            lower_bound: encoded_interval_lower_bound(target, start_color, color),
        };
        let right = EncodedGradientInterval {
            start: midpoint,
            end: interval.end,
            lower_bound: encoded_interval_lower_bound(target, color, end_color),
        };
        for child in [left, right] {
            if child.lower_bound < best.2 - ENCODED_GRADIENT_SEARCH_TOLERANCE {
                push_encoded_interval(&mut intervals, &mut interval_count, child)?;
            }
        }
        subdivisions += 1;
    }
    Ok(best)
}

fn exact_gradient_match(
    requested: LinearRgb,
    first: GradientStop,
    second: GradientStop,
) -> Result<Option<(f64, LinearRgb)>, PaletteError> {
    let start = rgb_channels(first.color);
    let end = rgb_channels(second.color);
    let target = rgb_channels(requested);
    let direction = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
    let mut axis = 0;
    if direction[1].abs() > direction[axis].abs() {
        axis = 1;
    }
    if direction[2].abs() > direction[axis].abs() {
        axis = 2;
    }
    if direction[axis] == 0.0 {
        return Ok(if requested == first.color {
            Some((0.0, first.color))
        } else {
            None
        });
    }

    let parameter = (target[axis] - start[axis]) / direction[axis];
    if !(0.0..=1.0).contains(&parameter) {
        return Ok(None);
    }
    let (color, _) = gradient_color(first, second, parameter)?;
    Ok(if color == requested {
        Some((parameter, color))
    } else {
        None
    })
}

fn encoded_distance(target: [f64; 3], color: LinearRgb) -> f64 {
    squared_distance(
        target,
        coordinates(color, PaletteDistanceSpace::EncodedSrgb),
    )
}

fn squared_distance(left: [f64; 3], right: [f64; 3]) -> f64 {
    (left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2) + (left[2] - right[2]).powi(2)
}

fn encoded_interval_lower_bound(target: [f64; 3], first: LinearRgb, second: LinearRgb) -> f64 {
    let first = coordinates(first, PaletteDistanceSpace::EncodedSrgb);
    let second = coordinates(second, PaletteDistanceSpace::EncodedSrgb);
    let mut lower_bound = 0.0;
    for channel in 0..3 {
        let minimum = first[channel].min(second[channel]);
        let maximum = first[channel].max(second[channel]);
        let difference = if target[channel] < minimum {
            minimum - target[channel]
        } else if target[channel] > maximum {
            target[channel] - maximum
        } else {
            0.0
        };
        lower_bound += difference * difference;
    }
    (lower_bound - ENCODED_GRADIENT_BOUND_ROUNDING_GUARD).max(0.0)
}

fn push_encoded_interval(
    intervals: &mut [EncodedGradientInterval; ENCODED_GRADIENT_MAX_SUBDIVISIONS + 1],
    interval_count: &mut usize,
    interval: EncodedGradientInterval,
) -> Result<(), PaletteError> {
    if *interval_count == intervals.len() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits).into());
    }
    let mut child = *interval_count;
    intervals[child] = interval;
    *interval_count += 1;
    while child > 0 {
        let parent = (child - 1) / 2;
        if encoded_interval_precedes(intervals[parent], intervals[child]) {
            break;
        }
        intervals.swap(parent, child);
        child = parent;
    }
    Ok(())
}

fn pop_encoded_interval(
    intervals: &mut [EncodedGradientInterval; ENCODED_GRADIENT_MAX_SUBDIVISIONS + 1],
    interval_count: &mut usize,
) -> Option<EncodedGradientInterval> {
    if *interval_count == 0 {
        return None;
    }
    let minimum = intervals[0];
    *interval_count -= 1;
    if *interval_count > 0 {
        intervals[0] = intervals[*interval_count];
        let mut parent = 0;
        loop {
            let left = parent * 2 + 1;
            if left >= *interval_count {
                break;
            }
            let right = left + 1;
            let child = if right < *interval_count
                && encoded_interval_precedes(intervals[right], intervals[left])
            {
                right
            } else {
                left
            };
            if encoded_interval_precedes(intervals[parent], intervals[child]) {
                break;
            }
            intervals.swap(parent, child);
            parent = child;
        }
    }
    Some(minimum)
}

fn encoded_interval_precedes(
    left: EncodedGradientInterval,
    right: EncodedGradientInterval,
) -> bool {
    left.lower_bound < right.lower_bound
        || (left.lower_bound == right.lower_bound && left.start < right.start)
}

fn ansi_color(index: usize) -> Result<LinearRgb, PaletteError> {
    if index >= 256 {
        return Err(PaletteError::InvalidDefinition);
    }
    let encoded = if index < 16 {
        ansi16_encoded(index)?
    } else if index < 232 {
        let cube_index = index - 16;
        let levels = [0_u8, 95, 135, 175, 215, 255];
        [
            levels[cube_index / 36],
            levels[(cube_index / 6) % 6],
            levels[cube_index % 6],
        ]
    } else {
        let gray = 8 + (index - 232) * 10;
        [gray as u8, gray as u8, gray as u8]
    };
    let encoded = EncodedSrgb::new(
        f64::from(encoded[0]) / 255.0,
        f64::from(encoded[1]) / 255.0,
        f64::from(encoded[2]) / 255.0,
    )
    .map_err(|_| PaletteError::InvalidDefinition)?;
    Ok(decode_srgb(encoded))
}

fn ansi16_encoded(index: usize) -> Result<[u8; 3], PaletteError> {
    match index {
        0 => Ok([0, 0, 0]),
        1 => Ok([205, 0, 0]),
        2 => Ok([0, 205, 0]),
        3 => Ok([205, 205, 0]),
        4 => Ok([0, 0, 238]),
        5 => Ok([205, 0, 205]),
        6 => Ok([0, 205, 205]),
        7 => Ok([229, 229, 229]),
        8 => Ok([127, 127, 127]),
        9 => Ok([255, 0, 0]),
        10 => Ok([0, 255, 0]),
        11 => Ok([255, 255, 0]),
        12 => Ok([92, 92, 255]),
        13 => Ok([255, 0, 255]),
        14 => Ok([0, 255, 255]),
        15 => Ok([255, 255, 255]),
        _ => Err(PaletteError::InvalidDefinition),
    }
}

fn identity_for(storage: &PaletteStorage, distance_space: PaletteDistanceSpace) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"asciiwall-palette");
    hasher.update(PALETTE_ALGORITHM_VERSION.to_le_bytes());
    hasher.update([match distance_space {
        PaletteDistanceSpace::LinearRgb => 0,
        PaletteDistanceSpace::EncodedSrgb => 1,
    }]);
    match storage {
        PaletteStorage::Original => hasher.update([0]),
        PaletteStorage::Monochrome {
            foreground,
            background,
        } => {
            hasher.update([1]);
            hash_color(&mut hasher, *foreground);
            hash_color(&mut hasher, *background);
        }
        PaletteStorage::Fixed(colors) => {
            hasher.update([2]);
            hasher.update((colors.len() as u64).to_le_bytes());
            for color in colors {
                hash_color(&mut hasher, *color);
            }
        }
        PaletteStorage::Ansi16 => hasher.update([3]),
        PaletteStorage::Ansi256 => hasher.update([4]),
        PaletteStorage::Gradient(stops) => {
            hasher.update([5]);
            hasher.update((stops.len() as u64).to_le_bytes());
            for stop in stops {
                hasher.update(stop.position.to_bits().to_le_bytes());
                hash_color(&mut hasher, stop.color);
            }
        }
    }
    hasher.finalize().into()
}

fn hash_color(hasher: &mut Sha256, color: LinearRgb) {
    for channel in rgb_channels(color) {
        hasher.update(channel.to_bits().to_le_bytes());
    }
}

pub(crate) fn rgb_channels(color: LinearRgb) -> [f64; 3] {
    [color.red(), color.green(), color.blue()]
}

pub(crate) fn rgb_from_channels(channels: [f64; 3]) -> Result<LinearRgb, PaletteError> {
    LinearRgb::new(channels[0], channels[1], channels[2])
        .map_err(|_| PaletteError::InvalidDefinition)
}

pub(crate) fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}
