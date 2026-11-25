//! Bounded, deterministic temporal glyph hysteresis for caller-owned frame matching.
//!
//! The caller supplies the independent-frame winner and that winner's score alongside the previous
//! glyph's score under the current frame. History contains glyph IDs only; media time and generation
//! come from the caller, never a wall clock. The module does not own native or UI resources.

use core::mem::size_of;

use ascii_core::{Budget, BudgetKind, CancellationToken, Error, MalformedReason};

/// Hard cap on retained or processed temporal cell state for one grid.
pub const MAX_TEMPORAL_CELLS: usize = 1_000_000;
/// Upper bound on candidate processing operations per frame.
pub const MAX_TEMPORAL_WORK_UNITS: u64 = 1_000_000;
/// Time scale over which hysteresis reaches its configured maximum threshold (one second).
pub const TEMPORAL_HYSTERESIS_RAMP_NS: u64 = 1_000_000_000;
const CANCELLATION_TILE: usize = 256;
/// Largest configured age for reusable temporal glyph history (30 seconds).
pub const MAX_TEMPORAL_HISTORY_GAP_NS: u64 = 30_000_000_000;

/// A caller-provided reason to discard all retained glyph history before this frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemporalReset {
    /// No explicit reset; compatibility and media-time checks still apply.
    None,
    /// The frame starts a new scene.
    HardCut,
    /// Playback sought to another position.
    Seek,
    /// The decoded source changed.
    SourceChange,
    /// Font metrics, glyph table, or grid geometry changed.
    FontOrGridChange,
    /// Decoding reported a missing or discontinuous frame.
    DecodeDiscontinuity,
}

/// Semantic context required to safely reuse history across adjacent frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalFrameContext {
    generation: u64,
    media_time_ns: u64,
    grid_width: u32,
    grid_height: u32,
    source_fingerprint: [u8; 32],
    glyph_fingerprint: [u8; 32],
    settings_fingerprint: [u8; 32],
    reset: TemporalReset,
}

impl TemporalFrameContext {
    /// Creates a context from normalized caller media time and semantic configuration identities.
    #[must_use]
    pub const fn new(
        generation: u64,
        media_time_ns: u64,
        grid_width: u32,
        grid_height: u32,
        source_fingerprint: [u8; 32],
        glyph_fingerprint: [u8; 32],
        settings_fingerprint: [u8; 32],
    ) -> Self {
        Self {
            generation,
            media_time_ns,
            grid_width,
            grid_height,
            source_fingerprint,
            glyph_fingerprint,
            settings_fingerprint,
            reset: TemporalReset::None,
        }
    }

    /// Marks a discontinuity that must clear all temporal state before processing this frame.
    #[must_use]
    pub const fn with_reset(mut self, reset: TemporalReset) -> Self {
        self.reset = reset;
        self
    }
}

/// Controls temporal retention. Thresholds are normalized score deltas in `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TemporalOptions {
    weight: f64,
    maximum_threshold: f64,
    maximum_history_gap_ns: u64,
}

impl TemporalOptions {
    /// Creates validated controls. A zero weight disables retention exactly.
    pub fn new(
        weight: f64,
        maximum_threshold: f64,
        maximum_history_gap_ns: u64,
    ) -> Result<Self, Error> {
        if !weight.is_finite()
            || !(0.0..=1.0).contains(&weight)
            || !maximum_threshold.is_finite()
            || !(0.0..=1.0).contains(&maximum_threshold)
            || maximum_history_gap_ns == 0
            || maximum_history_gap_ns > MAX_TEMPORAL_HISTORY_GAP_NS
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(Self {
            weight,
            maximum_threshold,
            maximum_history_gap_ns,
        })
    }
}

/// One independent-frame winner and its score against the current frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TemporalCandidate {
    /// Glyph selected by the independent-frame matcher.
    pub glyph_id: u32,
    /// Current-frame score of the independent winner; lower is better.
    pub best_score: f64,
    /// Current-frame score of the retained previous glyph; lower is better.
    pub previous_score: f64,
    /// Local motion estimate in `[0, 1]`; larger motion suppresses hysteresis.
    pub motion: f64,
}

/// Reusable, bounded storage for prior glyph IDs and one completed output grid.
#[derive(Debug, Default)]
pub struct TemporalScratch {
    history: Vec<u32>,
    output: Vec<u32>,
    active_generation: Option<u64>,
    context: Option<TemporalFrameContext>,
}

impl TemporalScratch {
    /// Invalidates history and establishes the only generation allowed to publish subsequently.
    ///
    /// This is the settings/seek invalidation barrier for asynchronous callers: old work carrying a
    /// prior generation is rejected and cannot replace the newly established state.
    pub fn invalidate(&mut self, generation: u64) {
        self.history.clear();
        self.output.clear();
        self.context = None;
        self.active_generation = Some(generation);
    }

    /// Drops prior temporal state without changing the generation barrier.
    pub fn reset(&mut self) {
        self.history.clear();
        self.output.clear();
        self.context = None;
    }

    /// Returns stabilized glyph IDs, or an error without publishing partial history.
    ///
    /// Scratch memory is bounded by both `MAX_TEMPORAL_CELLS` and the supplied raster-byte budget.
    /// Each frame is bounded by its work-unit budget and checks cancellation every 256 cells. For
    /// each cell, the retention threshold is `maximum_threshold * weight * min(elapsed / 1s, 1) *
    /// (1 - motion)`. The prior glyph is kept only when its current-frame score improvement does
    /// not exceed that threshold. Thus time builds a bounded stability margin, while motion always
    /// reduces it; no threshold depends on a global frame freeze or wall-clock elapsed time.
    pub fn stabilize<'a>(
        &'a mut self,
        context: TemporalFrameContext,
        candidates: &[TemporalCandidate],
        options: TemporalOptions,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<&'a [u32], Error> {
        self.stabilize_with_poll(context, candidates, options, budget, || {
            cancellation.is_cancelled()
        })
    }

    fn stabilize_with_poll<'a>(
        &'a mut self,
        context: TemporalFrameContext,
        candidates: &[TemporalCandidate],
        options: TemporalOptions,
        budget: &Budget,
        mut is_cancelled: impl FnMut() -> bool,
    ) -> Result<&'a [u32], Error> {
        if is_cancelled() {
            return Err(Error::Cancelled);
        }
        let count = usize::try_from(context.grid_width)
            .map_err(|_| Error::SizeOverflow)?
            .checked_mul(usize::try_from(context.grid_height).map_err(|_| Error::SizeOverflow)?)
            .ok_or(Error::SizeOverflow)?;
        if context.grid_width == 0
            || context.grid_height == 0
            || count != candidates.len()
            || count > MAX_TEMPORAL_CELLS
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let work = u64::try_from(count).map_err(|_| Error::SizeOverflow)?;
        if work > MAX_TEMPORAL_WORK_UNITS || work > budget.max_work_units() {
            return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
        }
        let per_buffer = work
            .checked_mul(u64::try_from(size_of::<u32>()).map_err(|_| Error::SizeOverflow)?)
            .ok_or(Error::SizeOverflow)?;
        let total_bytes = per_buffer.checked_mul(2).ok_or(Error::SizeOverflow)?;
        let _allocation_layout = isize::try_from(total_bytes).map_err(|_| Error::SizeOverflow)?;
        if total_bytes > budget.max_raster_bytes() {
            return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
        }
        for (index, candidate) in candidates.iter().enumerate() {
            if index % CANCELLATION_TILE == 0 && is_cancelled() {
                return Err(Error::Cancelled);
            }
            if !candidate.best_score.is_finite()
                || !(0.0..=1.0).contains(&candidate.best_score)
                || !candidate.previous_score.is_finite()
                || !(0.0..=1.0).contains(&candidate.previous_score)
                || !candidate.motion.is_finite()
                || !(0.0..=1.0).contains(&candidate.motion)
            {
                return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
            }
        }
        let compatible = self.compatible(context, options);
        if self
            .active_generation
            .is_some_and(|generation| generation != context.generation)
        {
            return Err(Error::Cancelled);
        }
        if !self.reserve_capacity(count, budget)? {
            return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
        }
        if is_cancelled() {
            return Err(Error::Cancelled);
        }
        let prior_context = self.context;
        let elapsed_ns = if compatible {
            context.media_time_ns.saturating_sub(
                prior_context.map_or(context.media_time_ns, |prior| prior.media_time_ns),
            )
        } else {
            0
        };
        let use_history = compatible && elapsed_ns > 0;
        self.output.clear();
        for (index, candidate) in candidates.iter().enumerate() {
            if index % CANCELLATION_TILE == 0 && is_cancelled() {
                self.output.clear();
                return Err(Error::Cancelled);
            }
            let threshold = if options.weight == 0.0 || !use_history || candidate.motion == 1.0 {
                0.0
            } else {
                options.maximum_threshold
                    * options.weight
                    * (elapsed_ns.min(TEMPORAL_HYSTERESIS_RAMP_NS) as f64
                        / TEMPORAL_HYSTERESIS_RAMP_NS as f64)
                    * (1.0 - candidate.motion)
            };
            let selected = if options.weight != 0.0
                && use_history
                && self.history[index] != candidate.glyph_id
                && candidate.previous_score - candidate.best_score <= threshold
            {
                self.history[index]
            } else {
                candidate.glyph_id
            };
            self.output.push(selected);
        }
        if is_cancelled() {
            self.output.clear();
            return Err(Error::Cancelled);
        }
        core::mem::swap(&mut self.history, &mut self.output);
        self.context = Some(context);
        self.active_generation.get_or_insert(context.generation);
        Ok(&self.history)
    }

    fn compatible(&self, context: TemporalFrameContext, options: TemporalOptions) -> bool {
        let Some(previous) = self.context else {
            return false;
        };
        let Some(active_generation) = self.active_generation else {
            return false;
        };
        let Some(elapsed) = context.media_time_ns.checked_sub(previous.media_time_ns) else {
            return false;
        };
        context.reset == TemporalReset::None
            && active_generation == context.generation
            && previous.generation == context.generation
            && previous.grid_width == context.grid_width
            && previous.grid_height == context.grid_height
            && previous.source_fingerprint == context.source_fingerprint
            && previous.glyph_fingerprint == context.glyph_fingerprint
            && previous.settings_fingerprint == context.settings_fingerprint
            && elapsed > 0
            && elapsed <= options.maximum_history_gap_ns
    }

    fn reserve_capacity(&mut self, count: usize, budget: &Budget) -> Result<bool, Error> {
        let item_bytes = u64::try_from(size_of::<u32>()).map_err(|_| Error::SizeOverflow)?;
        let required_bytes = u64::try_from(count)
            .map_err(|_| Error::SizeOverflow)?
            .checked_mul(item_bytes)
            .and_then(|bytes| bytes.checked_mul(2))
            .ok_or(Error::SizeOverflow)?;
        let current_bytes = u64::try_from(self.history.capacity())
            .map_err(|_| Error::SizeOverflow)?
            .checked_add(u64::try_from(self.output.capacity()).map_err(|_| Error::SizeOverflow)?)
            .and_then(|capacity| capacity.checked_mul(item_bytes))
            .ok_or(Error::SizeOverflow)?;
        if current_bytes.max(required_bytes) > budget.max_raster_bytes() {
            return Ok(false);
        }
        if self.history.capacity() < count {
            self.history
                .try_reserve_exact(count - self.history.len())
                .map_err(|_| Error::AllocationFailed)?;
        }
        if self.output.capacity() < count {
            self.output
                .try_reserve_exact(count - self.output.len())
                .map_err(|_| Error::AllocationFailed)?;
        }
        let allocated_bytes = u64::try_from(self.history.capacity())
            .map_err(|_| Error::SizeOverflow)?
            .checked_add(u64::try_from(self.output.capacity()).map_err(|_| Error::SizeOverflow)?)
            .and_then(|capacity| capacity.checked_mul(item_bytes))
            .ok_or(Error::SizeOverflow)?;
        Ok(allocated_bytes <= budget.max_raster_bytes())
    }
}
