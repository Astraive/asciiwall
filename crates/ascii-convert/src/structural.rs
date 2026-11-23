//! Bounded patch features and a deterministic coverage/structure candidate shortlist.
//!
//! Patch samples use the same normalized foreground/background coverage as density matching.
//! Gradient variation and edge orientation are dimensionless and bounded, and candidate ranking
//! allocates only the caller-owned, reusable shortlist scratch.

use core::{cmp::Ordering, mem::size_of};

use ascii_core::{Budget, BudgetKind, CancellationToken, Error, MalformedReason};
use sha2::{Digest, Sha256};

use crate::{
    font::{GlyphRecord, GlyphTable},
    glyphs::{EdgeOrientation, GradientDescriptor, edge_orientation_from_samples},
    sampling::{PatchDimensions, SamplingIdentity},
};

/// Maximum glyph candidates accepted by one structural shortlist.
pub const MAX_STRUCTURAL_CANDIDATES: usize = 4096;

const DESCRIPTOR_VERSION: u32 = 2;
const CANCELLATION_TILE: u64 = 256;
// Per sample: one mean read, up to three gradient reads, and five orientation reads.
const FEATURE_WORK_UNITS_PER_SAMPLE: u64 = 9;

/// Normalized weights for tone, shape-gradient, and edge-orientation distances.
///
/// Each weight is in `[0, 1]`; their sum must be one (within `1e-9`). Values are canonicalized by
/// that sum so the combined candidate score remains in `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StructuralWeights {
    tone: f64,
    shape: f64,
    edge: f64,
}

impl StructuralWeights {
    /// Creates normalized tone, shape, and edge weights.
    pub fn new(tone: f64, shape: f64, edge: f64) -> Result<Self, Error> {
        let sum = tone + shape + edge;
        if !tone.is_finite()
            || !shape.is_finite()
            || !edge.is_finite()
            || !(0.0..=1.0).contains(&tone)
            || !(0.0..=1.0).contains(&shape)
            || !(0.0..=1.0).contains(&edge)
            || !sum.is_finite()
            || (sum - 1.0).abs() > 1.0e-9
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(Self {
            tone: tone / sum,
            shape: shape / sum,
            edge: edge / sum,
        })
    }

    /// Returns the normalized tone-distance weight.
    #[must_use]
    pub const fn tone(self) -> f64 {
        self.tone
    }

    /// Returns the normalized gradient-shape weight.
    #[must_use]
    pub const fn shape(self) -> f64 {
        self.shape
    }

    /// Returns the normalized edge-orientation weight.
    #[must_use]
    pub const fn edge(self) -> f64 {
        self.edge
    }
}

/// Validated candidate-count and feature-weight controls for one shortlist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShortlistOptions {
    max_candidates: usize,
    weights: StructuralWeights,
}

impl ShortlistOptions {
    /// Creates controls with a maximum shortlist length in `1..=4096`.
    pub fn new(max_candidates: usize, weights: StructuralWeights) -> Result<Self, Error> {
        if max_candidates == 0 || max_candidates > MAX_STRUCTURAL_CANDIDATES {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(Self {
            max_candidates,
            weights,
        })
    }

    /// Returns the maximum number of returned glyph candidates.
    #[must_use]
    pub const fn max_candidates(self) -> usize {
        self.max_candidates
    }

    /// Returns the normalized score weights.
    #[must_use]
    pub const fn weights(self) -> StructuralWeights {
        self.weights
    }
}

/// Coverage, dimensionless gradient variation, and edge orientation for one sampled patch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StructuralDescriptor<'a> {
    samples: &'a [f32],
    dimensions: PatchDimensions,
    mean_coverage: f32,
    gradients: GradientDescriptor,
    edge_orientation: EdgeOrientation,
    foreground_luminance: f64,
    background_luminance: f64,
}

impl<'a> StructuralDescriptor<'a> {
    /// Describes normalized linear-light patch luminance using the density-match foreground and
    /// background luminances. Samples and both color luminances must be in `[0, 1]`.
    ///
    /// Samples are row-major and contain exactly `width * height` values. The returned descriptor
    /// borrows this slice so shortlist scoring can resample it to glyph-cell geometry without a copy.
    /// Tone is mean foreground coverage. X/Y shape are the mean per-row/per-column total
    /// variations, clamped to `[0, 1]`. Edge orientation uses the same finite-difference bins and
    /// boundary convention as immutable glyph masks. The operation allocates no memory, checks its
    /// work ceiling before scanning, and polls cancellation at most every 256 samples.
    pub fn from_luminance(
        samples: &'a [f32],
        dimensions: PatchDimensions,
        foreground_luminance: f64,
        background_luminance: f64,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<Self, Error> {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let width = usize::try_from(dimensions.width()).map_err(|_| Error::SizeOverflow)?;
        let height = usize::try_from(dimensions.height()).map_err(|_| Error::SizeOverflow)?;
        let sample_count = width.checked_mul(height).ok_or(Error::SizeOverflow)?;
        if samples.len() != sample_count
            || sample_count == 0
            || !foreground_luminance.is_finite()
            || !background_luminance.is_finite()
            || !(0.0..=1.0).contains(&foreground_luminance)
            || !(0.0..=1.0).contains(&background_luminance)
            || foreground_luminance == background_luminance
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let work_bound = u64::try_from(sample_count)
            .map_err(|_| Error::SizeOverflow)?
            .checked_mul(FEATURE_WORK_UNITS_PER_SAMPLE)
            .ok_or(Error::SizeOverflow)?;
        if work_bound > budget.max_work_units() {
            return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
        }

        let mut coverage_sum = 0.0_f64;
        for (index, &sample) in samples.iter().enumerate() {
            if (index as u64).is_multiple_of(CANCELLATION_TILE) && cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if !sample.is_finite() || !(0.0..=1.0).contains(&sample) {
                return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
            }
            coverage_sum += normalized_coverage(
                f64::from(sample),
                foreground_luminance,
                background_luminance,
            );
        }
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let mut x_variation = 0.0_f64;
        let mut y_variation = 0.0_f64;
        for index in 0..sample_count {
            if (index as u64).is_multiple_of(CANCELLATION_TILE) && cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let x = index % width;
            let y = index / width;
            let current = normalized_coverage(
                f64::from(samples[index]),
                foreground_luminance,
                background_luminance,
            );
            if x + 1 < width {
                let right = normalized_coverage(
                    f64::from(samples[index + 1]),
                    foreground_luminance,
                    background_luminance,
                );
                x_variation += (right - current).abs();
            }
            if y + 1 < height {
                let below_index = index.checked_add(width).ok_or(Error::SizeOverflow)?;
                let below = normalized_coverage(
                    f64::from(samples[below_index]),
                    foreground_luminance,
                    background_luminance,
                );
                y_variation += (below - current).abs();
            }
        }
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let gradients = GradientDescriptor::new(
            (x_variation / height as f64).clamp(0.0, 1.0) as f32,
            (y_variation / width as f64).clamp(0.0, 1.0) as f32,
        );
        let edge_orientation = edge_orientation_from_samples(
            width,
            height,
            |index| {
                Ok(normalized_coverage(
                    f64::from(samples[index]),
                    foreground_luminance,
                    background_luminance,
                ))
            },
            || cancellation.is_cancelled(),
        )?;
        Ok(Self {
            samples,
            dimensions,
            mean_coverage: (coverage_sum / sample_count as f64) as f32,
            gradients,
            edge_orientation,
            foreground_luminance,
            background_luminance,
        })
    }

    /// Returns the patch dimensions used for this descriptor.
    #[must_use]
    pub const fn dimensions(self) -> PatchDimensions {
        self.dimensions
    }

    /// Returns native-resolution mean coverage normalized against the configured foreground/background range.
    #[must_use]
    pub const fn mean_coverage(self) -> f32 {
        self.mean_coverage
    }

    /// Returns native-resolution X/Y total-variation features.
    #[must_use]
    pub const fn gradients(self) -> GradientDescriptor {
        self.gradients
    }

    /// Returns the native-resolution normalized four-bin edge-orientation distribution.
    #[must_use]
    pub const fn edge_orientation(self) -> EdgeOrientation {
        self.edge_orientation
    }
}

/// One glyph's normalized distance from a patch, with stable identity tie-break fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StructuralCandidate {
    table_position: u32,
    glyph_index: u16,
    character: char,
    score: f64,
}

impl StructuralCandidate {
    /// Returns the glyph-table position expected by frame construction.
    #[must_use]
    pub const fn table_position(self) -> u32 {
        self.table_position
    }

    /// Returns the font's glyph index used to break equal scores.
    #[must_use]
    pub const fn glyph_index(self) -> u16 {
        self.glyph_index
    }

    /// Returns the glyph scalar used to break equal scores.
    #[must_use]
    pub const fn character(self) -> char {
        self.character
    }

    /// Returns the normalized weighted score in `[0, 1]`.
    #[must_use]
    pub const fn score(self) -> f64 {
        self.score
    }
}

/// Semantic cache identity for the bounded structural candidate result.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StructuralIdentity([u8; 32]);

impl StructuralIdentity {
    /// Returns the digest bytes in canonical SHA-256 order.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Reusable bounded storage for structural shortlist ranking.
///
/// The buffer retains at most `max_candidates` scored records (never more than 4096). A returned
/// shortlist borrows this scratch and remains valid only until the next mutable scratch operation.
#[derive(Debug, Default)]
pub struct StructuralScratch {
    candidates: Vec<StructuralCandidate>,
}

impl StructuralScratch {
    /// Returns the ranked candidates and semantic identity, or a typed error with no partial list.
    ///
    /// `budget.max_raster_bytes()` bounds retained shortlist scratch bytes and
    /// `budget.max_work_units()` bounds scoring and heap operations. Results are ordered by score,
    /// then font glyph index, Unicode scalar, and glyph-table position.
    pub fn shortlist<'a>(
        &'a mut self,
        descriptor: &StructuralDescriptor<'_>,
        glyphs: &GlyphTable,
        sampling_identity: SamplingIdentity,
        options: ShortlistOptions,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<StructuralShortlist<'a>, Error> {
        self.shortlist_with_poll(
            descriptor,
            glyphs,
            sampling_identity,
            options,
            budget,
            || cancellation.is_cancelled(),
        )
    }

    fn shortlist_with_poll<'a>(
        &'a mut self,
        descriptor: &StructuralDescriptor<'_>,
        glyphs: &GlyphTable,
        sampling_identity: SamplingIdentity,
        options: ShortlistOptions,
        budget: &Budget,
        mut is_cancelled: impl FnMut() -> bool,
    ) -> Result<StructuralShortlist<'a>, Error> {
        self.candidates.clear();
        let identity = self.populate(
            descriptor,
            glyphs,
            sampling_identity,
            options,
            budget,
            &mut is_cancelled,
        );
        match identity {
            Ok(identity) => Ok(StructuralShortlist {
                identity,
                candidates: &self.candidates,
            }),
            Err(error) => {
                self.candidates.clear();
                Err(error)
            }
        }
    }

    fn populate(
        &mut self,
        descriptor: &StructuralDescriptor<'_>,
        glyphs: &GlyphTable,
        sampling_identity: SamplingIdentity,
        options: ShortlistOptions,
        budget: &Budget,
        is_cancelled: &mut impl FnMut() -> bool,
    ) -> Result<StructuralIdentity, Error> {
        if is_cancelled() {
            return Err(Error::Cancelled);
        }
        let glyph_records = glyphs.glyphs();
        if glyph_records.is_empty()
            || options.max_candidates > glyph_records.len()
            || options.max_candidates > MAX_STRUCTURAL_CANDIDATES
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let target_width = usize::try_from(glyphs.cell_width()).map_err(|_| Error::SizeOverflow)?;
        let target_height =
            usize::try_from(glyphs.cell_height()).map_err(|_| Error::SizeOverflow)?;
        let feature_work_bound = common_feature_work_bound(target_width, target_height)?;
        let work_bound = shortlist_work_bound(glyph_records.len(), options.max_candidates)?
            .checked_add(feature_work_bound)
            .ok_or(Error::SizeOverflow)?;
        if work_bound > budget.max_work_units() {
            return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
        }
        let needed_bytes = u64::try_from(options.max_candidates)
            .map_err(|_| Error::SizeOverflow)?
            .checked_mul(
                u64::try_from(size_of::<StructuralCandidate>()).map_err(|_| Error::SizeOverflow)?,
            )
            .ok_or(Error::SizeOverflow)?;
        let _allocation_layout = isize::try_from(needed_bytes).map_err(|_| Error::SizeOverflow)?;
        let current_bytes = u64::try_from(self.candidates.capacity())
            .map_err(|_| Error::SizeOverflow)?
            .checked_mul(
                u64::try_from(size_of::<StructuralCandidate>()).map_err(|_| Error::SizeOverflow)?,
            )
            .ok_or(Error::SizeOverflow)?;
        if current_bytes.max(needed_bytes) > budget.max_raster_bytes() {
            return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
        }
        if self.candidates.capacity() < options.max_candidates {
            self.candidates
                .try_reserve_exact(options.max_candidates)
                .map_err(|_| Error::AllocationFailed)?;
        }
        let allocated_bytes = u64::try_from(self.candidates.capacity())
            .map_err(|_| Error::SizeOverflow)?
            .checked_mul(
                u64::try_from(size_of::<StructuralCandidate>()).map_err(|_| Error::SizeOverflow)?,
            )
            .ok_or(Error::SizeOverflow)?;
        if allocated_bytes > budget.max_raster_bytes() {
            self.candidates = Vec::new();
            return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
        }
        if is_cancelled() {
            return Err(Error::Cancelled);
        }

        let target_features =
            common_patch_features(descriptor, target_width, target_height, is_cancelled)?;
        let mut work = 0_u64;
        for (table_position, glyph) in glyph_records.iter().enumerate() {
            charge_shortlist_work(&mut work, budget, is_cancelled)?;
            let score = structural_score(target_features, glyph, options.weights)?;
            let candidate = StructuralCandidate {
                table_position: u32::try_from(table_position).map_err(|_| Error::SizeOverflow)?,
                glyph_index: glyph.glyph_index(),
                character: glyph.character(),
                score,
            };
            if self.candidates.len() < options.max_candidates {
                self.candidates.push(candidate);
                let child = self
                    .candidates
                    .len()
                    .checked_sub(1)
                    .ok_or(Error::SizeOverflow)?;
                sift_up(&mut self.candidates, child, &mut work, budget, is_cancelled)?;
            } else {
                charge_shortlist_work(&mut work, budget, is_cancelled)?;
                if candidate_order(&candidate, &self.candidates[0]) == Ordering::Less {
                    self.candidates[0] = candidate;
                    let len = self.candidates.len();
                    sift_down(
                        &mut self.candidates,
                        0,
                        len,
                        &mut work,
                        budget,
                        is_cancelled,
                    )?;
                }
            }
        }

        for end in (1..self.candidates.len()).rev() {
            charge_shortlist_work(&mut work, budget, is_cancelled)?;
            self.candidates.swap(0, end);
            sift_down(
                &mut self.candidates,
                0,
                end,
                &mut work,
                budget,
                is_cancelled,
            )?;
        }
        if is_cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(structural_identity(
            descriptor,
            target_features,
            glyphs,
            sampling_identity,
            options,
        ))
    }
}

/// Completed shortlist borrowed from its reusable scratch owner.
#[derive(Clone, Copy, Debug)]
pub struct StructuralShortlist<'a> {
    identity: StructuralIdentity,
    candidates: &'a [StructuralCandidate],
}

impl<'a> StructuralShortlist<'a> {
    /// Returns the semantic identity for this patch, glyph table, and shortlist configuration.
    #[must_use]
    pub const fn identity(&self) -> StructuralIdentity {
        self.identity
    }

    /// Returns candidates in stable ascending score order.
    #[must_use]
    pub const fn candidates(&self) -> &'a [StructuralCandidate] {
        self.candidates
    }
}

fn normalized_coverage(sample: f64, foreground: f64, background: f64) -> f64 {
    ((sample - background) / (foreground - background)).clamp(0.0, 1.0)
}

#[derive(Clone, Copy)]
struct PatchFeatures {
    mean_coverage: f32,
    gradients: GradientDescriptor,
    edge_orientation: EdgeOrientation,
}

fn common_feature_work_bound(width: usize, height: usize) -> Result<u64, Error> {
    if width == 0 || height == 0 {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }
    let sample_count = width.checked_mul(height).ok_or(Error::SizeOverflow)?;
    u64::try_from(sample_count)
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(FEATURE_WORK_UNITS_PER_SAMPLE)
        .ok_or(Error::SizeOverflow)
}

fn common_patch_features(
    descriptor: &StructuralDescriptor<'_>,
    target_width: usize,
    target_height: usize,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<PatchFeatures, Error> {
    if target_width == 0 || target_height == 0 {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }
    let target_sample_count = target_width
        .checked_mul(target_height)
        .ok_or(Error::SizeOverflow)?;
    let source_width =
        usize::try_from(descriptor.dimensions.width()).map_err(|_| Error::SizeOverflow)?;
    let source_height =
        usize::try_from(descriptor.dimensions.height()).map_err(|_| Error::SizeOverflow)?;
    let mut coverage_sum = 0.0_f64;
    for index in 0..target_sample_count {
        if (index as u64).is_multiple_of(CANCELLATION_TILE) && is_cancelled() {
            return Err(Error::Cancelled);
        }
        let x = index % target_width;
        let y = index / target_width;
        coverage_sum += common_sample_coverage(
            descriptor,
            source_width,
            source_height,
            target_width,
            target_height,
            x,
            y,
        )?;
    }

    let mut x_variation = 0.0_f64;
    let mut y_variation = 0.0_f64;
    for index in 0..target_sample_count {
        if (index as u64).is_multiple_of(CANCELLATION_TILE) && is_cancelled() {
            return Err(Error::Cancelled);
        }
        let x = index % target_width;
        let y = index / target_width;
        let current = common_sample_coverage(
            descriptor,
            source_width,
            source_height,
            target_width,
            target_height,
            x,
            y,
        )?;
        if x + 1 < target_width {
            let right = common_sample_coverage(
                descriptor,
                source_width,
                source_height,
                target_width,
                target_height,
                x + 1,
                y,
            )?;
            x_variation += (right - current).abs();
        }
        if y + 1 < target_height {
            let below = common_sample_coverage(
                descriptor,
                source_width,
                source_height,
                target_width,
                target_height,
                x,
                y + 1,
            )?;
            y_variation += (below - current).abs();
        }
    }
    let gradients = GradientDescriptor::new(
        (x_variation / target_height as f64).clamp(0.0, 1.0) as f32,
        (y_variation / target_width as f64).clamp(0.0, 1.0) as f32,
    );
    let edge_orientation = edge_orientation_from_samples(
        target_width,
        target_height,
        |index| {
            common_sample_coverage(
                descriptor,
                source_width,
                source_height,
                target_width,
                target_height,
                index % target_width,
                index / target_width,
            )
        },
        &mut *is_cancelled,
    )?;
    if is_cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(PatchFeatures {
        mean_coverage: (coverage_sum / target_sample_count as f64) as f32,
        gradients,
        edge_orientation,
    })
}

fn common_sample_coverage(
    descriptor: &StructuralDescriptor<'_>,
    source_width: usize,
    source_height: usize,
    target_width: usize,
    target_height: usize,
    target_x: usize,
    target_y: usize,
) -> Result<f64, Error> {
    let source_x = nearest_center_index(target_x, source_width, target_width)?;
    let source_y = nearest_center_index(target_y, source_height, target_height)?;
    let source_index = source_y
        .checked_mul(source_width)
        .and_then(|row| row.checked_add(source_x))
        .ok_or(Error::SizeOverflow)?;
    let sample = descriptor
        .samples
        .get(source_index)
        .ok_or(Error::SizeOverflow)?;
    Ok(normalized_coverage(
        f64::from(*sample),
        descriptor.foreground_luminance,
        descriptor.background_luminance,
    ))
}

fn nearest_center_index(
    target_index: usize,
    source_extent: usize,
    target_extent: usize,
) -> Result<usize, Error> {
    if source_extent == 0 || target_extent == 0 || target_index >= target_extent {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }
    let target_index = u64::try_from(target_index).map_err(|_| Error::SizeOverflow)?;
    let source_extent_u64 = u64::try_from(source_extent).map_err(|_| Error::SizeOverflow)?;
    let target_extent_u64 = u64::try_from(target_extent).map_err(|_| Error::SizeOverflow)?;
    let numerator = target_index
        .checked_mul(2)
        .and_then(|value| value.checked_add(1))
        .and_then(|value| value.checked_mul(source_extent_u64))
        .ok_or(Error::SizeOverflow)?;
    let denominator = target_extent_u64
        .checked_mul(2)
        .ok_or(Error::SizeOverflow)?;
    let source_index = usize::try_from(numerator / denominator).map_err(|_| Error::SizeOverflow)?;
    if source_index >= source_extent {
        return Err(Error::SizeOverflow);
    }
    Ok(source_index)
}

fn structural_score(
    target: PatchFeatures,
    glyph: &GlyphRecord,
    weights: StructuralWeights,
) -> Result<f64, Error> {
    let glyph_coverage = f64::from(glyph.mean_coverage());
    let target_coverage = f64::from(target.mean_coverage);
    let glyph_gradients = glyph.gradients();
    let target_gradients = target.gradients;
    let glyph_edges = glyph.edge_orientation();
    let target_edges = target.edge_orientation;
    let features = [
        glyph_coverage,
        target_coverage,
        f64::from(glyph_gradients.x_variation()),
        f64::from(glyph_gradients.y_variation()),
        f64::from(target_gradients.x_variation()),
        f64::from(target_gradients.y_variation()),
        f64::from(glyph_edges.horizontal()),
        f64::from(glyph_edges.vertical()),
        f64::from(glyph_edges.diagonal_positive()),
        f64::from(glyph_edges.diagonal_negative()),
        f64::from(target_edges.horizontal()),
        f64::from(target_edges.vertical()),
        f64::from(target_edges.diagonal_positive()),
        f64::from(target_edges.diagonal_negative()),
    ];
    if features
        .iter()
        .any(|feature| !feature.is_finite() || !(0.0..=1.0).contains(feature))
    {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }

    let tone_distance = (glyph_coverage - target_coverage).abs();
    let shape_distance = 0.5
        * ((f64::from(glyph_gradients.x_variation()) - f64::from(target_gradients.x_variation()))
            .abs()
            + (f64::from(glyph_gradients.y_variation())
                - f64::from(target_gradients.y_variation()))
            .abs());
    let edge_distance = 0.5
        * ((f64::from(glyph_edges.horizontal()) - f64::from(target_edges.horizontal())).abs()
            + (f64::from(glyph_edges.vertical()) - f64::from(target_edges.vertical())).abs()
            + (f64::from(glyph_edges.diagonal_positive())
                - f64::from(target_edges.diagonal_positive()))
            .abs()
            + (f64::from(glyph_edges.diagonal_negative())
                - f64::from(target_edges.diagonal_negative()))
            .abs());
    let score = weights.tone * tone_distance
        + weights.shape * shape_distance
        + weights.edge * edge_distance;
    if !score.is_finite() {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }
    Ok(score.clamp(0.0, 1.0))
}

fn candidate_order(left: &StructuralCandidate, right: &StructuralCandidate) -> Ordering {
    left.score
        .total_cmp(&right.score)
        .then_with(|| left.glyph_index.cmp(&right.glyph_index))
        .then_with(|| (left.character as u32).cmp(&(right.character as u32)))
        .then_with(|| left.table_position.cmp(&right.table_position))
}

fn sift_up(
    candidates: &mut [StructuralCandidate],
    mut child: usize,
    work: &mut u64,
    budget: &Budget,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), Error> {
    while child > 0 {
        charge_shortlist_work(work, budget, is_cancelled)?;
        let parent = (child - 1) / 2;
        if candidate_order(&candidates[child], &candidates[parent]) != Ordering::Greater {
            break;
        }
        candidates.swap(child, parent);
        child = parent;
    }
    Ok(())
}

fn sift_down(
    candidates: &mut [StructuralCandidate],
    mut root: usize,
    end: usize,
    work: &mut u64,
    budget: &Budget,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), Error> {
    loop {
        let left = root.checked_mul(2).and_then(|value| value.checked_add(1));
        let Some(left) = left.filter(|&left| left < end) else {
            break;
        };
        charge_shortlist_work(work, budget, is_cancelled)?;
        let right = left.checked_add(1).ok_or(Error::SizeOverflow)?;
        let mut worst = root;
        if candidate_order(&candidates[left], &candidates[worst]) == Ordering::Greater {
            worst = left;
        }
        if right < end
            && candidate_order(&candidates[right], &candidates[worst]) == Ordering::Greater
        {
            worst = right;
        }
        if worst == root {
            break;
        }
        candidates.swap(root, worst);
        root = worst;
    }
    Ok(())
}

fn charge_shortlist_work(
    work: &mut u64,
    budget: &Budget,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), Error> {
    *work = work.checked_add(1).ok_or(Error::SizeOverflow)?;
    if *work > budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }
    if work.is_multiple_of(CANCELLATION_TILE) && is_cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(())
}

fn shortlist_work_bound(glyph_count: usize, candidate_count: usize) -> Result<u64, Error> {
    let glyph_count = u64::try_from(glyph_count).map_err(|_| Error::SizeOverflow)?;
    let candidate_count = u64::try_from(candidate_count).map_err(|_| Error::SizeOverflow)?;
    let candidate_bits = u64::BITS - candidate_count.saturating_sub(1).leading_zeros();
    let levels = u64::from(candidate_bits.max(1));
    glyph_count
        .checked_mul(
            levels
                .checked_mul(3)
                .and_then(|value| value.checked_add(1))
                .ok_or(Error::SizeOverflow)?,
        )
        .and_then(|work| {
            candidate_count
                .checked_mul(levels.checked_mul(2)?)
                .and_then(|sort_work| work.checked_add(sort_work))
        })
        .ok_or(Error::SizeOverflow)
}

fn structural_identity(
    descriptor: &StructuralDescriptor<'_>,
    features: PatchFeatures,
    glyphs: &GlyphTable,
    sampling_identity: SamplingIdentity,
    options: ShortlistOptions,
) -> StructuralIdentity {
    let mut hash = Sha256::new();
    hash.update(b"asciiwall-structural-shortlist\0");
    hash.update(DESCRIPTOR_VERSION.to_le_bytes());
    hash.update(sampling_identity.as_bytes());
    hash.update(descriptor.dimensions.width().to_le_bytes());
    hash.update(descriptor.dimensions.height().to_le_bytes());
    hash.update(glyphs.fingerprint());
    hash.update((options.max_candidates as u32).to_le_bytes());
    hash.update(options.weights.tone.to_bits().to_le_bytes());
    hash.update(options.weights.shape.to_bits().to_le_bytes());
    hash.update(options.weights.edge.to_bits().to_le_bytes());
    hash.update(descriptor.foreground_luminance.to_bits().to_le_bytes());
    hash.update(descriptor.background_luminance.to_bits().to_le_bytes());
    hash.update(descriptor.mean_coverage.to_bits().to_le_bytes());
    hash.update(descriptor.gradients.x_variation().to_bits().to_le_bytes());
    hash.update(descriptor.gradients.y_variation().to_bits().to_le_bytes());
    hash.update(
        descriptor
            .edge_orientation
            .horizontal()
            .to_bits()
            .to_le_bytes(),
    );
    hash.update(
        descriptor
            .edge_orientation
            .vertical()
            .to_bits()
            .to_le_bytes(),
    );
    hash.update(
        descriptor
            .edge_orientation
            .diagonal_positive()
            .to_bits()
            .to_le_bytes(),
    );
    hash.update(
        descriptor
            .edge_orientation
            .diagonal_negative()
            .to_bits()
            .to_le_bytes(),
    );
    hash.update(features.mean_coverage.to_bits().to_le_bytes());
    hash.update(features.gradients.x_variation().to_bits().to_le_bytes());
    hash.update(features.gradients.y_variation().to_bits().to_le_bytes());
    hash.update(
        features
            .edge_orientation
            .horizontal()
            .to_bits()
            .to_le_bytes(),
    );
    hash.update(features.edge_orientation.vertical().to_bits().to_le_bytes());
    hash.update(
        features
            .edge_orientation
            .diagonal_positive()
            .to_bits()
            .to_le_bytes(),
    );
    hash.update(
        features
            .edge_orientation
            .diagonal_negative()
            .to_bits()
            .to_le_bytes(),
    );
    StructuralIdentity(hash.finalize().into())
}

/// Verifies that cancellation clears ranked scratch before returning an error.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        color::{LinearRgb, NormalizedRasterResources},
        density::{ConversionSettings, GridSizing, convert_density_detailed},
        font::{GlyphConfig, GlyphRepertoire, build_glyph_table},
        sampling::{FitMode, SamplingFilter, SamplingOptions},
    };
    use ascii_core::{
        AlphaMode, ColorDescription, ColorPrimaries, FrameMetadata, GlyphTableId, PixelFormat,
        RasterFrameView, RasterOrientation, TransferFunction,
    };

    #[allow(dead_code)]
    mod synthetic_font {
        include!("../tests/support/synthetic_font.rs");
    }

    static BLANK_PATCH: [f32; 8 * 12] = [0.0; 8 * 12];

    fn table() -> GlyphTable {
        build_glyph_table(
            &synthetic_font::synthetic_font(true),
            &GlyphRepertoire::new("BA ").expect("supported fixture repertoire"),
            &GlyphConfig::new(8.0, 8, 12, 9).expect("valid fixture geometry"),
            &Budget::new(1024, 1_000_000, 1_000_000),
            &CancellationToken::new(),
        )
        .expect("synthetic fixture table builds")
    }

    fn descriptor() -> StructuralDescriptor<'static> {
        StructuralDescriptor::from_luminance(
            &BLANK_PATCH,
            PatchDimensions::new(8, 12).expect("valid patch dimensions"),
            1.0,
            0.0,
            &Budget::new(1024, 1_000_000, 1_000_000),
            &CancellationToken::new(),
        )
        .expect("blank patch describes")
    }

    fn sampling_identity(table: &GlyphTable) -> SamplingIdentity {
        let cancellation = CancellationToken::new();
        let settings = ConversionSettings::new(
            GridSizing::fixed_pixel_surface(8, 12),
            FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 1, 0, 0, 1),
            &cancellation,
            LinearRgb::WHITE,
            LinearRgb::BLACK,
        )
        .expect("valid settings")
        .with_sampling(SamplingOptions::new(
            FitMode::Contain,
            SamplingFilter::Box,
            Some(PatchDimensions::new(8, 12).expect("valid patch dimensions")),
        ));
        let pixels = [0_u8; 8 * 12];
        let raster = RasterFrameView::new(
            8,
            12,
            8,
            PixelFormat::gray8(),
            ColorDescription::new(
                ColorPrimaries::Srgb,
                TransferFunction::Linear,
                AlphaMode::Opaque,
            ),
            RasterOrientation::TopDown,
            &pixels,
        );
        convert_density_detailed(
            raster,
            table,
            &settings,
            &Budget::new(1024, 1_000_000, 1_000_000),
            &NormalizedRasterResources::new(1_000_000),
        )
        .expect("sampling identity is produced by density conversion")
        .sampling_identity()
    }

    #[test]
    fn descriptor_feature_budget_is_checked_at_a_conservative_boundary() {
        let dimensions = PatchDimensions::new(2, 2).expect("valid 2x2 patch dimensions");
        let samples = [0.0, 1.0, 0.0, 1.0];
        let descriptor = StructuralDescriptor::from_luminance(
            &samples,
            dimensions,
            1.0,
            0.0,
            &Budget::new(0, 0, 36),
            &CancellationToken::new(),
        )
        .expect("the 36-unit conservative feature bound is sufficient for four samples");
        assert_eq!(descriptor.mean_coverage(), 0.5);

        // If extraction started, this invalid sample would produce InvalidConfiguration.
        let invalid_samples = [f32::NAN; 4];
        assert_eq!(
            StructuralDescriptor::from_luminance(
                &invalid_samples,
                dimensions,
                1.0,
                0.0,
                &Budget::new(0, 0, 35),
                &CancellationToken::new(),
            )
            .err(),
            Some(Error::BudgetExceeded(BudgetKind::WorkUnits))
        );
    }

    #[test]
    fn cancellation_clears_partial_ranked_scratch() {
        let table = table();
        let descriptor = descriptor();
        let sampling_identity = sampling_identity(&table);
        let weights = StructuralWeights::new(1.0, 0.0, 0.0).expect("normalized weights");
        let options = ShortlistOptions::new(2, weights).expect("valid shortlist options");
        let mut scratch = StructuralScratch::default();
        let mut polls = 0;
        let result = scratch.shortlist_with_poll(
            &descriptor,
            &table,
            sampling_identity,
            options,
            &Budget::new(1024, 1_000_000, 1_000_000),
            || {
                polls += 1;
                polls == 8
            },
        );
        assert_eq!(result.err(), Some(Error::Cancelled));
        assert!(scratch.candidates.is_empty());
    }
}
