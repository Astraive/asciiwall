//! Portable bounded handoff, shared-buffer ownership, and aggregate resource budgets.
//!
//! This crate models transport and ownership without claiming an OS shared-memory or GPU API.
//! The process-local pool is safe Rust storage; a platform adapter must preserve the same bounds,
//! descriptor checks, and acknowledgements when it maps a real region.
//!
//! # Locking and callbacks
//!
//! The global acquisition order is the [`BoundedHandoff`] operation lock, then an individual
//! queue, pool, or budget lock. Implementations release each component lock before acquiring a
//! different component lock; completion signals are atomic. A [`FrameLease::with_pixels`] callback
//! runs while the pool ownership lock is held and therefore MUST be short CPU-only work: it MUST
//! NOT call UI/native APIs, re-enter this pool, or wait for decode, presentation, or a settings lock.
//! Never hold a settings lock while waiting for decode or present. Native owners must arrange their
//! own thread affinity and completion callbacks outside this crate.
//!
//! A [`CompletionToken`] is an explicit caller attestation that external use (including native or
//! GPU work) has completed. Reference counts and dropping a lease do not acknowledge completion.

mod buffer_pool;
pub mod cache;
mod queues;
pub use buffer_pool::{
    BufferPool, CompletionToken, FillLease, FrameDescriptor, FrameLease, RasterDescriptorMetadata,
    SlotState,
};
pub use queues::{BoundedHandoff, ControlEvent, ControlQueue, FrameQueue};

use std::sync::{Arc, Mutex, MutexGuard};

/// Runtime failures retain bounded-resource, state, mapping, and core-validation categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeError {
    /// A requested queue, slot pool, or resource configuration has an unusable bound.
    InvalidCapacity,
    /// A checked mapping or memory-size calculation overflowed.
    SizeOverflow,
    /// An allocation failed while constructing fixed-capacity storage.
    AllocationFailed,
    /// The buffer pool has no reusable slot.
    NoFreeSlots,
    /// A bounded control queue is full.
    QueueFull,
    /// The requested transition is not valid for the current slot or lease state.
    InvalidState,
    /// The descriptor or lease belongs to a prior pool generation.
    StaleGeneration,
    /// Descriptor identity or metadata does not match the published slot.
    InvalidDescriptor,
    /// The descriptor offset or length escapes its slot or mapped region.
    MappingBounds,
    /// A release was attempted before its explicit completion token was acknowledged.
    CompletionPending,
    /// The lease was invalidated or is no longer the active consumer owner.
    StaleLease,
    /// A shared concurrency limit has already been reached.
    ConcurrencyLimit(PermitKind),
    /// A CPU/GPU memory reservation would exceed an aggregate budget.
    MemoryBudgetExceeded,
    /// The core raster contract rejected the descriptor or bytes.
    CoreValidation(ascii_core::Error),
}

impl core::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidCapacity => formatter.write_str("invalid runtime capacity"),
            Self::SizeOverflow => formatter.write_str("runtime size overflow"),
            Self::AllocationFailed => formatter.write_str("runtime allocation failed"),
            Self::NoFreeSlots => formatter.write_str("no free shared-buffer slot"),
            Self::QueueFull => formatter.write_str("bounded queue is full"),
            Self::InvalidState => formatter.write_str("invalid ownership state transition"),
            Self::StaleGeneration => formatter.write_str("stale runtime generation"),
            Self::InvalidDescriptor => formatter.write_str("invalid frame descriptor"),
            Self::MappingBounds => formatter.write_str("descriptor is outside the mapped region"),
            Self::CompletionPending => {
                formatter.write_str("external completion is not acknowledged")
            }
            Self::StaleLease => formatter.write_str("lease is no longer valid"),
            Self::ConcurrencyLimit(kind) => write!(formatter, "{kind:?} concurrency limit reached"),
            Self::MemoryBudgetExceeded => formatter.write_str("aggregate memory budget exceeded"),
            Self::CoreValidation(error) => write!(formatter, "core validation failed: {error}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// One aggregate concurrency class shared by all clones of a [`ResourceBudget`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermitKind {
    /// CPU conversion or other bounded CPU work.
    Cpu,
    /// Media or image decoding work.
    Decode,
    /// An active output/presentation pipeline.
    Output,
}

impl PermitKind {
    const fn index(self) -> usize {
        match self {
            Self::Cpu => 0,
            Self::Decode => 1,
            Self::Output => 2,
        }
    }
}

/// Explicit aggregate concurrency and CPU/GPU memory ceilings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceLimits {
    /// Maximum concurrent CPU jobs.
    pub cpu_jobs: usize,
    /// Maximum concurrent decode jobs.
    pub decode_jobs: usize,
    /// Maximum active outputs.
    pub outputs: usize,
    /// Maximum aggregate CPU-side resource bytes.
    pub cpu_memory_bytes: u64,
    /// Maximum aggregate GPU-side resource bytes.
    pub gpu_memory_bytes: u64,
}

impl ResourceLimits {
    /// Creates limits with no implicit or unlimited defaults.
    #[must_use]
    pub const fn new(
        cpu_jobs: usize,
        decode_jobs: usize,
        outputs: usize,
        cpu_memory_bytes: u64,
        gpu_memory_bytes: u64,
    ) -> Self {
        Self {
            cpu_jobs,
            decode_jobs,
            outputs,
            cpu_memory_bytes,
            gpu_memory_bytes,
        }
    }

    const fn concurrency(self, kind: PermitKind) -> usize {
        match kind {
            PermitKind::Cpu => self.cpu_jobs,
            PermitKind::Decode => self.decode_jobs,
            PermitKind::Output => self.outputs,
        }
    }
}

/// CPU and GPU bytes charged to one resource owner.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MemoryUsage {
    /// Bytes resident in CPU-side buffers or caches.
    pub cpu_bytes: u64,
    /// Bytes resident in GPU-side buffers or caches.
    pub gpu_bytes: u64,
}

impl MemoryUsage {
    /// Creates an explicit two-domain memory charge.
    #[must_use]
    pub const fn new(cpu_bytes: u64, gpu_bytes: u64) -> Self {
        Self {
            cpu_bytes,
            gpu_bytes,
        }
    }

    fn checked_add(self, other: Self) -> Result<Self, RuntimeError> {
        Ok(Self {
            cpu_bytes: self
                .cpu_bytes
                .checked_add(other.cpu_bytes)
                .ok_or(RuntimeError::SizeOverflow)?,
            gpu_bytes: self
                .gpu_bytes
                .checked_add(other.gpu_bytes)
                .ok_or(RuntimeError::SizeOverflow)?,
        })
    }

    fn checked_sub(self, other: Self) -> Self {
        // Every permit is private, non-cloneable ownership and is subtracted exactly once.
        Self {
            cpu_bytes: self.cpu_bytes - other.cpu_bytes,
            gpu_bytes: self.gpu_bytes - other.gpu_bytes,
        }
    }
}

#[derive(Debug)]
struct BudgetState {
    active: [usize; 3],
    memory: MemoryUsage,
}

#[derive(Debug)]
struct BudgetInner {
    limits: ResourceLimits,
    state: Mutex<BudgetState>,
}

/// Cloneable application-wide permit ledger. Clones share the same aggregate counters.
#[derive(Clone, Debug)]
pub struct ResourceBudget {
    inner: Arc<BudgetInner>,
}

impl ResourceBudget {
    /// Creates one aggregate ledger with explicit limits.
    #[must_use]
    pub fn new(limits: ResourceLimits) -> Self {
        Self {
            inner: Arc::new(BudgetInner {
                limits,
                state: Mutex::new(BudgetState {
                    active: [0; 3],
                    memory: MemoryUsage::default(),
                }),
            }),
        }
    }

    /// Acquires one bounded CPU-work permit without waiting.
    pub fn try_acquire_cpu(&self) -> Result<ConcurrencyPermit, RuntimeError> {
        self.try_acquire(PermitKind::Cpu)
    }

    /// Acquires one bounded decoder permit without waiting.
    pub fn try_acquire_decode(&self) -> Result<ConcurrencyPermit, RuntimeError> {
        self.try_acquire(PermitKind::Decode)
    }

    /// Acquires one bounded output permit without waiting.
    pub fn try_acquire_output(&self) -> Result<ConcurrencyPermit, RuntimeError> {
        self.try_acquire(PermitKind::Output)
    }

    fn try_acquire(&self, kind: PermitKind) -> Result<ConcurrencyPermit, RuntimeError> {
        let mut state = lock_unpoisoned(&self.inner.state);
        let index = kind.index();
        if state.active[index] >= self.inner.limits.concurrency(kind) {
            return Err(RuntimeError::ConcurrencyLimit(kind));
        }
        state.active[index] += 1;
        Ok(ConcurrencyPermit {
            inner: Arc::clone(&self.inner),
            kind,
            active: true,
        })
    }

    /// Reserves CPU and GPU memory against the shared aggregate ceilings.
    pub fn try_reserve_memory(&self, usage: MemoryUsage) -> Result<MemoryPermit, RuntimeError> {
        self.reserve(usage)?;
        Ok(MemoryPermit {
            inner: Arc::clone(&self.inner),
            usage,
            active: true,
        })
    }

    /// Charges both old and new resources simultaneously for a transition.
    ///
    /// The returned permit retains both charges until [`TransitionMemoryPermit::complete`] or
    /// drop. Completing releases the old charge and returns a permit retaining the new charge.
    pub fn try_begin_transition(
        &self,
        old: MemoryUsage,
        new: MemoryUsage,
    ) -> Result<TransitionMemoryPermit, RuntimeError> {
        let total = old.checked_add(new)?;
        self.reserve(total)?;
        Ok(TransitionMemoryPermit {
            inner: Arc::clone(&self.inner),
            old,
            new,
            active: true,
        })
    }

    /// Returns current shared usage for diagnostics and deterministic tests.
    #[must_use]
    pub fn snapshot(&self) -> BudgetSnapshot {
        let state = lock_unpoisoned(&self.inner.state);
        BudgetSnapshot {
            cpu_jobs: state.active[PermitKind::Cpu.index()],
            decode_jobs: state.active[PermitKind::Decode.index()],
            outputs: state.active[PermitKind::Output.index()],
            memory: state.memory,
        }
    }

    fn reserve(&self, usage: MemoryUsage) -> Result<(), RuntimeError> {
        let mut state = lock_unpoisoned(&self.inner.state);
        let next = state.memory.checked_add(usage)?;
        if next.cpu_bytes > self.inner.limits.cpu_memory_bytes
            || next.gpu_bytes > self.inner.limits.gpu_memory_bytes
        {
            return Err(RuntimeError::MemoryBudgetExceeded);
        }
        state.memory = next;
        Ok(())
    }
}

/// A point-in-time view of aggregate resource use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BudgetSnapshot {
    /// Active CPU jobs.
    pub cpu_jobs: usize,
    /// Active decoder jobs.
    pub decode_jobs: usize,
    /// Active outputs.
    pub outputs: usize,
    /// Current CPU and GPU memory charges.
    pub memory: MemoryUsage,
}

/// RAII permit for one aggregate CPU, decode, or output slot.
#[derive(Debug)]
pub struct ConcurrencyPermit {
    inner: Arc<BudgetInner>,
    kind: PermitKind,
    active: bool,
}

impl ConcurrencyPermit {
    /// Returns the class held by this permit.
    #[must_use]
    pub const fn kind(&self) -> PermitKind {
        self.kind
    }

    /// Releases this permit before its owner goes out of scope.
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if self.active {
            let mut state = lock_unpoisoned(&self.inner.state);
            state.active[self.kind.index()] -= 1;
            self.active = false;
        }
    }
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        self.release_inner();
    }
}

/// RAII charge for CPU/GPU memory retained by one owner.
#[derive(Debug)]
pub struct MemoryPermit {
    inner: Arc<BudgetInner>,
    usage: MemoryUsage,
    active: bool,
}

impl MemoryPermit {
    /// Returns the memory retained by this permit.
    #[must_use]
    pub const fn usage(&self) -> MemoryUsage {
        self.usage
    }

    /// Releases this charge before its owner goes out of scope.
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if self.active {
            let mut state = lock_unpoisoned(&self.inner.state);
            state.memory = state.memory.checked_sub(self.usage);
            self.active = false;
        }
    }
}

impl Drop for MemoryPermit {
    fn drop(&mut self) {
        self.release_inner();
    }
}

/// A transition reservation holding old and new scene charges concurrently.
#[derive(Debug)]
pub struct TransitionMemoryPermit {
    inner: Arc<BudgetInner>,
    old: MemoryUsage,
    new: MemoryUsage,
    active: bool,
}

impl TransitionMemoryPermit {
    /// Releases the old-scene charge and returns a permit retaining the new scene.
    pub fn complete(mut self) -> MemoryPermit {
        {
            let mut state = lock_unpoisoned(&self.inner.state);
            state.memory = state.memory.checked_sub(self.old);
        }
        self.active = false;
        MemoryPermit {
            inner: Arc::clone(&self.inner),
            usage: self.new,
            active: true,
        }
    }

    /// Returns both old and new charges currently held.
    pub fn usage(&self) -> Result<MemoryUsage, RuntimeError> {
        self.old.checked_add(self.new)
    }
}

impl Drop for TransitionMemoryPermit {
    fn drop(&mut self) {
        if self.active {
            if let Ok(total) = self.old.checked_add(self.new) {
                let mut state = lock_unpoisoned(&self.inner.state);
                state.memory = state.memory.checked_sub(total);
            }
            self.active = false;
        }
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
