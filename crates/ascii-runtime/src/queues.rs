//! Independently bounded control and frame queues plus a portable handoff coordinator.
//!
//! Frame traffic can evict the oldest queued frame to preserve freshness, but every evicted
//! descriptor is explicitly discarded from the pool. Stop and permission revocation use a
//! two-bit atomic urgent mailbox, independent from both frame capacity and ordinary control queue
//! capacity; duplicate urgent events coalesce. Ordinary control messages remain FIFO and bounded.
//!
//! Queue locks are released before pool operations. The handoff coordinator's operation lock is
//! outermost and serializes publish, receive, and generation invalidation; it is never held while
//! invoking user code. Urgent publication and the final ordinary dequeue share an atomic commit
//! point, described at [`ControlQueue::try_recv`]. See the crate-level lock and callback contract.

use std::collections::VecDeque;
use std::mem::size_of;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(test)]
use std::sync::{Arc, Barrier};

use ascii_core::{Budget, CancellationToken, Error as CoreError};

use crate::lock_unpoisoned;
use crate::{
    BufferPool, FrameDescriptor, FrameLease, MemoryPermit, MemoryUsage, PermitKind,
    RasterDescriptorMetadata, ResourceBudget, RuntimeError,
};

const URGENT_STOP: u8 = 0b01;
const URGENT_REVOKE: u8 = 0b10;
const ORDINARY_DEQUEUE_COMMITTED: u8 = 0b100;
const URGENT_MASK: u8 = URGENT_STOP | URGENT_REVOKE;

/// Control messages that are independent from the frame-data queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlEvent {
    /// Stop the owning assignment or host. This uses the urgent mailbox.
    Stop,
    /// Immediately revoke the relevant permission/work generation. This uses the urgent mailbox.
    RevokePermissions,
    /// Ordinary bounded FIFO state update.
    AssignmentChanged { generation: u64 },
    /// Ordinary bounded FIFO state update.
    Refresh,
}

#[derive(Debug)]
struct QueueInner<T> {
    items: VecDeque<T>,
    capacity: usize,
    _memory_permit: MemoryPermit,
}

/// Fixed-capacity frame descriptor queue.
///
/// `push_latest` retains the newest frame and returns an evicted descriptor to its caller. A caller
/// that owns the corresponding buffer pool MUST discard/reclaim that descriptor rather than merely
/// dropping it.
#[derive(Debug)]
pub struct FrameQueue {
    inner: Mutex<QueueInner<FrameDescriptor>>,
}

impl FrameQueue {
    /// Creates a frame queue with a fixed nonzero capacity charged to the shared memory budget.
    pub fn new(resource_budget: &ResourceBudget, capacity: usize) -> Result<Self, RuntimeError> {
        Ok(Self {
            inner: Mutex::new(new_queue_inner(resource_budget, capacity)?),
        })
    }

    /// Returns the fixed queue capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        lock_unpoisoned(&self.inner).capacity
    }

    /// Returns the current number of queued descriptors.
    #[must_use]
    pub fn len(&self) -> usize {
        lock_unpoisoned(&self.inner).items.len()
    }

    /// Returns whether the queue is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Attempts to append one descriptor without evicting queued frames.
    pub fn try_send(&self, descriptor: FrameDescriptor) -> Result<(), FrameDescriptor> {
        let mut inner = lock_unpoisoned(&self.inner);
        if inner.items.len() == inner.capacity {
            return Err(descriptor);
        }
        inner.items.push_back(descriptor);
        Ok(())
    }

    /// Enqueues the newest frame, evicting and returning the oldest descriptor if full.
    pub fn push_latest(&self, descriptor: FrameDescriptor) -> Option<FrameDescriptor> {
        let mut inner = lock_unpoisoned(&self.inner);
        let evicted = if inner.items.len() == inner.capacity {
            inner.items.pop_front()
        } else {
            None
        };
        inner.items.push_back(descriptor);
        evicted
    }

    /// Removes the oldest queued descriptor without touching its buffer slot.
    pub fn try_recv(&self) -> Option<FrameDescriptor> {
        lock_unpoisoned(&self.inner).items.pop_front()
    }

    fn restore_front(&self, descriptor: FrameDescriptor) {
        let mut inner = lock_unpoisoned(&self.inner);
        debug_assert!(inner.items.len() < inner.capacity);
        inner.items.push_front(descriptor);
    }
}

/// Bounded FIFO ordinary controls plus reserved, coalescing Stop/revocation signals.
///
/// The urgent events occupy two fixed mailbox bits; a third atomic bit commits the final ordinary
/// dequeue against concurrent urgent publication. Urgent events therefore remain independent of
/// ordinary queue capacity, including when that queue is full.
#[derive(Debug)]
pub struct ControlQueue {
    ordinary: Mutex<QueueInner<ControlEvent>>,
    urgent: AtomicU8,
}

impl ControlQueue {
    /// Creates a bounded ordinary control queue charged to the shared budget. Stop/revocation
    /// remain available even at capacity.
    pub fn new(resource_budget: &ResourceBudget, capacity: usize) -> Result<Self, RuntimeError> {
        Ok(Self {
            ordinary: Mutex::new(new_queue_inner(resource_budget, capacity)?),
            urgent: AtomicU8::new(0),
        })
    }

    /// Returns the ordinary FIFO capacity (urgent flags have fixed capacity two).
    #[must_use]
    pub fn capacity(&self) -> usize {
        lock_unpoisoned(&self.ordinary).capacity
    }

    /// Returns the number of ordinary queued messages; urgent events are not included.
    #[must_use]
    pub fn len(&self) -> usize {
        lock_unpoisoned(&self.ordinary).items.len()
    }

    /// Returns whether no ordinary messages or urgent Stop/revocation events are pending.
    ///
    /// Unlike [`Self::len`], this includes the urgent mailbox, so it can be false when `len()`
    /// is zero.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let ordinary = lock_unpoisoned(&self.ordinary);
        ordinary.items.is_empty() && self.urgent.load(Ordering::Acquire) == 0
    }

    /// Queues an event without blocking. Stop and revocation bypass the ordinary bounded FIFO.
    pub fn try_send(&self, event: ControlEvent) -> Result<(), RuntimeError> {
        match event {
            ControlEvent::Stop => {
                self.urgent.fetch_or(URGENT_STOP, Ordering::Release);
                Ok(())
            }
            ControlEvent::RevokePermissions => {
                self.urgent.fetch_or(URGENT_REVOKE, Ordering::Release);
                Ok(())
            }
            ordinary => {
                let mut queue = match self.ordinary.try_lock() {
                    Ok(queue) => queue,
                    Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
                    Err(std::sync::TryLockError::WouldBlock) => {
                        return Err(RuntimeError::QueueContended);
                    }
                };
                if queue.items.len() == queue.capacity {
                    return Err(RuntimeError::QueueFull);
                }
                queue.items.push_back(ordinary);
                Ok(())
            }
        }
    }

    /// Receives urgent Stop first, then revocation, then ordinary FIFO controls.
    pub fn try_recv(&self) -> Option<ControlEvent> {
        self.try_recv_with_before_dequeue_commit(|| {})
    }

    fn try_recv_with_before_dequeue_commit<F>(
        &self,
        before_dequeue_commit: F,
    ) -> Option<ControlEvent>
    where
        F: FnOnce(),
    {
        if let Some(event) = self.take_urgent() {
            return Some(event);
        }
        self.try_recv_after_urgent_check(before_dequeue_commit)
    }

    fn try_recv_after_urgent_check<F>(&self, before_dequeue_commit: F) -> Option<ControlEvent>
    where
        F: FnOnce(),
    {
        let mut ordinary = lock_unpoisoned(&self.ordinary);
        let mut state = self.urgent.load(Ordering::Acquire);
        if state & URGENT_MASK != 0 {
            if let Some(event) = self.take_urgent() {
                return Some(event);
            }
            state = self.urgent.load(Ordering::Acquire);
        }
        if ordinary.items.is_empty() {
            return None;
        }

        before_dequeue_commit();
        loop {
            if state & URGENT_MASK != 0 {
                if let Some(event) = self.take_urgent() {
                    return Some(event);
                }
                state = self.urgent.load(Ordering::Acquire);
                continue;
            }

            match self.urgent.compare_exchange(
                state,
                state | ORDINARY_DEQUEUE_COMMITTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    // This CAS is the ordinary dequeue's linearization point: urgent publication
                    // either changes `state` first and wins, or follows this committed dequeue.
                    let event = ordinary.items.pop_front();
                    self.urgent
                        .fetch_and(!ORDINARY_DEQUEUE_COMMITTED, Ordering::AcqRel);
                    return event;
                }
                Err(actual) => state = actual,
            }
        }
    }

    fn take_urgent(&self) -> Option<ControlEvent> {
        loop {
            let pending = self.urgent.load(Ordering::Acquire);
            let (bit, event) = if pending & URGENT_STOP != 0 {
                (URGENT_STOP, ControlEvent::Stop)
            } else if pending & URGENT_REVOKE != 0 {
                (URGENT_REVOKE, ControlEvent::RevokePermissions)
            } else {
                return None;
            };
            if self
                .urgent
                .compare_exchange_weak(pending, pending & !bit, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Some(event);
            }
        }
    }
}

/// Coordinated latest-frame handoff with separate urgent control and fixed buffer slots.
///
/// The coordinator publishes a valid replacement before discarding older queued frames whenever
/// a free slot exists. Publication into a free slot commits atomically against cancellation: if
/// cancellation wins before the `Published` transition, the previous queued frame remains intact;
/// if publication wins, it completes. With no free slot, it stages replacement bytes before taking
/// over the oldest queued publication as a replacement fill lease. Cancellation that wins before
/// that acquisition leaves the prior publication intact. This replacement lease acquisition is
/// the point of no return: cancellation during or after copying no longer aborts replacement
/// publication. The staging buffer is charged to the shared memory budget. Consumed slots retain
/// output permits after acknowledgement while their lease remains attached; lease release/drop,
/// generation invalidation, or final pool-owner drop releases the permit without slot self-pinning.
#[derive(Debug)]
pub struct BoundedHandoff {
    operation: Mutex<()>,
    resource_budget: ResourceBudget,
    pool: BufferPool,
    replacement_staging: Mutex<Vec<u8>>,
    _replacement_staging_permit: MemoryPermit,
    frames: FrameQueue,
    controls: ControlQueue,
    #[cfg(test)]
    cancel_before_replacement_fill: Mutex<bool>,
    #[cfg(test)]
    cancel_after_replacement_fill_acquire: Mutex<bool>,
    #[cfg(test)]
    cancel_during_replacement_staging: Mutex<bool>,
    #[cfg(test)]
    pause_before_free_slot_publication: Mutex<Option<Arc<Barrier>>>,
}

impl BoundedHandoff {
    /// Creates fixed slot, replacement-staging, frame-queue, and ordinary-control capacities
    /// using a shared budget.
    pub fn new(
        resource_budget: &ResourceBudget,
        generation: u64,
        slot_count: usize,
        bytes_per_slot: usize,
        frame_queue_capacity: usize,
        control_queue_capacity: usize,
    ) -> Result<Self, RuntimeError> {
        let pool = BufferPool::new(resource_budget, generation, slot_count, bytes_per_slot)?;
        let staging_bytes =
            u64::try_from(bytes_per_slot).map_err(|_| RuntimeError::SizeOverflow)?;
        let replacement_staging_permit =
            resource_budget.try_reserve_memory(MemoryUsage::new(staging_bytes, 0))?;
        let mut replacement_staging = Vec::new();
        replacement_staging
            .try_reserve_exact(bytes_per_slot)
            .map_err(|_| RuntimeError::AllocationFailed)?;
        replacement_staging.resize(bytes_per_slot, 0);

        Ok(Self {
            operation: Mutex::new(()),
            resource_budget: resource_budget.clone(),
            pool,
            replacement_staging: Mutex::new(replacement_staging),
            _replacement_staging_permit: replacement_staging_permit,
            frames: FrameQueue::new(resource_budget, frame_queue_capacity)?,
            controls: ControlQueue::new(resource_budget, control_queue_capacity)?,
            #[cfg(test)]
            cancel_before_replacement_fill: Mutex::new(false),
            #[cfg(test)]
            cancel_after_replacement_fill_acquire: Mutex::new(false),
            #[cfg(test)]
            cancel_during_replacement_staging: Mutex::new(false),
            #[cfg(test)]
            pause_before_free_slot_publication: Mutex::new(None),
        })
    }

    /// Returns the control queue. It is independent from frame operations and can receive urgent
    /// signals even when frame traffic is saturated.
    #[must_use]
    pub const fn controls(&self) -> &ControlQueue {
        &self.controls
    }

    /// Returns the current frame queue length.
    #[must_use]
    pub fn queued_frames(&self) -> usize {
        self.frames.len()
    }

    /// Returns the current pool generation.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.pool.generation()
    }

    /// Returns one slot's state for bounded diagnostics/tests.
    pub fn slot_state(&self, slot_id: u32) -> Result<crate::SlotState, RuntimeError> {
        self.pool.slot_state(slot_id)
    }

    #[cfg(test)]
    fn pause_before_next_free_slot_publication(&self) -> Arc<Barrier> {
        let pause = Arc::new(Barrier::new(2));
        *lock_unpoisoned(&self.pause_before_free_slot_publication) = Some(Arc::clone(&pause));
        pause
    }

    #[cfg(test)]
    fn apply_test_pause_before_free_slot_publication(&self) {
        let pause = lock_unpoisoned(&self.pause_before_free_slot_publication).take();
        if let Some(pause) = pause {
            pause.wait();
            pause.wait();
        }
    }

    #[cfg(test)]
    fn cancel_during_next_replacement_staging(&self) {
        *lock_unpoisoned(&self.cancel_during_replacement_staging) = true;
    }

    #[cfg(test)]
    fn apply_test_replacement_staging_cancellation(&self, cancellation: &CancellationToken) {
        if std::mem::take(&mut *lock_unpoisoned(
            &self.cancel_during_replacement_staging,
        )) {
            cancellation.cancel();
        }
    }

    #[cfg(test)]
    fn cancel_before_next_replacement_fill(&self) {
        *lock_unpoisoned(&self.cancel_before_replacement_fill) = true;
    }

    #[cfg(test)]
    fn apply_test_cancellation_before_replacement_fill(&self, cancellation: &CancellationToken) {
        if std::mem::take(&mut *lock_unpoisoned(&self.cancel_before_replacement_fill)) {
            cancellation.cancel();
        }
    }

    #[cfg(test)]
    fn cancel_after_next_replacement_fill_acquire(&self) {
        *lock_unpoisoned(&self.cancel_after_replacement_fill_acquire) = true;
    }

    #[cfg(test)]
    fn apply_test_cancellation_after_replacement_fill_acquire(
        &self,
        cancellation: &CancellationToken,
    ) {
        if std::mem::take(&mut *lock_unpoisoned(
            &self.cancel_after_replacement_fill_acquire,
        )) {
            cancellation.cancel();
        }
    }

    /// Publishes the newest valid frame before releasing older queued frames when a free slot
    /// exists. Cancellation and the free-slot `Published` transition are linearized: cancellation
    /// that wins first preserves the old queue; publication that wins first completes. With no free
    /// slot, it stages replacement bytes before taking over the oldest queued publication. If
    /// cancellation wins before that acquisition, the prior publication is preserved. Replacement
    /// lease acquisition is the no-return point: cancellation during or after copying no longer
    /// aborts publication.
    pub fn submit_frame(
        &self,
        metadata: RasterDescriptorMetadata,
        pixels: &[u8],
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<FrameDescriptor, RuntimeError> {
        self.submit_frame_before_publish(metadata, pixels, budget, cancellation, || {})
    }

    fn submit_frame_before_publish<F>(
        &self,
        metadata: RasterDescriptorMetadata,
        pixels: &[u8],
        budget: &Budget,
        cancellation: &CancellationToken,
        before_publish: F,
    ) -> Result<FrameDescriptor, RuntimeError>
    where
        F: FnOnce(),
    {
        let _operation = lock_unpoisoned(&self.operation);
        let _cpu_permit = self.resource_budget.try_acquire_cpu()?;
        if pixels.len() > self.pool.bytes_per_slot() {
            return Err(RuntimeError::MappingBounds);
        }
        if cancellation.is_cancelled() {
            return Err(RuntimeError::CoreValidation(CoreError::Cancelled));
        }
        self.pool
            .validate_frame_input(metadata, pixels, budget, cancellation)?;
        self.pool.reclaim_released();
        let descriptor = match self.pool.begin_fill() {
            Ok(mut fill) => {
                fill.write_with_cpu_permit(pixels)?;
                before_publish();
                if cancellation.is_cancelled() {
                    return Err(RuntimeError::CoreValidation(CoreError::Cancelled));
                }
                #[cfg(test)]
                self.apply_test_pause_before_free_slot_publication();
                fill.publish_prevalidated_if_not_cancelled(metadata, cancellation)?
            }
            Err(RuntimeError::NoFreeSlots) => {
                before_publish();
                if cancellation.is_cancelled() {
                    return Err(RuntimeError::CoreValidation(CoreError::Cancelled));
                }
                let mut staging = lock_unpoisoned(&self.replacement_staging);
                staging[..pixels.len()].copy_from_slice(pixels);
                #[cfg(test)]
                self.apply_test_replacement_staging_cancellation(cancellation);
                if cancellation.is_cancelled() {
                    return Err(RuntimeError::CoreValidation(CoreError::Cancelled));
                }
                let Some(previous) = self.frames.try_recv() else {
                    return Err(RuntimeError::NoFreeSlots);
                };
                #[cfg(test)]
                self.apply_test_cancellation_before_replacement_fill(cancellation);
                let mut fill = match self.pool.begin_replacement_fill(&previous, cancellation) {
                    Ok(fill) => fill,
                    Err(error) => {
                        self.frames.restore_front(previous);
                        return Err(error);
                    }
                };
                #[cfg(test)]
                self.apply_test_cancellation_after_replacement_fill_acquire(cancellation);
                fill.write_with_cpu_permit(&staging[..pixels.len()])?;
                fill.publish_prevalidated(metadata)?
            }
            Err(error) => return Err(error),
        };
        while let Some(stale) = self.frames.try_recv() {
            let _ = self.pool.discard(&stale)?;
        }
        self.pool.reclaim_released();
        if let Some(evicted) = self.frames.push_latest(descriptor) {
            let _ = self.pool.discard(&evicted)?;
            self.pool.reclaim_released();
        }
        Ok(descriptor)
    }

    /// Receives the next valid current-generation frame, releasing stale queued descriptors.
    pub fn receive_frame(
        &self,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<Option<FrameLease>, RuntimeError> {
        let _operation = lock_unpoisoned(&self.operation);
        while let Some(descriptor) = self.frames.try_recv() {
            match self.pool.consume(&descriptor, budget, cancellation) {
                Ok(lease) => return Ok(Some(lease)),
                Err(RuntimeError::StaleGeneration) => {
                    let _ = self.pool.discard(&descriptor)?;
                    self.pool.reclaim_released();
                }
                Err(error) if is_retryable_receive_error(error) => {
                    self.frames.restore_front(descriptor);
                    return Err(error);
                }
                Err(error) => {
                    let _ = self.pool.discard(&descriptor)?;
                    self.pool.reclaim_released();
                    return Err(error);
                }
            }
        }
        Ok(None)
    }

    /// Invalidates the old mapping generation and drops all queued descriptors from it.
    ///
    /// The caller must first make the old transport mapping unavailable, as documented on
    /// [`BufferPool::invalidate_generation`].
    pub fn invalidate_generation(&self, new_generation: u64) -> Result<usize, RuntimeError> {
        let _operation = lock_unpoisoned(&self.operation);
        let invalidated = self.pool.invalidate_generation(new_generation)?;
        while self.frames.try_recv().is_some() {}
        Ok(invalidated)
    }
}

fn new_queue_inner<T>(
    resource_budget: &ResourceBudget,
    capacity: usize,
) -> Result<QueueInner<T>, RuntimeError> {
    if capacity == 0 {
        return Err(RuntimeError::InvalidCapacity);
    }
    let charged_len = capacity
        .checked_mul(size_of::<T>())
        .filter(|len| *len <= isize::MAX as usize)
        .ok_or(RuntimeError::SizeOverflow)?;
    let charged_bytes = u64::try_from(charged_len).map_err(|_| RuntimeError::SizeOverflow)?;
    let memory_permit = resource_budget.try_reserve_memory(MemoryUsage::new(charged_bytes, 0))?;

    let mut items = VecDeque::new();
    items
        .try_reserve_exact(capacity)
        .map_err(|_| RuntimeError::AllocationFailed)?;
    Ok(QueueInner {
        items,
        capacity,
        _memory_permit: memory_permit,
    })
}

fn is_retryable_receive_error(error: RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::CoreValidation(
            ascii_core::Error::Cancelled
                | ascii_core::Error::BudgetExceeded(_)
                | ascii_core::Error::TemporarilyUnavailable
        ) | RuntimeError::ConcurrencyLimit(PermitKind::Cpu | PermitKind::Output)
    )
}

#[cfg(test)]
mod tests {
    use super::{BoundedHandoff, ControlEvent, ControlQueue};
    use crate::{
        PermitKind, RasterDescriptorMetadata, ResourceBudget, ResourceLimits, RuntimeError,
        SlotState,
    };
    use ascii_core::{
        AlphaMode, Budget, CancellationToken, ColorDescription, PixelFormat, RasterOrientation,
    };

    fn budget() -> Budget {
        Budget::new(1 << 20, 1 << 20, 1 << 20)
    }

    fn resource_budget() -> ResourceBudget {
        ResourceBudget::new(ResourceLimits::new(1, 1, 1, u64::MAX, u64::MAX))
    }

    fn metadata(timestamp: u64) -> RasterDescriptorMetadata {
        RasterDescriptorMetadata {
            width: 1,
            height: 1,
            row_stride: 3,
            format: PixelFormat::rgb8(),
            color: ColorDescription::srgb(AlphaMode::Opaque),
            orientation: RasterOrientation::TopDown,
            media_timestamp_ns: timestamp,
        }
    }

    #[test]
    fn ordinary_control_send_reports_lock_contention_without_waiting() {
        let controls = ControlQueue::new(&resource_budget(), 1).unwrap();
        let _queue_lock = controls.ordinary.lock().unwrap();

        assert_eq!(
            controls.try_send(ControlEvent::Refresh),
            Err(RuntimeError::QueueContended)
        );
    }

    #[test]
    fn cancellation_winning_at_free_slot_publication_keeps_previous_frame() {
        let resource_budget = resource_budget();
        let handoff =
            std::sync::Arc::new(BoundedHandoff::new(&resource_budget, 1, 2, 3, 1, 1).unwrap());
        let limits = budget();
        let first = handoff
            .submit_frame(metadata(1), &[1, 2, 3], &limits, &CancellationToken::new())
            .unwrap();
        let cancellation = std::sync::Arc::new(CancellationToken::new());
        let pause = handoff.pause_before_next_free_slot_publication();
        let submission_handoff = std::sync::Arc::clone(&handoff);
        let submission_cancellation = std::sync::Arc::clone(&cancellation);
        let submission_limits = budget();
        let submission = std::thread::spawn(move || {
            submission_handoff.submit_frame(
                metadata(2),
                &[4, 5, 6],
                &submission_limits,
                &submission_cancellation,
            )
        });

        // The public submission pauses after its final cancellation check, before the commit gate.
        pause.wait();
        cancellation.cancel();
        pause.wait();

        assert_eq!(
            submission.join().unwrap(),
            Err(RuntimeError::CoreValidation(ascii_core::Error::Cancelled))
        );
        assert!(cancellation.is_cancelled());
        assert_eq!(handoff.queued_frames(), 1);
        assert_eq!(
            handoff.slot_state(first.slot_id).unwrap(),
            SlotState::Published
        );
        assert_eq!(handoff.slot_state(1).unwrap(), SlotState::Free);
        let mut lease = handoff
            .receive_frame(&limits, &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(*lease.descriptor(), first);
        lease
            .with_pixels(|pixels| assert_eq!(pixels, &[1, 2, 3]))
            .unwrap();
        lease.completion_token().unwrap().acknowledge();
        lease.release().unwrap();
    }

    #[test]
    fn cancelled_one_slot_replacement_keeps_the_previous_queued_frame() {
        let resource_budget = resource_budget();
        let handoff = BoundedHandoff::new(&resource_budget, 1, 1, 3, 1, 1).unwrap();
        let limits = budget();
        handoff
            .submit_frame(metadata(1), &[1, 2, 3], &limits, &CancellationToken::new())
            .unwrap();
        let cancellation = CancellationToken::new();

        assert_eq!(
            handoff.submit_frame_before_publish(
                metadata(2),
                &[4, 5, 6],
                &limits,
                &cancellation,
                || cancellation.cancel(),
            ),
            Err(RuntimeError::CoreValidation(ascii_core::Error::Cancelled))
        );
        assert_eq!(handoff.queued_frames(), 1);
        assert_eq!(handoff.slot_state(0).unwrap(), SlotState::Published);
        let mut lease = handoff
            .receive_frame(&limits, &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(lease.descriptor().media_timestamp_ns, 1);
        lease.completion_token().unwrap().acknowledge();
        lease.release().unwrap();
    }

    #[test]
    fn cancellation_during_one_slot_replacement_staging_preserves_previous_frame() {
        let resource_budget = resource_budget();
        let handoff = BoundedHandoff::new(&resource_budget, 1, 1, 3, 1, 1).unwrap();
        let limits = budget();
        handoff
            .submit_frame(metadata(1), &[1, 2, 3], &limits, &CancellationToken::new())
            .unwrap();
        let cancellation = CancellationToken::new();
        handoff.cancel_during_next_replacement_staging();

        assert_eq!(
            handoff.submit_frame(metadata(2), &[4, 5, 6], &limits, &cancellation),
            Err(RuntimeError::CoreValidation(ascii_core::Error::Cancelled))
        );
        assert!(cancellation.is_cancelled());
        assert_eq!(handoff.queued_frames(), 1);
        assert_eq!(handoff.slot_state(0).unwrap(), SlotState::Published);
        let mut lease = handoff
            .receive_frame(&limits, &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(lease.descriptor().media_timestamp_ns, 1);
        lease
            .with_pixels(|pixels| assert_eq!(pixels, &[1, 2, 3]))
            .unwrap();
        lease.completion_token().unwrap().acknowledge();
        lease.release().unwrap();
    }

    #[test]
    fn cancellation_after_staging_check_before_fill_acquisition_preserves_previous_frame() {
        let resource_budget = resource_budget();
        let handoff = BoundedHandoff::new(&resource_budget, 1, 1, 3, 1, 1).unwrap();
        let limits = budget();
        handoff
            .submit_frame(metadata(1), &[1, 2, 3], &limits, &CancellationToken::new())
            .unwrap();
        let cancellation = CancellationToken::new();
        handoff.cancel_before_next_replacement_fill();

        assert_eq!(
            handoff.submit_frame(metadata(2), &[4, 5, 6], &limits, &cancellation),
            Err(RuntimeError::CoreValidation(ascii_core::Error::Cancelled))
        );
        assert!(cancellation.is_cancelled());
        assert_eq!(handoff.queued_frames(), 1);
        assert_eq!(handoff.slot_state(0).unwrap(), SlotState::Published);
        let mut lease = handoff
            .receive_frame(&limits, &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(lease.descriptor().media_timestamp_ns, 1);
        lease
            .with_pixels(|pixels| assert_eq!(pixels, &[1, 2, 3]))
            .unwrap();
        lease.completion_token().unwrap().acknowledge();
        lease.release().unwrap();
    }

    #[test]
    fn cancellation_after_replacement_fill_acquisition_finishes_publication() {
        let resource_budget = resource_budget();
        let handoff = BoundedHandoff::new(&resource_budget, 1, 1, 3, 1, 1).unwrap();
        let limits = budget();
        handoff
            .submit_frame(metadata(1), &[1, 2, 3], &limits, &CancellationToken::new())
            .unwrap();
        let cancellation = CancellationToken::new();
        handoff.cancel_after_next_replacement_fill_acquire();

        let replacement = handoff
            .submit_frame(metadata(2), &[4, 5, 6], &limits, &cancellation)
            .unwrap();

        assert!(cancellation.is_cancelled());
        assert_eq!(handoff.queued_frames(), 1);
        assert_eq!(handoff.slot_state(0).unwrap(), SlotState::Published);
        let mut lease = handoff
            .receive_frame(&limits, &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(*lease.descriptor(), replacement);
        assert_eq!(lease.descriptor().media_timestamp_ns, 2);
        lease
            .with_pixels(|pixels| assert_eq!(pixels, &[4, 5, 6]))
            .unwrap();
        lease.completion_token().unwrap().acknowledge();
        lease.release().unwrap();
    }

    #[test]
    fn pending_stop_before_fifo_commit_wins_the_dequeue_race() {
        let resource_budget = resource_budget();
        let controls = ControlQueue::new(&resource_budget, 1).unwrap();
        controls.try_send(ControlEvent::Refresh).unwrap();

        assert_eq!(
            controls.try_recv_with_before_dequeue_commit(|| {
                controls.try_send(ControlEvent::Stop).unwrap();
            }),
            Some(ControlEvent::Stop)
        );
        assert_eq!(controls.try_recv(), Some(ControlEvent::Refresh));
    }

    #[test]
    fn cpu_submission_permit_is_shared_and_nonblocking() {
        let resource_budget = ResourceBudget::new(ResourceLimits::new(1, 1, 1, u64::MAX, u64::MAX));
        let handoff = BoundedHandoff::new(&resource_budget, 1, 1, 3, 1, 1).unwrap();
        let limits = budget();

        handoff
            .submit_frame_before_publish(
                metadata(1),
                &[1, 2, 3],
                &limits,
                &CancellationToken::new(),
                || {
                    assert_eq!(
                        resource_budget.try_acquire_cpu().unwrap_err(),
                        RuntimeError::ConcurrencyLimit(PermitKind::Cpu)
                    );
                },
            )
            .unwrap();

        assert_eq!(resource_budget.snapshot().cpu_jobs, 0);
    }
}
