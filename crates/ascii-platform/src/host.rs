//! Host adapter boundary and generation-checked display-surface leases.

use ascii_core::{Generation, MonitorIdentity};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{CapabilitySnapshot, HostEventBatch, HostStateSnapshot};

static NEXT_REGISTRY_EPOCH: AtomicU64 = AtomicU64::new(1);

fn next_registry_epoch() -> Option<NonZeroU64> {
    NEXT_REGISTRY_EPOCH
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |epoch| {
            if epoch == 0 {
                None
            } else {
                Some(epoch.checked_add(1).unwrap_or(0))
            }
        })
        .ok()
        .and_then(NonZeroU64::new)
}

/// Portable host contract. Native handles and OS actions remain inside the adapter implementation.
pub trait HostAdapter {
    /// Current bounded host/feed capability snapshot.
    fn capabilities(&self) -> CapabilitySnapshot;

    /// Authoritative feed and lease values captured at one revision from the capability revision
    /// sequence. The snapshot includes matching host identity and capability metadata.
    ///
    /// Adapters must include every currently known value and active lease. Capacity overflow is
    /// reported by the snapshot itself; consumers must not treat a truncated snapshot as complete.
    fn reconciliation_snapshot(&self) -> HostStateSnapshot;

    /// Returns complete authoritative current state only if it covers the dropped-event revision.
    ///
    /// A snapshot behind the requested revision is reported as `SnapshotBehind`, even when it is
    /// incomplete. Otherwise, a truncated snapshot is returned in `IncompleteSnapshot` so callers
    /// can inspect its retained values without treating it as fully reconciled.
    #[allow(clippy::result_large_err)] // Carries the retained snapshot inline without allocating.
    fn reconcile(&self, minimum_revision: u64) -> Result<HostStateSnapshot, ReconciliationError> {
        let snapshot = self.reconciliation_snapshot();
        if snapshot.revision() < minimum_revision {
            return Err(ReconciliationError::SnapshotBehind {
                requested_revision: minimum_revision,
                snapshot_revision: snapshot.revision(),
            });
        }
        if !snapshot.is_complete() {
            return Err(ReconciliationError::IncompleteSnapshot { snapshot });
        }
        Ok(snapshot)
    }

    /// Drains a bounded batch of observations; event overflow is reported for reconciliation.
    fn poll(&mut self) -> HostEventBatch;
}

/// Why a reconciliation request cannot return a complete authoritative snapshot.
#[allow(clippy::large_enum_variant)] // Keep incomplete snapshots inspectable without allocating.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconciliationError {
    /// The current snapshot does not cover the requested dropped-event revision.
    SnapshotBehind {
        /// Minimum revision requested by the event batch.
        requested_revision: u64,
        /// Revision of the snapshot the adapter returned.
        snapshot_revision: u64,
    },
    /// The revision is sufficient, but bounded storage omitted feed or lease values.
    IncompleteSnapshot {
        /// The retained partial state and details of which collections were truncated.
        snapshot: HostStateSnapshot,
    },
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
    registry_epoch: NonZeroU64,
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

    /// Non-reusable identity of the registry that issued this lease.
    #[must_use]
    pub const fn registry_epoch(self) -> u64 {
        self.registry_epoch.get()
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
    /// A process-local registry identity could not be allocated without reuse.
    RegistryIdentityExhausted,
    /// The token belongs to another host instance.
    WrongOwner,
    /// The token was issued by a different registry for the same host.
    WrongRegistry,
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
    registry_epoch: NonZeroU64,
    leases: [Option<SurfaceLease>; CAPACITY],
    next_generation: Option<Generation>,
}

impl<const CAPACITY: usize> SurfaceLeaseRegistry<CAPACITY> {
    /// Creates an empty per-host registry with a unique process-local identity and generation zero.
    ///
    /// Fails closed if the registry identity space is exhausted rather than reusing an identity.
    pub fn new(owner: crate::HostIdentity) -> Result<Self, SurfaceLeaseError> {
        let Some(registry_epoch) = next_registry_epoch() else {
            return Err(SurfaceLeaseError::RegistryIdentityExhausted);
        };
        Ok(Self {
            owner,
            registry_epoch,
            leases: [None; CAPACITY],
            next_generation: Some(Generation::INITIAL),
        })
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

    /// Iterates current active lease tokens without allocation.
    pub fn active_leases(&self) -> impl Iterator<Item = &SurfaceLease> {
        self.leases.iter().flatten()
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
            registry_epoch: self.registry_epoch,
        };
        self.leases[slot] = Some(lease);
        Ok(lease)
    }

    /// Retires the exact active token; stale tokens cannot retire a replacement surface.
    pub fn retire(&mut self, lease: SurfaceLease) -> Result<SurfaceLease, SurfaceLeaseError> {
        self.check_owner(lease)?;
        for slot in &mut self.leases {
            if let Some(current) = *slot
                && current.display == lease.display
            {
                if current.generation != lease.generation {
                    return Err(SurfaceLeaseError::StaleGeneration);
                }
                *slot = None;
                return Ok(current);
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
        } else if lease.registry_epoch != self.registry_epoch {
            Err(SurfaceLeaseError::WrongRegistry)
        } else {
            Ok(())
        }
    }
}
