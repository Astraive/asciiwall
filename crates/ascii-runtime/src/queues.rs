//! Independently bounded control and frame queues plus a portable handoff coordinator.
//!
//! Frame traffic can evict the oldest queued frame to preserve freshness, but every evicted
//! descriptor is explicitly discarded from the pool. Stop and permission revocation use a
//! two-bit atomic urgent mailbox, independent from both frame capacity and ordinary control queue
//! capacity; duplicate urgent events coalesce. Ordinary control messages remain FIFO and bounded.
//!
//! Queue locks are released before pool operations. The handoff coordinator's operation lock is
//! outermost and serializes publish, receive, and generation invalidation; it is never held while
//! invoking user code. See the crate-level lock and callback contract.

use std::collections::VecDeque;
use std::mem::size_of;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

use ascii_core::{Budget, CancellationToken, Error as CoreError};

use crate::lock_unpoisoned;
use crate::{
    BufferPool, FrameDescriptor, FrameLease, MemoryPermit, MemoryUsage, RasterDescriptorMetadata,
    ResourceBudget, RuntimeError,
};

const URGENT_STOP: u8 = 0b01;
const URGENT_REVOKE: u8 = 0b10;

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
/// The urgent mailbox is a fixed two-bit atomic state, so urgent shutdown/revocation events neither
/// allocate nor contend with an ordinary control queue that has reached capacity.
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
                let mut queue = lock_unpoisoned(&self.ordinary);
                if queue.items.len() == queue.capacity {
                    return Err(RuntimeError::QueueFull);
                }
                queue.items.push_back(ordinary);
                Ok(())
            }
        }
    }

    /// Receives urgent revocation/Stop first, then ordinary FIFO controls.
    pub fn try_recv(&self) -> Option<ControlEvent> {
        if let Some(event) = self.take_urgent() {
            return Some(event);
        }
        lock_unpoisoned(&self.ordinary).items.pop_front()
    }

    fn take_urgent(&self) -> Option<ControlEvent> {
        loop {
            let pending = self.urgent.load(Ordering::Acquire);
            let (bit, event) = if pending & URGENT_REVOKE != 0 {
                (URGENT_REVOKE, ControlEvent::RevokePermissions)
            } else if pending & URGENT_STOP != 0 {
                (URGENT_STOP, ControlEvent::Stop)
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
/// The coordinator first publishes a valid replacement into a free slot, then discards older
/// descriptors and reclaims their slots. A cancelled or invalid replacement leaves the queued
/// frame available; without a free slot, submission fails without evicting queued frames. A
/// consumed lease is never evicted or reused without its completion acknowledgement.
#[derive(Debug)]
pub struct BoundedHandoff {
    operation: Mutex<()>,
    pool: BufferPool,
    frames: FrameQueue,
    controls: ControlQueue,
}

impl BoundedHandoff {
    /// Creates fixed slot, frame-queue, and ordinary-control capacities using a shared budget.
    pub fn new(
        resource_budget: &ResourceBudget,
        generation: u64,
        slot_count: usize,
        bytes_per_slot: usize,
        frame_queue_capacity: usize,
        control_queue_capacity: usize,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            operation: Mutex::new(()),
            pool: BufferPool::new(resource_budget, generation, slot_count, bytes_per_slot)?,
            frames: FrameQueue::new(resource_budget, frame_queue_capacity)?,
            controls: ControlQueue::new(resource_budget, control_queue_capacity)?,
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

    /// Publishes the newest valid frame before releasing older queued frames.
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
        if pixels.len() > self.pool.bytes_per_slot() {
            return Err(RuntimeError::MappingBounds);
        }
        if cancellation.is_cancelled() {
            return Err(RuntimeError::CoreValidation(CoreError::Cancelled));
        }
        self.pool.reclaim_released();
        let mut fill = self.pool.begin_fill()?;
        fill.write(pixels)?;
        before_publish();
        let descriptor = fill.publish(metadata, budget, cancellation)?;
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
        )
    )
}

#[cfg(test)]
mod tests {
    use super::BoundedHandoff;
    use crate::{
        RasterDescriptorMetadata, ResourceBudget, ResourceLimits, RuntimeError, SlotState,
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
    fn cancelled_replacement_keeps_the_previous_queued_frame() {
        let resource_budget = resource_budget();
        let handoff = BoundedHandoff::new(&resource_budget, 1, 2, 3, 1, 1).unwrap();
        let limits = budget();
        let first = handoff
            .submit_frame(metadata(1), &[1, 2, 3], &limits, &CancellationToken::new())
            .unwrap();
        let cancellation = CancellationToken::new();

        let error = handoff
            .submit_frame_before_publish(metadata(2), &[4, 5, 6], &limits, &cancellation, || {
                cancellation.cancel()
            })
            .unwrap_err();

        assert_eq!(
            error,
            RuntimeError::CoreValidation(ascii_core::Error::Cancelled)
        );
        assert_eq!(handoff.queued_frames(), 1);
        assert_eq!(
            handoff.slot_state(first.slot_id).unwrap(),
            SlotState::Published
        );
        let mut lease = handoff
            .receive_frame(&limits, &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(lease.descriptor().media_timestamp_ns, 1);
        lease.completion_token().unwrap().acknowledge();
        lease.release().unwrap();
    }
}
