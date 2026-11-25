//! Host adapter boundary and generation-checked display-surface leases.

use ascii_core::{Generation, MonitorIdentity};

use crate::{CapabilitySnapshot, HostEventBatch};

/// Portable host contract. Native handles and OS actions remain inside the adapter implementation.
pub trait HostAdapter {
    /// Current bounded host/feed capability snapshot.
    fn capabilities(&self) -> CapabilitySnapshot;

    /// Drains a bounded batch of observations; event overflow is reported for reconciliation.
    fn poll(&mut self) -> HostEventBatch;
}

/// A generation-bound permission to use one adapter-owned native presentation surface.
///
/// The lease intentionally contains no native handle. The owning adapter keeps handles private and
/// checks this token immediately before presentation, serialized with attach/detach operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceLease {
    owner: crate::HostIdentity,
    display: MonitorIdentity,
    generation: Generation,
}

impl SurfaceLease {
    /// Host adapter instance that owns the native surface.
    #[must_use]
    pub const fn owner(self) -> crate::HostIdentity {
        self.owner
    }

    /// Local display identity for which the surface was created.
    #[must_use]
    pub const fn display(self) -> MonitorIdentity {
        self.display
    }

    /// Non-wrapping generation for this adapter's surface lifecycle.
    #[must_use]
    pub const fn generation(self) -> Generation {
        self.generation
    }
}

/// Why an active lease was retired.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceRetirementReason {
    /// The display was detached.
    DisplayDetached,
    /// The native surface was recreated or resized into a new surface lifetime.
    SurfaceRecreated,
    /// The host or its owning shell was lost.
    HostLost,
    /// The adapter is shutting down.
    AdapterShutdown,
}

/// Lease lifecycle failure. No failure permits use of the native surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceLeaseError {
    /// The display already has an active lease in this registry.
    AlreadyAttached,
    /// No empty registry slot remains.
    CapacityExhausted,
    /// The generation counter cannot advance without wrapping.
    GenerationExhausted,
    /// The token belongs to another host instance.
    WrongOwner,
    /// The display has a different active generation than this token.
    StaleGeneration,
    /// The caller is presenting against a different display identity than the token names.
    DisplayMismatch,
    /// No active lease exists for this token's display.
    RetiredLease,
}

/// Fixed-capacity active-lease registry for one host instance.
///
/// This value is deliberately not internally synchronized. Host implementations must serialize
/// lease checks with native present calls and all attach/detach/recreate operations.
#[derive(Debug, Eq, PartialEq)]
pub struct SurfaceLeaseRegistry<const CAPACITY: usize> {
    owner: crate::HostIdentity,
    leases: [Option<SurfaceLease>; CAPACITY],
    next_generation: Option<Generation>,
}

impl<const CAPACITY: usize> SurfaceLeaseRegistry<CAPACITY> {
    /// Creates an empty per-host registry with the first generation at zero.
    #[must_use]
    pub const fn new(owner: crate::HostIdentity) -> Self {
        Self {
            owner,
            leases: [None; CAPACITY],
            next_generation: Some(Generation::INITIAL),
        }
    }

    /// Host identity whose surfaces this registry may authorize.
    #[must_use]
    pub const fn owner(&self) -> crate::HostIdentity {
        self.owner
    }

    /// Number of currently active display surfaces.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.leases.iter().filter(|lease| lease.is_some()).count()
    }

    /// Registers a newly created adapter-owned surface for one display.
    pub fn attach(&mut self, display: MonitorIdentity) -> Result<SurfaceLease, SurfaceLeaseError> {
        if self
            .leases
            .iter()
            .flatten()
            .any(|lease| lease.display == display)
        {
            return Err(SurfaceLeaseError::AlreadyAttached);
        }
        let Some(slot) = self.leases.iter().position(Option::is_none) else {
            return Err(SurfaceLeaseError::CapacityExhausted);
        };
        let Some(generation) = self.next_generation else {
            return Err(SurfaceLeaseError::GenerationExhausted);
        };
        self.next_generation = generation.checked_next();
        let lease = SurfaceLease {
            owner: self.owner,
            display,
            generation,
        };
        self.leases[slot] = Some(lease);
        Ok(lease)
    }

    /// Retires the exact active token; stale tokens cannot retire a replacement surface.
    pub fn retire(&mut self, lease: SurfaceLease) -> Result<SurfaceLease, SurfaceLeaseError> {
        self.check_owner(lease)?;
        for slot in &mut self.leases {
            if let Some(current) = *slot {
                if current.display == lease.display {
                    if current.generation != lease.generation {
                        return Err(SurfaceLeaseError::StaleGeneration);
                    }
                    *slot = None;
                    return Ok(current);
                }
            }
        }
        Err(SurfaceLeaseError::RetiredLease)
    }

    /// Checks a present request against the caller's current display and active generation.
    ///
    /// Adapters must pass the display identity selected for this present, call immediately before
    /// presentation, and serialize the check with lifecycle changes. Passing does not expose or
    /// transfer ownership of a native handle to the caller.
    pub fn validate_present(
        &self,
        lease: SurfaceLease,
        current_display: MonitorIdentity,
    ) -> Result<(), SurfaceLeaseError> {
        self.check_owner(lease)?;
        if lease.display != current_display {
            return Err(SurfaceLeaseError::DisplayMismatch);
        }
        for current in self.leases.iter().flatten() {
            if current.display == current_display {
                return if current.generation == lease.generation {
                    Ok(())
                } else {
                    Err(SurfaceLeaseError::StaleGeneration)
                };
            }
        }
        Err(SurfaceLeaseError::RetiredLease)
    }

    fn check_owner(&self, lease: SurfaceLease) -> Result<(), SurfaceLeaseError> {
        if lease.owner != self.owner {
            Err(SurfaceLeaseError::WrongOwner)
        } else {
            Ok(())
        }
    }
}
