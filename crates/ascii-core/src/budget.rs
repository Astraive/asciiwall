//! Explicit, allocation-free preflight for bounded core work.

use core::sync::atomic::{AtomicU8, Ordering};

use crate::GridSize;
use crate::error::{BudgetKind, Error, MalformedReason};
use crate::frame::AsciiCell;

const CANCELLATION_REQUESTED: u8 = 0b01;
const CANCELLATION_COMMIT_IN_PROGRESS: u8 = 0b10;

/// Independent byte and work ceilings for frame and raster validation.
///
/// There is intentionally no default or unlimited constructor: each operation receives an
/// explicit budget from its caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Budget {
    max_frame_bytes: u64,
    max_raster_bytes: u64,
    max_work_units: u64,
}

impl Budget {
    /// Creates a budget with explicit frame-byte, raster-byte, and work-unit ceilings.
    #[must_use]
    pub const fn new(max_frame_bytes: u64, max_raster_bytes: u64, max_work_units: u64) -> Self {
        Self {
            max_frame_bytes,
            max_raster_bytes,
            max_work_units,
        }
    }

    /// Returns the maximum bytes permitted for the owned ASCII cell-vector capacity.
    #[must_use]
    pub const fn max_frame_bytes(self) -> u64 {
        self.max_frame_bytes
    }

    /// Returns the maximum readable raster extent permitted.
    #[must_use]
    pub const fn max_raster_bytes(self) -> u64 {
        self.max_raster_bytes
    }

    /// Returns the maximum estimated cell or pixel work for one validation.
    #[must_use]
    pub const fn max_work_units(self) -> u64 {
        self.max_work_units
    }
}

/// Cancellation state that may be shared safely with an in-progress core operation.
#[derive(Debug)]
pub struct CancellationToken {
    state: AtomicU8,
}

impl CancellationToken {
    /// Creates a token that has not been cancelled.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: AtomicU8::new(0),
        }
    }

    /// Requests cancellation of operations using this token.
    ///
    /// A short state transition protected by [`Self::commit_if_not_cancelled`] completes before
    /// this request takes effect. This keeps cancellation and that transition in one order.
    pub fn cancel(&self) {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            if state & CANCELLATION_REQUESTED != 0 {
                return;
            }
            if state & CANCELLATION_COMMIT_IN_PROGRESS != 0 {
                core::hint::spin_loop();
                state = self.state.load(Ordering::Acquire);
                continue;
            }
            match self.state.compare_exchange_weak(
                state,
                state | CANCELLATION_REQUESTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(actual) => state = actual,
            }
        }
    }

    /// Returns whether cancellation has taken effect.
    ///
    /// A cancellation racing with a committed state transition is visible after that transition
    /// completes.
    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) & CANCELLATION_REQUESTED != 0
    }

    /// Runs a short state transition only if cancellation has not taken effect.
    ///
    /// The transition and [`Self::cancel`] are ordered atomically: if this method acquires the
    /// transition first, a concurrent cancellation takes effect after it completes; otherwise the
    /// transition is not run. Keep the closure short and nonblocking. It MUST NOT call
    /// [`Self::cancel`] on this token.
    pub fn commit_if_not_cancelled<T>(&self, transition: impl FnOnce() -> T) -> Result<T, Error> {
        let _guard = self.begin_commit()?;
        Ok(transition())
    }

    fn begin_commit(&self) -> Result<CancellationCommitGuard<'_>, Error> {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            if state & CANCELLATION_REQUESTED != 0 {
                return Err(Error::Cancelled);
            }
            if state & CANCELLATION_COMMIT_IN_PROGRESS != 0 {
                core::hint::spin_loop();
                state = self.state.load(Ordering::Acquire);
                continue;
            }
            match self.state.compare_exchange_weak(
                state,
                state | CANCELLATION_COMMIT_IN_PROGRESS,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(CancellationCommitGuard { state: &self.state }),
                Err(actual) => state = actual,
            }
        }
    }
}

struct CancellationCommitGuard<'a> {
    state: &'a AtomicU8,
}

impl Drop for CancellationCommitGuard<'_> {
    fn drop(&mut self) {
        self.state
            .fetch_and(!CANCELLATION_COMMIT_IN_PROGRESS, Ordering::Release);
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// A successful, allocation-free grid preflight result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridBudget {
    cell_count: u64,
    cell_count_usize: usize,
    frame_bytes: u64,
    work_units: u64,
}

impl GridBudget {
    /// Returns the exact `u64` cell count.
    #[must_use]
    pub const fn cell_count(self) -> u64 {
        self.cell_count
    }

    /// Returns the cell count converted to the current platform's `usize`.
    #[must_use]
    pub const fn cell_count_usize(self) -> usize {
        self.cell_count_usize
    }

    /// Returns the minimum checked cell-storage bytes required for this grid.
    #[must_use]
    pub const fn frame_bytes(self) -> u64 {
        self.frame_bytes
    }

    /// Returns the estimated cell-validation work.
    #[must_use]
    pub const fn work_units(self) -> u64 {
        self.work_units
    }
}

/// Validates grid dimensions and resource limits before callers allocate cell storage.
///
/// The `u32 × u32` cell product is exact in `u64`. Byte multiplication, platform-size
/// conversion, and Rust's allocation-layout limit are checked separately. This function allocates
/// nothing.
pub fn validate_grid_budget(
    size: GridSize,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<GridBudget, Error> {
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if size.is_empty() {
        return Err(Error::Malformed(MalformedReason::EmptyDimensions));
    }

    let cell_count = size.cell_count();
    let cell_count_usize = usize::try_from(cell_count).map_err(|_| Error::SizeOverflow)?;
    let cell_size =
        u64::try_from(core::mem::size_of::<AsciiCell>()).map_err(|_| Error::SizeOverflow)?;
    let frame_bytes = cell_count
        .checked_mul(cell_size)
        .ok_or(Error::SizeOverflow)?;
    // `Vec` allocation layouts are bounded by `isize::MAX`, not merely by `usize::MAX`.
    let _frame_bytes_isize = isize::try_from(frame_bytes).map_err(|_| Error::SizeOverflow)?;
    let work_units = cell_count;

    if frame_bytes > budget.max_frame_bytes {
        return Err(Error::BudgetExceeded(BudgetKind::FrameBytes));
    }
    if work_units > budget.max_work_units {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }

    Ok(GridBudget {
        cell_count,
        cell_count_usize,
        frame_bytes,
        work_units,
    })
}
