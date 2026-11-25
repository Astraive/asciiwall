//! Bounded portable conversion-worker admission and generation-aware completion.
//!
//! Work is split into stable contiguous row tiles. Each admitted worker receives one range of
//! tiles, so no work queue or per-cell allocation is needed and error-diffusion stages remain
//! outside this independent-work scheduler.

use core::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ascii_core::{BudgetKind, CancellationToken, Error};

const TILE_ROWS: usize = 4;

/// Shared aggregate memory and worker admission for concurrently executing conversions.
///
/// Clone this value and pass it to every conversion preview that must share the same ceiling.
/// A conversion reserves its complete preflighted input, output, glyph-table, queue, and scratch
/// footprint before allocating, then releases that reservation on success, cancellation, or error.
/// Worker slots are admitted independently and bounded across all conversions using this owner.
#[derive(Clone, Debug)]
pub struct ConversionResources {
    shared: Arc<SharedResources>,
}

#[derive(Debug)]
struct SharedResources {
    max_active_bytes: u64,
    active_bytes: AtomicU64,
    max_workers: usize,
    active_workers: AtomicU64,
}

impl ConversionResources {
    /// Creates aggregate conversion admission with explicit memory and worker ceilings.
    #[must_use]
    pub fn new(max_active_bytes: u64, max_workers: NonZeroUsize) -> Self {
        Self {
            shared: Arc::new(SharedResources {
                max_active_bytes,
                active_bytes: AtomicU64::new(0),
                max_workers: max_workers.get(),
                active_workers: AtomicU64::new(0),
            }),
        }
    }

    /// Returns the aggregate live-job byte ceiling.
    #[must_use]
    pub fn max_active_bytes(&self) -> u64 {
        self.shared.max_active_bytes
    }

    /// Returns the aggregate worker-slot ceiling.
    #[must_use]
    pub fn max_workers(&self) -> usize {
        self.shared.max_workers
    }

    /// Returns bytes currently reserved by admitted conversion jobs.
    #[must_use]
    pub fn active_bytes(&self) -> u64 {
        self.shared.active_bytes.load(Ordering::Acquire)
    }

    /// Returns worker slots currently reserved by admitted conversion jobs.
    #[must_use]
    pub fn active_workers(&self) -> usize {
        usize::try_from(self.shared.active_workers.load(Ordering::Acquire))
            .unwrap_or(self.shared.max_workers)
    }

    fn reserve_workers(
        &self,
        requested: usize,
        tile_count: usize,
    ) -> Result<WorkerReservation, Error> {
        let requested = requested
            .min(tile_count)
            .min(self.shared.max_workers)
            .max(1);
        let mut active = self.shared.active_workers.load(Ordering::Acquire);
        loop {
            let available = self
                .shared
                .max_workers
                .checked_sub(usize::try_from(active).map_err(|_| Error::SizeOverflow)?)
                .ok_or(Error::BudgetExceeded(BudgetKind::WorkUnits))?;
            if available == 0 {
                return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
            }
            let count = requested.min(available);
            let next = active
                .checked_add(u64::try_from(count).map_err(|_| Error::SizeOverflow)?)
                .ok_or(Error::BudgetExceeded(BudgetKind::WorkUnits))?;
            match self.shared.active_workers.compare_exchange_weak(
                active,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(WorkerReservation {
                        shared: Arc::clone(&self.shared),
                        workers: count,
                    });
                }
                Err(observed) => active = observed,
            }
        }
    }

    fn reserve_bytes(&self, bytes: u64) -> Result<ByteReservation, Error> {
        let mut active = self.shared.active_bytes.load(Ordering::Acquire);
        loop {
            let next = active
                .checked_add(bytes)
                .ok_or(Error::BudgetExceeded(BudgetKind::RasterBytes))?;
            if next > self.shared.max_active_bytes {
                return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
            }
            match self.shared.active_bytes.compare_exchange_weak(
                active,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(ByteReservation {
                        shared: Arc::clone(&self.shared),
                        bytes,
                    });
                }
                Err(observed) => active = observed,
            }
        }
    }
}

#[derive(Debug)]
struct WorkerReservation {
    shared: Arc<SharedResources>,
    workers: usize,
}

impl Drop for WorkerReservation {
    fn drop(&mut self) {
        let previous = self.shared.active_workers.fetch_sub(
            u64::try_from(self.workers).expect("worker count fits u64"),
            Ordering::AcqRel,
        );
        debug_assert!(
            previous >= u64::try_from(self.workers).expect("worker count fits u64"),
            "conversion worker reservation underflow"
        );
    }
}

#[derive(Debug)]
struct ByteReservation {
    shared: Arc<SharedResources>,
    bytes: u64,
}

impl Drop for ByteReservation {
    fn drop(&mut self) {
        let previous = self
            .shared
            .active_bytes
            .fetch_sub(self.bytes, Ordering::AcqRel);
        debug_assert!(
            previous >= self.bytes,
            "conversion byte reservation underflow"
        );
    }
}

/// Monotonic conversion-generation owner used to supersede older preview requests.
///
/// Generation checks happen at each worker tile. [`Self::publish_if_current`] serializes the
/// final publication callback against generation advancement, so a result that became stale
/// cannot publish preview or runtime state.
#[derive(Debug)]
pub struct GenerationController {
    current: Mutex<u64>,
}

impl GenerationController {
    /// Creates a controller with the initial active generation.
    #[must_use]
    pub fn new(initial_generation: u64) -> Self {
        Self {
            current: Mutex::new(initial_generation),
        }
    }

    /// Returns the currently active generation.
    #[must_use]
    pub fn current(&self) -> u64 {
        *self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Advances the active generation and returns its identity.
    pub fn advance(&self) -> Result<u64, Error> {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = current.checked_add(1).ok_or(Error::SizeOverflow)?;
        Ok(*current)
    }

    /// Returns whether `generation` is still active.
    #[must_use]
    pub fn is_current(&self, generation: u64) -> bool {
        self.current() == generation
    }

    /// Publishes a completed value only while its generation is still current.
    ///
    /// The callback runs while generation advancement is excluded. A stale value is dropped and
    /// `false` is returned without invoking the callback.
    pub fn publish_if_current(&self, generation: u64, value: impl FnOnce()) -> bool {
        let current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *current != generation {
            return false;
        }
        value();
        true
    }
}

/// One stable contiguous group of row tiles assigned to a worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WorkerRange {
    pub(crate) first_row: usize,
    pub(crate) end_row: usize,
    pub(crate) generation_id: u64,
}

impl WorkerRange {
    pub(crate) fn tiles(self) -> WorkerTiles {
        WorkerTiles {
            next_row: self.first_row,
            end_row: self.end_row,
        }
    }
}

pub(crate) struct WorkerRanges {
    rows: usize,
    tile_count: usize,
    worker_count: usize,
    generation_id: u64,
    next_worker: usize,
}

impl Iterator for WorkerRanges {
    type Item = WorkerRange;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_worker == self.worker_count {
            return None;
        }
        let worker_index = self.next_worker;
        self.next_worker += 1;
        let tiles_per_worker = self.tile_count / self.worker_count;
        let extra_tiles = self.tile_count % self.worker_count;
        let first_tile = worker_index * tiles_per_worker + worker_index.min(extra_tiles);
        let worker_tiles = tiles_per_worker + usize::from(worker_index < extra_tiles);
        let end_tile = first_tile + worker_tiles;
        Some(WorkerRange {
            first_row: first_tile * TILE_ROWS,
            end_row: (end_tile * TILE_ROWS).min(self.rows),
            generation_id: self.generation_id,
        })
    }
}

pub(crate) struct WorkerTiles {
    next_row: usize,
    end_row: usize,
}

impl Iterator for WorkerTiles {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_row >= self.end_row {
            return None;
        }
        let start = self.next_row;
        self.next_row = self.next_row.saturating_add(TILE_ROWS).min(self.end_row);
        Some((start, self.next_row))
    }
}

/// One admitted conversion's generation and aggregate-resource lease.
pub(crate) struct JobExecution<'a> {
    resources: Option<&'a ConversionResources>,
    generation: Option<&'a GenerationController>,
    generation_id: u64,
    worker_count: usize,
    _worker_reservation: Option<WorkerReservation>,
    _byte_reservation: Option<ByteReservation>,
}

impl<'a> JobExecution<'a> {
    pub(crate) fn begin(
        resources: Option<&'a ConversionResources>,
        generation: Option<&'a GenerationController>,
        generation_id: u64,
        requested_workers: NonZeroUsize,
        rows: usize,
    ) -> Result<Self, Error> {
        if generation.is_some_and(|controller| !controller.is_current(generation_id)) {
            return Err(Error::Cancelled);
        }
        let tile_count = rows.div_ceil(TILE_ROWS);
        let worker_reservation = resources
            .map(|owner| owner.reserve_workers(requested_workers.get(), tile_count))
            .transpose()?;
        let worker_count = worker_reservation
            .as_ref()
            .map_or(1, |reservation| reservation.workers);
        Ok(Self {
            resources,
            generation,
            generation_id,
            worker_count,
            _worker_reservation: worker_reservation,
            _byte_reservation: None,
        })
    }

    pub(crate) const fn worker_count(&self) -> usize {
        self.worker_count
    }

    pub(crate) fn reserve_memory(&mut self, bytes: u64) -> Result<(), Error> {
        self._byte_reservation = self
            .resources
            .map(|owner| owner.reserve_bytes(bytes))
            .transpose()?;
        Ok(())
    }

    pub(crate) fn ranges(&self, rows: usize) -> WorkerRanges {
        let tile_count = rows.div_ceil(TILE_ROWS);
        WorkerRanges {
            rows,
            tile_count,
            worker_count: self.worker_count.min(tile_count).max(1),
            generation_id: self.generation_id,
            next_worker: 0,
        }
    }

    /// Checks that the completed work item still belongs to this active generation.
    pub(crate) fn check_range(
        &self,
        range: WorkerRange,
        cancellation: &CancellationToken,
    ) -> Result<(), Error> {
        if range.generation_id != self.generation_id {
            return Err(Error::Cancelled);
        }
        self.check(cancellation)
    }

    /// Checks explicit cancellation and generation freshness at a tile boundary.
    pub(crate) fn check(&self, cancellation: &CancellationToken) -> Result<(), Error> {
        if cancellation.is_cancelled()
            || self
                .generation
                .is_some_and(|gate| !gate.is_current(self.generation_id))
        {
            return Err(Error::Cancelled);
        }
        Ok(())
    }

    pub(crate) fn is_current(&self) -> bool {
        self.generation
            .is_none_or(|gate| gate.is_current(self.generation_id))
    }

    #[cfg(test)]
    pub(crate) fn generation_id(&self) -> u64 {
        self.generation_id
    }
}

#[cfg(test)]
mod tests {
    use super::{ConversionResources, GenerationController, JobExecution};
    use ascii_core::{BudgetKind, CancellationToken, Error};
    use core::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn row_ranges_are_stable_contiguous_and_tile_bounded() {
        let resources = ConversionResources::new(10_000, NonZeroUsize::new(3).unwrap());
        let job = JobExecution::begin(Some(&resources), None, 9, NonZeroUsize::new(3).unwrap(), 19)
            .unwrap();
        let ranges = job.ranges(19).collect::<Vec<_>>();
        assert!(ranges.iter().all(|range| range.generation_id == 9));

        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].first_row, 0);
        assert_eq!(ranges[0].end_row, 8);
        assert_eq!(ranges[1].first_row, 8);
        assert_eq!(ranges[1].end_row, 16);
        assert_eq!(ranges[2].first_row, 16);
        assert_eq!(ranges[2].end_row, 19);
        assert_eq!(resources.active_workers(), 3);
    }

    #[test]
    fn aggregate_admission_is_bounded_and_releases_on_failure_and_drop() {
        let resources = ConversionResources::new(32, NonZeroUsize::new(2).unwrap());
        let mut first =
            JobExecution::begin(Some(&resources), None, 1, NonZeroUsize::new(2).unwrap(), 8)
                .unwrap();
        first.reserve_memory(32).unwrap();
        assert_eq!(resources.active_bytes(), 32);
        assert_eq!(resources.active_workers(), 2);
        assert_eq!(
            JobExecution::begin(Some(&resources), None, 2, NonZeroUsize::new(1).unwrap(), 4,).err(),
            Some(Error::BudgetExceeded(BudgetKind::WorkUnits))
        );
        drop(first);
        assert_eq!(resources.active_bytes(), 0);
        assert_eq!(resources.active_workers(), 0);
    }

    #[test]
    fn generation_advance_rejects_old_completion_and_publication() {
        let generation = GenerationController::new(7);
        assert_eq!(generation.advance().unwrap(), 8);
        assert!(!generation.is_current(7));
        let published = AtomicUsize::new(0);
        assert!(!generation.publish_if_current(7, || {
            published.store(1, Ordering::Release);
        }));
        assert_eq!(published.load(Ordering::Acquire), 0);
        assert!(generation.publish_if_current(8, || {
            published.store(2, Ordering::Release);
        }));
        assert_eq!(published.load(Ordering::Acquire), 2);
    }

    #[test]
    fn stale_generation_is_cancellation_at_admission() {
        let generation = GenerationController::new(12);
        generation.advance().unwrap();
        assert_eq!(
            JobExecution::begin(
                None,
                Some(&generation),
                12,
                NonZeroUsize::new(1).unwrap(),
                1,
            )
            .err(),
            Some(Error::Cancelled)
        );
        assert!(
            JobExecution::begin(
                None,
                Some(&generation),
                13,
                NonZeroUsize::new(1).unwrap(),
                1,
            )
            .unwrap()
            .check(&CancellationToken::new())
            .is_ok()
        );
    }

    #[test]
    fn advancing_generation_cancels_queued_range_and_releases_worker_slot() {
        let resources = ConversionResources::new(1024, NonZeroUsize::new(1).unwrap());
        let generation = GenerationController::new(4);
        let job = JobExecution::begin(
            Some(&resources),
            Some(&generation),
            4,
            NonZeroUsize::new(1).unwrap(),
            8,
        )
        .unwrap();
        let range = job.ranges(8).next().unwrap();
        generation.advance().unwrap();

        assert_eq!(
            job.check_range(range, &CancellationToken::new()),
            Err(Error::Cancelled)
        );
        drop(job);
        assert_eq!(resources.active_workers(), 0);
    }

    #[test]
    fn simultaneous_jobs_share_the_aggregate_byte_ceiling() {
        let resources = ConversionResources::new(32, NonZeroUsize::new(2).unwrap());
        let mut first =
            JobExecution::begin(Some(&resources), None, 1, NonZeroUsize::new(1).unwrap(), 8)
                .unwrap();
        let mut second =
            JobExecution::begin(Some(&resources), None, 2, NonZeroUsize::new(1).unwrap(), 8)
                .unwrap();

        first.reserve_memory(24).unwrap();
        assert_eq!(
            second.reserve_memory(9),
            Err(Error::BudgetExceeded(BudgetKind::RasterBytes))
        );
        assert_eq!(resources.active_bytes(), 24);
        assert_eq!(resources.active_workers(), 2);

        drop(second);
        assert_eq!(resources.active_bytes(), 24);
        assert_eq!(resources.active_workers(), 1);
        drop(first);
        assert_eq!(resources.active_bytes(), 0);
        assert_eq!(resources.active_workers(), 0);
    }
}
