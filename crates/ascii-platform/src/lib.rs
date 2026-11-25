#![cfg(windows)]
#![deny(unsafe_code)]

mod probe;

pub use probe::{
    Architecture, EnvironmentTuple, GateObservations, GpuBackend, HierarchyObservation,
    HierarchySummary, HostProbeReport, Observation, ObservationReason, ProbeStatus, SessionKind,
    WindowsVersion, WindowClass, probe_host,
};
