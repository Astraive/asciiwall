//! Bounded contour-oriented conversion with explicit texture suppression.

use ascii_core::{
    AsciiCell, AsciiFrame, Budget, BudgetKind, CancellationToken, Error, MalformedReason,
    validate_grid_budget,
};
use sha2::{Digest, Sha256};

use crate::{
    ConversionSettings, DensityConversionError, GlyphTable, NormalizedRasterResources,
    PatchDescriptors, PatchDimensions, SamplingOptions, ShortlistOptions, StructuralDescriptor,
    StructuralScratch, StructuralWeights, convert_density_detailed,
    structural::shortlist_grid_work_bound,
};

const CONTOUR_VERSION: u32 = 1;
const CANCEL_INTERVAL: usize = 32;

/// Validated contour controls. Thresholds operate on normalized calibrated glyph coverage.
///
/// `contrast_threshold` maps target samples below the threshold to the configured background,
/// suppressing faint texture. `edge_threshold` ignores weaker coverage gradients.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContourOptions {
    max_candidates: usize,
    contrast_threshold: f64,
    edge_threshold: f64,
    tone_weight: f64,
    contour_weight: f64,
    shape_weight: f64,
}

impl ContourOptions {
    /// Creates validated contour thresholds, weights, and a bounded reranking shortlist.
    pub fn new(
        max_candidates: usize,
        contrast_threshold: f64,
        edge_threshold: f64,
        tone_weight: f64,
        contour_weight: f64,
        shape_weight: f64,
    ) -> Result<Self, Error> {
        let sum = tone_weight + contour_weight + shape_weight;
        if max_candidates == 0
            || max_candidates > crate::MAX_STRUCTURAL_CANDIDATES
            || !contrast_threshold.is_finite()
            || !(0.0..=1.0).contains(&contrast_threshold)
            || !edge_threshold.is_finite()
            || !(0.0..=1.0).contains(&edge_threshold)
            || !tone_weight.is_finite()
            || !contour_weight.is_finite()
            || !shape_weight.is_finite()
            || !(0.0..=1.0).contains(&tone_weight)
            || !(0.0..=1.0).contains(&contour_weight)
            || !(0.0..=1.0).contains(&shape_weight)
            || !sum.is_finite()
            || (sum - 1.0).abs() > 1.0e-9
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(Self {
            max_candidates,
            contrast_threshold: canonical_zero(contrast_threshold),
            edge_threshold: canonical_zero(edge_threshold),
            tone_weight: canonical_zero(tone_weight / sum),
            contour_weight: canonical_zero(contour_weight / sum),
            shape_weight: canonical_zero(shape_weight / sum),
        })
    }

    /// Returns the maximum shortlist size.
    #[must_use]
    pub const fn max_candidates(self) -> usize {
        self.max_candidates
    }
    /// Returns the minimum normalized coverage retained as foreground.
    #[must_use]
    pub const fn contrast_threshold(self) -> f64 {
        self.contrast_threshold
    }
    /// Returns the minimum normalized gradient magnitude treated as a contour edge.
    #[must_use]
    pub const fn edge_threshold(self) -> f64 {
        self.edge_threshold
    }
    /// Returns the tone-loss weight.
    #[must_use]
    pub const fn tone_weight(self) -> f64 {
        self.tone_weight
    }
    /// Returns the oriented-contour weight.
    #[must_use]
    pub const fn contour_weight(self) -> f64 {
        self.contour_weight
    }
    /// Returns the coarse shape-gradient weight.
    #[must_use]
    pub const fn shape_weight(self) -> f64 {
        self.shape_weight
    }
}

/// Identity of a contour conversion, including threshold and scoring semantics.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ContourIdentity([u8; 32]);

impl ContourIdentity {
    /// Returns the digest bytes in canonical SHA-256 order.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Completed bounded contour conversion and its complete semantic identity.
#[derive(Debug, PartialEq)]
pub struct ContourConversion {
    frame: AsciiFrame,
    identity: ContourIdentity,
    work_units_bound: u64,
}

impl ContourConversion {
    /// Returns the converted frame.
    #[must_use]
    pub const fn frame(&self) -> &AsciiFrame {
        &self.frame
    }
    /// Returns the semantic identity for settings, sampling, glyph masks, and patches.
    #[must_use]
    pub const fn identity(&self) -> ContourIdentity {
        self.identity
    }
    /// Returns the reserved shortlist, contour-reranking, and frame-validation work bound.
    #[must_use]
    pub const fn work_units_bound(&self) -> u64 {
        self.work_units_bound
    }
    /// Consumes the result and returns its frame.
    #[must_use]
    pub fn into_frame(self) -> AsciiFrame {
        self.frame
    }
}

/// Converts a raster using calibrated coverage, a bounded structural shortlist, and contour reranking.
///
/// Uniform/background cells emit the least-coverage glyph, with deterministic glyph-index ties.
/// Faint target coverage below `contrast_threshold` is suppressed. Settings that enable
/// candidate-specific palette fitting are rejected; output uses the configured fixed colors.
pub fn convert_contour(
    input: ascii_core::RasterFrameView<'_>,
    glyphs: &GlyphTable,
    settings: &ConversionSettings<'_>,
    options: ContourOptions,
    budget: &Budget,
) -> Result<ContourConversion, Error> {
    let settings = *settings;
    let cancellation = settings.cancellation();
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if settings.metadata().glyph_table_id() != ascii_core::GlyphTableId::new(*glyphs.fingerprint())
    {
        return Err(Error::Malformed(MalformedReason::GlyphTableMismatch));
    }
    if settings.color_fit_enabled() {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }
    let glyph_count = glyphs.glyphs().len();
    if glyph_count == 0 || options.max_candidates > glyph_count {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }
    let dimensions = PatchDimensions::new(glyphs.cell_width(), glyphs.cell_height())?;
    let sampling = SamplingOptions::new(
        settings.sampling().fit(),
        settings.sampling().filter(),
        Some(dimensions),
    );
    let size = settings.sizing().resolve_for_sampling(
        input.width(),
        input.height(),
        glyphs.cell_width(),
        glyphs.cell_height(),
        sampling,
    )?;
    let grid_budget = validate_grid_budget(size, budget, cancellation)?;
    let cell_count = grid_budget.cell_count_usize();
    let sample_count =
        usize::try_from(dimensions.sample_count()).map_err(|_| Error::SizeOverflow)?;
    let rerank_work = u64::try_from(cell_count)
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(u64::try_from(options.max_candidates).map_err(|_| Error::SizeOverflow)?)
        .and_then(|count| count.checked_mul(u64::try_from(sample_count).ok()?))
        .and_then(|work| work.checked_mul(4))
        .ok_or(Error::SizeOverflow)?;
    let shortlist_work = shortlist_grid_work_bound(
        glyph_count,
        options.max_candidates,
        dimensions,
        size.cell_count(),
    )?;
    let reserved_work =
        reserved_contour_work(rerank_work, shortlist_work, grid_budget.work_units())?;
    if reserved_work >= budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }
    let raster_limit = budget.max_raster_bytes();
    let patch_values = u64::try_from(cell_count)
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(u64::try_from(sample_count).map_err(|_| Error::SizeOverflow)?)
        .ok_or(Error::SizeOverflow)?;
    let patch_bytes = patch_values
        .checked_mul(u64::try_from(core::mem::size_of::<f32>()).map_err(|_| Error::SizeOverflow)?)
        .and_then(|bytes| {
            bytes.checked_add(
                u64::try_from(cell_count)
                    .ok()?
                    .checked_mul(u64::try_from(core::mem::size_of::<PatchDescriptors>()).ok()?)?,
            )
        })
        .ok_or(Error::SizeOverflow)?;
    let shortlist_raster_limit = raster_limit
        .checked_sub(patch_bytes)
        .ok_or(Error::BudgetExceeded(BudgetKind::RasterBytes))?;
    let shortlist_budget = Budget::new(
        budget.max_frame_bytes(),
        shortlist_raster_limit,
        budget.max_work_units(),
    );
    let sampling_budget = Budget::new(
        budget.max_frame_bytes(),
        raster_limit,
        budget.max_work_units() - reserved_work,
    );
    let sampled = convert_density_detailed(
        input,
        glyphs,
        &settings.with_sampling(sampling),
        &sampling_budget,
        &NormalizedRasterResources::new(raster_limit),
    )
    .map_err(DensityConversionError::into_core)?;
    let (intermediate, patches, sampling_identity) = sampled.into_contour_parts();
    let sampled_size = intermediate.size();
    drop(intermediate);
    let patches = patches.ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
    if sampled_size != size
        || patches.dimensions() != dimensions
        || patches.cell_count() != cell_count
    {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }

    let shortlist_options = ShortlistOptions::new(
        options.max_candidates,
        StructuralWeights::new(
            options.tone_weight,
            options.shape_weight,
            options.contour_weight,
        )?,
    )?;
    let foreground = settings.foreground().luminance().value();
    let background = settings.background().luminance().value();
    let mut output = Vec::new();
    output
        .try_reserve_exact(cell_count)
        .map_err(|_| Error::AllocationFailed)?;
    if output.capacity() != cell_count {
        return Err(Error::BudgetExceeded(BudgetKind::FrameBytes));
    }
    let mut scratch = StructuralScratch::default();
    let mut digest = Sha256::new();
    digest.update(b"asciiwall-contour-identity");
    digest.update(CONTOUR_VERSION.to_le_bytes());
    digest.update(glyphs.fingerprint());
    digest.update(sampling_identity.as_bytes());
    digest.update(
        u64::try_from(options.max_candidates)
            .map_err(|_| Error::SizeOverflow)?
            .to_le_bytes(),
    );
    digest.update(size.columns().to_le_bytes());
    digest.update(size.rows().to_le_bytes());
    for value in [
        options.contrast_threshold,
        options.edge_threshold,
        options.tone_weight,
        options.contour_weight,
        options.shape_weight,
        foreground,
        background,
        settings.foreground().red(),
        settings.foreground().green(),
        settings.foreground().blue(),
        settings.background().red(),
        settings.background().green(),
        settings.background().blue(),
    ] {
        digest.update(value.to_bits().to_le_bytes());
    }
    let glyph_table = glyphs.glyphs();
    let background_glyph = glyph_table
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            a.mean_coverage()
                .total_cmp(&b.mean_coverage())
                .then_with(|| a.glyph_index().cmp(&b.glyph_index()))
        })
        .map(|(index, _)| u32::try_from(index).map_err(|_| Error::SizeOverflow))
        .transpose()?
        .ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
    let mut scorer = ContourScorer {
        foreground,
        background,
        options,
        work: 0,
        work_limit: rerank_work,
        cancellation,
    };
    for cell in 0..cell_count {
        if cell.is_multiple_of(CANCEL_INTERVAL) && cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let samples = patches
            .luminance_for_cell(cell)
            .ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
        let descriptor = StructuralDescriptor::from_luminance(
            samples,
            dimensions,
            foreground,
            background,
            &shortlist_budget,
            cancellation,
        )?;
        let shortlist = scratch.shortlist(
            &descriptor,
            glyphs,
            sampling_identity,
            shortlist_options,
            &shortlist_budget,
            cancellation,
        )?;
        digest.update(shortlist.identity().as_bytes());
        for sample in samples {
            digest.update(sample.to_bits().to_le_bytes());
        }
        if !patch_has_contour(
            samples,
            foreground,
            background,
            dimensions,
            options,
            cancellation,
        )? {
            output.push(AsciiCell::new(
                background_glyph,
                encode_color(settings.foreground()),
                encode_color(settings.background()),
            ));
            continue;
        }
        let mut best: Option<(usize, f64, u16, char)> = None;
        for candidate in shortlist.candidates() {
            let position =
                usize::try_from(candidate.table_position()).map_err(|_| Error::SizeOverflow)?;
            let glyph = glyph_table
                .get(position)
                .ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
            let score = scorer.score(samples, glyph.mask(), dimensions)?;
            let key = (score, glyph.glyph_index(), glyph.character());
            if best.is_none_or(|(_, best_score, best_index, best_character)| {
                key.0.total_cmp(&best_score).is_lt()
                    || (key.0.total_cmp(&best_score).is_eq()
                        && (key.1, key.2) < (best_index, best_character))
            }) {
                best = Some((position, score, key.1, key.2));
            }
        }
        let (position, _, _, _) =
            best.ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
        output.push(AsciiCell::new(
            u32::try_from(position).map_err(|_| Error::SizeOverflow)?,
            encode_color(settings.foreground()),
            encode_color(settings.background()),
        ));
    }
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let glyph_count = u32::try_from(glyph_count).map_err(|_| Error::SizeOverflow)?;
    let frame = AsciiFrame::try_new(
        size,
        settings.metadata(),
        output,
        glyph_count,
        budget,
        cancellation,
    )?;
    Ok(ContourConversion {
        frame,
        identity: ContourIdentity(digest.finalize().into()),
        work_units_bound: reserved_work,
    })
}

fn reserved_contour_work(
    rerank_work: u64,
    shortlist_work: u64,
    grid_work: u64,
) -> Result<u64, Error> {
    rerank_work
        .checked_add(shortlist_work)
        .and_then(|work| work.checked_add(grid_work))
        .ok_or(Error::SizeOverflow)
}

fn patch_has_contour(
    samples: &[f32],
    foreground: f64,
    background: f64,
    dimensions: PatchDimensions,
    options: ContourOptions,
    cancellation: &CancellationToken,
) -> Result<bool, Error> {
    let width = usize::try_from(dimensions.width()).map_err(|_| Error::SizeOverflow)?;
    let height = usize::try_from(dimensions.height()).map_err(|_| Error::SizeOverflow)?;
    if samples.len() != width.checked_mul(height).ok_or(Error::SizeOverflow)? {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }
    for index in 0..samples.len() {
        if index.is_multiple_of(256) && cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let x = index % width;
        let y = index / width;
        if x + 1 < width {
            let edge = signed_delta(
                samples[index],
                samples[index + 1],
                foreground,
                background,
                options,
            )
            .abs();
            if edge > 0.0 && edge >= options.edge_threshold {
                return Ok(true);
            }
        }
        if y + 1 < height {
            let edge = signed_delta(
                samples[index],
                samples[index + width],
                foreground,
                background,
                options,
            )
            .abs();
            if edge > 0.0 && edge >= options.edge_threshold {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn normalized_coverage(sample: f32, foreground: f64, background: f64) -> f64 {
    ((f64::from(sample) - background) / (foreground - background)).clamp(0.0, 1.0)
}

struct ContourScorer<'a> {
    foreground: f64,
    background: f64,
    options: ContourOptions,
    work: u64,
    work_limit: u64,
    cancellation: &'a CancellationToken,
}

impl ContourScorer<'_> {
    fn score(
        &mut self,
        samples: &[f32],
        mask: &[u8],
        dimensions: PatchDimensions,
    ) -> Result<f64, Error> {
        let width = usize::try_from(dimensions.width()).map_err(|_| Error::SizeOverflow)?;
        let height = usize::try_from(dimensions.height()).map_err(|_| Error::SizeOverflow)?;
        if samples.len() != mask.len()
            || samples.len() != width.checked_mul(height).ok_or(Error::SizeOverflow)?
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let mut tone_loss = 0.0;
        let mut target_edges = [0.0; 4];
        let mut mask_edges = [0.0; 4];
        let mut target_total = 0.0;
        let mut mask_total = 0.0;
        let mut shape_loss = 0.0;
        for index in 0..samples.len() {
            if self.work.is_multiple_of(256) && self.cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            self.work = self.work.checked_add(1).ok_or(Error::SizeOverflow)?;
            if self.work > self.work_limit {
                return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
            }
            let raw_target = normalized_coverage(samples[index], self.foreground, self.background);
            let target = if raw_target < self.options.contrast_threshold {
                0.0
            } else {
                raw_target
            };
            let mask_value = f64::from(mask[index]) / 255.0;
            tone_loss += (target - mask_value).abs();
            let x = index % width;
            let y = index / width;
            let mut target_dx = 0.0;
            let mut target_dy = 0.0;
            let mut mask_dx = 0.0;
            let mut mask_dy = 0.0;
            if x + 1 < width {
                target_dx = signed_delta(
                    samples[index],
                    samples[index + 1],
                    self.foreground,
                    self.background,
                    self.options,
                );
                mask_dx = mask_value - f64::from(mask[index + 1]) / 255.0;
                shape_loss += (target_dx.abs() - mask_dx.abs()).abs();
            }
            if y + 1 < height {
                target_dy = signed_delta(
                    samples[index],
                    samples[index + width],
                    self.foreground,
                    self.background,
                    self.options,
                );
                mask_dy = mask_value - f64::from(mask[index + width]) / 255.0;
                shape_loss += (target_dy.abs() - mask_dy.abs()).abs();
            }
            add_oriented_edge(
                &mut target_edges,
                &mut target_total,
                target_dx,
                target_dy,
                self.options.edge_threshold,
            );
            add_oriented_edge(
                &mut mask_edges,
                &mut mask_total,
                mask_dx,
                mask_dy,
                self.options.edge_threshold,
            );
        }
        normalize_bins(&mut target_edges, target_total);
        normalize_bins(&mut mask_edges, mask_total);
        let edge_loss = target_edges
            .iter()
            .zip(mask_edges)
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            * 0.5;
        let gradient_count = width
            .saturating_sub(1)
            .checked_mul(height)
            .and_then(|n| n.checked_add(width.checked_mul(height.saturating_sub(1))?))
            .ok_or(Error::SizeOverflow)?
            .max(1);
        Ok(self.options.tone_weight * tone_loss / samples.len() as f64
            + self.options.contour_weight * edge_loss
            + self.options.shape_weight * shape_loss / gradient_count as f64)
    }
}

fn signed_delta(
    left: f32,
    right: f32,
    foreground: f64,
    background: f64,
    options: ContourOptions,
) -> f64 {
    let left = normalized_coverage(left, foreground, background);
    let right = normalized_coverage(right, foreground, background);
    let left = if left < options.contrast_threshold {
        0.0
    } else {
        left
    };
    let right = if right < options.contrast_threshold {
        0.0
    } else {
        right
    };
    right - left
}

fn add_oriented_edge(bins: &mut [f64; 4], total: &mut f64, dx: f64, dy: f64, threshold: f64) {
    let magnitude = dx.hypot(dy);
    if magnitude < threshold || magnitude == 0.0 {
        return;
    }
    let bin = if dx.abs() >= 2.0 * dy.abs() {
        0
    } else if dy.abs() >= 2.0 * dx.abs() {
        1
    } else if dx.is_sign_positive() == dy.is_sign_positive() {
        2
    } else {
        3
    };
    bins[bin] += magnitude;
    *total += magnitude;
}

fn normalize_bins(bins: &mut [f64; 4], total: f64) {
    if total > 0.0 {
        for bin in bins {
            *bin /= total;
        }
    }
}

fn encode_color(color: crate::LinearRgb) -> [u8; 4] {
    let channels = crate::encode_output(color, crate::OutputEncoding::Srgb8).channels();
    [channels[0], channels[1], channels[2], u8::MAX]
}

fn canonical_zero(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}

#[cfg(test)]
mod tests {
    use super::reserved_contour_work;

    #[test]
    fn reservation_includes_rerank_shortlist_and_grid_work() {
        let cells = 1_u64;
        let candidates = 2_u64;
        let samples = 8_u64 * 12;
        let shortlist_work = 12_345;
        let grid_work = 678;
        let rerank_work = cells * candidates * samples * 4;

        assert_eq!(
            reserved_contour_work(rerank_work, shortlist_work, grid_work)
                .expect("fixture work reservation fits u64"),
            768 + shortlist_work + grid_work
        );
    }
}
