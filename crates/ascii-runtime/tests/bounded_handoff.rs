use std::mem::size_of;

use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, Error as CoreError,
    MalformedReason, PixelChannels, PixelFormat, RasterOrientation, UnsupportedReason,
};
use ascii_runtime::{
    BoundedHandoff, BufferPool, ControlEvent, ControlQueue, FrameDescriptor, FrameQueue,
    MemoryUsage, RasterDescriptorMetadata, ResourceBudget, ResourceLimits, RuntimeError, SlotState,
};

fn budget() -> Budget {
    Budget::new(1 << 20, 1 << 20, 1 << 20)
}

fn runtime_budget() -> ResourceBudget {
    ResourceBudget::new(ResourceLimits::new(1, 1, 1, u64::MAX, u64::MAX))
}

fn resource_budget_with_cpu_limit(cpu_memory_bytes: u64) -> ResourceBudget {
    ResourceBudget::new(ResourceLimits::new(1, 1, 1, cpu_memory_bytes, u64::MAX))
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

fn publish(pool: &BufferPool, timestamp: u64) -> FrameDescriptor {
    let mut fill = pool.begin_fill().unwrap();
    fill.write(&[timestamp as u8, 2, 3]).unwrap();
    fill.publish(metadata(timestamp), &budget(), &CancellationToken::new())
        .unwrap()
}

#[test]
fn control_queue_emptiness_includes_urgent_events_but_len_counts_only_ordinary() {
    let controls = ControlQueue::new(&runtime_budget(), 1).unwrap();

    assert!(controls.is_empty());
    assert_eq!(controls.len(), 0);

    controls.try_send(ControlEvent::Stop).unwrap();
    assert_eq!(controls.len(), 0);
    assert!(!controls.is_empty());
    assert_eq!(controls.try_recv(), Some(ControlEvent::Stop));
    assert!(controls.is_empty());

    controls.try_send(ControlEvent::RevokePermissions).unwrap();
    assert_eq!(controls.len(), 0);
    assert!(!controls.is_empty());
    assert_eq!(controls.try_recv(), Some(ControlEvent::RevokePermissions));
    assert!(controls.is_empty());

    controls.try_send(ControlEvent::Refresh).unwrap();
    assert_eq!(controls.len(), 1);
    assert!(!controls.is_empty());
    assert_eq!(controls.try_recv(), Some(ControlEvent::Refresh));
    assert!(controls.is_empty());
}

#[test]
fn frame_flood_stays_bounded_and_stop_bypasses_full_control_queue() {
    let handoff = BoundedHandoff::new(&runtime_budget(), 1, 2, 3, 2, 1).unwrap();
    let limits = budget();
    let cancellation = CancellationToken::new();

    handoff.controls().try_send(ControlEvent::Refresh).unwrap();
    assert_eq!(
        handoff
            .controls()
            .try_send(ControlEvent::AssignmentChanged { generation: 1 }),
        Err(RuntimeError::QueueFull)
    );

    for timestamp in 0..128 {
        handoff
            .submit_frame(
                metadata(timestamp),
                &[timestamp as u8, 2, 3],
                &limits,
                &cancellation,
            )
            .unwrap();
        assert!(handoff.queued_frames() <= 1);
    }

    handoff.controls().try_send(ControlEvent::Stop).unwrap();
    handoff
        .controls()
        .try_send(ControlEvent::RevokePermissions)
        .unwrap();
    assert_eq!(
        handoff.controls().try_recv(),
        Some(ControlEvent::RevokePermissions)
    );
    assert_eq!(handoff.controls().try_recv(), Some(ControlEvent::Stop));
    assert_eq!(handoff.controls().try_recv(), Some(ControlEvent::Refresh));
    assert_eq!(handoff.queued_frames(), 1);
    let published_slots = (0..2)
        .filter(|&slot| handoff.slot_state(slot).unwrap() == SlotState::Published)
        .count();
    let free_slots = (0..2)
        .filter(|&slot| handoff.slot_state(slot).unwrap() == SlotState::Free)
        .count();
    assert_eq!(published_slots, 1);
    assert_eq!(free_slots, 1);
    let mut latest = handoff
        .receive_frame(&limits, &cancellation)
        .unwrap()
        .unwrap();
    assert_eq!(latest.descriptor().media_timestamp_ns, 127);
    latest.completion_token().unwrap().acknowledge();
    latest.release().unwrap();
}

#[test]
fn retryable_receive_failures_retain_published_frame_for_later_receive() {
    let handoff = BoundedHandoff::new(&runtime_budget(), 1, 1, 3, 1, 1).unwrap();
    handoff
        .submit_frame(
            metadata(1),
            &[1, 2, 3],
            &budget(),
            &CancellationToken::new(),
        )
        .unwrap();

    let too_small = Budget::new(1 << 20, 2, 1 << 20);
    assert_eq!(
        handoff
            .receive_frame(&too_small, &CancellationToken::new())
            .unwrap_err(),
        RuntimeError::CoreValidation(CoreError::BudgetExceeded(BudgetKind::RasterBytes))
    );
    assert_eq!(handoff.queued_frames(), 1);
    assert_eq!(handoff.slot_state(0).unwrap(), SlotState::Published);

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        handoff.receive_frame(&budget(), &cancelled).unwrap_err(),
        RuntimeError::CoreValidation(CoreError::Cancelled)
    );
    assert_eq!(handoff.queued_frames(), 1);
    assert_eq!(handoff.slot_state(0).unwrap(), SlotState::Published);

    let mut lease = handoff
        .receive_frame(&budget(), &CancellationToken::new())
        .unwrap()
        .unwrap();
    lease.completion_token().unwrap().acknowledge();
    lease.release().unwrap();
}

#[test]
fn slot_ownership_requires_explicit_completion_and_release() {
    let pool = BufferPool::new(&runtime_budget(), 1, 1, 3).unwrap();
    assert_eq!(pool.slot_state(0).unwrap(), SlotState::Free);

    let mut fill = pool.begin_fill().unwrap();
    assert_eq!(pool.slot_state(0).unwrap(), SlotState::Filling);
    fill.write(&[1, 2, 3]).unwrap();
    let descriptor = fill
        .publish(metadata(0), &budget(), &CancellationToken::new())
        .unwrap();
    assert_eq!(pool.slot_state(0).unwrap(), SlotState::Published);

    let mut lease = pool
        .consume(&descriptor, &budget(), &CancellationToken::new())
        .unwrap();
    assert_eq!(pool.slot_state(0).unwrap(), SlotState::Consumed);
    assert_eq!(
        pool.consume(&descriptor, &budget(), &CancellationToken::new())
            .unwrap_err(),
        RuntimeError::InvalidState
    );
    assert_eq!(lease.with_pixels(|pixels| pixels.len()).unwrap(), 3);
    assert_eq!(lease.release(), Err(RuntimeError::CompletionPending));
    assert_eq!(pool.begin_fill().unwrap_err(), RuntimeError::NoFreeSlots);

    let token = lease.completion_token().unwrap();
    assert_eq!(
        lease.completion_token().unwrap_err(),
        RuntimeError::InvalidState
    );
    assert_eq!(lease.release(), Err(RuntimeError::CompletionPending));
    token.acknowledge();
    lease.release().unwrap();
    assert_eq!(pool.slot_state(0).unwrap(), SlotState::Released);
    assert_eq!(pool.reclaim_released(), 1);
    assert_eq!(pool.slot_state(0).unwrap(), SlotState::Free);
}

#[test]
fn dropped_unacknowledged_completion_token_keeps_lease_pending() {
    let pool = BufferPool::new(&runtime_budget(), 1, 1, 3).unwrap();
    let descriptor = publish(&pool, 1);
    let mut lease = pool
        .consume(&descriptor, &budget(), &CancellationToken::new())
        .unwrap();

    drop(lease.completion_token().unwrap());
    assert_eq!(
        lease.completion_token().unwrap_err(),
        RuntimeError::InvalidState
    );
    assert_eq!(lease.release(), Err(RuntimeError::CompletionPending));
    assert_eq!(pool.slot_state(0).unwrap(), SlotState::Consumed);

    assert_eq!(pool.invalidate_generation(2).unwrap(), 1);
    lease.release().unwrap();
    assert_eq!(pool.slot_state(0).unwrap(), SlotState::Free);
}

#[test]
fn descriptor_mapping_and_generation_are_checked_before_reading() {
    let pool = BufferPool::new(&runtime_budget(), 1, 2, 4).unwrap();
    let descriptor = publish(&pool, 1);

    let mut outside_slot = descriptor;
    // Still within the full 8-byte region, but not within the declared slot 0 mapping.
    outside_slot.mapped_offset = 4;
    assert_eq!(
        pool.consume(&outside_slot, &budget(), &CancellationToken::new())
            .unwrap_err(),
        RuntimeError::MappingBounds
    );

    let mut outside_region = descriptor;
    outside_region.mapped_offset = u64::MAX;
    assert_eq!(
        pool.consume(&outside_region, &budget(), &CancellationToken::new())
            .unwrap_err(),
        RuntimeError::MappingBounds
    );
    assert_eq!(pool.slot_state(0).unwrap(), SlotState::Published);

    assert_eq!(pool.invalidate_generation(2).unwrap(), 1);
    assert_eq!(
        pool.consume(&descriptor, &budget(), &CancellationToken::new())
            .unwrap_err(),
        RuntimeError::StaleGeneration
    );
    assert_eq!(pool.slot_state(0).unwrap(), SlotState::Free);
}

fn invalid_publish(metadata: RasterDescriptorMetadata, pixels: &[u8]) -> RuntimeError {
    let pool = BufferPool::new(&runtime_budget(), 1, 1, 8).unwrap();
    let mut fill = pool.begin_fill().unwrap();
    fill.write(pixels).unwrap();
    fill.publish(metadata, &budget(), &CancellationToken::new())
        .unwrap_err()
}

#[test]
fn raster_dimensions_stride_format_and_length_use_core_validation() {
    let mut empty = metadata(0);
    empty.width = 0;
    assert_eq!(
        invalid_publish(empty, &[1, 2, 3]),
        RuntimeError::CoreValidation(CoreError::Malformed(MalformedReason::EmptyDimensions))
    );

    let mut short_stride = metadata(0);
    short_stride.width = 2;
    assert_eq!(
        invalid_publish(short_stride, &[1, 2, 3, 4, 5, 6]),
        RuntimeError::CoreValidation(CoreError::Malformed(MalformedReason::RowStrideTooSmall))
    );

    let mut unsupported_format = metadata(0);
    unsupported_format.format = PixelFormat::new(PixelChannels::Other(1), 8);
    assert_eq!(
        invalid_publish(unsupported_format, &[1, 2, 3]),
        RuntimeError::CoreValidation(CoreError::Unsupported(UnsupportedReason::PixelFormat))
    );

    assert_eq!(
        invalid_publish(metadata(0), &[1, 2]),
        RuntimeError::CoreValidation(CoreError::Malformed(MalformedReason::RasterBackingTooShort))
    );
}

#[test]
fn crash_generation_invalidation_severs_an_outstanding_lease() {
    let handoff = BoundedHandoff::new(&runtime_budget(), 1, 1, 3, 1, 1).unwrap();
    handoff
        .submit_frame(
            metadata(1),
            &[1, 2, 3],
            &budget(),
            &CancellationToken::new(),
        )
        .unwrap();
    let mut old_lease = handoff
        .receive_frame(&budget(), &CancellationToken::new())
        .unwrap()
        .unwrap();
    let old_token = old_lease.completion_token().unwrap();

    assert_eq!(handoff.invalidate_generation(2).unwrap(), 1);
    assert_eq!(
        old_lease.with_pixels(|pixels| pixels.len()),
        Err(RuntimeError::StaleLease)
    );
    // The invalidated slot is reusable without waiting for an unreachable old acknowledgement.
    let new_descriptor = handoff
        .submit_frame(
            metadata(2),
            &[4, 5, 6],
            &budget(),
            &CancellationToken::new(),
        )
        .unwrap();
    old_token.acknowledge();
    old_lease.release().unwrap();
    assert_eq!(
        handoff.slot_state(new_descriptor.slot_id).unwrap(),
        SlotState::Published
    );

    let mut new_lease = handoff
        .receive_frame(&budget(), &CancellationToken::new())
        .unwrap()
        .unwrap();
    assert_eq!(new_lease.descriptor().generation, 2);
    new_lease.completion_token().unwrap().acknowledge();
    new_lease.release().unwrap();
    assert_eq!(
        handoff.slot_state(new_descriptor.slot_id).unwrap(),
        SlotState::Released
    );
}

#[test]
fn completed_lease_releases_its_pool_pin_without_dropping_the_handle() {
    let resources = runtime_budget();
    let pool = BufferPool::new(&resources, 1, 1, 3).unwrap();
    let descriptor = publish(&pool, 1);
    let charged = resources.snapshot().memory;
    let mut lease = pool
        .consume(&descriptor, &budget(), &CancellationToken::new())
        .unwrap();

    lease.completion_token().unwrap().acknowledge();
    drop(pool);
    assert_eq!(resources.snapshot().memory, charged);

    lease.release().unwrap();
    assert_eq!(resources.snapshot().memory, MemoryUsage::default());
    assert_eq!(
        lease.with_pixels(|pixels| pixels.len()),
        Err(RuntimeError::StaleLease)
    );
}

#[test]
fn buffer_pool_storage_uses_shared_cpu_budget_until_last_clone_drops() {
    let probe_budget = runtime_budget();
    let probe_pool = BufferPool::new(&probe_budget, 1, 2, 3).unwrap();
    let charged_bytes = probe_budget.snapshot().memory.cpu_bytes;
    assert!(charged_bytes > 6);
    drop(probe_pool);
    assert_eq!(probe_budget.snapshot().memory, MemoryUsage::default());

    let too_small_budget = resource_budget_with_cpu_limit(charged_bytes);
    let existing_charge = too_small_budget
        .try_reserve_memory(MemoryUsage::new(1, 0))
        .unwrap();
    assert_eq!(too_small_budget.snapshot().memory, MemoryUsage::new(1, 0));
    assert_eq!(
        BufferPool::new(&too_small_budget, 1, 2, 3).unwrap_err(),
        RuntimeError::MemoryBudgetExceeded
    );
    assert_eq!(
        BoundedHandoff::new(&too_small_budget, 1, 2, 3, 2, 1).unwrap_err(),
        RuntimeError::MemoryBudgetExceeded
    );
    assert_eq!(
        too_small_budget.snapshot().memory,
        MemoryUsage::new(1, 0),
        "failed reservations must leave prior accounting unchanged"
    );
    drop(existing_charge);
    assert_eq!(too_small_budget.snapshot().memory, MemoryUsage::default());

    let exact_budget = resource_budget_with_cpu_limit(charged_bytes);
    let pool = BufferPool::new(&exact_budget, 1, 2, 3).unwrap();
    assert_eq!(
        exact_budget.snapshot().memory,
        MemoryUsage::new(charged_bytes, 0)
    );
    let clone = pool.clone();
    assert_eq!(
        exact_budget.snapshot().memory,
        MemoryUsage::new(charged_bytes, 0),
        "cloning a pool shares its single memory permit"
    );

    drop(pool);
    assert_eq!(
        exact_budget.snapshot().memory,
        MemoryUsage::new(charged_bytes, 0),
        "the reservation remains while a pool clone exists"
    );
    drop(clone);
    assert_eq!(exact_budget.snapshot().memory, MemoryUsage::default());

    let later_pool = BufferPool::new(&exact_budget, 1, 2, 3).unwrap();
    assert_eq!(
        exact_budget.snapshot().memory,
        MemoryUsage::new(charged_bytes, 0)
    );
    drop(later_pool);
    assert_eq!(exact_budget.snapshot().memory, MemoryUsage::default());
}

#[test]
fn queue_storage_is_charged_and_released_with_queue_owners() {
    let frame_queue_bytes = u64::try_from(size_of::<FrameDescriptor>())
        .unwrap()
        .checked_mul(2)
        .unwrap();
    let control_queue_bytes = u64::try_from(size_of::<ControlEvent>())
        .unwrap()
        .checked_mul(3)
        .unwrap();

    let insufficient = resource_budget_with_cpu_limit(frame_queue_bytes - 1);
    assert_eq!(
        FrameQueue::new(&insufficient, 2).unwrap_err(),
        RuntimeError::MemoryBudgetExceeded
    );
    assert_eq!(
        insufficient.snapshot().memory,
        MemoryUsage::default(),
        "a rejected queue reservation leaves accounting unchanged"
    );

    let budget = runtime_budget();
    let frames = FrameQueue::new(&budget, 2).unwrap();
    assert_eq!(
        budget.snapshot().memory,
        MemoryUsage::new(frame_queue_bytes, 0)
    );
    let controls = ControlQueue::new(&budget, 3).unwrap();
    assert_eq!(
        budget.snapshot().memory,
        MemoryUsage::new(frame_queue_bytes + control_queue_bytes, 0)
    );
    drop(frames);
    assert_eq!(
        budget.snapshot().memory,
        MemoryUsage::new(control_queue_bytes, 0)
    );
    drop(controls);
    assert_eq!(budget.snapshot().memory, MemoryUsage::default());
}

#[test]
fn handoff_constructor_releases_earlier_permits_when_control_queue_charge_fails() {
    let probe_budget = runtime_budget();
    let probe_pool = BufferPool::new(&probe_budget, 1, 1, 3).unwrap();
    let pool_bytes = probe_budget.snapshot().memory.cpu_bytes;
    drop(probe_pool);

    let frame_queue_bytes = u64::try_from(size_of::<FrameDescriptor>()).unwrap();
    let control_queue_bytes = u64::try_from(size_of::<ControlEvent>()).unwrap();
    let existing_bytes = 1_u64;
    let limit = existing_bytes
        .checked_add(pool_bytes)
        .and_then(|bytes| bytes.checked_add(frame_queue_bytes))
        .and_then(|bytes| bytes.checked_add(control_queue_bytes))
        .and_then(|bytes| bytes.checked_sub(1))
        .unwrap();
    let budget = resource_budget_with_cpu_limit(limit);
    let existing_charge = budget
        .try_reserve_memory(MemoryUsage::new(existing_bytes, 0))
        .unwrap();

    assert_eq!(
        BoundedHandoff::new(&budget, 1, 1, 3, 1, 1).unwrap_err(),
        RuntimeError::MemoryBudgetExceeded,
        "pool and frame queue fit; the control queue backing exceeds the remaining budget"
    );
    assert_eq!(
        budget.snapshot().memory,
        MemoryUsage::new(existing_bytes, 0),
        "failed construction releases pool and frame queue permits"
    );
    drop(existing_charge);
    assert_eq!(budget.snapshot().memory, MemoryUsage::default());
}

#[test]
fn fixed_bounds_and_aggregate_permits_are_enforced_and_released() {
    assert_eq!(
        BufferPool::new(&runtime_budget(), 0, 1, 4).unwrap_err(),
        RuntimeError::InvalidCapacity
    );
    assert_eq!(
        BufferPool::new(&runtime_budget(), 1, 0, 4).unwrap_err(),
        RuntimeError::InvalidCapacity
    );
    assert_eq!(
        BufferPool::new(&runtime_budget(), 1, 1, usize::MAX).unwrap_err(),
        RuntimeError::SizeOverflow
    );
    assert_eq!(
        ControlQueue::new(&runtime_budget(), 0).unwrap_err(),
        RuntimeError::InvalidCapacity
    );

    let pool = BufferPool::new(&runtime_budget(), 1, 1, 3).unwrap();
    let mut fill = pool.begin_fill().unwrap();
    assert_eq!(fill.write(&[0, 1, 2, 3]), Err(RuntimeError::MappingBounds));
    drop(fill);
    assert_eq!(pool.slot_state(0).unwrap(), SlotState::Free);
    assert_eq!(pool.mapped_region_len(), 3);

    let budget = ResourceBudget::new(ResourceLimits::new(1, 1, 1, 10, 10));
    let clone = budget.clone();
    let cpu = budget.try_acquire_cpu().unwrap();
    assert!(matches!(
        clone.try_acquire_cpu(),
        Err(RuntimeError::ConcurrencyLimit(_))
    ));
    let decode = clone.try_acquire_decode().unwrap();
    let output = budget.try_acquire_output().unwrap();
    assert_eq!(budget.snapshot().cpu_jobs, 1);
    assert_eq!(budget.snapshot().decode_jobs, 1);
    assert_eq!(budget.snapshot().outputs, 1);
    drop(cpu);
    assert_eq!(clone.snapshot().cpu_jobs, 0);
    let cpu_again = clone.try_acquire_cpu().unwrap();
    drop((cpu_again, decode, output));
    assert_eq!(budget.snapshot().cpu_jobs, 0);
    assert_eq!(budget.snapshot().decode_jobs, 0);
    assert_eq!(budget.snapshot().outputs, 0);

    assert_eq!(
        budget
            .try_begin_transition(MemoryUsage::new(6, 6), MemoryUsage::new(5, 5))
            .unwrap_err(),
        RuntimeError::MemoryBudgetExceeded
    );
    let transition = budget
        .try_begin_transition(MemoryUsage::new(4, 4), MemoryUsage::new(5, 5))
        .unwrap();
    assert_eq!(budget.snapshot().memory, MemoryUsage::new(9, 9));
    assert_eq!(
        budget
            .try_reserve_memory(MemoryUsage::new(2, 0))
            .unwrap_err(),
        RuntimeError::MemoryBudgetExceeded
    );
    let new_scene = transition.complete();
    assert_eq!(budget.snapshot().memory, MemoryUsage::new(5, 5));
    drop(new_scene);
    assert_eq!(budget.snapshot().memory, MemoryUsage::default());
}
