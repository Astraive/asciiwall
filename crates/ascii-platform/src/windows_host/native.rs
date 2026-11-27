use std::{
    ffi::c_void,
    mem::size_of,
    ptr::{null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

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
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumChildWindows,
                EnumWindows, GW_HWNDNEXT, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetClientRect,
                GetForegroundWindow, GetParent, GetPropW, GetWindow, GetWindowLongPtrW,
                GetWindowThreadProcessId, HWND_BOTTOM, IsWindow, IsWindowVisible, MA_NOACTIVATE,
                MSG, PM_REMOVE, PeekMessageW, RegisterClassExW, RegisterWindowMessageW, SW_HIDE,
                SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SetParent,
                SetPropW, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
                UnregisterClassW, WM_DISPLAYCHANGE as wm_displaychange,
                WM_ERASEBKGND as wm_erasebkgnd, WM_MOUSEACTIVATE as wm_mouseactivate, WNDCLASSEXW,
                WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_DISABLED, WS_EX_NOACTIVATE,
                WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::BOOL,
};

pub(super) const MAX_NATIVE_DISPLAYS: usize = 16;
const MAX_HIERARCHY_RECORDS: u32 = 512;
const HIERARCHY_DEADLINE: Duration = Duration::from_millis(250);
const WM_WTSSESSION_CHANGE: u32 = 0x02b1;
const WTS_SESSION_REMOTE_CONNECT: usize = 0x3;
const WTS_SESSION_REMOTE_DISCONNECT: usize = 0x4;
const WTS_SESSION_LOCK: usize = 0x7;
const WTS_SESSION_UNLOCK: usize = 0x8;
const NOTIFY_FOR_THIS_SESSION: u32 = 0;
const DESKTOP_NAME_CAPACITY: usize = 64;
const OWNED_CHILD_STYLE: u32 = WS_CHILD | WS_DISABLED | WS_CLIPSIBLINGS | WS_CLIPCHILDREN;
const OWNED_WINDOW_EX_STYLE: u32 = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
static NEXT_WINDOW_ID: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeError {
    Win32 { operation: &'static str, code: u32 },
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
    IdentityExhausted,
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
        })
    }

    pub fn create_surface_window(
        &self,
        dpi_context: DPI_AWARENESS_CONTEXT,
        screen_left: i32,
        screen_top: i32,
        width: u32,
        height: u32,
    ) -> Result<Window, NativeError> {
        let width = i32::try_from(width).map_err(|_| NativeError::InvalidDisplayBounds)?;
        let height = i32::try_from(height).map_err(|_| NativeError::InvalidDisplayBounds)?;
        self.create(WindowCreateOptions {
            extended_style: OWNED_WINDOW_EX_STYLE,
            style: WS_POPUP | WS_DISABLED | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
            bounds: WindowCreateBounds {
                left: screen_left,
                top: screen_top,
                width,
                height,
            },
            parent: null_mut(),
            dpi_context: Some(dpi_context),
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

        // SAFETY: all pointers reference stable, NUL-terminated UTF-16 storage. The class owns the
        // registered class, and this call creates only this process's hidden, disabled HWND.
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
        let creation_error = (!hwnd.is_null())
            .then_some(())
            .map_or_else(|| Some(unsafe { GetLastError() }), |_| None);

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
            return Err(NativeError::Win32 {
                operation: "CreateWindowExW",
                code: creation_error.unwrap_or(0),
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
        parent: HWND,
        parent_process_id: u32,
        screen_left: i32,
        screen_top: i32,
        width: u32,
        height: u32,
    ) -> Result<(), NativeError> {
        if !self.is_owned()
            || parent.is_null()
            || !self.same_shell_parent(parent, parent_process_id)
        {
            return Err(NativeError::OwnedWindowInvalid);
        }
        // SAFETY: these queries return system DPI-awareness pseudo-handles for live HWNDs.
        let window_context = unsafe { GetWindowDpiAwarenessContext(self.hwnd) };
        let parent_context = unsafe { GetWindowDpiAwarenessContext(parent) };
        if window_context.is_null() || parent_context.is_null() {
            return Err(NativeError::DpiContextUnavailable);
        }
        // SAFETY: contexts are system-provided pseudo-handles and remain valid for comparison.
        if unsafe { AreDpiAwarenessContextsEqual(window_context, parent_context) } == 0 {
            return Err(NativeError::DpiContextMismatch);
        }
        let width = i32::try_from(width).map_err(|_| NativeError::InvalidDisplayBounds)?;
        let height = i32::try_from(height).map_err(|_| NativeError::InvalidDisplayBounds)?;

        with_thread_dpi_context(parent_context, || {
            // Set required child/nonactivation styles before SetParent; Win32 does not infer them.
            set_window_long(
                self.hwnd,
                GWL_STYLE,
                OWNED_CHILD_STYLE as isize,
                "SetWindowLongPtrW(style)",
            )?;
            set_window_long(
                self.hwnd,
                GWL_EXSTYLE,
                OWNED_WINDOW_EX_STYLE as isize,
                "SetWindowLongPtrW(exstyle)",
            )?;
            // SAFETY: `parent` was revalidated as a shell-owned Progman/WorkerW HWND and
            // `self.hwnd` is this process's hidden owned window. No foreign window is destroyed.
            unsafe { SetLastError(0) };
            let previous_parent = unsafe { SetParent(self.hwnd, parent) };
            let last_error = unsafe { GetLastError() };
            if previous_parent.is_null() && last_error != 0 {
                return Err(NativeError::Win32 {
                    operation: "SetParent",
                    code: last_error,
                });
            }

            let mut point = windows_sys::Win32::Foundation::POINT {
                x: screen_left,
                y: screen_top,
            };
            // SAFETY: `parent` is a live shell-owned window and point is writable. The thread is
            // temporarily using the same DPI-awareness context as the parent.
            if unsafe { ScreenToClient(parent, &mut point) } == 0 {
                return Err(win32_error("ScreenToClient"));
            }
            // SAFETY: positions only our owned hidden child, at the parent's bottom non-topmost
            // child Z-order, and forbids activation and owner-Z-order changes.
            if unsafe {
                SetWindowPos(
                    self.hwnd,
                    HWND_BOTTOM,
                    point.x,
                    point.y,
                    width,
                    height,
                    SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                )
            } == 0
            {
                return Err(win32_error("SetWindowPos(HWND_BOTTOM)"));
            }
            self.expected_client_rect = Some((point.x, point.y, width, height));
            Ok(())
        })
    }

    pub fn verify_hidden_placement(
        &self,
        parent: HWND,
        parent_process_id: u32,
    ) -> Result<(), NativeError> {
        self.verify_placement(parent, parent_process_id, false)
    }

    pub fn verify_visible_placement(
        &self,
        parent: HWND,
        parent_process_id: u32,
    ) -> Result<(), NativeError> {
        self.verify_placement(parent, parent_process_id, true)
    }

    fn verify_placement(
        &self,
        parent: HWND,
        parent_process_id: u32,
        visible: bool,
    ) -> Result<(), NativeError> {
        if !self.is_owned()
            || self.is_visible() != visible
            || !self.same_shell_parent(parent, parent_process_id)
            || unsafe { GetParent(self.hwnd) } != parent
            || !unsafe { GetWindow(self.hwnd, GW_HWNDNEXT) }.is_null()
            || unsafe { IsWindowEnabled(self.hwnd) } != 0
        {
            return Err(NativeError::PlacementVerificationFailed);
        }
        let Some((expected_x, expected_y, expected_width, expected_height)) =
            self.expected_client_rect
        else {
            return Err(NativeError::PlacementVerificationFailed);
        };
        let window_context = unsafe { GetWindowDpiAwarenessContext(self.hwnd) };
        let parent_context = unsafe { GetWindowDpiAwarenessContext(parent) };
        if window_context.is_null() || parent_context.is_null() {
            return Err(NativeError::DpiContextUnavailable);
        }
        if unsafe { AreDpiAwarenessContextsEqual(window_context, parent_context) } == 0 {
            return Err(NativeError::DpiContextMismatch);
        }
        let geometry_matches = with_thread_dpi_context(parent_context, || {
            let mut client_rect = RECT::default();
            let mut client_origin = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
            // SAFETY: hwnd and parent were validated as live owned/shell windows; both output
            // records are writable. The APIs only read geometry and do not change activation.
            if unsafe { GetClientRect(self.hwnd, &mut client_rect) } == 0
                || unsafe { ClientToScreen(self.hwnd, &mut client_origin) } == 0
                || unsafe { ScreenToClient(parent, &mut client_origin) } == 0
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
        let style = unsafe { GetWindowLongPtrW(self.hwnd, GWL_STYLE) } as u32;
        let extended = unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) } as u32;
        if style & WS_CHILD == 0
            || style & WS_POPUP != 0
            || style & WS_DISABLED == 0
            || style & (WS_CLIPSIBLINGS | WS_CLIPCHILDREN) != WS_CLIPSIBLINGS | WS_CLIPCHILDREN
            || extended & WS_EX_NOACTIVATE == 0
            || extended & WS_EX_TOOLWINDOW == 0
            || extended & WS_EX_TOPMOST != 0
        {
            return Err(NativeError::PlacementVerificationFailed);
        }
        Ok(())
    }

    pub fn show_no_activate(
        &self,
        parent: HWND,
        parent_process_id: u32,
        foreground_before: HWND,
    ) -> Result<(), NativeError> {
        self.verify_hidden_placement(parent, parent_process_id)?;
        // SAFETY: this is our own disabled, WS_EX_NOACTIVATE child; SW_SHOWNOACTIVATE never
        // requests activation and the window is already at the parent's bottom child Z-order.
        unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
        if unsafe { GetForegroundWindow() } != foreground_before {
            self.hide();
            return Err(NativeError::PlacementVerificationFailed);
        }
        self.verify_visible_placement(parent, parent_process_id)
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

    fn same_shell_parent(&self, parent: HWND, process_id: u32) -> bool {
        if parent.is_null() || unsafe { IsWindow(parent) } == 0 {
            return false;
        }
        let mut actual_process_id = 0;
        // SAFETY: parent was checked live and the function only writes a process ID.
        let thread_id = unsafe { GetWindowThreadProcessId(parent, &mut actual_process_id) };
        thread_id != 0
            && actual_process_id == process_id
            && matches!(
                class_of(parent),
                Some(WindowKind::Progman | WindowKind::WorkerW)
            )
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

pub(super) struct ShellTarget {
    pub parent: HWND,
    pub process_id: u32,
    pub dpi_context: DPI_AWARENESS_CONTEXT,
    pub process: ShellProcess,
}

pub(super) fn discover_shell_target() -> Result<ShellTarget, NativeError> {
    // SAFETY: GetShellWindow returns a borrowed shell HWND and does not modify desktop state.
    let shell = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetShellWindow() };
    if shell.is_null() {
        return Err(NativeError::ShellUnavailable);
    }
    let mut shell_pid = 0;
    // SAFETY: shell is borrowed from GetShellWindow and shell_pid is writable.
    if unsafe { GetWindowThreadProcessId(shell, &mut shell_pid) } == 0 || shell_pid == 0 {
        return Err(win32_error("GetWindowThreadProcessId(shell)"));
    }
    let started = Instant::now();
    let deadline = started.checked_add(HIERARCHY_DEADLINE).unwrap_or(started);
    let mut scan = HierarchyScan {
        shell,
        shell_pid,
        candidate: null_mut(),
        work: 0,
        deadline,
        failure: None,
    };
    let pointer = &mut scan as *mut HierarchyScan;
    // Inspect the known shell root first so unrelated roots cannot consume its shared bounded work.
    if !scan_root(pointer, shell, true) {
        return Err(scan.failure.unwrap_or(NativeError::UnsupportedHierarchy));
    }
    // SAFETY: EnumWindows invokes this callback synchronously while scan remains alive.
    let enumerated = unsafe { EnumWindows(Some(top_level_callback), pointer as LPARAM) };
    if let Some(failure) = scan.failure {
        return Err(failure);
    }
    if enumerated == 0 || Instant::now() >= scan.deadline {
        return Err(NativeError::HierarchyBoundExceeded);
    }
    if scan.candidate.is_null() {
        return Err(NativeError::UnsupportedHierarchy);
    }
    let dpi_context = unsafe { GetWindowDpiAwarenessContext(scan.candidate) };
    if dpi_context.is_null() {
        return Err(NativeError::DpiContextUnavailable);
    }
    // Monitor bounds and SetWindowPos coordinates are physical pixels, so refuse a shell parent
    // whose DPI virtualization would make the coordinate spaces ambiguous.
    // SAFETY: dpi_context is a system-provided context for the live candidate HWND.
    if unsafe { GetAwarenessFromDpiAwarenessContext(dpi_context) }
        != DPI_AWARENESS_PER_MONITOR_AWARE
    {
        return Err(NativeError::DpiContextMismatch);
    }
    // SAFETY: the process handle is opened read-only for SYNCHRONIZE, solely to detect Explorer
    // termination. No process memory, token, or window state is changed.
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, shell_pid) };
    if process.is_null() {
        return Err(win32_error("OpenProcess(shell SYNCHRONIZE)"));
    }
    let process = ShellProcess {
        handle: process,
        process_id: shell_pid,
    };
    if !process.is_alive()? {
        return Err(NativeError::ShellUnavailable);
    }
    Ok(ShellTarget {
        parent: scan.candidate,
        process_id: shell_pid,
        dpi_context,
        process,
    })
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
    // SAFETY: class buffer is writable and hwnd is a borrowed window from enumeration.
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

struct HierarchyScan {
    shell: HWND,
    shell_pid: u32,
    candidate: HWND,
    work: u32,
    deadline: Instant,
    failure: Option<NativeError>,
}

fn check_hierarchy_deadline(scan: &mut HierarchyScan) -> bool {
    if Instant::now() >= scan.deadline {
        scan.failure = Some(NativeError::HierarchyBoundExceeded);
        false
    } else {
        true
    }
}

fn reserve_hierarchy_work(scan: &mut HierarchyScan) -> bool {
    if !check_hierarchy_deadline(scan) || scan.work >= MAX_HIERARCHY_RECORDS {
        scan.failure = Some(NativeError::HierarchyBoundExceeded);
        return false;
    }
    scan.work += 1;
    true
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TopLevelAction {
    Continue,
    ScanCandidate,
    Abort,
}

fn decide_top_level_action(
    scan: &mut HierarchyScan,
    is_shell_duplicate: bool,
    class_query: impl FnOnce() -> Option<WindowKind>,
) -> TopLevelAction {
    if !check_hierarchy_deadline(scan) {
        return TopLevelAction::Abort;
    }
    if is_shell_duplicate || !is_top_level_candidate(class_query()) {
        return TopLevelAction::Continue;
    }
    if !reserve_hierarchy_work(scan) {
        TopLevelAction::Abort
    } else {
        TopLevelAction::ScanCandidate
    }
}

fn is_top_level_candidate(kind: Option<WindowKind>) -> bool {
    matches!(kind, Some(WindowKind::Progman | WindowKind::WorkerW))
}

fn scan_root(pointer: *mut HierarchyScan, root: HWND, known_shell: bool) -> bool {
    if pointer.is_null() {
        return false;
    }
    // SAFETY: pointer refers to the live stack scan used by the synchronous Win32 enumeration.
    let scan = unsafe { &mut *pointer };
    if !reserve_hierarchy_work(scan) {
        return false;
    }
    scan_root_with_reserved_work(pointer, root, known_shell)
}

fn scan_root_with_reserved_work(
    pointer: *mut HierarchyScan,
    root: HWND,
    known_shell: bool,
) -> bool {
    if pointer.is_null() {
        return false;
    }
    // SAFETY: pointer refers to the live stack scan used by the synchronous Win32 enumeration.
    let scan = unsafe { &mut *pointer };
    let kind = class_of(root);
    let mut process_id = 0;
    // SAFETY: root is a borrowed live HWND from GetShellWindow/EnumWindows.
    let thread_id = unsafe { GetWindowThreadProcessId(root, &mut process_id) };
    if thread_id == 0 || process_id != scan.shell_pid {
        return true;
    }
    if !known_shell && !matches!(kind, Some(WindowKind::Progman | WindowKind::WorkerW)) {
        return true;
    }
    // SAFETY: EnumChildWindows runs synchronously and the scan pointer remains valid throughout.
    let enumerated = unsafe { EnumChildWindows(root, Some(child_callback), pointer as LPARAM) };
    enumerated != 0 || scan.failure.is_none()
}

unsafe extern "system" fn child_callback(child: HWND, parameter: LPARAM) -> BOOL {
    if parameter == 0 {
        return 0;
    }
    // SAFETY: EnumChildWindows invokes the callback synchronously with the live scan pointer.
    let scan = unsafe { &mut *(parameter as *mut HierarchyScan) };
    if !reserve_hierarchy_work(scan) {
        return 0;
    }
    if class_of(child) != Some(WindowKind::ShellDefView) {
        return 1;
    }
    let parent = unsafe { GetParent(child) };
    let Some(parent_kind @ (WindowKind::Progman | WindowKind::WorkerW)) = class_of(parent) else {
        return 1;
    };
    let _ = parent_kind;
    let mut parent_process = 0;
    // SAFETY: parent is the direct parent of a borrowed child HWND.
    if unsafe { GetWindowThreadProcessId(parent, &mut parent_process) } == 0
        || parent_process != scan.shell_pid
    {
        return 1;
    }
    if scan.candidate.is_null() {
        scan.candidate = parent;
    } else if scan.candidate != parent {
        scan.failure = Some(NativeError::AmbiguousHierarchy);
        return 0;
    }
    1
}
unsafe extern "system" fn top_level_callback(window: HWND, parameter: LPARAM) -> BOOL {
    if parameter == 0 {
        return 0;
    }
    // SAFETY: EnumWindows invokes the callback synchronously while scan remains alive.
    let scan = unsafe { &mut *(parameter as *mut HierarchyScan) };
    let action = decide_top_level_action(scan, window == scan.shell, || class_of(window));
    match action {
        TopLevelAction::Continue => 1,
        TopLevelAction::ScanCandidate => {
            if scan_root_with_reserved_work(parameter as *mut HierarchyScan, window, false) {
                1
            } else {
                0
            }
        }
        TopLevelAction::Abort => 0,
    }
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

    #[test]
    fn recovery_window_style_is_child_disabled_and_never_topmost() {
        assert_ne!(OWNED_CHILD_STYLE & WS_CHILD, 0);
        assert_ne!(OWNED_CHILD_STYLE & WS_DISABLED, 0);
        assert_eq!(OWNED_WINDOW_EX_STYLE & WS_EX_TOPMOST, 0);
        assert_ne!(OWNED_WINDOW_EX_STYLE & WS_EX_NOACTIVATE, 0);
    }

    #[test]
    fn native_display_capacity_is_fixed_and_bounded() {
        assert_eq!(MAX_NATIVE_DISPLAYS, 16);
        assert_eq!(MAX_HIERARCHY_RECORDS, 512);
        assert_eq!(HIERARCHY_DEADLINE, Duration::from_millis(250));
    }
    fn hierarchy_scan(deadline: Instant) -> HierarchyScan {
        HierarchyScan {
            shell: null_mut(),
            shell_pid: 1,
            candidate: null_mut(),
            work: 0,
            deadline,
            failure: None,
        }
    }

    #[test]
    fn unrelated_top_level_classes_do_not_consume_candidate_budget() {
        let mut scan = hierarchy_scan(Instant::now() + HIERARCHY_DEADLINE);
        for _ in 0..=MAX_HIERARCHY_RECORDS {
            assert_eq!(
                decide_top_level_action(&mut scan, false, || Some(WindowKind::Other)),
                TopLevelAction::Continue
            );
        }
        assert_eq!(scan.work, 0);
        assert_eq!(
            decide_top_level_action(&mut scan, false, || Some(WindowKind::WorkerW)),
            TopLevelAction::ScanCandidate
        );
        assert_eq!(scan.work, 1);
    }

    #[test]
    fn candidate_work_budget_exhaustion_fails_closed() {
        let mut scan = hierarchy_scan(Instant::now() + HIERARCHY_DEADLINE);
        scan.work = MAX_HIERARCHY_RECORDS;
        assert_eq!(
            decide_top_level_action(&mut scan, false, || Some(WindowKind::Progman)),
            TopLevelAction::Abort
        );
        assert_eq!(scan.failure, Some(NativeError::HierarchyBoundExceeded));
    }

    #[test]
    fn deadline_is_checked_for_skipped_unrelated_top_level_classes() {
        let mut scan = hierarchy_scan(Instant::now());
        let mut queried = false;
        assert_eq!(
            decide_top_level_action(&mut scan, false, || {
                queried = true;
                Some(WindowKind::Other)
            }),
            TopLevelAction::Abort
        );
        assert!(!queried);
        assert_eq!(scan.work, 0);
        assert_eq!(scan.failure, Some(NativeError::HierarchyBoundExceeded));
    }
}
