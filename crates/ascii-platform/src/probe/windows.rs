//! The only module containing Win32 FFI. Handles exist only during this read-only query.

use super::{
    Architecture, EnvironmentTuple, GpuBackend, HierarchySnapshot, MAX_WINDOW_RECORDS,
    PROBE_TIMEOUT_MILLIS, ScanFailure, SessionKind, WindowClass, WindowRecord, WindowsVersion,
};
use ascii_core::Budget;
use core::ffi::c_void;
use std::time::{Duration, Instant};

const MAX_ENUMERATION_WORK: u64 = MAX_WINDOW_RECORDS as u64;
const ACTIVE_CONSOLE_SESSION_UNAVAILABLE: u32 = u32::MAX;
const PROCESSOR_ARCHITECTURE_INTEL: u16 = 0;
const PROCESSOR_ARCHITECTURE_AMD64: u16 = 9;
const PROCESSOR_ARCHITECTURE_ARM64: u16 = 12;

type Hwnd = *mut c_void;
type Bool = i32;
type Lparam = isize;
type EnumWindowsProc = unsafe extern "system" fn(Hwnd, Lparam) -> Bool;

#[repr(C)]
struct SystemInfo {
    processor_architecture: u16,
    reserved: u16,
    page_size: u32,
    minimum_application_address: *mut c_void,
    maximum_application_address: *mut c_void,
    active_processor_mask: usize,
    number_of_processors: u32,
    processor_type: u32,
    allocation_granularity: u32,
    processor_level: u16,
    processor_revision: u16,
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self {
            processor_architecture: 0,
            reserved: 0,
            page_size: 0,
            minimum_application_address: core::ptr::null_mut(),
            maximum_application_address: core::ptr::null_mut(),
            active_processor_mask: 0,
            number_of_processors: 0,
            processor_type: 0,
            allocation_granularity: 0,
            processor_level: 0,
            processor_revision: 0,
        }
    }
}

#[repr(C)]
struct OsVersionInfoExW {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform_id: u32,
    service_pack: [u16; 128],
    service_pack_major: u16,
    service_pack_minor: u16,
    suite_mask: u16,
    product_type: u8,
    reserved: u8,
}
impl Default for OsVersionInfoExW {
    fn default() -> Self {
        Self {
            size: 0,
            major: 0,
            minor: 0,
            build: 0,
            platform_id: 0,
            service_pack: [0; 128],
            service_pack_major: 0,
            service_pack_minor: 0,
            suite_mask: 0,
            product_type: 0,
            reserved: 0,
        }
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn EnumChildWindows(parent: Hwnd, callback: Option<EnumWindowsProc>, parameter: Lparam)
    -> Bool;
    fn EnumWindows(callback: Option<EnumWindowsProc>, parameter: Lparam) -> Bool;
    fn GetClassNameW(window: Hwnd, class_name: *mut u16, maximum: i32) -> i32;
    fn IsChild(parent: Hwnd, child: Hwnd) -> Bool;
    fn GetParent(window: Hwnd) -> Hwnd;
    fn GetShellWindow() -> Hwnd;
    fn GetWindowThreadProcessId(window: Hwnd, process_id: *mut u32) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLastError() -> u32;
    fn GetCurrentProcessId() -> u32;
    fn GetNativeSystemInfo(system_info: *mut SystemInfo);
    fn ProcessIdToSessionId(process_id: u32, session_id: *mut u32) -> Bool;
    fn WTSGetActiveConsoleSessionId() -> u32;
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(version: *mut OsVersionInfoExW) -> i32;
}

struct NativeScan {
    snapshot: HierarchySnapshot,
    shell_window: Hwnd,
    shell_process_id: u32,
    current_root_class: WindowClass,
    current_root_is_shell_window: bool,
    deadline: Instant,
    work_units: u64,
    budget: Budget,
}

impl NativeScan {
    fn new(shell_window: Hwnd, shell_process_id: u32, shell_class: WindowClass) -> Self {
        let started = Instant::now();
        let deadline = started
            .checked_add(Duration::from_millis(PROBE_TIMEOUT_MILLIS))
            .unwrap_or(started);
        let mut snapshot = HierarchySnapshot::empty();
        snapshot.shell_window_available = true;
        snapshot.shell_window_class = Some(shell_class);
        Self {
            snapshot,
            shell_window,
            shell_process_id,
            current_root_class: WindowClass::Unknown,
            current_root_is_shell_window: false,
            deadline,
            work_units: 0,
            budget: Budget::new(0, 0, MAX_ENUMERATION_WORK),
        }
    }

    fn reserve_work(&mut self) -> bool {
        if Instant::now() >= self.deadline {
            self.snapshot.failure = Some(ScanFailure::TimedOut);
            return false;
        }
        if self.work_units >= self.budget.max_work_units() {
            self.snapshot.failure = Some(ScanFailure::WorkBudgetExceeded);
            return false;
        }
        self.work_units += 1;
        true
    }

    fn push(&mut self, record: WindowRecord) -> bool {
        if self.snapshot.record_count >= MAX_WINDOW_RECORDS {
            self.snapshot.failure = Some(ScanFailure::WorkBudgetExceeded);
            return false;
        }
        let index = self.snapshot.record_count;
        self.snapshot.records[index] = record;
        self.snapshot.record_count += 1;
        true
    }
}

/// Queries only OS identity and session IDs; no graphics device or window is created.
pub(super) fn environment_tuple() -> EnvironmentTuple {
    let mut system_info = SystemInfo::default();
    // SAFETY: the pointer targets a writable, correctly laid-out SYSTEM_INFO value.
    unsafe { GetNativeSystemInfo(&mut system_info) };
    let architecture = match system_info.processor_architecture {
        PROCESSOR_ARCHITECTURE_INTEL => Architecture::X86,
        PROCESSOR_ARCHITECTURE_AMD64 => Architecture::X64,
        PROCESSOR_ARCHITECTURE_ARM64 => Architecture::Arm64,
        other => Architecture::Other(other),
    };

    let windows_version = version_tuple();
    // SAFETY: these APIs read the current process/session identity into valid scalar outputs.
    let process_id = unsafe { GetCurrentProcessId() };
    let mut session_id = 0;
    let has_session_id = unsafe { ProcessIdToSessionId(process_id, &mut session_id) } != 0;
    let active_console_session_id = unsafe { WTSGetActiveConsoleSessionId() };
    let (session_kind, session_id) = if !has_session_id {
        (SessionKind::Unknown, None)
    } else if active_console_session_id == ACTIVE_CONSOLE_SESSION_UNAVAILABLE {
        (SessionKind::Unknown, Some(session_id))
    } else if session_id == active_console_session_id {
        (SessionKind::Console, Some(session_id))
    } else {
        (SessionKind::Other, Some(session_id))
    };

    EnvironmentTuple {
        windows_version,
        architecture,
        session_kind,
        session_id,
        gpu_backend: GpuBackend::NotQueried,
    }
}

/// Inspects the known shell window first, then top-level roots under the shared work/time budget.
pub(super) fn enumerate_hierarchy() -> HierarchySnapshot {
    // SAFETY: GetShellWindow is a read-only query and returns a borrowed system window handle.
    let shell_window = unsafe { GetShellWindow() };
    if shell_window.is_null() {
        return HierarchySnapshot::empty();
    }

    let shell_class = class_of(shell_window);
    let mut shell_process_id = 0;
    // SAFETY: `shell_process_id` is a valid writable output for this borrowed HWND.
    let shell_thread_id = unsafe { GetWindowThreadProcessId(shell_window, &mut shell_process_id) };
    if shell_thread_id == 0 || shell_process_id == 0 {
        let mut snapshot = HierarchySnapshot::empty();
        snapshot.shell_window_available = true;
        snapshot.shell_window_class = Some(shell_class);
        snapshot.failure = Some(ScanFailure::NativeEnumerationFailed(last_error()));
        return snapshot;
    }

    let mut scan = NativeScan::new(shell_window, shell_process_id, shell_class);
    let scan_ptr = &mut scan as *mut NativeScan;
    let succeeded = scan_shell_before_top_level(
        || {
            // SAFETY: the shell handle is borrowed from GetShellWindow immediately above, and
            // scan_ptr remains live for this synchronous traversal.
            unsafe { scan_root(scan_ptr, shell_window, true) }
        },
        || {
            // SAFETY: EnumWindows invokes the callback synchronously while `scan` remains alive;
            // the callback stops before the fixed record/work limits are exceeded.
            unsafe { EnumWindows(Some(top_level_callback), scan_ptr as Lparam) != 0 }
        },
    );
    if !succeeded && scan.snapshot.failure.is_none() {
        scan.snapshot.failure = Some(ScanFailure::NativeEnumerationFailed(last_error()));
    }
    if scan.snapshot.failure.is_none() && Instant::now() >= scan.deadline {
        scan.snapshot.failure = Some(ScanFailure::TimedOut);
    }
    scan.snapshot
}

fn scan_shell_before_top_level(
    inspect_shell: impl FnOnce() -> bool,
    enumerate_top_level: impl FnOnce() -> bool,
) -> bool {
    inspect_shell() && enumerate_top_level()
}

unsafe extern "system" fn top_level_callback(window: Hwnd, parameter: Lparam) -> Bool {
    let scan_ptr = parameter as *mut NativeScan;
    if scan_ptr.is_null() {
        return 0;
    }
    // Every root is deadline-checked, while unrelated classes consume no work unit.
    // SAFETY: scan_ptr is live for this synchronous callback.
    if Instant::now() >= unsafe { (*scan_ptr).deadline } {
        unsafe { set_timeout(scan_ptr) };
        return 0;
    }
    // SAFETY: caller supplies a live NativeScan throughout EnumWindows.
    if window == unsafe { (*scan_ptr).shell_window } {
        unsafe { note_shell_window_enumerated(&mut *scan_ptr) };
        return 1;
    }
    if !is_candidate_root(class_of(window)) {
        return 1;
    }
    unsafe { scan_root(scan_ptr, window, false) as Bool }
}

fn is_candidate_root(class: WindowClass) -> bool {
    matches!(class, WindowClass::Progman | WindowClass::WorkerW)
}

unsafe extern "system" fn child_callback(window: Hwnd, parameter: Lparam) -> Bool {
    let scan_ptr = parameter as *mut NativeScan;
    if scan_ptr.is_null() {
        return 0;
    }
    // SAFETY: EnumChildWindows is nested synchronously under scan_root with the same live context.
    unsafe { scan_child(scan_ptr, window) as Bool }
}

unsafe fn scan_root(scan_ptr: *mut NativeScan, window: Hwnd, is_shell_window: bool) -> bool {
    if !reserve_work(scan_ptr) {
        return false;
    }
    let class = class_of(window);
    let is_desktop_root = is_shell_window || is_candidate_root(class);
    let shell_owned = if is_desktop_root {
        let Some(shell_owned) = belongs_to_shell_process(scan_ptr, window) else {
            return false;
        };
        shell_owned
    } else {
        false
    };
    // SAFETY: scan_ptr was validated by reserve_work and is live for the synchronous callback.
    let action = unsafe { record_root(&mut *scan_ptr, class, shell_owned, is_shell_window) };
    match action {
        RootScanAction::Abort => false,
        RootScanAction::SkipChildren => true,
        RootScanAction::EnumerateChildren => {
            let parameter = scan_ptr as Lparam;
            // SAFETY: EnumChildWindows is synchronous; `scan_ptr` remains valid and all callbacks
            // are bounded by the shared deadline, work ceiling, and fixed-size snapshot.
            // EnumChildWindows documents its return value as unused; callbacks record early-stop causes.
            let api_result = unsafe { EnumChildWindows(window, Some(child_callback), parameter) };
            let scan = unsafe { &*scan_ptr };
            child_enumeration_succeeded(api_result, scan)
        }
    }
}
/// Callback state, not EnumChildWindows' BOOL return, determines whether the bounded walk failed.
fn child_enumeration_succeeded(_api_result: Bool, scan: &NativeScan) -> bool {
    scan.snapshot.failure.is_none()
}

unsafe fn scan_child(scan_ptr: *mut NativeScan, window: Hwnd) -> bool {
    if !reserve_work(scan_ptr) {
        return false;
    }
    let class = class_of(window);
    // Only shell desktop-view nodes need parent and owner-process inspection.
    if class != WindowClass::ShellDefView {
        return true;
    }
    // SAFETY: GetParent and IsChild inspect borrowed handles; IsChild rejects an owner mistaken for a parent.
    let parent_window = unsafe { GetParent(window) };
    let is_child_relationship = !parent_window.is_null()
        // SAFETY: the two handles are borrowed during this synchronous enumeration callback.
        && unsafe { IsChild(parent_window, window) } != 0;
    let parent_class = if is_child_relationship {
        class_of(parent_window)
    } else {
        WindowClass::Unknown
    };
    let Some(shell_owned) = belongs_to_shell_process(scan_ptr, window) else {
        return false;
    };
    // SAFETY: scan_ptr was validated by reserve_work and is live for the synchronous callback.
    let scan = unsafe { &mut *scan_ptr };
    let succeeded = record_child(scan, class, parent_class, shell_owned);
    succeeded && scan.snapshot.failure.is_none()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RootScanAction {
    Abort,
    SkipChildren,
    EnumerateChildren,
}

fn record_root(
    scan: &mut NativeScan,
    class: WindowClass,
    shell_owned: bool,
    is_shell_window: bool,
) -> RootScanAction {
    let root = WindowRecord {
        class,
        parent_class: WindowClass::Unknown,
        root_class: class,
        root_is_shell_window: is_shell_window,
        shell_owned,
        top_level: true,
    };
    if !scan.push(root) {
        return RootScanAction::Abort;
    }
    scan.current_root_class = class;
    scan.current_root_is_shell_window = is_shell_window;
    if is_shell_window || is_candidate_root(class) {
        RootScanAction::EnumerateChildren
    } else {
        RootScanAction::SkipChildren
    }
}

fn record_child(
    scan: &mut NativeScan,
    class: WindowClass,
    parent_class: WindowClass,
    shell_owned: bool,
) -> bool {
    scan.push(WindowRecord {
        class,
        parent_class,
        root_class: scan.current_root_class,
        root_is_shell_window: scan.current_root_is_shell_window,
        shell_owned,
        top_level: false,
    })
}

fn note_shell_window_enumerated(scan: &mut NativeScan) {
    scan.snapshot.shell_window_enumerated = true;
}

fn reserve_work(scan_ptr: *mut NativeScan) -> bool {
    if scan_ptr.is_null() {
        return false;
    }
    // SAFETY: only the active synchronous enumeration callback mutates this context.
    unsafe { (&mut *scan_ptr).reserve_work() }
}

fn belongs_to_shell_process(scan_ptr: *mut NativeScan, window: Hwnd) -> Option<bool> {
    let mut process_id = 0;
    // SAFETY: `process_id` is a valid output pointer and window is borrowed for this callback only.
    let thread_id = unsafe { GetWindowThreadProcessId(window, &mut process_id) };
    if thread_id == 0 || process_id == 0 {
        if !scan_ptr.is_null() {
            // SAFETY: the scan is active and this callback owns its sequential mutation window.
            unsafe { set_native_failure(scan_ptr, last_error()) };
        }
        return None;
    }
    if scan_ptr.is_null() {
        return None;
    }
    // SAFETY: the scan pointer is supplied by the active synchronous callback.
    Some(unsafe { process_id == (&*scan_ptr).shell_process_id })
}

unsafe fn set_native_failure(scan_ptr: *mut NativeScan, error: u32) {
    if scan_ptr.is_null() {
        return;
    }
    // SAFETY: callers hold the live scan pointer from the synchronous enumeration operation.
    let scan = unsafe { &mut *scan_ptr };
    if scan.snapshot.failure.is_none() {
        scan.snapshot.failure = Some(ScanFailure::NativeEnumerationFailed(error));
    }
}
unsafe fn set_timeout(scan_ptr: *mut NativeScan) {
    // SAFETY: caller holds the active synchronous scan pointer.
    let scan = unsafe { &mut *scan_ptr };
    if scan.snapshot.failure.is_none() {
        scan.snapshot.failure = Some(ScanFailure::TimedOut);
    }
}

fn class_of(window: Hwnd) -> WindowClass {
    if window.is_null() {
        return WindowClass::Unknown;
    }
    let mut buffer = [0_u16; 256];
    // SAFETY: `buffer` is writable for the supplied capacity; the borrowed HWND is queried only.
    let length = unsafe { GetClassNameW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    if length <= 0 {
        return WindowClass::Unknown;
    }
    let name = &buffer[..length as usize];
    if is_ascii_class(name, "Progman") {
        WindowClass::Progman
    } else if is_ascii_class(name, "WorkerW") {
        WindowClass::WorkerW
    } else if is_ascii_class(name, "SHELLDLL_DefView") {
        WindowClass::ShellDefView
    } else {
        WindowClass::Other
    }
}

fn is_ascii_class(name: &[u16], expected: &str) -> bool {
    name.len() == expected.len()
        && name
            .iter()
            .zip(expected.bytes())
            .all(|(wide, byte)| *wide == u16::from(byte))
}

fn last_error() -> u32 {
    // SAFETY: GetLastError has no arguments and returns the calling thread's last Win32 error.
    unsafe { GetLastError() }
}

fn version_tuple() -> Option<WindowsVersion> {
    let mut info = OsVersionInfoExW {
        size: core::mem::size_of::<OsVersionInfoExW>() as u32,
        ..OsVersionInfoExW::default()
    };
    // SAFETY: `info` is initialized and writable with the expected RtlGetVersion layout.
    (unsafe { RtlGetVersion(&mut info) } == 0).then_some(WindowsVersion {
        major: info.major,
        minor: info.minor,
        build: info.build,
    })
}

#[cfg(test)]
mod tests {
    use super::super::{ObservationReason, ProbeStatus, ScanFailure, classify_snapshot};
    use super::{
        MAX_ENUMERATION_WORK, NativeScan, RootScanAction, WindowClass, child_enumeration_succeeded,
        is_candidate_root, note_shell_window_enumerated, record_child, record_root,
        scan_shell_before_top_level,
    };
    use std::cell::RefCell;

    fn visit_synthetic_root(
        scan: &mut NativeScan,
        class: WindowClass,
        shell_owned: bool,
        is_shell_window: bool,
        descendants: &[WindowClass],
    ) -> bool {
        if !scan.reserve_work() {
            return false;
        }
        match record_root(scan, class, shell_owned, is_shell_window) {
            RootScanAction::Abort => false,
            RootScanAction::SkipChildren => true,
            RootScanAction::EnumerateChildren => {
                let parent_class = scan.current_root_class;
                descendants.iter().all(|child_class| {
                    scan.reserve_work() && record_child(scan, *child_class, parent_class, true)
                })
            }
        }
    }

    #[test]
    fn zero_enumchildwindows_return_is_ignored_without_callback_failure() {
        let shell_window = std::ptr::NonNull::<std::ffi::c_void>::dangling().as_ptr();
        let mut scan = NativeScan::new(shell_window, 1, WindowClass::Progman);

        assert!(child_enumeration_succeeded(0, &scan));
        scan.snapshot.failure = Some(ScanFailure::WorkBudgetExceeded);
        assert!(!child_enumeration_succeeded(0, &scan));
    }

    #[test]
    fn unrelated_top_level_roots_do_not_starve_candidate_or_duplicate_shell_record() {
        let shell_window = std::ptr::NonNull::<std::ffi::c_void>::dangling().as_ptr();
        let scan = RefCell::new(NativeScan::new(shell_window, 1, WindowClass::Progman));
        let unrelated_roots = [WindowClass::Other; MAX_ENUMERATION_WORK as usize + 1];

        let succeeded = scan_shell_before_top_level(
            || {
                let mut scan = scan.borrow_mut();
                visit_synthetic_root(
                    &mut scan,
                    WindowClass::Progman,
                    true,
                    true,
                    &[WindowClass::ShellDefView],
                )
            },
            || {
                let mut scan = scan.borrow_mut();
                // EnumWindows returns the pre-inspected shell again; mirror the production
                // callback's mark-and-continue branch without recording it a second time.
                note_shell_window_enumerated(&mut scan);
                for class in unrelated_roots {
                    if is_candidate_root(class)
                        && !visit_synthetic_root(&mut scan, class, false, false, &[])
                    {
                        return false;
                    }
                }
                visit_synthetic_root(
                    &mut scan,
                    WindowClass::WorkerW,
                    false,
                    false,
                    &[WindowClass::ShellDefView],
                )
            },
        );
        let scan = scan.into_inner();

        assert!(succeeded);
        assert_eq!(scan.snapshot.failure, None);
        assert_eq!(scan.work_units, 4);
        assert_eq!(scan.snapshot.record_count, 4);
        assert!(scan.snapshot.shell_window_enumerated);
        assert_eq!(
            scan.snapshot.records[..scan.snapshot.record_count]
                .iter()
                .filter(|record| record.root_is_shell_window && record.top_level)
                .count(),
            1
        );

        let hierarchy = classify_snapshot(&scan.snapshot);
        assert_eq!(hierarchy.observation.status, ProbeStatus::Experimental);
        assert_eq!(
            hierarchy.observation.reason,
            ObservationReason::ShellHierarchyObserved
        );
        assert_eq!(hierarchy.summary.desktop_views_parented_by_workerw, 1);
    }

    #[test]
    fn candidate_roots_still_use_bounded_recording() {
        let shell_window = std::ptr::NonNull::<std::ffi::c_void>::dangling().as_ptr();
        let mut scan = NativeScan::new(shell_window, 1, WindowClass::Progman);
        scan.work_units = MAX_ENUMERATION_WORK;
        assert!(is_candidate_root(WindowClass::WorkerW));
        assert!(!visit_synthetic_root(
            &mut scan,
            WindowClass::WorkerW,
            false,
            false,
            &[]
        ));
        assert_eq!(scan.snapshot.failure, Some(ScanFailure::WorkBudgetExceeded));
    }
}
