//! Fixed-capacity raster slots and explicit producer/consumer ownership transitions.
//!
//! Each slot follows `Free -> Filling -> Published -> Consumed -> Released -> Free`. A fill lease
//! dropped before publication returns its slot to `Free`; a consumer lease dropped without an
//! explicit completion acknowledgement deliberately leaves its slot `Consumed`. Generation
//! invalidation is the only alternate reclamation boundary and must only be called by an owner that
//! has invalidated the mapped transport region, not merely observed a process exit.
//!
//! Pool access is serialized by one short-lived mutex. The only API that runs caller code while
//! holding it is [`FrameLease::with_pixels`]; that callback MUST be short CPU-only work and MUST NOT
//! call UI/native APIs, re-enter the pool, or wait on decode/presentation. Do not hold a settings
//! lock while waiting on decode or present. Queue and pool locks are never held simultaneously.

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ascii_core::{
    Budget, CancellationToken, ColorDescription, PixelFormat, RasterFrameView, RasterOrientation,
    ValidatedRasterFrameView, validate_raster_frame,
};

use crate::{MemoryPermit, MemoryUsage, ResourceBudget, RuntimeError, lock_unpoisoned};

/// Observable state of one fixed shared-buffer slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotState {
    /// Available for a producer.
    Free,
    /// Exclusively owned by a producer that is filling bytes.
    Filling,
    /// Complete and waiting in the data queue for consumer validation.
    Published,
    /// Leased by a consumer; unavailable until completion or generation invalidation.
    Consumed,
    /// Consumer completion was explicitly acknowledged; waiting for pool reclamation.
    Released,
}

/// Raster metadata supplied when a producer publishes a filled slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RasterDescriptorMetadata {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Distance in bytes between starts of adjacent rows.
    pub row_stride: usize,
    /// Supported or explicitly rejected core pixel description.
    pub format: PixelFormat,
    /// Explicit color and alpha metadata checked by `ascii-core`.
    pub color: ColorDescription,
    /// Row orientation checked by `ascii-core`.
    pub orientation: RasterOrientation,
    /// Media timestamp carried through the descriptor without interpreting its clock domain.
    pub media_timestamp_ns: u64,
}

/// Transport metadata identifying one publication in one pool generation.
///
/// This value contains no pointers and is not itself a wire encoding. Adapters may serialize its
/// fields, but consumers MUST validate the complete descriptor against the local pool before use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameDescriptor {
    /// Fixed slot index.
    pub slot_id: u32,
    /// Pool/connection generation in which the slot was published.
    pub generation: u64,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Byte distance between starts of adjacent rows.
    pub row_stride: u64,
    /// Pixel layout and sample depth.
    pub format: PixelFormat,
    /// Color and alpha interpretation.
    pub color: ColorDescription,
    /// Row orientation.
    pub orientation: RasterOrientation,
    /// Exact bytes written into this slot.
    pub byte_length: u64,
    /// Absolute byte offset from the start of the mapped region.
    pub mapped_offset: u64,
    /// Opaque timestamp; clock semantics belong to the caller.
    pub media_timestamp_ns: u64,
    /// Monotonic slot-publication identity preventing same-generation ABA reuse.
    pub publication_id: u64,
}

#[derive(Debug)]
struct Slot {
    state: SlotState,
    generation: u64,
    written_len: usize,
    descriptor: Option<FrameDescriptor>,
}

#[derive(Debug)]
struct PoolInner {
    generation: u64,
    bytes_per_slot: usize,
    next_publication_id: u64,
    region: Vec<u8>,
    slots: Vec<Slot>,
    _memory_permit: MemoryPermit,
}

/// A fixed-size, process-local mapped-region model with generation-aware ownership.
///
/// Construction allocates the complete region and slot table once. It never grows the region or
/// slot count. A native shared-memory adapter can preserve this contract while replacing the
/// backing implementation; no native mapping semantics are inferred here.
#[derive(Clone, Debug)]
pub struct BufferPool {
    inner: Arc<Mutex<PoolInner>>,
}

impl BufferPool {
    /// Allocates `slot_count` equal-sized slots in a fixed region for a nonzero generation.
    ///
    /// The fixed region and slot-table backing are charged to the shared CPU memory budget for
    /// the lifetime of the pool and all its clones.
    pub fn new(
        resource_budget: &ResourceBudget,
        generation: u64,
        slot_count: usize,
        bytes_per_slot: usize,
    ) -> Result<Self, RuntimeError> {
        if generation == 0 || slot_count == 0 || bytes_per_slot == 0 {
            return Err(RuntimeError::InvalidCapacity);
        }
        if u32::try_from(slot_count).is_err() {
            return Err(RuntimeError::InvalidCapacity);
        }
        let region_len = slot_count
            .checked_mul(bytes_per_slot)
            .filter(|len| *len <= isize::MAX as usize)
            .ok_or(RuntimeError::SizeOverflow)?;
        let slots_len = slot_count
            .checked_mul(size_of::<Slot>())
            .filter(|len| *len <= isize::MAX as usize)
            .ok_or(RuntimeError::SizeOverflow)?;
        let charged_len = region_len
            .checked_add(slots_len)
            .ok_or(RuntimeError::SizeOverflow)?;
        let charged_bytes = u64::try_from(charged_len).map_err(|_| RuntimeError::SizeOverflow)?;
        let memory_permit =
            resource_budget.try_reserve_memory(MemoryUsage::new(charged_bytes, 0))?;

        let mut region = Vec::new();
        region
            .try_reserve_exact(region_len)
            .map_err(|_| RuntimeError::AllocationFailed)?;
        region.resize(region_len, 0);

        let mut slots = Vec::new();
        slots
            .try_reserve_exact(slot_count)
            .map_err(|_| RuntimeError::AllocationFailed)?;
        slots.resize_with(slot_count, || Slot {
            state: SlotState::Free,
            generation,
            written_len: 0,
            descriptor: None,
        });

        Ok(Self {
            inner: Arc::new(Mutex::new(PoolInner {
                generation,
                bytes_per_slot,
                next_publication_id: 1,
                region,
                slots,
                _memory_permit: memory_permit,
            })),
        })
    }

    /// Returns the current pool generation.
    #[must_use]
    pub fn generation(&self) -> u64 {
        lock_unpoisoned(&self.inner).generation
    }

    /// Returns the fixed number of slots.
    #[must_use]
    pub fn slot_count(&self) -> usize {
        lock_unpoisoned(&self.inner).slots.len()
    }

    /// Returns the maximum bytes in each slot.
    #[must_use]
    pub fn bytes_per_slot(&self) -> usize {
        lock_unpoisoned(&self.inner).bytes_per_slot
    }

    /// Returns the immutable mapped-region size.
    #[must_use]
    pub fn mapped_region_len(&self) -> usize {
        lock_unpoisoned(&self.inner).region.len()
    }

    /// Starts filling the first free slot without waiting.
    pub fn begin_fill(&self) -> Result<FillLease, RuntimeError> {
        let mut inner = lock_unpoisoned(&self.inner);
        let Some(index) = inner
            .slots
            .iter()
            .position(|slot| slot.state == SlotState::Free)
        else {
            return Err(RuntimeError::NoFreeSlots);
        };
        let generation = inner.generation;
        let slot = &mut inner.slots[index];
        slot.state = SlotState::Filling;
        slot.generation = generation;
        slot.written_len = 0;
        slot.descriptor = None;
        Ok(FillLease {
            inner: Arc::clone(&self.inner),
            slot_id: index as u32,
            generation,
            active: true,
        })
    }

    /// Returns a slot's current state, or `InvalidDescriptor` for an unknown slot id.
    pub fn slot_state(&self, slot_id: u32) -> Result<SlotState, RuntimeError> {
        let inner = lock_unpoisoned(&self.inner);
        inner
            .slots
            .get(slot_id as usize)
            .map(|slot| slot.state)
            .ok_or(RuntimeError::InvalidDescriptor)
    }

    /// Validates a published descriptor and changes its slot from `Published` to `Consumed`.
    ///
    /// The descriptor's generation, slot identity, all mapped-region bounds, metadata, and raster
    /// extent are checked before the lease can expose bytes. Raster validation is delegated to the
    /// shared `ascii-core` contract.
    pub fn consume(
        &self,
        descriptor: &FrameDescriptor,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<FrameLease, RuntimeError> {
        let mut inner = lock_unpoisoned(&self.inner);
        if descriptor.generation != inner.generation {
            return Err(RuntimeError::StaleGeneration);
        }
        let index = descriptor.slot_id as usize;
        let slot = inner
            .slots
            .get(index)
            .ok_or(RuntimeError::InvalidDescriptor)?;
        let (start, end) = checked_descriptor_range(&inner, descriptor, index)?;
        if slot.state != SlotState::Published || slot.generation != inner.generation {
            return Err(RuntimeError::InvalidState);
        }
        if slot.descriptor.as_ref() != Some(descriptor) {
            return Err(RuntimeError::InvalidDescriptor);
        }
        validate_raster(descriptor, &inner.region[start..end], budget, cancellation)?;
        inner.slots[index].state = SlotState::Consumed;
        Ok(FrameLease {
            inner: Some(Arc::clone(&self.inner)),
            descriptor: *descriptor,
            completion: Arc::new(AtomicBool::new(false)),
            completion_token_issued: false,
            released: false,
        })
    }

    /// Marks a still-published descriptor released, if it identifies that exact publication.
    ///
    /// A descriptor from an invalidated generation is a harmless no-op. Publication identity is
    /// checked independently of mutable metadata so a malformed copy of the correct queue entry
    /// cannot strand its original slot. This never releases a consumed lease.
    pub fn discard(&self, descriptor: &FrameDescriptor) -> Result<bool, RuntimeError> {
        let mut inner = lock_unpoisoned(&self.inner);
        if descriptor.generation != inner.generation {
            return Ok(false);
        }
        let index = descriptor.slot_id as usize;
        let Some(slot) = inner.slots.get_mut(index) else {
            return Err(RuntimeError::InvalidDescriptor);
        };
        if slot.state != SlotState::Published
            || slot.generation != descriptor.generation
            || slot
                .descriptor
                .is_none_or(|published| published.publication_id != descriptor.publication_id)
        {
            return Ok(false);
        }
        slot.state = SlotState::Released;
        Ok(true)
    }

    /// Returns all acknowledged `Released` slots to `Free` for later producer reuse.
    pub fn reclaim_released(&self) -> usize {
        let mut inner = lock_unpoisoned(&self.inner);
        let mut reclaimed = 0;
        for slot in &mut inner.slots {
            if slot.state == SlotState::Released {
                slot.state = SlotState::Free;
                slot.written_len = 0;
                slot.descriptor = None;
                reclaimed += 1;
            }
        }
        reclaimed
    }

    /// Invalidates all descriptors and leases from the old generation, returning every slot to
    /// `Free`.
    ///
    /// The caller MUST first make the old mapped region unavailable to its transport/native owner.
    /// This logical invalidation is not a claim that a GPU or device has completed work referencing
    /// some external mapping. A strictly greater generation prevents stale descriptors and leases
    /// from aliasing newly published slots.
    pub fn invalidate_generation(&self, new_generation: u64) -> Result<usize, RuntimeError> {
        let mut inner = lock_unpoisoned(&self.inner);
        if new_generation <= inner.generation {
            return Err(RuntimeError::StaleGeneration);
        }
        let mut invalidated = 0;
        for slot in &mut inner.slots {
            if slot.state != SlotState::Free {
                invalidated += 1;
            }
            slot.state = SlotState::Free;
            slot.generation = new_generation;
            slot.written_len = 0;
            slot.descriptor = None;
        }
        inner.generation = new_generation;
        Ok(invalidated)
    }
}

/// Exclusive producer handle for a slot in `Filling` state.
#[derive(Debug)]
pub struct FillLease {
    inner: Arc<Mutex<PoolInner>>,
    slot_id: u32,
    generation: u64,
    active: bool,
}

impl FillLease {
    /// Returns this fill's slot identifier.
    #[must_use]
    pub const fn slot_id(&self) -> u32 {
        self.slot_id
    }

    /// Appends bytes to the slot, rejecting any write that exceeds its fixed capacity.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), RuntimeError> {
        let mut inner = lock_unpoisoned(&self.inner);
        if inner.generation != self.generation {
            return Err(RuntimeError::StaleGeneration);
        }
        let index = self.slot_id as usize;
        let slot = inner
            .slots
            .get(index)
            .ok_or(RuntimeError::InvalidDescriptor)?;
        if slot.state != SlotState::Filling || slot.generation != self.generation {
            return Err(RuntimeError::InvalidState);
        }
        let end_in_slot = slot
            .written_len
            .checked_add(bytes.len())
            .filter(|end| *end <= inner.bytes_per_slot)
            .ok_or(RuntimeError::MappingBounds)?;
        let start = index
            .checked_mul(inner.bytes_per_slot)
            .and_then(|base| base.checked_add(slot.written_len))
            .ok_or(RuntimeError::SizeOverflow)?;
        let end = start
            .checked_add(bytes.len())
            .ok_or(RuntimeError::SizeOverflow)?;
        inner.region[start..end].copy_from_slice(bytes);
        inner.slots[index].written_len = end_in_slot;
        Ok(())
    }

    /// Validates the filled bytes and publishes one immutable descriptor.
    ///
    /// On any error this fill lease is dropped and the slot returns to `Free`; callers cannot
    /// publish an incomplete or invalid raster.
    pub fn publish(
        mut self,
        metadata: RasterDescriptorMetadata,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<FrameDescriptor, RuntimeError> {
        let mut inner = lock_unpoisoned(&self.inner);
        if inner.generation != self.generation {
            return Err(RuntimeError::StaleGeneration);
        }
        let index = self.slot_id as usize;
        let slot = inner
            .slots
            .get(index)
            .ok_or(RuntimeError::InvalidDescriptor)?;
        if slot.state != SlotState::Filling || slot.generation != self.generation {
            return Err(RuntimeError::InvalidState);
        }
        let written_len = slot.written_len;
        let start = index
            .checked_mul(inner.bytes_per_slot)
            .ok_or(RuntimeError::SizeOverflow)?;
        let end = start
            .checked_add(written_len)
            .ok_or(RuntimeError::SizeOverflow)?;
        let mut descriptor = FrameDescriptor {
            slot_id: self.slot_id,
            generation: self.generation,
            width: metadata.width,
            height: metadata.height,
            row_stride: u64::try_from(metadata.row_stride)
                .map_err(|_| RuntimeError::SizeOverflow)?,
            format: metadata.format,
            color: metadata.color,
            orientation: metadata.orientation,
            byte_length: u64::try_from(written_len).map_err(|_| RuntimeError::SizeOverflow)?,
            mapped_offset: u64::try_from(start).map_err(|_| RuntimeError::SizeOverflow)?,
            media_timestamp_ns: metadata.media_timestamp_ns,
            publication_id: 0,
        };
        validate_raster(&descriptor, &inner.region[start..end], budget, cancellation)?;
        let publication_id = inner.next_publication_id;
        inner.next_publication_id = publication_id
            .checked_add(1)
            .ok_or(RuntimeError::SizeOverflow)?;
        descriptor.publication_id = publication_id;
        let slot = &mut inner.slots[index];
        slot.state = SlotState::Published;
        slot.descriptor = Some(descriptor);
        self.active = false;
        Ok(descriptor)
    }
}

impl Drop for FillLease {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut inner = lock_unpoisoned(&self.inner);
        if inner.generation == self.generation
            && let Some(slot) = inner.slots.get_mut(self.slot_id as usize)
            && slot.state == SlotState::Filling
            && slot.generation == self.generation
        {
            slot.state = SlotState::Free;
            slot.written_len = 0;
            slot.descriptor = None;
        }
        self.active = false;
    }
}

/// Caller-owned, explicit attestation that external work using a [`FrameLease`] has completed.
///
/// The runtime permits exactly one token per consumer lease. The caller MUST acknowledge it only
/// after its own CPU/native/GPU work is actually complete; this type does not wait on or infer any
/// device semantics.
#[must_use = "acknowledge the token only after the external work completes"]
#[derive(Debug)]
pub struct CompletionToken {
    completed: Arc<AtomicBool>,
}

impl CompletionToken {
    /// Explicitly acknowledges completion to the corresponding frame lease.
    pub fn acknowledge(self) {
        self.completed.store(true, Ordering::Release);
    }
}

/// Consumer ownership of a validated slot. Dropping this value does not release its slot.
#[must_use = "release or invalidate the leased slot after external work completes"]
#[derive(Debug)]
pub struct FrameLease {
    inner: Option<Arc<Mutex<PoolInner>>>,
    descriptor: FrameDescriptor,
    completion: Arc<AtomicBool>,
    completion_token_issued: bool,
    released: bool,
}

impl FrameLease {
    /// Returns the validated descriptor backing this lease.
    #[must_use]
    pub const fn descriptor(&self) -> &FrameDescriptor {
        &self.descriptor
    }

    /// Creates the one explicit completion token for the caller's external work.
    ///
    /// A duplicate request returns `InvalidState`; dropping an unacknowledged token leaves
    /// completion pending until generation invalidation.
    pub fn completion_token(&mut self) -> Result<CompletionToken, RuntimeError> {
        if self.released || self.completion_token_issued {
            return Err(RuntimeError::InvalidState);
        }
        self.completion_token_issued = true;
        Ok(CompletionToken {
            completed: Arc::clone(&self.completion),
        })
    }

    /// Provides a borrow of the leased bytes while the pool ownership lock is held.
    ///
    /// The callback is restricted by contract to short CPU-only work. It MUST NOT call UI/native
    /// APIs, re-enter this pool, or wait for decode or present while the pool lock is held. Keep the
    /// lease alive and acknowledge its completion separately after any external work finishes.
    pub fn with_pixels<R>(&self, use_pixels: impl FnOnce(&[u8]) -> R) -> Result<R, RuntimeError> {
        if self.released {
            return Err(RuntimeError::StaleLease);
        }
        let pool = self.inner.as_ref().ok_or(RuntimeError::StaleLease)?;
        let inner = lock_unpoisoned(pool);
        if inner.generation != self.descriptor.generation {
            return Err(RuntimeError::StaleLease);
        }
        let index = self.descriptor.slot_id as usize;
        let slot = inner.slots.get(index).ok_or(RuntimeError::StaleLease)?;
        if slot.state != SlotState::Consumed
            || slot.generation != self.descriptor.generation
            || slot
                .descriptor
                .is_none_or(|published| published.publication_id != self.descriptor.publication_id)
        {
            return Err(RuntimeError::StaleLease);
        }
        let (start, end) = checked_descriptor_range(&inner, &self.descriptor, index)?;
        Ok(use_pixels(&inner.region[start..end]))
    }

    /// Changes the slot from `Consumed` to `Released` only after explicit external completion.
    ///
    /// Calling this before the matching [`CompletionToken`] is acknowledged returns
    /// `CompletionPending` and leaves the lease usable for a later retry.
    pub fn release(&mut self) -> Result<(), RuntimeError> {
        if self.released {
            return Ok(());
        }
        let Some(pool) = self.inner.take() else {
            self.released = true;
            return Ok(());
        };
        let result = {
            let mut inner = lock_unpoisoned(&pool);
            if inner.generation != self.descriptor.generation {
                // Generation invalidation already severed this lease from the active mapping.
                Ok(())
            } else if !self.completion.load(Ordering::Acquire) {
                Err(RuntimeError::CompletionPending)
            } else {
                let index = self.descriptor.slot_id as usize;
                match inner.slots.get_mut(index) {
                    Some(slot)
                        if slot.state == SlotState::Consumed
                            && slot.generation == self.descriptor.generation
                            && slot.descriptor.is_some_and(|published| {
                                published.publication_id == self.descriptor.publication_id
                            }) =>
                    {
                        slot.state = SlotState::Released;
                        Ok(())
                    }
                    Some(_) | None => Err(RuntimeError::StaleLease),
                }
            }
        };
        if result.is_ok() {
            self.released = true;
        } else {
            self.inner = Some(pool);
        }
        result
    }
}

fn checked_descriptor_range(
    inner: &PoolInner,
    descriptor: &FrameDescriptor,
    slot_index: usize,
) -> Result<(usize, usize), RuntimeError> {
    let expected_start = slot_index
        .checked_mul(inner.bytes_per_slot)
        .ok_or(RuntimeError::SizeOverflow)?;
    let start =
        usize::try_from(descriptor.mapped_offset).map_err(|_| RuntimeError::MappingBounds)?;
    let len = usize::try_from(descriptor.byte_length).map_err(|_| RuntimeError::MappingBounds)?;
    let end = start.checked_add(len).ok_or(RuntimeError::MappingBounds)?;
    let slot_end = expected_start
        .checked_add(inner.bytes_per_slot)
        .ok_or(RuntimeError::SizeOverflow)?;
    if start != expected_start || end > slot_end || end > inner.region.len() {
        return Err(RuntimeError::MappingBounds);
    }
    Ok((start, end))
}

fn validate_raster<'a>(
    descriptor: &FrameDescriptor,
    pixels: &'a [u8],
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<ValidatedRasterFrameView<'a>, RuntimeError> {
    let row_stride =
        usize::try_from(descriptor.row_stride).map_err(|_| RuntimeError::SizeOverflow)?;
    validate_raster_frame(
        RasterFrameView::new(
            descriptor.width,
            descriptor.height,
            row_stride,
            descriptor.format,
            descriptor.color,
            descriptor.orientation,
            pixels,
        ),
        budget,
        cancellation,
    )
    .map_err(RuntimeError::CoreValidation)
}
