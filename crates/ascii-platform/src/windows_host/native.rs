use std::{
    cell::Cell,
    ffi::c_void,
    mem::size_of,
    ptr::{null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use ascii_core::CancellationToken;

use windows_sys::{
    Win32::{
        Foundation::{
            CloseHandle, GetLastError, HANDLE, HWND, LPARAM, LRESULT, RECT, SetLastError, WPARAM,
        },
        Graphics::Gdi::{
            ClientToScreen, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
            ScreenToClient,
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification},
            StationsAndDesktops::{
                CloseDesktop, DESKTOP_READOBJECTS, GetUserObjectInformationW, OpenInputDesktop,
                UOI_NAME,
            },
            Threading::{
                GetCurrentProcessId, GetCurrentThreadId, OpenProcess, PROCESS_SYNCHRONIZE,
                WaitForSingleObject,
            },
        },
        UI::{
            HiDpi::{
                AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT,
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE, DPI_AWARENESS_PER_MONITOR_AWARE,
                GetAwarenessFromDpiAwarenessContext, GetWindowDpiAwarenessContext,
                SetThreadDpiAwarenessContext,
            },
            Input::KeyboardAndMouse::IsWindowEnabled,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows,
                GW_CHILD, GW_HWNDNEXT, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetClientRect,
                GetForegroundWindow, GetParent, GetPropW, GetWindow, GetWindowLongPtrW,
                GetWindowThreadProcessId, IsWindow, IsWindowVisible, LWA_ALPHA, MA_NOACTIVATE, MSG,
                PM_REMOVE, PeekMessageW, RegisterClassExW, RegisterWindowMessageW,
                SMTO_ABORTIFHUNG, SW_HIDE, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE,
                SWP_NOOWNERZORDER, SWP_NOZORDER, SendMessageTimeoutW, SetLayeredWindowAttributes,
                SetParent, SetPropW, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
                UnregisterClassW, WM_DISPLAYCHANGE as wm_displaychange,
                WM_ERASEBKGND as wm_erasebkgnd, WM_MOUSEACTIVATE as wm_mouseactivate, WNDCLASSEXW,
                WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_DISABLED, WS_EX_LAYERED,
                WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
                WS_POPUP,
            },
        },
    },
    core::BOOL,
};

pub(super) const MAX_NATIVE_DISPLAYS: usize = 16;
const MAX_HIERARCHY_RECORDS: usize = 512;
const HIERARCHY_DEADLINE: Duration = Duration::from_millis(250);
const WM_SPAWN_WORKERW: u32 = 0x052c;
const WORKERW_REQUEST_TIMEOUT_MS: u32 = 250;
const ERROR_GEN_FAILURE: u32 = 31;
const WM_WTSSESSION_CHANGE: u32 = 0x02b1;
const WTS_SESSION_REMOTE_CONNECT: usize = 0x3;
const WTS_SESSION_REMOTE_DISCONNECT: usize = 0x4;
const WTS_SESSION_LOCK: usize = 0x7;
const WTS_SESSION_UNLOCK: usize = 0x8;
const NOTIFY_FOR_THIS_SESSION: u32 = 0;
const DESKTOP_NAME_CAPACITY: usize = 64;
const OWNED_CHILD_STYLE: u32 = WS_CHILD | WS_DISABLED | WS_CLIPSIBLINGS | WS_CLIPCHILDREN;
const OWNED_POPUP_CREATE_STYLE: u32 = WS_POPUP | WS_DISABLED | WS_CLIPSIBLINGS | WS_CLIPCHILDREN;
const OWNED_WINDOW_EX_STYLE: u32 = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
const OWNED_CHILD_EX_STYLE: u32 = OWNED_WINDOW_EX_STYLE | WS_EX_LAYERED;
const RAISED_PROGMAN_EX_STYLE: u32 = WS_EX_NOREDIRECTIONBITMAP;
const RAISED_DEFVIEW_EX_STYLE: u32 = WS_EX_LAYERED;
const WM_NCCREATE: u32 = 0x0081;
const WM_CREATE: u32 = 0x0001;
const CREATION_REACHED_NCCREATE: u32 = 1;
const CREATION_REACHED_CREATE: u32 = 2;
const OPAQUE_LAYER_ALPHA: u8 = 255;
static NEXT_WINDOW_ID: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeError {
    Win32 {
        operation: &'static str,
        code: u32,
    },
    CreateWindowFailed {
        kind: &'static str,
        code: u32,
        parent: usize,
        style: u32,
        extended_style: u32,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        stage: u32,
    },
    DisplayEnumerationFailed,
    DisplayCapacityExceeded,
    InvalidDisplayBounds,
    ShellUnavailable,
    UnsupportedHierarchy,
    AmbiguousHierarchy,
    HierarchyBoundExceeded,
    DpiContextUnavailable,
    DpiContextMismatch,
    OwnedWindowInvalid,
    PlacementVerificationFailed,
    SecureDesktop,
    Cancelled,
    IdentityExhausted,
}

thread_local! {
    static CREATION_STAGE: Cell<u32> = const { Cell::new(0) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NativeMonitor {
    pub device_name: [u16; 32],
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl NativeMonitor {
    const EMPTY: Self = Self {
        device_name: [0; 32],
        left: 0,
        top: 0,
        width: 0,
        height: 0,
    };
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NativeMonitorSnapshot {
    pub records: [NativeMonitor; MAX_NATIVE_DISPLAYS],
    pub len: usize,
}

impl NativeMonitorSnapshot {
    pub fn iter(&self) -> impl Iterator<Item = &NativeMonitor> {
        self.records[..self.len].iter()
    }
}

struct MonitorScan {
    records: [NativeMonitor; MAX_NATIVE_DISPLAYS],
    len: usize,
    failure: Option<NativeError>,
}

/// Re-enumerates the entire active output topology into fixed storage using physical-pixel
/// coordinates. DPI virtualization is disabled only for this read-only enumeration and restored.
pub(super) fn enumerate_monitors() -> Result<NativeMonitorSnapshot, NativeError> {
    let mut scan = MonitorScan {
        records: [NativeMonitor::EMPTY; MAX_NATIVE_DISPLAYS],
        len: 0,
        failure: None,
    };
    with_thread_dpi_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE, || {
        // SAFETY: EnumDisplayMonitors calls the callback synchronously. `scan` remains live until
        // the function returns, and the callback writes only within its fixed-capacity record array.
        let succeeded = unsafe {
            EnumDisplayMonitors(
                null_mut(),
                null(),
                Some(monitor_callback),
                (&mut scan as *mut MonitorScan) as LPARAM,
            )
        };
        if let Some(failure) = scan.failure {
            return Err(failure);
        }
        if succeeded == 0 || scan.len == 0 {
            return Err(NativeError::DisplayEnumerationFailed);
        }
        Ok(NativeMonitorSnapshot {
            records: scan.records,
            len: scan.len,
        })
    })
}

unsafe extern "system" fn monitor_callback(
    monitor: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    parameter: LPARAM,
) -> BOOL {
    if parameter == 0 {
        return 0;
    }
    // SAFETY: the caller passes a live MonitorScan pointer for this synchronous callback.
    let scan = unsafe { &mut *(parameter as *mut MonitorScan) };
    if scan.len == MAX_NATIVE_DISPLAYS {
        scan.failure = Some(NativeError::DisplayCapacityExceeded);
        return 0;
    }
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `info` is writable with the layout expected by GetMonitorInfoW and `monitor` is
    // borrowed from the active EnumDisplayMonitors callback.
    if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) } == 0 {
        scan.failure = Some(NativeError::DisplayEnumerationFailed);
        return 0;
    }
    let bounds = info.monitorInfo.rcMonitor;
    let width = i64::from(bounds.right) - i64::from(bounds.left);
    let height = i64::from(bounds.bottom) - i64::from(bounds.top);
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        scan.failure = Some(NativeError::InvalidDisplayBounds);
        return 0;
    };
    if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
        scan.failure = Some(NativeError::InvalidDisplayBounds);
        return 0;
    }
    scan.records[scan.len] = NativeMonitor {
        device_name: info.szDevice,
        left: bounds.left,
        top: bounds.top,
        width,
        height,
    };
    scan.len += 1;
    1
}

pub(super) struct WindowClass {
    instance: *mut c_void,
    name: Arc<[u16]>,
    registered: bool,
}
#[derive(Clone, Copy)]
struct WindowCreateBounds {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

#[derive(Clone, Copy)]
struct WindowCreateOptions {
    extended_style: u32,
    style: u32,
    bounds: WindowCreateBounds,
    parent: HWND,
    dpi_context: Option<DPI_AWARENESS_CONTEXT>,
    kind: &'static str,
}

fn next_window_id() -> Option<usize> {
    NEXT_WINDOW_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            (current != 0).then(|| current.checked_add(1)).flatten()
        })
        .ok()
}

impl WindowClass {
    pub fn register(name: String) -> Result<Self, NativeError> {
        let mut name: Vec<u16> = name.encode_utf16().collect();
        name.push(0);
        // SAFETY: null requests the current executable module; the returned module handle remains
        // valid for the lifetime of all windows created by this process.
        let instance = unsafe { GetModuleHandleW(null()) };
        if instance.is_null() {
            return Err(win32_error("GetModuleHandleW"));
        }
        let name: Arc<[u16]> = name.into();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: name.as_ptr(),
            hIconSm: null_mut(),
        };
        // SAFETY: class points to initialized storage, and its name allocation is retained by the
        // WindowClass until after every owned HWND has been destroyed.
        if unsafe { RegisterClassExW(&class) } == 0 {
            return Err(win32_error("RegisterClassExW"));
        }
        Ok(Self {
            instance,
            name,
            registered: true,
        })
    }

    pub fn create_controller(&self) -> Result<Window, NativeError> {
        self.create(WindowCreateOptions {
            extended_style: OWNED_WINDOW_EX_STYLE,
            style: WS_POPUP | WS_DISABLED,
            bounds: WindowCreateBounds {
                left: 0,
                top: 0,
                width: 1,
                height: 1,
            },
            parent: null_mut(),
            dpi_context: None,
            kind: "controller",
        })
    }

    pub fn create_surface_window(
        &self,
        target: &ShellHierarchy,
        screen_left: i32,
        screen_top: i32,
        width: u32,
        height: u32,
    ) -> Result<Window, NativeError> {
        let width = i32::try_from(width).map_err(|_| NativeError::InvalidDisplayBounds)?;
        let height = i32::try_from(height).map_err(|_| NativeError::InvalidDisplayBounds)?;
        let mut point = windows_sys::Win32::Foundation::POINT {
            x: screen_left,
            y: screen_top,
        };
        with_thread_dpi_context(target.dpi_context, || {
            if unsafe { ScreenToClient(target.parent, &mut point) } == 0 {
                return Err(win32_error("ScreenToClient"));
            }
            // Match the passing isolated WGPU control: direct layered-child creation and late
            // layered setup fail on the observed build, so create a hidden popup before conversion.
            let window = self.create(WindowCreateOptions {
                extended_style: OWNED_WINDOW_EX_STYLE,
                style: OWNED_POPUP_CREATE_STYLE,
                bounds: WindowCreateBounds {
                    left: 0,
                    top: 0,
                    width,
                    height,
                },
                parent: null_mut(),
                dpi_context: Some(target.dpi_context),
                kind: "surface",
            })?;
            set_window_long(
                window.hwnd,
                GWL_STYLE,
                OWNED_CHILD_STYLE as isize,
                "SetWindowLongPtrW(GWL_STYLE)",
            )?;
            set_window_long(
                window.hwnd,
                GWL_EXSTYLE,
                OWNED_CHILD_EX_STYLE as isize,
                "SetWindowLongPtrW(GWL_EXSTYLE)",
            )?;
            // SetParent returns NULL both when the previous parent was NULL and on failure. Clear
            // last error so those cases are distinguishable, then verify the resulting parent.
            unsafe { SetLastError(0) };
            let previous_parent = unsafe { SetParent(window.hwnd, target.parent) };
            let parent_error = unsafe { GetLastError() };
            if previous_parent.is_null() && parent_error != 0 {
                return Err(NativeError::Win32 {
                    operation: "SetParent(surface)",
                    code: parent_error,
                });
            }
            if unsafe { GetParent(window.hwnd) } != target.parent {
                return Err(NativeError::PlacementVerificationFailed);
            }
            if unsafe {
                SetWindowPos(
                    window.hwnd,
                    null_mut(),
                    point.x,
                    point.y,
                    width,
                    height,
                    SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
                )
            } == 0
            {
                return Err(win32_error("SetWindowPos(surface child)"));
            }
            if unsafe { SetLayeredWindowAttributes(window.hwnd, 0, OPAQUE_LAYER_ALPHA, LWA_ALPHA) }
                == 0
            {
                return Err(win32_error("SetLayeredWindowAttributes(surface)"));
            }
            Ok(window)
        })
    }

    fn create(&self, options: WindowCreateOptions) -> Result<Window, NativeError> {
        let identity = next_window_id().ok_or(NativeError::IdentityExhausted)?;
        let previous_context = if let Some(context) = options.dpi_context {
            // SAFETY: `context` is a system DPI-awareness pseudo-handle obtained from a live shell
            // parent. It is used only for this window creation and restored before returning.
            let previous = unsafe { SetThreadDpiAwarenessContext(context) };
            if previous.is_null() {
                return Err(NativeError::DpiContextUnavailable);
            }
            Some(previous)
        } else {
            None
        };

        // Track which synchronous creation messages ran, so an error code of zero still
        // distinguishes API setup failure from window-procedure rejection.
        CREATION_STAGE.with(|stage| stage.set(0));
        // SAFETY: all pointers reference stable, NUL-terminated UTF-16 storage. The class owns the
        // registered class, and this call creates only this process's hidden, disabled HWND.
        unsafe { SetLastError(0) };
        let hwnd = unsafe {
            CreateWindowExW(
                options.extended_style,
                self.name.as_ptr(),
                null(),
                options.style,
                options.bounds.left,
                options.bounds.top,
                options.bounds.width,
                options.bounds.height,
                options.parent,
                null_mut(),
                self.instance,
                null(),
            )
        };
        let creation_failure = if hwnd.is_null() {
            Some((unsafe { GetLastError() }, CREATION_STAGE.with(Cell::get)))
        } else {
            None
        };

        let restore_error = previous_context.and_then(|previous| {
            // SAFETY: restores exactly the caller thread's prior DPI-awareness context.
            unsafe { SetThreadDpiAwarenessContext(previous) }
                .is_null()
                .then_some(NativeError::DpiContextUnavailable)
        });
        if let Some(error) = restore_error {
            if !hwnd.is_null() {
                // SAFETY: this handle was just created on the current thread and is still owned.
                unsafe { DestroyWindow(hwnd) };
            }
            return Err(error);
        }
        if hwnd.is_null() {
            let (code, stage) = creation_failure.unwrap_or((0, 0));
            return Err(NativeError::CreateWindowFailed {
                kind: options.kind,
                code,
                parent: options.parent as usize,
                style: options.style,
                extended_style: options.extended_style,
                left: options.bounds.left,
                top: options.bounds.top,
                width: options.bounds.width,
                height: options.bounds.height,
                stage,
            });
        }

        // SAFETY: hwnd is our own live window and the retained class name is a valid unique
        // property key. The nonzero token distinguishes this HWND from later handle reuse.
        if unsafe { SetPropW(hwnd, self.name.as_ptr(), identity as *mut c_void) } == 0 {
            let error = win32_error("SetPropW(owned HWND)");
            // SAFETY: this exact HWND was created above on the current thread.
            unsafe { DestroyWindow(hwnd) };
            return Err(error);
        }
        let process_id = unsafe { GetCurrentProcessId() };
        let thread_id = unsafe { GetCurrentThreadId() };
        if let Some(context) = options.dpi_context {
            // SAFETY: hwnd is the hidden window just created above.
            let actual = unsafe { GetWindowDpiAwarenessContext(hwnd) };
            // SAFETY: both values are system DPI-awareness pseudo-handles.
            if actual.is_null() || unsafe { AreDpiAwarenessContextsEqual(actual, context) } == 0 {
                // SAFETY: this handle was just created on the current thread and is still owned.
                unsafe { DestroyWindow(hwnd) };
                return Err(NativeError::DpiContextMismatch);
            }
        }
        Ok(Window {
            hwnd,
            class_name: self.name.clone(),
            process_id,
            thread_id,
            expected_client_rect: None,
            identity,
        })
    }
}

impl Drop for WindowClass {
    fn drop(&mut self) {
        if self.registered {
            // SAFETY: the host drops all owned windows before this class; the name allocation and
            // module handle remain valid through this call.
            unsafe { UnregisterClassW(self.name.as_ptr(), self.instance) };
            self.registered = false;
        }
    }
}
struct DpiContextGuard {
    previous: Option<DPI_AWARENESS_CONTEXT>,
}

impl DpiContextGuard {
    fn enter(context: DPI_AWARENESS_CONTEXT) -> Result<Self, NativeError> {
        // SAFETY: context is a system-provided pseudo-handle obtained from a live shell HWND.
        let previous = unsafe { SetThreadDpiAwarenessContext(context) };
        if previous.is_null() {
            Err(NativeError::DpiContextUnavailable)
        } else {
            Ok(Self {
                previous: Some(previous),
            })
        }
    }

    fn restore(&mut self) -> Result<(), NativeError> {
        let Some(previous) = self.previous else {
            return Ok(());
        };
        // SAFETY: restores the exact context captured on this same owner thread by `enter`.
        if unsafe { SetThreadDpiAwarenessContext(previous) }.is_null() {
            return Err(NativeError::DpiContextUnavailable);
        }
        self.previous = None;
        Ok(())
    }
}

impl Drop for DpiContextGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            // SAFETY: best-effort unwind fallback for restoring the owner thread's prior context.
            unsafe { SetThreadDpiAwarenessContext(previous) };
        }
    }
}

fn with_thread_dpi_context<T>(
    context: DPI_AWARENESS_CONTEXT,
    action: impl FnOnce() -> Result<T, NativeError>,
) -> Result<T, NativeError> {
    let mut guard = DpiContextGuard::enter(context)?;
    let result = action();
    guard.restore()?;
    result
}

pub(super) struct Window {
    hwnd: HWND,
    class_name: Arc<[u16]>,
    process_id: u32,
    thread_id: u32,
    expected_client_rect: Option<(i32, i32, i32, i32)>,
    identity: usize,
}

impl Window {
    pub fn raw_handle(&self) -> isize {
        self.hwnd as isize
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn is_owned(&self) -> bool {
        if self.hwnd.is_null() || unsafe { IsWindow(self.hwnd) } == 0 {
            return false;
        }
        // SAFETY: self.class_name is retained, NUL-terminated storage; this only queries an
        // adapter-owned property attached to the candidate HWND.
        if unsafe { GetPropW(self.hwnd, self.class_name.as_ptr()) } != self.identity as *mut c_void
        {
            return false;
        }
        let mut process_id = 0;
        // SAFETY: hwnd is checked as a live window; this query only writes a scalar process ID.
        let thread_id = unsafe { GetWindowThreadProcessId(self.hwnd, &mut process_id) };
        if process_id != self.process_id || thread_id != self.thread_id {
            return false;
        }
        let mut class_name = [0_u16; 128];
        // SAFETY: the buffer is writable and hwnd is live for this synchronous query.
        let length =
            unsafe { GetClassNameW(self.hwnd, class_name.as_mut_ptr(), class_name.len() as i32) };
        length > 0
            && class_name[..length as usize]
                == self.class_name[..self.class_name.len().saturating_sub(1)]
    }

    pub fn is_visible(&self) -> bool {
        self.is_owned() && unsafe { IsWindowVisible(self.hwnd) } != 0
    }

    pub fn hide(&self) {
        if self.is_owned() {
            // SAFETY: this call affects only the HWND created and retained by this object.
            unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
    }

    pub fn attach_to(
        &mut self,
        target: &ShellHierarchy,
        screen_left: i32,
        screen_top: i32,
        width: u32,
        height: u32,
    ) -> Result<(), NativeError> {
        if !self.is_owned() || unsafe { GetParent(self.hwnd) } != target.parent {
            return Err(NativeError::OwnedWindowInvalid);
        }
        let window_context = unsafe { GetWindowDpiAwarenessContext(self.hwnd) };
        let parent_context = unsafe { GetWindowDpiAwarenessContext(target.parent) };
        if window_context.is_null() || parent_context.is_null() {
            return Err(NativeError::DpiContextUnavailable);
        }
        if unsafe { AreDpiAwarenessContextsEqual(window_context, parent_context) } == 0 {
            return Err(NativeError::DpiContextMismatch);
        }
        let width = i32::try_from(width).map_err(|_| NativeError::InvalidDisplayBounds)?;
        let height = i32::try_from(height).map_err(|_| NativeError::InvalidDisplayBounds)?;

        with_thread_dpi_context(parent_context, || {
            set_window_long(
                self.hwnd,
                GWL_STYLE,
                OWNED_CHILD_STYLE as isize,
                "SetWindowLongPtrW(style)",
            )?;
            let extended = unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) } as u32;
            if extended & OWNED_CHILD_EX_STYLE != OWNED_CHILD_EX_STYLE
                || extended & WS_EX_TOPMOST != 0
            {
                return Err(NativeError::PlacementVerificationFailed);
            }
            let mut point = windows_sys::Win32::Foundation::POINT {
                x: screen_left,
                y: screen_top,
            };
            if unsafe { ScreenToClient(target.parent, &mut point) } == 0 {
                return Err(win32_error("ScreenToClient"));
            }
            // Insert immediately after DefView: never behind the WorkerW slot or at HWND_BOTTOM.
            if unsafe {
                SetWindowPos(
                    self.hwnd,
                    target.defview,
                    point.x,
                    point.y,
                    width,
                    height,
                    SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                )
            } == 0
            {
                return Err(win32_error("SetWindowPos(after DefView)"));
            }
            self.expected_client_rect = Some((point.x, point.y, width, height));
            Ok(())
        })
    }

    pub fn verify_hidden_placement(
        &self,
        target: &ShellHierarchy,
        owned_windows: &[Option<&Window>],
    ) -> Result<(), NativeError> {
        self.verify_placement(target, owned_windows, false)
    }

    pub fn verify_visible_placement(
        &self,
        target: &ShellHierarchy,
        owned_windows: &[Option<&Window>],
    ) -> Result<(), NativeError> {
        self.verify_placement(target, owned_windows, true)
    }

    pub fn verify_present_placement(&self, target: &ShellHierarchy) -> Result<(), NativeError> {
        if !self.is_visible() {
            return Err(NativeError::PlacementVerificationFailed);
        }
        verify_shell_parent_identity(target)?;
        verify_owned_child(self, target)
    }

    fn verify_placement(
        &self,
        target: &ShellHierarchy,
        owned_windows: &[Option<&Window>],
        visible: bool,
    ) -> Result<(), NativeError> {
        if !self.is_owned() || self.is_visible() != visible {
            return Err(NativeError::PlacementVerificationFailed);
        }
        verify_shell_hierarchy(target, owned_windows)?;
        let mut contains_self = false;
        for window in owned_windows.iter().flatten() {
            verify_owned_child(window, target)?;
            contains_self |= window.hwnd == self.hwnd;
        }
        if !contains_self {
            return Err(NativeError::PlacementVerificationFailed);
        }
        Ok(())
    }

    pub fn show_no_activate(
        &self,
        target: &ShellHierarchy,
        owned_windows: &[Option<&Window>],
        foreground_before: HWND,
    ) -> Result<(), NativeError> {
        self.verify_hidden_placement(target, owned_windows)?;
        unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
        if unsafe { GetForegroundWindow() } != foreground_before {
            self.hide();
            return Err(NativeError::PlacementVerificationFailed);
        }
        self.verify_visible_placement(target, owned_windows)
    }

    pub fn pump_messages(
        &self,
        taskbar_created: u32,
        remaining: &mut usize,
        signals: &mut NativeSignals,
    ) {
        if !self.is_owned() {
            signals.owned_window_destroyed = true;
            return;
        }
        while *remaining != 0 {
            let mut message = MSG::default();
            // SAFETY: the message storage is writable and hwnd restricts removal to this adapter's
            // own window, so the host never consumes the application's unrelated thread messages.
            if unsafe { PeekMessageW(&mut message, self.hwnd, 0, 0, PM_REMOVE) } == 0 {
                break;
            }
            *remaining -= 1;
            match message.message {
                value if value == wm_displaychange => signals.display_changed = true,
                WM_WTSSESSION_CHANGE => match message.wParam {
                    WTS_SESSION_LOCK => signals.locked = Some(true),
                    WTS_SESSION_UNLOCK | WTS_SESSION_REMOTE_CONNECT => signals.locked = Some(false),
                    WTS_SESSION_REMOTE_DISCONNECT => signals.locked = Some(true),
                    _ => {}
                },
                value if value == taskbar_created => signals.explorer_restarted = true,
                _ => {}
            }
            // SAFETY: the MSG was removed from this window's queue and is dispatched to its
            // registered WndProc, which only answers hit-test/activation/erase messages.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        if self.is_owned() {
            // SAFETY: the property's token confirms this is still the exact HWND created by this
            // object. Destroy it only after its associated WGPU surface has been released.
            unsafe { DestroyWindow(self.hwnd) };
        }
        self.hwnd = null_mut();
    }
}

pub(super) struct ShellProcess {
    handle: HANDLE,
    process_id: u32,
}

impl ShellProcess {
    pub fn process_id(&self) -> u32 {
        self.process_id
    }

    pub fn is_alive(&self) -> Result<bool, NativeError> {
        // SAFETY: handle is an owned SYNCHRONIZE-only process handle.
        match unsafe { WaitForSingleObject(self.handle, 0) } {
            0x0000_0102 => Ok(true), // WAIT_TIMEOUT
            0 => Ok(false),          // WAIT_OBJECT_0
            _ => Err(win32_error("WaitForSingleObject(shell process)")),
        }
    }
}

impl Drop for ShellProcess {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: this process synchronization handle was opened by this object.
            unsafe { CloseHandle(self.handle) };
            self.handle = null_mut();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ShellHierarchy {
    pub parent: HWND,
    pub process_id: u32,
    pub thread_id: u32,
    pub dpi_context: DPI_AWARENESS_CONTEXT,
    pub defview: HWND,
    pub workerw: HWND,
}

pub(super) struct ShellTarget {
    pub hierarchy: ShellHierarchy,
    pub process: ShellProcess,
}

pub(super) fn discover_shell_target_read_only(
    owned_windows: &[Option<&Window>],
) -> Result<ShellTarget, NativeError> {
    discover_shell_target(owned_windows, None)
}

pub(super) fn discover_shell_target_for_attach(
    owned_windows: &[Option<&Window>],
    cancellation: &CancellationToken,
) -> Result<ShellTarget, NativeError> {
    discover_shell_target(owned_windows, Some(cancellation))
}

fn discover_shell_target(
    owned_windows: &[Option<&Window>],
    allow_missing_slot_request: Option<&CancellationToken>,
) -> Result<ShellTarget, NativeError> {
    let mut scan = scan_raised_hierarchy(owned_windows)?;
    if scan.hierarchy.workerw.is_null() {
        let Some(cancellation) = allow_missing_slot_request else {
            return Err(NativeError::UnsupportedHierarchy);
        };
        let target_thread_is_distinct = scan.hierarchy.thread_id != unsafe { GetCurrentThreadId() };
        if !default_input_desktop()? {
            return Err(NativeError::SecureDesktop);
        }
        if cancellation.is_cancelled() {
            return Err(NativeError::Cancelled);
        }
        if !decide_missing_worker_request(MissingWorkerRequestConditions {
            attach_or_recovery: true,
            unique_progman: scan.unique_progman,
            unique_defview: scan.unique_defview,
            raised_styles: scan.raised_styles,
            worker_count: scan.worker_count,
            unambiguous_siblings: scan.missing_slot_is_unambiguous,
            target_thread_is_distinct,
            default_desktop: true,
            cancelled: cancellation.is_cancelled(),
        }) {
            return if cancellation.is_cancelled() {
                Err(NativeError::Cancelled)
            } else {
                Err(NativeError::UnsupportedHierarchy)
            };
        }
        let requested_hierarchy = scan.hierarchy;
        let mut message_result = 0_usize;
        // This private shell message is used only for a verified missing raised slot. The target
        // thread differs from this thread because Win32 ignores same-thread message timeouts.
        // It is never sent by read-only hierarchy checks or the diagnostic probe.
        unsafe { SetLastError(0) };
        let sent = unsafe {
            SendMessageTimeoutW(
                scan.hierarchy.parent,
                WM_SPAWN_WORKERW,
                0,
                0,
                SMTO_ABORTIFHUNG,
                WORKERW_REQUEST_TIMEOUT_MS,
                &mut message_result,
            )
        };
        let request_error = (sent == 0).then(|| unsafe { GetLastError() });
        // A single fresh enumeration is mandatory after the bounded request, even if it timed out.
        scan = scan_raised_hierarchy(owned_windows)?;
        if let Some(code) = request_error {
            return Err(NativeError::Win32 {
                operation: "SendMessageTimeoutW(WM_SPAWN_WORKERW)",
                code: if code == 0 { ERROR_GEN_FAILURE } else { code },
            });
        }
        if scan.hierarchy.parent != requested_hierarchy.parent
            || scan.hierarchy.process_id != requested_hierarchy.process_id
            || scan.hierarchy.thread_id != requested_hierarchy.thread_id
            || scan.hierarchy.dpi_context != requested_hierarchy.dpi_context
            || scan.hierarchy.defview != requested_hierarchy.defview
        {
            return Err(NativeError::ShellUnavailable);
        }
        if scan.hierarchy.workerw.is_null() {
            return Err(NativeError::UnsupportedHierarchy);
        }
        if cancellation.is_cancelled() {
            return Err(NativeError::Cancelled);
        }
    }
    let hierarchy = scan.hierarchy;
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, hierarchy.process_id) };
    if process.is_null() {
        return Err(win32_error("OpenProcess(shell SYNCHRONIZE)"));
    }
    let process = ShellProcess {
        handle: process,
        process_id: hierarchy.process_id,
    };
    if !process.is_alive()? {
        return Err(NativeError::ShellUnavailable);
    }
    Ok(ShellTarget { hierarchy, process })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowKind {
    Progman,
    WorkerW,
    ShellDefView,
    Other,
}

fn class_of(hwnd: HWND) -> Option<WindowKind> {
    if hwnd.is_null() {
        return None;
    }
    let mut name = [0_u16; 64];
    let length = unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32) };
    if length <= 0 {
        return None;
    }
    let name = &name[..length as usize];
    if is_class(name, "Progman") {
        Some(WindowKind::Progman)
    } else if is_class(name, "WorkerW") {
        Some(WindowKind::WorkerW)
    } else if is_class(name, "SHELLDLL_DefView") {
        Some(WindowKind::ShellDefView)
    } else {
        Some(WindowKind::Other)
    }
}

fn is_class(actual: &[u16], expected: &str) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected.bytes())
            .all(|(wide, byte)| *wide == u16::from(byte))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SiblingNode {
    DefView,
    Owned(usize),
    WorkerW,
    Foreign,
}

#[derive(Clone, Copy)]
struct ScannedHierarchy {
    hierarchy: ShellHierarchy,
    unique_progman: bool,
    unique_defview: bool,
    raised_styles: bool,
    worker_count: usize,
    missing_slot_is_unambiguous: bool,
}

struct ProgmanScan {
    shell_pid: u32,
    candidate: HWND,
    count: usize,
    foreign_owner: bool,
    work: usize,
    deadline: Instant,
    failure: Option<NativeError>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProgmanCallbackDecision {
    Skip,
    CountCandidate,
    DeadlineExceeded,
    CandidateBudgetExceeded,
}

fn decide_progman_callback(
    is_progman: bool,
    before_deadline: bool,
    candidate_work: usize,
) -> ProgmanCallbackDecision {
    if !before_deadline {
        ProgmanCallbackDecision::DeadlineExceeded
    } else if !is_progman {
        ProgmanCallbackDecision::Skip
    } else if candidate_work >= MAX_HIERARCHY_RECORDS {
        ProgmanCallbackDecision::CandidateBudgetExceeded
    } else {
        ProgmanCallbackDecision::CountCandidate
    }
}

unsafe extern "system" fn progman_callback(window: HWND, parameter: LPARAM) -> BOOL {
    if parameter == 0 {
        return 0;
    }
    let scan = unsafe { &mut *(parameter as *mut ProgmanScan) };
    let before_deadline = Instant::now() < scan.deadline;
    let is_progman = before_deadline && class_of(window) == Some(WindowKind::Progman);
    match decide_progman_callback(is_progman, before_deadline, scan.work) {
        ProgmanCallbackDecision::Skip => return 1,
        ProgmanCallbackDecision::DeadlineExceeded
        | ProgmanCallbackDecision::CandidateBudgetExceeded => {
            scan.failure = Some(NativeError::HierarchyBoundExceeded);
            return 0;
        }
        ProgmanCallbackDecision::CountCandidate => {}
    }
    scan.work += 1;
    scan.count += 1;
    let mut process_id = 0;
    if unsafe { GetWindowThreadProcessId(window, &mut process_id) } == 0 {
        scan.failure = Some(NativeError::AmbiguousHierarchy);
        return 0;
    }
    if process_id != scan.shell_pid {
        scan.foreign_owner = true;
    } else {
        scan.candidate = window;
    }
    1
}

fn scan_raised_hierarchy(
    owned_windows: &[Option<&Window>],
) -> Result<ScannedHierarchy, NativeError> {
    let owned_count = validate_owned_windows(owned_windows)?;
    let shell = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetShellWindow() };
    if shell.is_null() {
        return Err(NativeError::ShellUnavailable);
    }
    if class_of(shell) != Some(WindowKind::Progman) {
        return Err(NativeError::UnsupportedHierarchy);
    }
    let mut shell_pid = 0;
    if unsafe { GetWindowThreadProcessId(shell, &mut shell_pid) } == 0 || shell_pid == 0 {
        return Err(win32_error("GetWindowThreadProcessId(shell)"));
    }
    let started = Instant::now();
    let deadline = started.checked_add(HIERARCHY_DEADLINE).unwrap_or(started);
    let mut top = ProgmanScan {
        shell_pid,
        candidate: null_mut(),
        count: 0,
        foreign_owner: false,
        work: 0,
        deadline,
        failure: None,
    };
    let enumerated = unsafe {
        EnumWindows(
            Some(progman_callback),
            (&mut top as *mut ProgmanScan) as LPARAM,
        )
    };
    if let Some(error) = top.failure {
        return Err(error);
    }
    if enumerated == 0 || Instant::now() >= deadline {
        return Err(NativeError::HierarchyBoundExceeded);
    }
    if top.count != 1 || top.foreign_owner || top.candidate.is_null() {
        return Err(if top.count == 0 {
            NativeError::UnsupportedHierarchy
        } else {
            NativeError::AmbiguousHierarchy
        });
    }
    let parent = top.candidate;
    if parent != shell {
        return Err(NativeError::AmbiguousHierarchy);
    }
    if unsafe { IsWindow(parent) } == 0
        || !unsafe { GetParent(parent) }.is_null()
        || class_of(parent) != Some(WindowKind::Progman)
    {
        return Err(NativeError::UnsupportedHierarchy);
    }
    let mut parent_process = 0;
    let parent_thread = unsafe { GetWindowThreadProcessId(parent, &mut parent_process) };
    if parent_thread == 0 || parent_process != shell_pid {
        return Err(NativeError::AmbiguousHierarchy);
    }
    let parent_style = unsafe { GetWindowLongPtrW(parent, GWL_STYLE) } as u32;
    let parent_ex_style = unsafe { GetWindowLongPtrW(parent, GWL_EXSTYLE) } as u32;
    let raised_styles = parent_ex_style & RAISED_PROGMAN_EX_STYLE == RAISED_PROGMAN_EX_STYLE;
    if parent_style & WS_CHILD != 0 || !raised_styles {
        return Err(NativeError::UnsupportedHierarchy);
    }
    let dpi_context = unsafe { GetWindowDpiAwarenessContext(parent) };
    if dpi_context.is_null() {
        return Err(NativeError::DpiContextUnavailable);
    }
    if unsafe { GetAwarenessFromDpiAwarenessContext(dpi_context) }
        != DPI_AWARENESS_PER_MONITOR_AWARE
    {
        return Err(NativeError::DpiContextMismatch);
    }

    let mut nodes = [SiblingNode::Foreign; MAX_HIERARCHY_RECORDS];
    let mut node_count = 0;
    let mut defview = null_mut();
    let mut defview_count = 0;
    let mut workerw = null_mut();
    let mut worker_count = 0;
    let mut child = unsafe { GetWindow(parent, GW_CHILD) };
    while !child.is_null() {
        if Instant::now() >= deadline || node_count >= MAX_HIERARCHY_RECORDS {
            return Err(NativeError::HierarchyBoundExceeded);
        }
        if unsafe { IsWindow(child) } == 0 || unsafe { GetParent(child) } != parent {
            return Err(NativeError::AmbiguousHierarchy);
        }
        let kind = class_of(child).ok_or(NativeError::AmbiguousHierarchy)?;
        let mut child_process = 0;
        if unsafe { GetWindowThreadProcessId(child, &mut child_process) } == 0 {
            return Err(NativeError::AmbiguousHierarchy);
        }
        let owned_index = owned_windows[..owned_count]
            .iter()
            .position(|window| window.is_some_and(|owned| owned.hwnd == child));
        if let Some(index) = owned_index {
            let owned = owned_windows[index].expect("validated owned-window prefix");
            if kind != WindowKind::Other || unsafe { GetParent(child) } != parent {
                return Err(NativeError::PlacementVerificationFailed);
            }
            nodes[node_count] = SiblingNode::Owned(index);
            if !owned.is_owned() {
                return Err(NativeError::OwnedWindowInvalid);
            }
        } else {
            match kind {
                WindowKind::ShellDefView => {
                    if !raised_mode_styles_valid(parent_ex_style, unsafe {
                        GetWindowLongPtrW(child, GWL_EXSTYLE)
                    } as u32)
                    {
                        return Err(NativeError::UnsupportedHierarchy);
                    }
                    defview_count += 1;
                    defview = child;
                    nodes[node_count] = SiblingNode::DefView;
                }
                WindowKind::WorkerW => {
                    if child_process != shell_pid {
                        return Err(NativeError::AmbiguousHierarchy);
                    }
                    worker_count += 1;
                    workerw = child;
                    nodes[node_count] = SiblingNode::WorkerW;
                }
                _ => nodes[node_count] = SiblingNode::Foreign,
            }
        }
        node_count += 1;
        child = unsafe { GetWindow(child, GW_HWNDNEXT) };
    }
    if defview_count != 1 || worker_count > 1 {
        return Err(NativeError::AmbiguousHierarchy);
    }
    let siblings = &nodes[..node_count];
    let missing_slot_is_unambiguous =
        worker_count == 0 && can_request_missing_worker(siblings, owned_count);
    if worker_count == 1 {
        if !valid_owned_band(siblings, owned_count) {
            return Err(NativeError::AmbiguousHierarchy);
        }
    } else if !missing_slot_is_unambiguous {
        return Err(NativeError::AmbiguousHierarchy);
    }
    let hierarchy = ShellHierarchy {
        parent,
        process_id: shell_pid,
        thread_id: parent_thread,
        dpi_context,
        defview,
        workerw,
    };
    verify_shell_parent_identity(&hierarchy)?;
    Ok(ScannedHierarchy {
        hierarchy,
        unique_progman: true,
        unique_defview: true,
        raised_styles,
        worker_count,
        missing_slot_is_unambiguous,
    })
}

fn validate_owned_windows(owned_windows: &[Option<&Window>]) -> Result<usize, NativeError> {
    if owned_windows.len() > MAX_NATIVE_DISPLAYS {
        return Err(NativeError::PlacementVerificationFailed);
    }
    let mut count = 0;
    let mut saw_empty = false;
    for slot in owned_windows {
        let Some(window) = slot else {
            saw_empty = true;
            continue;
        };
        if saw_empty || !window.is_owned() {
            return Err(NativeError::OwnedWindowInvalid);
        }
        if owned_windows[..count]
            .iter()
            .flatten()
            .any(|previous| previous.hwnd == window.hwnd || previous.identity == window.identity)
        {
            return Err(NativeError::PlacementVerificationFailed);
        }
        count += 1;
    }
    Ok(count)
}

fn valid_owned_band(nodes: &[SiblingNode], owned_count: usize) -> bool {
    let Some(defview_index) = unique_node_index(nodes, SiblingNode::DefView) else {
        return false;
    };
    let Some(worker_index) = unique_node_index(nodes, SiblingNode::WorkerW) else {
        return false;
    };
    if worker_index <= defview_index {
        return false;
    }
    let mut seen = [false; MAX_NATIVE_DISPLAYS];
    for (index, node) in nodes.iter().enumerate() {
        if let SiblingNode::Owned(owned_index) = *node {
            if index <= defview_index
                || index >= worker_index
                || owned_index >= owned_count
                || seen[owned_index]
            {
                return false;
            }
            seen[owned_index] = true;
        } else if index > defview_index && index < worker_index {
            return false;
        }
    }
    seen[..owned_count].iter().all(|seen| *seen)
}

fn can_request_missing_worker(nodes: &[SiblingNode], owned_count: usize) -> bool {
    let Some(defview_index) = unique_node_index(nodes, SiblingNode::DefView) else {
        return false;
    };
    if nodes.contains(&SiblingNode::WorkerW) {
        return false;
    }
    let mut seen = [false; MAX_NATIVE_DISPLAYS];
    for (index, node) in nodes.iter().enumerate() {
        if index <= defview_index {
            if matches!(node, SiblingNode::Owned(_)) {
                return false;
            }
            continue;
        }
        let SiblingNode::Owned(owned_index) = *node else {
            return false;
        };
        if owned_index >= owned_count || seen[owned_index] {
            return false;
        }
        seen[owned_index] = true;
    }
    seen[..owned_count].iter().all(|seen| *seen)
}

fn unique_node_index(nodes: &[SiblingNode], expected: SiblingNode) -> Option<usize> {
    let mut found = None;
    for (index, node) in nodes.iter().enumerate() {
        if *node == expected {
            if found.is_some() {
                return None;
            }
            found = Some(index);
        }
    }
    found
}

#[derive(Clone, Copy)]
struct MissingWorkerRequestConditions {
    attach_or_recovery: bool,
    unique_progman: bool,
    unique_defview: bool,
    raised_styles: bool,
    worker_count: usize,
    unambiguous_siblings: bool,
    target_thread_is_distinct: bool,
    default_desktop: bool,
    cancelled: bool,
}

fn decide_missing_worker_request(conditions: MissingWorkerRequestConditions) -> bool {
    conditions.attach_or_recovery
        && conditions.unique_progman
        && conditions.unique_defview
        && conditions.raised_styles
        && conditions.worker_count == 0
        && conditions.unambiguous_siblings
        && conditions.target_thread_is_distinct
        && conditions.default_desktop
        && !conditions.cancelled
}

fn verify_shell_parent_identity(target: &ShellHierarchy) -> Result<(), NativeError> {
    if target.parent.is_null()
        || unsafe { IsWindow(target.parent) } == 0
        || !unsafe { GetParent(target.parent) }.is_null()
        || class_of(target.parent) != Some(WindowKind::Progman)
    {
        return Err(NativeError::PlacementVerificationFailed);
    }
    let mut process_id = 0;
    let thread_id = unsafe { GetWindowThreadProcessId(target.parent, &mut process_id) };
    if thread_id != target.thread_id || process_id != target.process_id {
        return Err(NativeError::PlacementVerificationFailed);
    }
    let style = unsafe { GetWindowLongPtrW(target.parent, GWL_STYLE) } as u32;
    let extended = unsafe { GetWindowLongPtrW(target.parent, GWL_EXSTYLE) } as u32;
    if style & WS_CHILD != 0 || extended & RAISED_PROGMAN_EX_STYLE != RAISED_PROGMAN_EX_STYLE {
        return Err(NativeError::PlacementVerificationFailed);
    }
    verify_defview_identity(target)?;
    let context = unsafe { GetWindowDpiAwarenessContext(target.parent) };
    if context.is_null() {
        return Err(NativeError::DpiContextUnavailable);
    }
    if unsafe { AreDpiAwarenessContextsEqual(context, target.dpi_context) } == 0 {
        return Err(NativeError::DpiContextMismatch);
    }
    Ok(())
}

fn raised_mode_styles_valid(progman_ex_style: u32, defview_ex_style: u32) -> bool {
    progman_ex_style & RAISED_PROGMAN_EX_STYLE == RAISED_PROGMAN_EX_STYLE
        && defview_ex_style & RAISED_DEFVIEW_EX_STYLE == RAISED_DEFVIEW_EX_STYLE
}

fn verify_defview_identity(target: &ShellHierarchy) -> Result<(), NativeError> {
    let defview = target.defview;
    if defview.is_null()
        || unsafe { IsWindow(defview) } == 0
        || unsafe { GetParent(defview) } != target.parent
        || class_of(defview) != Some(WindowKind::ShellDefView)
    {
        return Err(NativeError::PlacementVerificationFailed);
    }
    let mut process_id = 0;
    if unsafe { GetWindowThreadProcessId(defview, &mut process_id) } == 0
        || process_id != target.process_id
    {
        return Err(NativeError::PlacementVerificationFailed);
    }
    let extended = unsafe { GetWindowLongPtrW(defview, GWL_EXSTYLE) } as u32;
    if extended & RAISED_DEFVIEW_EX_STYLE != RAISED_DEFVIEW_EX_STYLE {
        return Err(NativeError::PlacementVerificationFailed);
    }
    Ok(())
}

fn verify_shell_hierarchy(
    target: &ShellHierarchy,
    owned_windows: &[Option<&Window>],
) -> Result<(), NativeError> {
    let current = scan_raised_hierarchy(owned_windows)?;
    if current.hierarchy != *target || current.worker_count != 1 {
        return Err(NativeError::PlacementVerificationFailed);
    }
    Ok(())
}

pub(super) fn verify_owned_windows(
    target: &ShellHierarchy,
    owned_windows: &[Option<&Window>],
    visible: bool,
) -> Result<(), NativeError> {
    let count = validate_owned_windows(owned_windows)?;
    for window in owned_windows[..count].iter().flatten() {
        if window.is_visible() != visible {
            return Err(NativeError::PlacementVerificationFailed);
        }
        verify_owned_child(window, target)?;
    }
    Ok(())
}

fn verify_owned_child(window: &Window, target: &ShellHierarchy) -> Result<(), NativeError> {
    if !window.is_owned()
        || unsafe { GetParent(window.hwnd) } != target.parent
        || unsafe { IsWindowEnabled(window.hwnd) } != 0
    {
        return Err(NativeError::PlacementVerificationFailed);
    }
    let style = unsafe { GetWindowLongPtrW(window.hwnd, GWL_STYLE) } as u32;
    let extended = unsafe { GetWindowLongPtrW(window.hwnd, GWL_EXSTYLE) } as u32;
    if style & OWNED_CHILD_STYLE != OWNED_CHILD_STYLE
        || style & WS_POPUP != 0
        || extended & OWNED_CHILD_EX_STYLE != OWNED_CHILD_EX_STYLE
        || extended & WS_EX_TOPMOST != 0
    {
        return Err(NativeError::PlacementVerificationFailed);
    }
    let window_context = unsafe { GetWindowDpiAwarenessContext(window.hwnd) };
    let parent_context = unsafe { GetWindowDpiAwarenessContext(target.parent) };
    if window_context.is_null() || parent_context.is_null() {
        return Err(NativeError::DpiContextUnavailable);
    }
    if unsafe { AreDpiAwarenessContextsEqual(window_context, target.dpi_context) } == 0
        || unsafe { AreDpiAwarenessContextsEqual(parent_context, target.dpi_context) } == 0
    {
        return Err(NativeError::DpiContextMismatch);
    }
    let Some((expected_x, expected_y, expected_width, expected_height)) =
        window.expected_client_rect
    else {
        return Err(NativeError::PlacementVerificationFailed);
    };
    let geometry_matches = with_thread_dpi_context(target.dpi_context, || {
        let mut client_rect = RECT::default();
        let mut client_origin = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
        if unsafe { GetClientRect(window.hwnd, &mut client_rect) } == 0
            || unsafe { ClientToScreen(window.hwnd, &mut client_origin) } == 0
            || unsafe { ScreenToClient(target.parent, &mut client_origin) } == 0
        {
            return Err(win32_error("GetClientRect/ClientToScreen/ScreenToClient"));
        }
        Ok(client_origin.x == expected_x
            && client_origin.y == expected_y
            && client_rect.left == 0
            && client_rect.top == 0
            && client_rect.right == expected_width
            && client_rect.bottom == expected_height)
    })?;
    if !geometry_matches {
        return Err(NativeError::PlacementVerificationFailed);
    }
    Ok(())
}

pub(super) fn foreground_window() -> HWND {
    // SAFETY: querying the current foreground HWND does not activate or mutate any window.
    unsafe { GetForegroundWindow() }
}

pub(super) fn default_input_desktop() -> Result<bool, NativeError> {
    // SAFETY: opens the current input desktop for read-only name inspection; no desktop is switched.
    let desktop = unsafe { OpenInputDesktop(0, 0, DESKTOP_READOBJECTS) };
    if desktop.is_null() {
        return Err(win32_error("OpenInputDesktop"));
    }
    let mut name = [0_u16; DESKTOP_NAME_CAPACITY];
    let mut needed = 0;
    // SAFETY: name is writable and its size is supplied in bytes, as required by the API.
    let queried = unsafe {
        GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            name.as_mut_ptr().cast::<c_void>(),
            (name.len() * size_of::<u16>()) as u32,
            &mut needed,
        )
    };
    // SAFETY: desktop is the handle opened immediately above.
    unsafe { CloseDesktop(desktop) };
    if queried == 0 {
        return Err(win32_error("GetUserObjectInformationW(UOI_NAME)"));
    }
    let end = name
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(name.len());
    let expected = *b"Default";
    Ok(end == expected.len()
        && name[..end]
            .iter()
            .zip(expected)
            .all(|(wide, byte)| *wide == u16::from(byte)))
}

pub(super) fn register_session_notifications(hwnd: HWND) -> Result<(), NativeError> {
    // SAFETY: hwnd is an owned hidden controller window with a message queue.
    if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } == 0 {
        return Err(win32_error("WTSRegisterSessionNotification"));
    }
    Ok(())
}

pub(super) fn unregister_session_notifications(hwnd: HWND) {
    // SAFETY: unregisters the notification registration owned by this adapter.
    unsafe { WTSUnRegisterSessionNotification(hwnd) };
}

pub(super) fn register_taskbar_created() -> Result<u32, NativeError> {
    let name = [
        84, 97, 115, 107, 98, 97, 114, 67, 114, 101, 97, 116, 101, 100, 0,
    ];
    // SAFETY: name is NUL-terminated and RegisterWindowMessageW is a read-only registration query.
    let message = unsafe { RegisterWindowMessageW(name.as_ptr()) };
    if message == 0 {
        Err(win32_error("RegisterWindowMessageW(TaskbarCreated)"))
    } else {
        Ok(message)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeSignals {
    pub display_changed: bool,
    pub explorer_restarted: bool,
    pub locked: Option<bool>,
    pub owned_window_destroyed: bool,
}

fn set_window_long(
    hwnd: HWND,
    index: i32,
    value: isize,
    operation: &'static str,
) -> Result<(), NativeError> {
    // SAFETY: hwnd is an owned window on the current thread; the style index and bit pattern are
    // defined by Win32. Clear last error to distinguish a previous zero style from failure.
    unsafe { SetLastError(0) };
    unsafe { SetWindowLongPtrW(hwnd, index, value) };
    let error = unsafe { GetLastError() };
    if error == 0 {
        Ok(())
    } else {
        Err(NativeError::Win32 {
            operation,
            code: error,
        })
    }
}

fn win32_error(operation: &'static str) -> NativeError {
    NativeError::Win32 {
        operation,
        // SAFETY: reads the calling thread's last Win32 error immediately after a failed API.
        code: unsafe { GetLastError() },
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        CREATION_STAGE.with(|stage| stage.set(stage.get() | CREATION_REACHED_NCCREATE));
    } else if message == WM_CREATE {
        CREATION_STAGE.with(|stage| stage.set(stage.get() | CREATION_REACHED_CREATE));
    }
    if message == wm_mouseactivate {
        MA_NOACTIVATE as LRESULT
    } else if message == wm_erasebkgnd {
        1
    } else {
        // SAFETY: all other messages are delegated to the default procedure; this adapter has no
        // callback state and never subclasses or invokes Explorer code.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::WS_VISIBLE;

    #[test]
    fn surface_window_applies_layered_style_after_hidden_creation() {
        assert_ne!(OWNED_POPUP_CREATE_STYLE & WS_POPUP, 0);
        assert_eq!(OWNED_POPUP_CREATE_STYLE & (WS_CHILD | WS_VISIBLE), 0);
        assert_eq!(OWNED_WINDOW_EX_STYLE & WS_EX_LAYERED, 0);

        let class_name = format!(
            "AsciiWallWnd002Lifecycle_{}_{}",
            unsafe { GetCurrentProcessId() },
            unsafe { GetCurrentThreadId() },
        );
        let class = WindowClass::register(class_name).expect("register private test class");
        let controller = class
            .create_controller()
            .expect("create hidden test controller");
        let dpi_context = unsafe { GetWindowDpiAwarenessContext(controller.hwnd) };
        assert!(!dpi_context.is_null());
        let target = ShellHierarchy {
            parent: controller.hwnd,
            process_id: unsafe { GetCurrentProcessId() },
            thread_id: unsafe { GetCurrentThreadId() },
            dpi_context,
            defview: null_mut(),
            workerw: null_mut(),
        };

        let child = class
            .create_surface_window(&target, 0, 0, 1, 1)
            .expect("create hidden surface child");
        assert_eq!(unsafe { GetParent(child.hwnd) }, controller.hwnd);
        let style = unsafe { GetWindowLongPtrW(child.hwnd, GWL_STYLE) as u32 };
        assert_ne!(style & WS_CHILD, 0);
        assert_eq!(style & WS_VISIBLE, 0);
        assert_eq!(unsafe { IsWindowVisible(child.hwnd) }, 0);
        let ex_style = unsafe { GetWindowLongPtrW(child.hwnd, GWL_EXSTYLE) as u32 };
        assert_ne!(ex_style & WS_EX_LAYERED, 0);
        assert_eq!(ex_style & WS_EX_TOPMOST, 0);

        let mut color_key = 0;
        let mut alpha = 0;
        let mut flags = 0;
        assert_ne!(
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::GetLayeredWindowAttributes(
                    child.hwnd,
                    &mut color_key,
                    &mut alpha,
                    &mut flags,
                )
            },
            0
        );
        assert_eq!(alpha, OPAQUE_LAYER_ALPHA);
        assert_eq!(flags, LWA_ALPHA);
    }

    #[test]
    fn recovery_window_style_is_child_disabled_and_never_topmost() {
        assert_ne!(OWNED_CHILD_STYLE & WS_CHILD, 0);
        assert_ne!(OWNED_CHILD_STYLE & WS_DISABLED, 0);
        assert_eq!(OWNED_WINDOW_EX_STYLE & WS_EX_TOPMOST, 0);
        assert_ne!(OWNED_WINDOW_EX_STYLE & WS_EX_NOACTIVATE, 0);
    }

    #[test]
    fn raised_window_style_is_layered_opaque_nonactivating_and_bounded() {
        assert_ne!(OWNED_CHILD_EX_STYLE & WS_EX_LAYERED, 0);
        assert_ne!(OWNED_CHILD_EX_STYLE & WS_EX_NOACTIVATE, 0);
        assert_ne!(OWNED_CHILD_STYLE & WS_CHILD, 0);
        assert_ne!(OWNED_CHILD_STYLE & WS_DISABLED, 0);
        assert_eq!(OWNED_CHILD_EX_STYLE & WS_EX_TOPMOST, 0);
        assert_eq!(OPAQUE_LAYER_ALPHA, 255);
        assert_eq!(WORKERW_REQUEST_TIMEOUT_MS, 250);
        assert_eq!(WM_SPAWN_WORKERW, 0x052c);
        assert_eq!(
            RAISED_PROGMAN_EX_STYLE & WS_EX_NOREDIRECTIONBITMAP,
            WS_EX_NOREDIRECTIONBITMAP
        );
        assert_eq!(RAISED_PROGMAN_EX_STYLE & WS_EX_LAYERED, 0);
        assert_eq!(RAISED_DEFVIEW_EX_STYLE & WS_EX_NOREDIRECTIONBITMAP, 0);
        assert_eq!(RAISED_DEFVIEW_EX_STYLE & WS_EX_LAYERED, WS_EX_LAYERED);
    }

    #[test]
    fn raised_style_bits_are_split_between_progman_and_defview() {
        assert!(raised_mode_styles_valid(
            WS_EX_NOREDIRECTIONBITMAP,
            WS_EX_LAYERED,
        ));
        assert!(raised_mode_styles_valid(
            WS_EX_NOREDIRECTIONBITMAP,
            WS_EX_LAYERED | WS_EX_TOOLWINDOW,
        ));
        assert!(!raised_mode_styles_valid(0, WS_EX_LAYERED));
        assert!(!raised_mode_styles_valid(WS_EX_NOREDIRECTIONBITMAP, 0));
    }

    #[test]
    fn unrelated_top_level_classes_do_not_consume_candidate_budget() {
        let mut candidate_work = MAX_HIERARCHY_RECORDS - 1;
        for _ in 0..=MAX_HIERARCHY_RECORDS {
            assert_eq!(
                decide_progman_callback(false, true, candidate_work),
                ProgmanCallbackDecision::Skip,
            );
        }
        assert_eq!(candidate_work, MAX_HIERARCHY_RECORDS - 1);
        assert_eq!(
            decide_progman_callback(true, true, candidate_work),
            ProgmanCallbackDecision::CountCandidate,
        );
        candidate_work += 1;
        assert_eq!(
            decide_progman_callback(true, true, candidate_work),
            ProgmanCallbackDecision::CandidateBudgetExceeded,
        );
    }

    #[test]
    fn candidate_work_budget_exhaustion_fails_closed() {
        assert_eq!(
            decide_progman_callback(true, true, MAX_HIERARCHY_RECORDS),
            ProgmanCallbackDecision::CandidateBudgetExceeded,
        );
    }

    #[test]
    fn deadline_is_checked_for_skipped_unrelated_top_level_classes() {
        assert_eq!(
            decide_progman_callback(false, false, 0),
            ProgmanCallbackDecision::DeadlineExceeded,
        );
    }

    #[test]
    fn native_display_and_hierarchy_capacity_are_fixed_and_bounded() {
        assert_eq!(MAX_NATIVE_DISPLAYS, 16);
        assert_eq!(MAX_HIERARCHY_RECORDS, 512);
        assert_eq!(HIERARCHY_DEADLINE, Duration::from_millis(250));
    }

    #[test]
    fn complete_owned_band_allows_multiple_surfaces_once_between_shell_views() {
        let nodes = [
            SiblingNode::Foreign,
            SiblingNode::DefView,
            SiblingNode::Owned(1),
            SiblingNode::Owned(0),
            SiblingNode::WorkerW,
            SiblingNode::Foreign,
        ];
        assert!(valid_owned_band(&nodes, 2));
        assert!(!valid_owned_band(&nodes, 3));
    }

    #[test]
    fn complete_owned_band_rejects_foreign_or_repeated_intervening_windows() {
        assert!(!valid_owned_band(
            &[
                SiblingNode::DefView,
                SiblingNode::Owned(0),
                SiblingNode::Foreign,
                SiblingNode::WorkerW,
            ],
            1,
        ));
        assert!(!valid_owned_band(
            &[
                SiblingNode::DefView,
                SiblingNode::Owned(0),
                SiblingNode::Owned(0),
                SiblingNode::WorkerW,
            ],
            1,
        ));
        assert!(!valid_owned_band(
            &[
                SiblingNode::DefView,
                SiblingNode::WorkerW,
                SiblingNode::Owned(0),
            ],
            1,
        ));
        assert!(!valid_owned_band(
            &[
                SiblingNode::DefView,
                SiblingNode::DefView,
                SiblingNode::WorkerW,
            ],
            0,
        ));
        assert!(!valid_owned_band(
            &[
                SiblingNode::DefView,
                SiblingNode::WorkerW,
                SiblingNode::WorkerW,
            ],
            0,
        ));
        assert!(!valid_owned_band(
            &[
                SiblingNode::Owned(0),
                SiblingNode::DefView,
                SiblingNode::Owned(0),
                SiblingNode::WorkerW,
            ],
            1,
        ));
    }

    #[test]
    fn missing_worker_request_requires_unambiguous_owned_tail() {
        assert!(can_request_missing_worker(
            &[
                SiblingNode::Foreign,
                SiblingNode::DefView,
                SiblingNode::Owned(0),
                SiblingNode::Owned(1),
            ],
            2,
        ));
        assert!(can_request_missing_worker(&[SiblingNode::DefView], 0));
        assert!(!can_request_missing_worker(
            &[SiblingNode::DefView, SiblingNode::Foreign],
            0,
        ));
        assert!(!can_request_missing_worker(
            &[
                SiblingNode::DefView,
                SiblingNode::Owned(0),
                SiblingNode::Owned(0),
            ],
            1,
        ));
        assert!(!can_request_missing_worker(
            &[
                SiblingNode::DefView,
                SiblingNode::Owned(0),
                SiblingNode::Owned(1),
            ],
            1,
        ));
        assert!(!can_request_missing_worker(
            &[
                SiblingNode::Owned(0),
                SiblingNode::DefView,
                SiblingNode::Owned(0),
            ],
            1,
        ));
    }

    #[test]
    fn missing_worker_request_is_gated_by_raised_mode_session_and_cancellation() {
        let conditions = MissingWorkerRequestConditions {
            attach_or_recovery: true,
            unique_progman: true,
            unique_defview: true,
            raised_styles: true,
            worker_count: 0,
            unambiguous_siblings: true,
            target_thread_is_distinct: true,
            default_desktop: true,
            cancelled: false,
        };
        assert!(decide_missing_worker_request(conditions));
        assert!(!decide_missing_worker_request(
            MissingWorkerRequestConditions {
                attach_or_recovery: false,
                ..conditions
            }
        ));
        assert!(!decide_missing_worker_request(
            MissingWorkerRequestConditions {
                unique_progman: false,
                ..conditions
            }
        ));
        assert!(!decide_missing_worker_request(
            MissingWorkerRequestConditions {
                unique_defview: false,
                ..conditions
            }
        ));
        assert!(!decide_missing_worker_request(
            MissingWorkerRequestConditions {
                raised_styles: false,
                ..conditions
            }
        ));
        assert!(!decide_missing_worker_request(
            MissingWorkerRequestConditions {
                worker_count: 1,
                ..conditions
            }
        ));
        assert!(!decide_missing_worker_request(
            MissingWorkerRequestConditions {
                unambiguous_siblings: false,
                ..conditions
            }
        ));
        assert!(!decide_missing_worker_request(
            MissingWorkerRequestConditions {
                target_thread_is_distinct: false,
                ..conditions
            }
        ));
        assert!(!decide_missing_worker_request(
            MissingWorkerRequestConditions {
                default_desktop: false,
                ..conditions
            }
        ));
        assert!(!decide_missing_worker_request(
            MissingWorkerRequestConditions {
                cancelled: true,
                ..conditions
            }
        ));
    }
}
