//! Read-only, bounded Windows desktop-hierarchy feasibility observation.

use ascii_core::{BudgetKind, Error};

#[allow(unsafe_code)]
mod windows;

const MAX_WINDOW_RECORDS: usize = 512;
const PROBE_TIMEOUT_MILLIS: u64 = 250;
const PROBE_STRATEGY: &str = "windows-shell-hierarchy-enumeration-v1";

/// Whether a probe result is established, exploratory, unavailable, or deliberately unrun.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeStatus {
    /// Directly validated behavior; this hierarchy-only probe never emits this status.
    Supported,
    /// A read-only hierarchy was observed; this is not placement or production support.
    Experimental,
    /// A required observation could not be established within the probe's limits.
    Unavailable,
    /// The gate requires user-visible interaction and was not performed.
    NotRun,
}

/// Result detail that avoids retaining window handles or other stale native identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservationReason {
    /// A shell-owned desktop-view relationship was observed.
    ShellHierarchyObserved,
    /// Windows did not provide a shell window to inspect.
    ShellWindowUnavailable,
    /// A shell window existed, but no desktop-view child was found.
    DesktopViewNotFound,
    /// Desktop-view windows were found, but none belonged to the shell-window process.
    DesktopViewOutsideShellProcess,
    /// A shell-owned desktop view had a parent class outside the recognized Progman/WorkerW set.
    UnexpectedHierarchy,
    /// The callback deadline expired while enumerating windows.
    TimedOut,
    /// The bounded enumeration work ceiling was exhausted.
    WorkBudgetExceeded,
    /// A native enumeration call failed; the Win32 error code is reported separately.
    NativeEnumerationFailed,
    /// The required observation would change or interact with the user's visible desktop.
    RequiresInteractiveValidation,
}

/// A probe status and its non-sensitive reason/category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    /// Explicit state; `NotRun` is not a pass or a failure result.
    pub status: ProbeStatus,
    /// Non-handle detail for the result.
    pub reason: ObservationReason,
    /// Existing `ascii-core` failure category when the bounded probe could not proceed.
    pub error: Option<Error>,
    /// Optional Win32 error code for failed enumeration APIs.
    pub win32_error: Option<u32>,
}

/// The Windows version returned by `RtlGetVersion`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsVersion {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Build number.
    pub build: u32,
}

/// Native machine architecture reported by `GetNativeSystemInfo`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Architecture {
    /// 32-bit x86.
    X86,
    /// 64-bit x86.
    X64,
    /// 64-bit ARM.
    Arm64,
    /// Another native processor architecture; the numeric Windows value is retained.
    Other(u16),
    /// The native system information call did not provide a value.
    Unknown,
}

/// Session classification from the current process and active-console session IDs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionKind {
    /// The process belongs to the active local console session.
    Console,
    /// The process has a valid session ID that is not the active console session.
    Other,
    /// Session identity could not be queried.
    Unknown,
}

/// GPU identity is intentionally not queried by this hierarchy-only probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuBackend {
    /// No graphics adapter, driver, or rendering backend was queried or initialized.
    NotQueried,
}

/// Environment identity available without creating a window or graphics surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnvironmentTuple {
    /// Windows version/build when the version query succeeded.
    pub windows_version: Option<WindowsVersion>,
    /// Native architecture.
    pub architecture: Architecture,
    /// Current process session, including the session ID when the API returned one.
    pub session_kind: SessionKind,
    /// Current process session ID; this is not a window or process handle.
    pub session_id: Option<u32>,
    /// Always `NotQueried` for this shell-hierarchy-only operation.
    pub gpu_backend: GpuBackend,
}

/// Window class category used in the relationship summary; unrecognized names are not retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowClass {
    /// `Progman`.
    Progman,
    /// `WorkerW`.
    WorkerW,
    /// `SHELLDLL_DefView`.
    ShellDefView,
    /// A class name was readable but did not match one of the classes above.
    Other,
    /// The class-name query did not return a readable name.
    Unknown,
}

/// Bounded, handle-free counts from one completed hierarchy enumeration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HierarchySummary {
    /// Class of `GetShellWindow`, when it could be read.
    pub shell_window_class: Option<WindowClass>,
    /// Whether the `GetShellWindow` window also appeared in top-level enumeration.
    pub shell_window_enumerated: bool,
    /// Number of top-level `Progman` windows observed.
    pub top_level_progman_count: u16,
    /// Number of top-level `WorkerW` windows observed.
    pub top_level_workerw_count: u16,
    /// Number of `SHELLDLL_DefView` descendants, regardless of owner.
    pub desktop_view_count: u16,
    /// Number of those desktop views owned by the shell-window process.
    pub shell_owned_desktop_view_count: u16,
    /// Shell-owned desktop views whose direct parent class was `Progman`.
    pub desktop_views_parented_by_progman: u16,
    /// Shell-owned desktop views whose direct parent class was `WorkerW`.
    pub desktop_views_parented_by_workerw: u16,
    /// Shell-owned desktop views with another or unreadable direct parent class.
    pub desktop_views_with_other_parent: u16,
    /// Shell-owned desktop views whose top-level ancestor class was `Progman`.
    pub desktop_views_under_progman_root: u16,
    /// Shell-owned desktop views whose top-level ancestor class was `WorkerW`.
    pub desktop_views_under_workerw_root: u16,
}

/// Read-only hierarchy result for the current Windows environment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HierarchyObservation {
    /// Experimental/unavailable state. A discovered hierarchy never means attach is supported.
    pub observation: Observation,
    /// Aggregate class relationships; no HWND or process ID is retained.
    pub summary: HierarchySummary,
}

/// User-session gates that require visible or interactive desktop changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GateObservations {
    /// Below-icon placement of a rendered surface.
    pub placement_below_icons: Observation,
    /// Non-activation and focus preservation.
    pub focus_preservation: Observation,
    /// Win+D/Show Desktop behavior.
    pub win_d_behavior: Observation,
    /// Explorer-restart recovery.
    pub explorer_restart_recovery: Observation,
    /// Desktop icon click/selection preservation.
    pub icon_interaction: Observation,
}

/// Bounded feasibility report; no surface is created and no attach strategy is selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostProbeReport {
    /// Stable identity of this read-only discovery strategy.
    pub strategy: &'static str,
    /// OS/session identity from non-interactive system queries; GPU is explicitly not queried.
    pub environment: EnvironmentTuple,
    /// Bounded shell-window hierarchy observation.
    pub hierarchy: HierarchyObservation,
    /// Gates that require user-session interaction and remain `NotRun`.
    pub gates: GateObservations,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScanFailure {
    TimedOut,
    WorkBudgetExceeded,
    NativeEnumerationFailed(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowRecord {
    class: WindowClass,
    parent_class: WindowClass,
    root_class: WindowClass,
    shell_owned: bool,
    top_level: bool,
}

impl WindowRecord {
    const EMPTY: Self = Self {
        class: WindowClass::Unknown,
        parent_class: WindowClass::Unknown,
        root_class: WindowClass::Unknown,
        shell_owned: false,
        top_level: false,
    };
}

#[derive(Clone)]
struct HierarchySnapshot {
    shell_window_available: bool,
    shell_window_class: Option<WindowClass>,
    shell_window_enumerated: bool,
    records: [WindowRecord; MAX_WINDOW_RECORDS],
    record_count: usize,
    failure: Option<ScanFailure>,
}

impl HierarchySnapshot {
    const fn empty() -> Self {
        Self {
            shell_window_available: false,
            shell_window_class: None,
            shell_window_enumerated: false,
            records: [WindowRecord::EMPTY; MAX_WINDOW_RECORDS],
            record_count: 0,
            failure: None,
        }
    }
}

/// Performs only bounded hierarchy and OS/session queries. It never creates or attaches a window.
#[must_use]
pub fn probe_host() -> HostProbeReport {
    let environment = windows::environment_tuple();
    let snapshot = windows::enumerate_hierarchy();
    let hierarchy = classify_snapshot(&snapshot);
    let gates = GateObservations {
        placement_below_icons: not_run(),
        focus_preservation: not_run(),
        win_d_behavior: not_run(),
        explorer_restart_recovery: not_run(),
        icon_interaction: not_run(),
    };

    HostProbeReport {
        strategy: PROBE_STRATEGY,
        environment,
        hierarchy,
        gates,
    }
}

fn not_run() -> Observation {
    Observation {
        status: ProbeStatus::NotRun,
        reason: ObservationReason::RequiresInteractiveValidation,
        error: None,
        win32_error: None,
    }
}

fn classify_snapshot(snapshot: &HierarchySnapshot) -> HierarchyObservation {
    let mut summary = HierarchySummary {
        shell_window_class: snapshot.shell_window_class,
        shell_window_enumerated: snapshot.shell_window_enumerated,
        ..HierarchySummary::default()
    };

    for record in &snapshot.records[..snapshot.record_count] {
        if record.top_level {
            match record.class {
                WindowClass::Progman => summary.top_level_progman_count += 1,
                WindowClass::WorkerW => summary.top_level_workerw_count += 1,
                _ => {}
            }
        }

        if record.class != WindowClass::ShellDefView {
            continue;
        }
        summary.desktop_view_count += 1;
        if !record.shell_owned {
            continue;
        }
        summary.shell_owned_desktop_view_count += 1;
        match record.parent_class {
            WindowClass::Progman => summary.desktop_views_parented_by_progman += 1,
            WindowClass::WorkerW => summary.desktop_views_parented_by_workerw += 1,
            _ => summary.desktop_views_with_other_parent += 1,
        }
        match record.root_class {
            WindowClass::Progman => summary.desktop_views_under_progman_root += 1,
            WindowClass::WorkerW => summary.desktop_views_under_workerw_root += 1,
            _ => {}
        }
    }

    let observation = if !snapshot.shell_window_available {
        unavailable(
            ObservationReason::ShellWindowUnavailable,
            Some(Error::TemporarilyUnavailable),
            None,
        )
    } else if let Some(failure) = snapshot.failure {
        match failure {
            ScanFailure::TimedOut => unavailable(
                ObservationReason::TimedOut,
                Some(Error::TemporarilyUnavailable),
                None,
            ),
            ScanFailure::WorkBudgetExceeded => unavailable(
                ObservationReason::WorkBudgetExceeded,
                Some(Error::BudgetExceeded(BudgetKind::WorkUnits)),
                None,
            ),
            ScanFailure::NativeEnumerationFailed(code) => unavailable(
                ObservationReason::NativeEnumerationFailed,
                Some(Error::TemporarilyUnavailable),
                Some(code),
            ),
        }
    } else if summary.shell_owned_desktop_view_count > 0 {
        let recognized_parent = summary.desktop_views_parented_by_progman > 0
            || summary.desktop_views_parented_by_workerw > 0;
        if recognized_parent {
            Observation {
                status: ProbeStatus::Experimental,
                reason: ObservationReason::ShellHierarchyObserved,
                error: None,
                win32_error: None,
            }
        } else {
            Observation {
                status: ProbeStatus::Experimental,
                reason: ObservationReason::UnexpectedHierarchy,
                error: None,
                win32_error: None,
            }
        }
    } else if summary.desktop_view_count > 0 {
        unavailable(
            ObservationReason::DesktopViewOutsideShellProcess,
            Some(Error::TemporarilyUnavailable),
            None,
        )
    } else {
        unavailable(
            ObservationReason::DesktopViewNotFound,
            Some(Error::TemporarilyUnavailable),
            None,
        )
    };

    HierarchyObservation {
        observation,
        summary,
    }
}

fn unavailable(
    reason: ObservationReason,
    error: Option<Error>,
    win32_error: Option<u32>,
) -> Observation {
    Observation {
        status: ProbeStatus::Unavailable,
        reason,
        error,
        win32_error,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HierarchySnapshot, ObservationReason, ProbeStatus, ScanFailure, WindowClass, WindowRecord,
        classify_snapshot,
    };

    fn snapshot(shell_available: bool, records: &[WindowRecord]) -> HierarchySnapshot {
        let mut snapshot = HierarchySnapshot::empty();
        snapshot.shell_window_available = shell_available;
        snapshot.shell_window_class = shell_available.then_some(WindowClass::Progman);
        for record in records {
            snapshot.records[snapshot.record_count] = *record;
            snapshot.record_count += 1;
        }
        snapshot
    }

    fn view(parent_class: WindowClass, root_class: WindowClass) -> WindowRecord {
        WindowRecord {
            class: WindowClass::ShellDefView,
            parent_class,
            root_class,
            shell_owned: true,
            top_level: false,
        }
    }

    #[test]
    fn missing_shell_is_explicitly_unavailable() {
        let result = classify_snapshot(&snapshot(false, &[]));
        assert_eq!(result.observation.status, ProbeStatus::Unavailable);
        assert_eq!(
            result.observation.reason,
            ObservationReason::ShellWindowUnavailable
        );
    }

    #[test]
    fn changed_workerw_hierarchy_is_experimental_not_assumed_unsupported() {
        let records = [
            WindowRecord {
                class: WindowClass::WorkerW,
                parent_class: WindowClass::Unknown,
                root_class: WindowClass::WorkerW,
                shell_owned: true,
                top_level: true,
            },
            view(WindowClass::WorkerW, WindowClass::WorkerW),
        ];
        let result = classify_snapshot(&snapshot(true, &records));
        assert_eq!(result.observation.status, ProbeStatus::Experimental);
        assert_eq!(
            result.observation.reason,
            ObservationReason::ShellHierarchyObserved
        );
        assert_eq!(result.summary.desktop_views_parented_by_workerw, 1);
        assert_eq!(result.summary.desktop_views_under_workerw_root, 1);
    }

    #[test]
    fn unrecognized_shell_owned_hierarchy_is_reported_as_experimental() {
        let records = [view(WindowClass::Other, WindowClass::Other)];
        let result = classify_snapshot(&snapshot(true, &records));
        assert_eq!(result.observation.status, ProbeStatus::Experimental);
        assert_eq!(
            result.observation.reason,
            ObservationReason::UnexpectedHierarchy
        );
    }

    #[test]
    fn enumeration_timeout_is_unavailable_and_keeps_partial_counts_non_authoritative() {
        let mut snapshot = snapshot(true, &[view(WindowClass::Progman, WindowClass::Progman)]);
        snapshot.failure = Some(ScanFailure::TimedOut);
        let result = classify_snapshot(&snapshot);
        assert_eq!(result.observation.status, ProbeStatus::Unavailable);
        assert_eq!(result.observation.reason, ObservationReason::TimedOut);
        assert_eq!(result.summary.shell_owned_desktop_view_count, 1);
    }

    #[test]
    fn work_budget_exhaustion_uses_ascii_core_budget_error() {
        let mut snapshot = snapshot(true, &[]);
        snapshot.failure = Some(ScanFailure::WorkBudgetExceeded);
        let result = classify_snapshot(&snapshot);
        assert_eq!(result.observation.status, ProbeStatus::Unavailable);
        assert_eq!(
            result.observation.error,
            Some(ascii_core::Error::BudgetExceeded(
                ascii_core::BudgetKind::WorkUnits
            ))
        );
    }
}
