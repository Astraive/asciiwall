//! Deterministic bounded scalar-coverage dithering for density matching.

use ascii_core::{Budget, BudgetKind, CancellationToken, Error};
use sha2::{Digest, Sha256};

/// Maximum additional dither-specific work units accepted for one conversion.
pub const MAX_DITHER_WORK_UNITS: u64 = 16_000_000;
const CANCELLATION_INTERVAL: usize = 32;

/// Ordered threshold pattern used in the normalized glyph-coverage domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderedPattern {
    /// Canonical 8x8 Bayer rank matrix.
    Bayer8,
}

/// Sequential error-diffusion kernel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorDiffusionKernel {
    /// Floyd–Steinberg weights (7/16, 3/16, 5/16, 1/16), left-to-right on each row.
    FloydSteinberg,
}

/// Density-coverage quantization policy. `Off` is the default and preserves legacy matching.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DitherMode {
    /// Do not perturb the sampled coverage.
    #[default]
    Off,
    /// Add a stable Bayer threshold; the seed selects a fixed 8x8 phase.
    Ordered { pattern: OrderedPattern, seed: u64 },
    /// Sequential, left-to-right error diffusion.
    ErrorDiffusion { kernel: ErrorDiffusionKernel },
}

/// Stable identity of the selected dithering policy, including all mode parameters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DitherCacheIdentity([u8; 32]);

impl DitherCacheIdentity {
    /// Returns the SHA-256 identity bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub(crate) fn cache_identity(mode: DitherMode) -> DitherCacheIdentity {
    let mut hash = Sha256::new();
    hash.update(b"ascii-convert-dither-v1\0");
    match mode {
        DitherMode::Off => hash.update([0]),
        DitherMode::Ordered {
            pattern: OrderedPattern::Bayer8,
            seed,
        } => {
            hash.update([1, 0]);
            hash.update(seed.to_le_bytes());
        }
        DitherMode::ErrorDiffusion {
            kernel: ErrorDiffusionKernel::FloydSteinberg,
        } => {
            hash.update([2, 0]);
        }
    }
    DitherCacheIdentity(hash.finalize().into())
}
pub(crate) fn estimate_work(mode: DitherMode, width: usize, height: usize) -> Result<u64, Error> {
    let width = u64::try_from(width).map_err(|_| Error::SizeOverflow)?;
    let height = u64::try_from(height).map_err(|_| Error::SizeOverflow)?;
    let cells = width.checked_mul(height).ok_or(Error::SizeOverflow)?;
    match mode {
        DitherMode::Off => Ok(0),
        DitherMode::Ordered { .. } => Ok(cells),
        DitherMode::ErrorDiffusion { .. } => {
            let row_buffer_len = width.checked_add(2).ok_or(Error::SizeOverflow)?;
            cells
                .checked_mul(5)
                .and_then(|work| work.checked_add(row_buffer_len.checked_mul(height)?))
                .and_then(|work| work.checked_add(row_buffer_len.checked_mul(2)?))
                .ok_or(Error::SizeOverflow)
        }
    }
}

/// Scratch-owning state for scalar coverage dithering; diffusion is dependency ordered.
pub(crate) struct DitherState {
    mode: DitherMode,
    levels: f64,
    ordered_phase: (usize, usize),
    current: Vec<f64>,
    next: Vec<f64>,
    width: usize,
}

impl DitherState {
    pub(crate) fn new(
        mode: DitherMode,
        width: usize,
        height: usize,
        glyph_count: usize,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<Self, Error> {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let dither_work = estimate_work(mode, width, height)?;
        if dither_work > MAX_DITHER_WORK_UNITS || dither_work > budget.max_work_units() {
            return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
        }
        let scratch_len = if matches!(mode, DitherMode::ErrorDiffusion { .. }) {
            width.checked_add(2).ok_or(Error::SizeOverflow)?
        } else {
            0
        };
        let scratch_bytes = u64::try_from(scratch_len)
            .map_err(|_| Error::SizeOverflow)?
            .checked_mul(
                2 * u64::try_from(core::mem::size_of::<f64>()).map_err(|_| Error::SizeOverflow)?,
            )
            .ok_or(Error::SizeOverflow)?;
        if scratch_bytes > budget.max_raster_bytes() {
            return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
        }
        let mut current = Vec::new();
        let mut next = Vec::new();
        for buffer in [&mut current, &mut next] {
            buffer
                .try_reserve_exact(scratch_len)
                .map_err(|_| Error::AllocationFailed)?;
            if buffer.capacity() != scratch_len {
                return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
            }
            for index in 0..scratch_len {
                if index.is_multiple_of(CANCELLATION_INTERVAL) && cancellation.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                buffer.push(0.0);
            }
        }
        let ordered_phase = match mode {
            DitherMode::Ordered { seed, .. } => {
                let phase = mix(seed);
                (
                    usize::try_from(phase & 7).map_err(|_| Error::SizeOverflow)?,
                    usize::try_from((phase >> 3) & 7).map_err(|_| Error::SizeOverflow)?,
                )
            }
            _ => (0, 0),
        };
        let levels = f64::from(
            u32::try_from(glyph_count.saturating_sub(1).max(1)).map_err(|_| Error::SizeOverflow)?,
        );
        Ok(Self {
            mode,
            levels,
            ordered_phase,
            current,
            next,
            width,
        })
    }

    pub(crate) fn adjusted(&self, _row: usize, column: usize, coverage: f64) -> Result<f64, Error> {
        if !coverage.is_finite() || !(0.0..=1.0).contains(&coverage) {
            return Err(Error::Malformed(
                ascii_core::MalformedReason::InvalidConfiguration,
            ));
        }
        let value = match self.mode {
            DitherMode::Off => coverage,
            DitherMode::Ordered {
                pattern: OrderedPattern::Bayer8,
                ..
            } => {
                const BAYER: [u8; 64] = [
                    0, 48, 12, 60, 3, 51, 15, 63, 32, 16, 44, 28, 35, 19, 47, 31, 8, 56, 4, 52, 11,
                    59, 7, 55, 40, 24, 36, 20, 43, 27, 39, 23, 2, 50, 14, 62, 1, 49, 13, 61, 34,
                    18, 46, 30, 33, 17, 45, 29, 10, 58, 6, 54, 9, 57, 5, 53, 42, 26, 38, 22, 41,
                    25, 37, 21,
                ];
                let x = (column + self.ordered_phase.0) & 7;
                let y = (_row + self.ordered_phase.1) & 7;
                (coverage + ((f64::from(BAYER[y * 8 + x]) + 0.5) / 64.0 - 0.5) / self.levels)
                    .clamp(0.0, 1.0)
            }
            DitherMode::ErrorDiffusion { .. } => {
                let residual = self.current[column + 1];
                if !residual.is_finite() {
                    return Err(Error::Malformed(
                        ascii_core::MalformedReason::InvalidConfiguration,
                    ));
                }
                (coverage + residual).clamp(0.0, 1.0)
            }
        };
        if !value.is_finite() {
            return Err(Error::Malformed(
                ascii_core::MalformedReason::InvalidConfiguration,
            ));
        }
        Ok(value)
    }

    pub(crate) fn record(
        &mut self,
        _row: usize,
        column: usize,
        adjusted: f64,
        selected: f64,
        cancellation: &CancellationToken,
    ) -> Result<(), Error> {
        if !adjusted.is_finite()
            || !selected.is_finite()
            || !(0.0..=1.0).contains(&adjusted)
            || !(0.0..=1.0).contains(&selected)
        {
            return Err(Error::Malformed(
                ascii_core::MalformedReason::InvalidConfiguration,
            ));
        }
        if let DitherMode::ErrorDiffusion {
            kernel: ErrorDiffusionKernel::FloydSteinberg,
        } = self.mode
        {
            let error = adjusted - selected;
            let base = column + 1;
            self.add_current(base + 1, error * 7.0 / 16.0)?;
            self.add_next(base - 1, error * 3.0 / 16.0)?;
            self.add_next(base, error * 5.0 / 16.0)?;
            self.add_next(base + 1, error / 16.0)?;
            if column + 1 == self.width {
                core::mem::swap(&mut self.current, &mut self.next);
                for index in 0..self.next.len() {
                    if index.is_multiple_of(CANCELLATION_INTERVAL) && cancellation.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    self.next[index] = 0.0;
                }
            }
        }
        Ok(())
    }

    fn add_current(&mut self, index: usize, value: f64) -> Result<(), Error> {
        let total = self.current[index] + value;
        if !total.is_finite() {
            return Err(Error::Malformed(
                ascii_core::MalformedReason::InvalidConfiguration,
            ));
        }
        self.current[index] = total;
        Ok(())
    }
    fn add_next(&mut self, index: usize, value: f64) -> Result<(), Error> {
        let total = self.next[index] + value;
        if !total.is_finite() {
            return Err(Error::Malformed(
                ascii_core::MalformedReason::InvalidConfiguration,
            ));
        }
        self.next[index] = total;
        Ok(())
    }
}

fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::{DitherMode, DitherState, ErrorDiffusionKernel, OrderedPattern};
    use ascii_core::{Budget, BudgetKind, CancellationToken, Error};

    fn budget() -> Budget {
        Budget::new(u64::MAX, u64::MAX, u64::MAX)
    }

    #[test]
    fn ordered_threshold_is_stable_and_spatially_varied() {
        let cancellation = CancellationToken::new();
        let state = DitherState::new(
            DitherMode::Ordered {
                pattern: OrderedPattern::Bayer8,
                seed: 123,
            },
            8,
            8,
            3,
            &budget(),
            &cancellation,
        )
        .expect("ordered state fits");
        let values: Vec<_> = (0..8)
            .map(|column| {
                state
                    .adjusted(0, column, 0.5)
                    .expect("finite ordered adjustment")
            })
            .collect();
        assert!(values.iter().any(|value| *value != values[0]));
        assert_eq!(
            state
                .adjusted(0, 3, 0.5)
                .expect("repeated coordinate is stable"),
            values[3]
        );
    }

    #[test]
    fn diffusion_residuals_follow_kernel_and_drop_at_edges() {
        let cancellation = CancellationToken::new();
        let mut state = DitherState::new(
            DitherMode::ErrorDiffusion {
                kernel: ErrorDiffusionKernel::FloydSteinberg,
            },
            2,
            2,
            2,
            &budget(),
            &cancellation,
        )
        .expect("diffusion state fits");
        state
            .record(0, 0, 1.0, 0.0, &cancellation)
            .expect("first-cell residual propagates");
        let right = state
            .adjusted(0, 1, 0.5)
            .expect("horizontal residual remains finite");
        assert_eq!(right, 0.5 + 7.0 / 16.0);
        state
            .record(0, 1, right, 1.0, &cancellation)
            .expect("row-end swaps bounded buffers");
        let below = state
            .adjusted(1, 0, 0.5)
            .expect("vertical residual remains finite");
        assert_eq!(below, 0.5 + 5.0 / 16.0 + (right - 1.0) * 3.0 / 16.0);
        state
            .record(1, 0, below, 1.0, &cancellation)
            .expect("left boundary discards off-grid residuals");
        state
            .record(1, 1, 0.5, 0.5, &cancellation)
            .expect("bottom boundary discards off-grid residuals");
    }

    #[test]
    fn dither_preflight_rejects_work_and_row_scratch_over_budget() {
        let cancellation = CancellationToken::new();
        let ordered_work = DitherState::new(
            DitherMode::Ordered {
                pattern: OrderedPattern::Bayer8,
                seed: 0,
            },
            1,
            1,
            2,
            &Budget::new(u64::MAX, u64::MAX, 0),
            &cancellation,
        );
        assert_eq!(
            ordered_work.err(),
            Some(Error::BudgetExceeded(BudgetKind::WorkUnits))
        );
        let diffusion_scratch = DitherState::new(
            DitherMode::ErrorDiffusion {
                kernel: ErrorDiffusionKernel::FloydSteinberg,
            },
            1,
            1,
            2,
            &Budget::new(u64::MAX, 0, u64::MAX),
            &cancellation,
        );
        assert_eq!(
            diffusion_scratch.err(),
            Some(Error::BudgetExceeded(BudgetKind::RasterBytes))
        );
    }

    #[test]
    fn nonfinite_targets_and_work_overflow_are_rejected() {
        let cancellation = CancellationToken::new();
        let state = DitherState::new(
            DitherMode::Ordered {
                pattern: OrderedPattern::Bayer8,
                seed: 1,
            },
            1,
            1,
            2,
            &budget(),
            &cancellation,
        )
        .expect("ordered state fits");
        assert!(matches!(
            state.adjusted(0, 0, f64::NAN),
            Err(Error::Malformed(_))
        ));
        assert_eq!(
            super::estimate_work(
                DitherMode::ErrorDiffusion {
                    kernel: ErrorDiffusionKernel::FloydSteinberg,
                },
                usize::MAX,
                usize::MAX,
            ),
            Err(Error::SizeOverflow)
        );
    }

    #[test]
    fn row_buffer_clear_observes_cancellation() {
        let cancellation = CancellationToken::new();
        let mut state = DitherState::new(
            DitherMode::ErrorDiffusion {
                kernel: ErrorDiffusionKernel::FloydSteinberg,
            },
            1,
            1,
            2,
            &budget(),
            &cancellation,
        )
        .expect("diffusion state fits");
        cancellation.cancel();
        assert_eq!(
            state.record(0, 0, 0.5, 0.0, &cancellation),
            Err(Error::Cancelled)
        );
    }
}
