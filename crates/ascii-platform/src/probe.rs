//! Read-only, bounded Windows desktop-hierarchy feasibility observation.

use ascii_core::{BudgetKind, Error};

#[allow(unsafe_code)]
mod windows;

const MAX_WINDOW_RECORDS: usize = 512;
const PROBE_TIMEOUT_MILLIS: u64 = 250;
const PROBE_STRATEGY: &str = "windows-shell-hierarchy-enumeration-v4";

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
    /// Number of `SHELLDLL_DefView` descendants under `GetShellWindow`, `Progman`, or `WorkerW`
    /// roots, regardless of owner.
    pub desktop_view_count: u16,
    /// Number of desktop views owned by the shell-window process.
    pub shell_owned_desktop_view_count: u16,
    /// Shell-owned desktop views whose direct parent class was `Progman`.
    pub desktop_views_parented_by_progman: u16,
    /// Shell-owned desktop views whose direct parent class was `WorkerW`.
    pub desktop_views_parented_by_workerw: u16,
    /// Shell-owned desktop views with another or unreadable direct parent class.
    pub desktop_views_with_other_parent: u16,
    /// Class of the top-level sibling immediately before the unique shell window.
    pub top_level_previous_sibling_class: Option<WindowClass>,
    /// Class of the top-level sibling immediately after the unique shell window.
    pub top_level_next_sibling_class: Option<WindowClass>,
    /// Whether the first shell-owned WorkerW after the shell window was observed.
    pub first_shell_owned_workerw_after_shell: Option<bool>,
    /// Shell-owned desktop views whose top-level ancestor class was `Progman`.
    pub desktop_views_under_progman_root: u16,
    /// Shell-owned desktop views whose top-level ancestor class was `WorkerW`.
    pub desktop_views_under_workerw_root: u16,
    /// Direct parent class for the unique shell-owned desktop view.
    pub desktop_view_parent_class: Option<WindowClass>,
    /// Previous sibling class for the unique shell-owned desktop view.
    pub desktop_view_previous_sibling_class: Option<WindowClass>,
    /// Next sibling class for the unique shell-owned desktop view.
    pub desktop_view_next_sibling_class: Option<WindowClass>,
    /// Whether a WorkerW sibling followed the unique shell-owned desktop view.
    pub workerw_sibling_after_desktop_view: Option<bool>,
    /// Whether the unique shell-owned top-level Progman has WS_EX_NOREDIRECTIONBITMAP.
    /// `None` means Progman was not unique/shell-owned or the style query failed.
    pub progman_no_redirection_bitmap: Option<bool>,
    /// Whether the unique direct-child shell-owned DefView has WS_EX_LAYERED.
    /// `None` means the required window was not unique/valid or the style query failed.
    pub desktop_view_layered: Option<bool>,
    /// Whether the unique shell-owned direct-child WorkerW is the immediate lower-Z sibling
    /// of the unique direct-child DefView under Progman. `None` means evidence was incomplete.
    pub workerw_immediately_after_desktop_view: Option<bool>,
    /// Count of shell-owned direct-child WorkerWs under the unique Progman.
    pub shell_owned_progman_child_workerw_count: u16,
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
    root_is_shell_window: bool,
    shell_owned: bool,
    top_level: bool,
    parent_is_root: bool,
    no_redirection_bitmap: Option<bool>,
    layered: Option<bool>,
    previous_sibling_class: Option<WindowClass>,
    next_sibling_class: Option<WindowClass>,
    workerw_sibling_after: Option<bool>,
}

impl WindowRecord {
    const EMPTY: Self = Self {
        class: WindowClass::Unknown,
        parent_class: WindowClass::Unknown,
        root_class: WindowClass::Unknown,
        root_is_shell_window: false,
        shell_owned: false,
        top_level: false,
        parent_is_root: false,
        no_redirection_bitmap: None,
        layered: None,
        previous_sibling_class: None,
        next_sibling_class: None,
        workerw_sibling_after: None,
    };
}

#[derive(Clone)]
struct HierarchySnapshot {
    shell_window_available: bool,
    shell_window_class: Option<WindowClass>,
    shell_window_enumerated: bool,
    top_level_previous_sibling_class: Option<WindowClass>,
    top_level_next_sibling_class: Option<WindowClass>,
    first_shell_owned_workerw_after_shell: Option<bool>,
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
            top_level_previous_sibling_class: None,
            top_level_next_sibling_class: None,
            first_shell_owned_workerw_after_shell: None,
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
        top_level_previous_sibling_class: snapshot.top_level_previous_sibling_class,
        top_level_next_sibling_class: snapshot.top_level_next_sibling_class,
        first_shell_owned_workerw_after_shell: snapshot.first_shell_owned_workerw_after_shell,
        ..HierarchySummary::default()
    };

    let mut unique_view = None;
    let mut unique_view_is_ambiguous = false;
    let mut unique_progman = None;
    let mut unique_progman_is_ambiguous = false;
    for record in &snapshot.records[..snapshot.record_count] {
        if record.top_level {
            match record.class {
                WindowClass::Progman => {
                    summary.top_level_progman_count += 1;
                    if record.shell_owned {
                        if unique_progman.is_none() {
                            unique_progman = Some(*record);
                        } else {
                            unique_progman_is_ambiguous = true;
                        }
                    }
                }
                WindowClass::WorkerW => summary.top_level_workerw_count += 1,
                _ => {}
            }
        }
        if record.class == WindowClass::WorkerW
            && record.shell_owned
            && record.parent_class == WindowClass::Progman
            && record.parent_is_root
            && record.root_class == WindowClass::Progman
        {
            summary.shell_owned_progman_child_workerw_count += 1;
        }

        if record.class != WindowClass::ShellDefView {
            continue;
        }
        let desktop_root = record.root_is_shell_window
            || matches!(
                record.root_class,
                WindowClass::Progman | WindowClass::WorkerW
            );
        if !desktop_root {
            continue;
        }
        summary.desktop_view_count += 1;
        if !record.shell_owned {
            continue;
        }
        summary.shell_owned_desktop_view_count += 1;
        if unique_view.is_none() {
            unique_view = Some(*record);
        } else {
            unique_view_is_ambiguous = true;
        }
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
    if summary.shell_owned_desktop_view_count == 1
        && !unique_view_is_ambiguous
        && let Some(view) = unique_view
    {
        summary.desktop_view_parent_class = Some(view.parent_class);
        summary.desktop_view_previous_sibling_class = view.previous_sibling_class;
        summary.desktop_view_next_sibling_class = view.next_sibling_class;
        summary.workerw_sibling_after_desktop_view = view.workerw_sibling_after;
    }
    if summary.top_level_progman_count == 1
        && !unique_progman_is_ambiguous
        && let Some(progman) = unique_progman
    {
        summary.progman_no_redirection_bitmap = progman.no_redirection_bitmap;
        let valid_view = unique_view.filter(|view| {
            !unique_view_is_ambiguous
                && view.shell_owned
                && view.parent_is_root
                && view.parent_class == WindowClass::Progman
                && view.root_class == WindowClass::Progman
        });
        if let Some(view) = valid_view {
            summary.desktop_view_layered = view.layered;
            let unique_workerw = summary.shell_owned_progman_child_workerw_count == 1;
            summary.workerw_immediately_after_desktop_view = match (
                progman.no_redirection_bitmap,
                view.layered,
                unique_workerw,
                view.workerw_sibling_after,
            ) {
                (Some(true), Some(true), true, Some(true)) => Some(true),
                (Some(false), _, _, _) | (_, Some(false), _, _) | (_, _, false, _) => Some(false),
                (_, _, true, Some(false)) => Some(false),
                _ => None,
            };
        }
    }

    if snapshot.failure.is_some() {
        summary.progman_no_redirection_bitmap = None;
        summary.desktop_view_layered = None;
        summary.workerw_immediately_after_desktop_view = None;
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
        let unexpected_parent = summary.desktop_views_with_other_parent > 0;
        if recognized_parent && !unexpected_parent {
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

    fn view(
        parent_class: WindowClass,
        root_class: WindowClass,
        root_is_shell_window: bool,
    ) -> WindowRecord {
        WindowRecord {
            class: WindowClass::ShellDefView,

            parent_class,
            root_class,
            root_is_shell_window,
            shell_owned: true,
            top_level: false,
            parent_is_root: parent_class == root_class,
            no_redirection_bitmap: None,
            layered: None,
            previous_sibling_class: None,
            next_sibling_class: None,
            workerw_sibling_after: None,
        }
    }

    fn view_with_siblings(
        parent_class: WindowClass,
        root_class: WindowClass,
        previous_sibling_class: Option<WindowClass>,
        next_sibling_class: Option<WindowClass>,
        workerw_sibling_after: Option<bool>,
    ) -> WindowRecord {
        let mut record = view(parent_class, root_class, false);
        record.previous_sibling_class = previous_sibling_class;
        record.next_sibling_class = next_sibling_class;
        record.workerw_sibling_after = workerw_sibling_after;
        record
    }

    fn raised_chain(
        styles: (Option<bool>, Option<bool>),
        view_case: (bool, WindowClass),
        worker_case: (bool, WindowClass, usize),
        order: Option<bool>,
    ) -> HierarchySnapshot {
        let progman = WindowRecord {
            class: WindowClass::Progman,
            parent_class: WindowClass::Unknown,
            root_class: WindowClass::Progman,
            root_is_shell_window: true,
            shell_owned: true,
            top_level: true,
            parent_is_root: false,
            no_redirection_bitmap: styles.0,
            layered: None,
            previous_sibling_class: None,
            next_sibling_class: None,
            workerw_sibling_after: None,
        };
        let mut defview = view(view_case.1, WindowClass::Progman, false);
        defview.shell_owned = view_case.0;
        defview.parent_is_root = view_case.1 == WindowClass::Progman;
        defview.layered = styles.1;
        defview.workerw_sibling_after = order;
        let worker = WindowRecord {
            class: WindowClass::WorkerW,
            parent_class: worker_case.1,
            root_class: WindowClass::Progman,
            root_is_shell_window: false,
            shell_owned: worker_case.0,
            top_level: false,
            parent_is_root: worker_case.1 == WindowClass::Progman,
            no_redirection_bitmap: None,
            layered: None,
            previous_sibling_class: None,
            next_sibling_class: None,
            workerw_sibling_after: None,
        };
        let records = [progman, defview, worker, worker];
        snapshot(true, &records[..2 + worker_case.2])
    }

    #[test]
    fn raised_workerw_candidate_requires_complete_unique_style_and_order_chain() {
        let valid = classify_snapshot(&raised_chain(
            (Some(true), Some(true)),
            (true, WindowClass::Progman),
            (true, WindowClass::Progman, 1),
            Some(true),
        ));
        assert_eq!(
            valid.summary.workerw_immediately_after_desktop_view,
            Some(true)
        );

        let cases = [
            raised_chain(
                (None, Some(true)),
                (true, WindowClass::Progman),
                (true, WindowClass::Progman, 1),
                Some(true),
            ),
            raised_chain(
                (Some(true), None),
                (true, WindowClass::Progman),
                (true, WindowClass::Progman, 1),
                Some(true),
            ),
            raised_chain(
                (Some(false), Some(true)),
                (true, WindowClass::Progman),
                (true, WindowClass::Progman, 1),
                Some(true),
            ),
            raised_chain(
                (Some(true), Some(false)),
                (true, WindowClass::Progman),
                (true, WindowClass::Progman, 1),
                Some(true),
            ),
            raised_chain(
                (Some(true), Some(true)),
                (false, WindowClass::Progman),
                (true, WindowClass::Progman, 1),
                Some(true),
            ),
            raised_chain(
                (Some(true), Some(true)),
                (true, WindowClass::Other),
                (true, WindowClass::Progman, 1),
                Some(true),
            ),
            raised_chain(
                (Some(true), Some(true)),
                (true, WindowClass::Progman),
                (false, WindowClass::Progman, 1),
                Some(true),
            ),
            raised_chain(
                (Some(true), Some(true)),
                (true, WindowClass::Progman),
                (true, WindowClass::Other, 1),
                Some(true),
            ),
            raised_chain(
                (Some(true), Some(true)),
                (true, WindowClass::Progman),
                (true, WindowClass::Progman, 0),
                Some(false),
            ),
            raised_chain(
                (Some(true), Some(true)),
                (true, WindowClass::Progman),
                (true, WindowClass::Progman, 2),
                Some(true),
            ),
            raised_chain(
                (Some(true), Some(true)),
                (true, WindowClass::Progman),
                (true, WindowClass::Progman, 1),
                Some(false),
            ),
            raised_chain(
                (Some(true), Some(true)),
                (true, WindowClass::Progman),
                (true, WindowClass::Progman, 1),
                None,
            ),
        ];
        for case in cases {
            assert_ne!(
                classify_snapshot(&case)
                    .summary
                    .workerw_immediately_after_desktop_view,
                Some(true)
            );
        }
    }

    #[test]
    fn raised_desktop_reports_nested_workerw_after_defview_not_top_level_counts() {
        let records = [view_with_siblings(
            WindowClass::Progman,
            WindowClass::Progman,
            None,
            Some(WindowClass::WorkerW),
            Some(true),
        )];
        let result = classify_snapshot(&snapshot(true, &records));
        assert_eq!(result.summary.top_level_workerw_count, 0);
        assert_eq!(
            result.summary.desktop_view_parent_class,
            Some(WindowClass::Progman)
        );
        assert_eq!(
            result.summary.desktop_view_next_sibling_class,
            Some(WindowClass::WorkerW)
        );
        assert_eq!(
            result.summary.workerw_sibling_after_desktop_view,
            Some(true)
        );
    }

    #[test]
    fn non_raised_desktop_reports_no_workerw_sibling_after_defview() {
        let records = [view_with_siblings(
            WindowClass::Progman,
            WindowClass::Progman,
            None,
            None,
            Some(false),
        )];
        let result = classify_snapshot(&snapshot(true, &records));
        assert_eq!(result.summary.desktop_view_next_sibling_class, None);
        assert_eq!(
            result.summary.workerw_sibling_after_desktop_view,
            Some(false)
        );
    }

    #[test]
    fn workerw_before_defview_is_not_reported_as_wallpaper_sibling_below_it() {
        let records = [view_with_siblings(
            WindowClass::Progman,
            WindowClass::Progman,
            Some(WindowClass::WorkerW),
            Some(WindowClass::Other),
            Some(false),
        )];
        let result = classify_snapshot(&snapshot(true, &records));
        assert_eq!(
            result.summary.desktop_view_previous_sibling_class,
            Some(WindowClass::WorkerW)
        );
        assert_eq!(
            result.summary.desktop_view_next_sibling_class,
            Some(WindowClass::Other)
        );
        assert_eq!(
            result.summary.workerw_sibling_after_desktop_view,
            Some(false)
        );
    }

    #[test]
    fn unreadable_sibling_makes_workerw_order_unknown() {
        let records = [view_with_siblings(
            WindowClass::Progman,
            WindowClass::Progman,
            None,
            Some(WindowClass::Unknown),
            None,
        )];
        let result = classify_snapshot(&snapshot(true, &records));
        assert_eq!(
            result.summary.desktop_view_next_sibling_class,
            Some(WindowClass::Unknown)
        );
        assert_eq!(result.summary.workerw_sibling_after_desktop_view, None);
    }

    #[test]
    fn multiple_desktop_views_suppress_unique_sibling_report() {
        let records = [
            view_with_siblings(
                WindowClass::Progman,
                WindowClass::Progman,
                None,
                Some(WindowClass::WorkerW),
                Some(true),
            ),
            view(WindowClass::WorkerW, WindowClass::Progman, false),
        ];
        let result = classify_snapshot(&snapshot(true, &records));
        assert_eq!(result.summary.shell_owned_desktop_view_count, 2);
        assert_eq!(result.summary.desktop_view_parent_class, None);
        assert_eq!(result.summary.desktop_view_next_sibling_class, None);
        assert_eq!(result.summary.workerw_sibling_after_desktop_view, None);
    }

    #[test]
    fn top_level_workerw_order_is_exposed_as_a_candidate_only() {
        let mut snapshot = snapshot(true, &[]);
        snapshot.top_level_previous_sibling_class = Some(WindowClass::Other);
        snapshot.top_level_next_sibling_class = Some(WindowClass::WorkerW);
        snapshot.first_shell_owned_workerw_after_shell = Some(true);

        let summary = classify_snapshot(&snapshot).summary;
        assert_eq!(
            summary.top_level_previous_sibling_class,
            Some(WindowClass::Other)
        );
        assert_eq!(
            summary.top_level_next_sibling_class,
            Some(WindowClass::WorkerW)
        );
        assert_eq!(summary.first_shell_owned_workerw_after_shell, Some(true));
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
                root_is_shell_window: false,
                shell_owned: true,
                top_level: true,
                parent_is_root: false,
                no_redirection_bitmap: None,
                layered: None,
                previous_sibling_class: None,
                next_sibling_class: None,
                workerw_sibling_after: None,
            },
            view(WindowClass::WorkerW, WindowClass::WorkerW, false),
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
    fn unrecognized_parent_under_desktop_root_is_reported_as_experimental() {
        let records = [view(WindowClass::Other, WindowClass::Progman, false)];
        let result = classify_snapshot(&snapshot(true, &records));
        assert_eq!(result.observation.status, ProbeStatus::Experimental);
        assert_eq!(
            result.observation.reason,
            ObservationReason::UnexpectedHierarchy
        );
    }

    #[test]
    fn unexpected_parent_takes_precedence_over_recognized_desktop_view() {
        let records = [
            view(WindowClass::Progman, WindowClass::Progman, false),
            view(WindowClass::Other, WindowClass::Progman, false),
        ];
        let result = classify_snapshot(&snapshot(true, &records));
        assert_eq!(result.observation.status, ProbeStatus::Experimental);
        assert_eq!(
            result.observation.reason,
            ObservationReason::UnexpectedHierarchy
        );
        assert_eq!(result.summary.desktop_views_with_other_parent, 1);
    }

    #[test]
    fn shell_view_under_unrelated_root_is_not_misreported_as_desktop() {
        let records = [view(WindowClass::Other, WindowClass::Other, false)];
        let result = classify_snapshot(&snapshot(true, &records));
        assert_eq!(result.observation.status, ProbeStatus::Unavailable);
        assert_eq!(
            result.observation.reason,
            ObservationReason::DesktopViewNotFound
        );
        assert_eq!(result.summary.desktop_view_count, 0);
        assert_eq!(result.summary.shell_owned_desktop_view_count, 0);
    }

    #[test]
    fn unrecognized_actual_shell_root_is_still_a_desktop_candidate() {
        let records = [view(WindowClass::Other, WindowClass::Other, true)];
        let result = classify_snapshot(&snapshot(true, &records));
        assert_eq!(result.observation.status, ProbeStatus::Experimental);
        assert_eq!(
            result.observation.reason,
            ObservationReason::UnexpectedHierarchy
        );
        assert_eq!(result.summary.desktop_view_count, 1);
    }

    #[test]
    fn enumeration_timeout_is_unavailable_and_keeps_partial_counts_non_authoritative() {
        let mut snapshot = snapshot(
            true,
            &[view(WindowClass::Progman, WindowClass::Progman, false)],
        );
        snapshot.failure = Some(ScanFailure::TimedOut);
        let result = classify_snapshot(&snapshot);
        assert_eq!(result.observation.status, ProbeStatus::Unavailable);
        assert_eq!(result.observation.reason, ObservationReason::TimedOut);
        assert_eq!(result.summary.shell_owned_desktop_view_count, 1);
    }

    #[test]
    fn incomplete_scan_suppresses_raised_desktop_proof_fields() {
        for failure in [ScanFailure::TimedOut, ScanFailure::WorkBudgetExceeded] {
            let mut snapshot = raised_chain(
                (Some(true), Some(true)),
                (true, WindowClass::Progman),
                (true, WindowClass::Progman, 1),
                Some(true),
            );
            snapshot.failure = Some(failure);

            let summary = classify_snapshot(&snapshot).summary;
            assert_eq!(summary.progman_no_redirection_bitmap, None);
            assert_eq!(summary.desktop_view_layered, None);
            assert_eq!(summary.workerw_immediately_after_desktop_view, None);
        }
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
