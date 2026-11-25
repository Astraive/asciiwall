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

/// Enumerates the shell and top-level/descendant classes under an item/time budget.
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
    // SAFETY: EnumWindows invokes the callback synchronously while `scan` remains alive; the
    // callback uses only this pointer and stops before the fixed record/work limits are exceeded.
    let enumerated = unsafe { EnumWindows(Some(top_level_callback), scan_ptr as Lparam) };
    if enumerated == 0 && scan.snapshot.failure.is_none() {
        scan.snapshot.failure = Some(ScanFailure::NativeEnumerationFailed(last_error()));
    }
    if scan.snapshot.failure.is_none() && !scan.snapshot.shell_window_enumerated {
        // Some shell windows can be returned by GetShellWindow without appearing in the top-level
        // pass. Inspect that exact borrowed window separately rather than relying on FindWindow.
        // SAFETY: this call is synchronous, uses the same live scan context, and the shell handle
        // was returned by GetShellWindow immediately above.
        let succeeded = unsafe { scan_root(scan_ptr, shell_window, true, false) };
        if !succeeded && scan.snapshot.failure.is_none() {
            scan.snapshot.failure = Some(ScanFailure::NativeEnumerationFailed(last_error()));
        }
    }
    if scan.snapshot.failure.is_none() && Instant::now() >= scan.deadline {
        scan.snapshot.failure = Some(ScanFailure::TimedOut);
    }
    scan.snapshot
}

unsafe extern "system" fn top_level_callback(window: Hwnd, parameter: Lparam) -> Bool {
    let scan_ptr = parameter as *mut NativeScan;
    if scan_ptr.is_null() {
        return 0;
    }
    // SAFETY: the caller supplies a live NativeScan pointer for the duration of synchronous EnumWindows.
    unsafe { scan_root(scan_ptr, window, window == (*scan_ptr).shell_window, true) as Bool }
}

unsafe extern "system" fn child_callback(window: Hwnd, parameter: Lparam) -> Bool {
    let scan_ptr = parameter as *mut NativeScan;
    if scan_ptr.is_null() {
        return 0;
    }
    // SAFETY: EnumChildWindows is nested synchronously under scan_root with the same live context.
    unsafe { scan_child(scan_ptr, window) as Bool }
}

unsafe fn scan_root(
    scan_ptr: *mut NativeScan,
    window: Hwnd,
    is_shell_window: bool,
    from_top_level_enumeration: bool,
) -> bool {
    if !reserve_work(scan_ptr) {
        return false;
    }
    let class = class_of(window);
    let Some(shell_owned) = belongs_to_shell_process(scan_ptr, window) else {
        return false;
    };
    let root = WindowRecord {
        class,
        parent_class: WindowClass::Unknown,
        root_class: class,
        shell_owned,
        top_level: true,
    };
    if !push_record(scan_ptr, root) {
        return false;
    }
    {
        // SAFETY: scan_ptr was validated from the synchronous Win32 callback parameter.
        let scan = unsafe { &mut *scan_ptr };
        scan.current_root_class = class;
        if is_shell_window && from_top_level_enumeration {
            scan.snapshot.shell_window_enumerated = true;
        }
    }
    let parameter = scan_ptr as Lparam;
    // SAFETY: EnumChildWindows is synchronous; `scan_ptr` remains valid and all callbacks are
    // bounded by the shared deadline, work ceiling, and fixed-size snapshot.
    let succeeded = unsafe { EnumChildWindows(window, Some(child_callback), parameter) };
    if succeeded == 0 {
        // SAFETY: GetLastError has no pointer arguments and records the immediately preceding API failure.
        unsafe { set_native_failure(scan_ptr, last_error()) };
        return false;
    }
    // SAFETY: the scan remains live until enumerate_hierarchy returns.
    unsafe { (&*scan_ptr).snapshot.failure.is_none() }
}

unsafe fn scan_child(scan_ptr: *mut NativeScan, window: Hwnd) -> bool {
    if !reserve_work(scan_ptr) {
        return false;
    }
    let class = class_of(window);
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
    // SAFETY: scan_ptr was validated from the synchronous Win32 callback parameter.
    let root_class = unsafe { (&*scan_ptr).current_root_class };
    push_record(
        scan_ptr,
        WindowRecord {
            class,
            parent_class,
            root_class,
            shell_owned,
            top_level: false,
        },
    )
}

fn reserve_work(scan_ptr: *mut NativeScan) -> bool {
    if scan_ptr.is_null() {
        return false;
    }
    // SAFETY: only the active synchronous enumeration callback mutates this context.
    unsafe { (&mut *scan_ptr).reserve_work() }
}

fn push_record(scan_ptr: *mut NativeScan, record: WindowRecord) -> bool {
    if scan_ptr.is_null() {
        return false;
    }
    // SAFETY: only the active synchronous enumeration callback mutates this context.
    unsafe { (&mut *scan_ptr).push(record) }
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
