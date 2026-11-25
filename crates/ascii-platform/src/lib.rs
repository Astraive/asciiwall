#![cfg(windows)]
#![deny(unsafe_code)]

mod capability;
mod events;
mod host;
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
    BatteryUpdate, FeedEvent, HostEvent, HostEventBatch, HostEventQueue, MAX_HOST_EVENTS,
    PointerButtons, PointerButtonsUpdate, PointerPosition, SessionLockState, StaticWallpaperUpdate,
    WindowIdentity, WindowRectangle, WindowState,
};
pub use host::{
    HostAdapter, SurfaceLease, SurfaceLeaseError, SurfaceLeaseRegistry, SurfaceRetirementReason,
};
pub use probe::{
    Architecture, EnvironmentTuple, GateObservations, GpuBackend, HierarchyObservation,
    HierarchySummary, HostProbeReport, Observation, ObservationReason, ProbeStatus, SessionKind,
    WindowClass, WindowsVersion, probe_host,
};
