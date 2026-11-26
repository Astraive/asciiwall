#![deny(unsafe_code)]

mod capability;
mod events;
mod host;
#[cfg(windows)]
mod observer;
#[cfg(windows)]
mod probe;

pub use capability::{
    AdapterVersion, CAPABILITY_CONTRACT_VERSION, CapabilityAvailability, CapabilityChange,
    CapabilityChangeSet, CapabilityError, CapabilityFeature, CapabilityReason, CapabilityRecord,
    CapabilityResolution, CapabilityScope, CapabilitySet, CapabilitySnapshot, Confidence,
    ContractVersion, DowngradeReason, EffectiveCapability, Freshness, FreshnessState, HostHealth,
    HostIdentity, HostRestrictions, MAX_CAPABILITY_RECORDS, PresentationMode, PresentationRequest,
    PresentationResolution, resolve_capability, resolve_presentation,
};
pub use events::{
    BatteryUpdate, FeedEvent, HostEvent, HostEventBatch, HostEventQueue, HostStateSnapshot,
    HostStateSnapshotError, MAX_HOST_EVENTS, MAX_RECONCILIATION_FEEDS, MAX_RECONCILIATION_LEASES,
    PointerButtons, PointerButtonsUpdate, PointerPosition, SessionLockState, StaticWallpaperUpdate,
    WindowIdentity, WindowRectangle, WindowState,
};
#[cfg(windows)]
pub use observer::{
    DesktopClickBatch, DesktopClickObservation, DesktopHitClassification, FullscreenCandidate,
    ObservedWindowBounds, WindowsObserver, WindowsObserverConsent, WindowsObserverError,
    WindowsObserverSnapshot, WindowBoundsSource, MAX_DESKTOP_CLICK_OBSERVATIONS,
    MAX_FULLSCREEN_OBSERVATIONS, MAX_WINDOWS_OBSERVED,
};
pub use host::{
    HostAdapter, ReconciliationError, SurfaceLease, SurfaceLeaseError, SurfaceLeaseRegistry,
    SurfaceRetirementReason,
};
#[cfg(windows)]
pub use probe::{
    Architecture, EnvironmentTuple, GateObservations, GpuBackend, HierarchyObservation,
    HierarchySummary, HostProbeReport, Observation, ObservationReason, ProbeStatus, SessionKind,
    WindowClass, WindowsVersion, probe_host,
};
