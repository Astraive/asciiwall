#[allow(unsafe_code)]
mod native;

use std::{
    marker::PhantomData,
    num::NonZeroIsize,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::ThreadId,
    time::{Duration, Instant},
};

use ascii_convert::GlyphTable;
use ascii_core::{
    AsciiFrame, Budget, CancellationToken, GlyphTableId, MonitorFallbackMatch, MonitorId,
    MonitorIdentity, validate_frame,
};
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};

use crate::{
    AdapterVersion, CapabilityAvailability, CapabilityFeature, CapabilityRecord, CapabilityScope,
    CapabilitySet, CapabilitySnapshot, Confidence, FeedEvent, Freshness, HostAdapter, HostEvent,
    HostEventBatch, HostEventQueue, HostHealth, HostIdentity, HostRestrictions, HostStateSnapshot,
    SessionLockState, SurfaceLease, SurfaceLeaseError, SurfaceLeaseRegistry,
    SurfaceRetirementReason,
};
/// Maximum number of outputs the Windows host will enumerate and own.
pub const MAX_WINDOWS_HOST_DISPLAYS: usize = native::MAX_NATIVE_DISPLAYS;
const MAX_RECOVERY_ATTEMPTS: u8 = 5;
const MAX_MESSAGES_PER_POLL: usize = 256;
const SESSION_CHECK_INTERVAL: Duration = Duration::from_millis(250);
const RECOVERY_BASE_DELAY: Duration = Duration::from_millis(100);
const RECOVERY_MAX_DELAY: Duration = Duration::from_millis(1_600);
const TOPOLOGY_CHECK_INTERVAL: Duration = Duration::from_secs(5);

static NEXT_HOST_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_MONITOR_ID: AtomicU64 = AtomicU64::new(1);

/// Physical output geometry captured from a full `EnumDisplayMonitors` enumeration.
///
/// The identity is adapter-local and changes when the OS device name disappears and is later
/// reused. Coordinates and dimensions are physical virtual-desktop pixels, not DPI-virtualized
/// window rectangles. Native monitor handles and device names are kept private by the host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayInfo {
    identity: MonitorIdentity,
    left_px: i32,
    top_px: i32,
    width_px: u32,
    height_px: u32,
}

impl DisplayInfo {
    /// Opaque local identity used for leases and stale-display rejection.
    #[must_use]
    pub const fn identity(self) -> MonitorIdentity {
        self.identity
    }

    /// Signed physical horizontal origin in virtual-desktop coordinates.
    #[must_use]
    pub const fn left_px(self) -> i32 {
        self.left_px
    }

    /// Signed physical vertical origin in virtual-desktop coordinates.
    #[must_use]
    pub const fn top_px(self) -> i32 {
        self.top_px
    }

    /// Physical width in pixels.
    #[must_use]
    pub const fn width_px(self) -> u32 {
        self.width_px
    }

    /// Physical height in pixels.
    #[must_use]
    pub const fn height_px(self) -> u32 {
        self.height_px
    }
}

/// Fixed-capacity output snapshot; it never retains an `HMONITOR` or OS device path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplaySnapshot {
    records: [Option<DisplayInfo>; MAX_WINDOWS_HOST_DISPLAYS],
    len: u8,
    revision: u64,
}

impl DisplaySnapshot {
    fn empty(revision: u64) -> Self {
        Self {
            records: [None; MAX_WINDOWS_HOST_DISPLAYS],
            len: 0,
            revision,
        }
    }

    /// Number of currently enumerated physical outputs.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Whether no active output was enumerated.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Monotonic topology revision, advanced after a changed full enumeration.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Iterates current output records without allocating.
    pub fn iter(&self) -> impl Iterator<Item = &DisplayInfo> {
        self.records[..usize::from(self.len)].iter().flatten()
    }
}

/// Inputs needed to build the host-owned WGPU presenter and its first valid frame.
pub struct WindowsHostConfig {
    glyph_table: Arc<GlyphTable>,
    initial_frame: Arc<AsciiFrame>,
    budget: Budget,
    cancellation: Arc<CancellationToken>,
    _not_send_or_sync: PhantomData<Rc<()>>,
}

impl WindowsHostConfig {
    /// Creates a host configuration. `initial_frame` is rendered while every wallpaper HWND is
    /// hidden, before any Explorer attachment can be revealed.
    #[must_use]
    pub fn new(
        glyph_table: Arc<GlyphTable>,
        initial_frame: Arc<AsciiFrame>,
        budget: Budget,
        cancellation: Arc<CancellationToken>,
    ) -> Self {
        Self {
            glyph_table,
            initial_frame,
            budget,
            cancellation,
            _not_send_or_sync: PhantomData,
        }
    }
}

/// Explicit Windows-host failure; placement uncertainty fails closed before lease issuance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowsHostError {
    WrongThread,
    InvalidFrame(ascii_core::Error),
    AdapterUnavailable,
    DeviceRequestFailed,
    DeviceLost,
    SessionUnavailable,
    SecureDesktop,
    ShellUnavailable,
    UnsupportedHierarchy,
    AmbiguousHierarchy,
    HierarchyBoundExceeded,
    DisplayEnumerationFailed,
    DisplayCapacityExceeded,
    InvalidDisplayBounds,
    DisplayNotFound,
    StaleDisplay,
    AlreadyAttached,
    CapacityExhausted,
    DpiContextUnavailable,
    DpiContextMismatch,
    OwnedWindowInvalid,
    PlacementVerificationFailed,
    SurfaceCreationFailed,
    SurfaceUnsupported,
    FirstFrameNotPresented,
    RecoveryCircuitOpen,
    RevisionExhausted,
    Win32 {
        operation: &'static str,
        code: u32,
    },
    StyleMutationFailed {
        operation: &'static str,
        code: u32,
        hwnd: usize,
        index: i32,
        is_window_before: bool,
        is_window_after: bool,
        current_pid: u32,
        current_tid: u32,
        owner_pid: u32,
        owner_tid: u32,
        observed_style: isize,
        observed_style_readable: bool,
        api_return: isize,
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
    Renderer(ascii_render::Error),
    Lease(SurfaceLeaseError),
}

impl std::fmt::Display for WindowsHostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongThread => {
                formatter.write_str("Windows host must run on its owning UI thread")
            }
            Self::InvalidFrame(error) => {
                write!(formatter, "invalid initial/presentation frame: {error}")
            }
            Self::AdapterUnavailable => formatter.write_str("no WGPU adapter is available"),
            Self::DeviceRequestFailed => formatter.write_str("WGPU device request failed"),
            Self::DeviceLost => formatter.write_str("WGPU device was lost; surfaces remain hidden"),
            Self::SessionUnavailable => {
                formatter.write_str("the interactive session is unavailable")
            }
            Self::SecureDesktop => formatter.write_str("the active input desktop is not Default"),
            Self::ShellUnavailable => formatter.write_str("Explorer shell is unavailable"),
            Self::UnsupportedHierarchy => {
                formatter.write_str("no supported shell desktop-view parent was found")
            }
            Self::AmbiguousHierarchy => {
                formatter.write_str("multiple shell desktop-view parents were found")
            }
            Self::HierarchyBoundExceeded => {
                formatter.write_str("bounded shell hierarchy discovery did not complete")
            }
            Self::DisplayEnumerationFailed => {
                formatter.write_str("full display topology enumeration failed")
            }
            Self::DisplayCapacityExceeded => {
                formatter.write_str("display count exceeds the Windows host capacity")
            }
            Self::InvalidDisplayBounds => {
                formatter.write_str("Windows returned invalid physical display bounds")
            }
            Self::DisplayNotFound => {
                formatter.write_str("display identity is not in the current topology")
            }
            Self::StaleDisplay => {
                formatter.write_str("display geometry changed; enumerate outputs again")
            }
            Self::AlreadyAttached => {
                formatter.write_str("display already has an active surface lease")
            }
            Self::CapacityExhausted => {
                formatter.write_str("Windows host surface capacity is exhausted")
            }
            Self::DpiContextUnavailable => {
                formatter.write_str("the shell DPI-awareness context could not be established")
            }
            Self::DpiContextMismatch => {
                formatter.write_str("the host and shell DPI-awareness contexts differ")
            }
            Self::OwnedWindowInvalid => {
                formatter.write_str("the owned HWND or shell parent became stale")
            }
            Self::PlacementVerificationFailed => formatter
                .write_str("hidden attachment verification failed; owned HWND was destroyed"),
            Self::SurfaceCreationFailed => {
                formatter.write_str("WGPU could not create a surface for the owned HWND")
            }
            Self::SurfaceUnsupported => formatter
                .write_str("the selected adapter has no supported sRGB/FIFO surface configuration"),
            Self::FirstFrameNotPresented => {
                formatter.write_str("the first valid frame was not submitted before attachment")
            }
            Self::RecoveryCircuitOpen => formatter.write_str(
                "bounded Explorer recovery is exhausted; call retry_recovery explicitly",
            ),
            Self::RevisionExhausted => {
                formatter.write_str("host revision space is exhausted; the adapter failed closed")
            }
            Self::Win32 { operation, code } => {
                write!(formatter, "{operation} failed with Win32 error {code}")
            }
            Self::StyleMutationFailed {
                operation,
                code,
                hwnd,
                index,
                is_window_before,
                is_window_after,
                current_pid,
                current_tid,
                owner_pid,
                owner_tid,
                observed_style,
                observed_style_readable,
                api_return,
            } => write!(
                formatter,
                "{operation} failed with Win32 error {code}; hwnd=0x{hwnd:x}, index={index}, IsWindow(before/after)={is_window_before}/{is_window_after}, current_pid/tid={current_pid}/{current_tid}, owner_pid/tid={owner_pid}/{owner_tid}, observed_style=0x{:x} (readable={observed_style_readable}), api_return=0x{:x}",
                *observed_style as usize, *api_return as usize
            ),
            Self::CreateWindowFailed {
                kind,
                code,
                parent,
                style,
                extended_style,
                left,
                top,
                width,
                height,
                stage,
            } => write!(
                formatter,
                "CreateWindowExW {kind} failed with Win32 error {code}; parent=0x{parent:x}, style=0x{style:08x}, ex_style=0x{extended_style:08x}, bounds=({left},{top},{width},{height}), WM_NCCREATE={}, WM_CREATE={}",
                stage & 1 != 0,
                stage & 2 != 0,
            ),
            Self::Renderer(error) => write!(formatter, "renderer failed: {error}"),
            Self::Lease(error) => write!(formatter, "surface lease failed: {error:?}"),
        }
    }
}

impl std::error::Error for WindowsHostError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DisplayRecord {
    native: native::NativeMonitor,
    info: DisplayInfo,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DesiredDisplay {
    device_name: [u16; 32],
    info: DisplayInfo,
}

struct HostedSurface {
    surface: Option<wgpu::Surface<'static>>,
    window: native::Window,
    configuration: wgpu::SurfaceConfiguration,
    identity: u64,
    revision: u64,
    display: DisplayInfo,
    device_name: [u16; 32],
    shell_hierarchy: native::ShellHierarchy,
}

impl Drop for HostedSurface {
    fn drop(&mut self) {
        // Raw WGPU surfaces must be released before their borrowed Win32 HWND is destroyed.
        self.surface.take();
    }
}

struct ActiveSurface {
    lease: SurfaceLease,
    hosted: HostedSurface,
}

/// Production Windows adapter for a bounded Explorer wallpaper-surface lifecycle.
///
/// Construction creates only a hidden controller window. Wallpaper HWNDs are created as hidden,
/// disabled, nonactivating layered children of Progman during `attach`, rendered once, then
/// positioned after DefView and before the unique WorkerW and structurally verified before reveal.
/// This structural check is not visual/manual acceptance evidence; the host remains experimental
/// until the documented Windows session gates have been exercised on a named machine.
pub struct WindowsHost {
    owner_thread: ThreadId,
    started_at: Instant,
    class: native::WindowClass,
    controller: Option<native::Window>,
    session_notifications_registered: bool,
    taskbar_created_message: u32,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    renderer: ascii_render::Renderer,
    device_status: ascii_render::DeviceStatus,
    current_frame: Arc<AsciiFrame>,
    cancellation: Arc<CancellationToken>,
    capabilities: CapabilitySnapshot,
    events: HostEventQueue,
    leases: SurfaceLeaseRegistry<MAX_WINDOWS_HOST_DISPLAYS>,
    surfaces: [Option<ActiveSurface>; MAX_WINDOWS_HOST_DISPLAYS],
    desired: [Option<DesiredDisplay>; MAX_WINDOWS_HOST_DISPLAYS],
    displays: [Option<DisplayRecord>; MAX_WINDOWS_HOST_DISPLAYS],
    display_count: usize,
    topology_available: bool,
    topology_revision: u64,
    next_surface_identity: u64,
    shell_process: Option<native::ShellProcess>,
    session_locked: bool,
    last_session_check: Instant,
    last_hierarchy_check: Instant,
    last_topology_check: Instant,
    recovery_pending: bool,
    recovery_attempts: u8,
    next_recovery_at: Instant,
    recovery_circuit_open: bool,
    _not_send_or_sync: PhantomData<Rc<()>>,
}

impl WindowsHost {
    /// Requests a WGPU device and creates the hidden session controller on the caller's UI thread.
    /// No wallpaper surface is created until `attach` is called.
    pub async fn new(config: WindowsHostConfig) -> Result<Self, WindowsHostError> {
        let owner_thread = std::thread::current().id();
        require_default_input_desktop(native::default_input_desktop().map_err(map_native)?)?;
        check_cancellation(&config.cancellation)?;
        let glyph_count = u32::try_from(config.glyph_table.glyphs().len())
            .map_err(|_| WindowsHostError::InvalidFrame(ascii_core::Error::SizeOverflow))?;
        validate_frame(
            &config.initial_frame,
            GlyphTableId::new(*config.glyph_table.fingerprint()),
            glyph_count,
            &config.budget,
            &config.cancellation,
        )
        .map_err(WindowsHostError::InvalidFrame)?;

        let instance_id =
            next_identifier(&NEXT_HOST_ID).ok_or(WindowsHostError::RevisionExhausted)?;
        let host_identity = HostIdentity::new(
            "ascii-platform.windows-explorer",
            AdapterVersion {
                major: 1,
                minor: 0,
                patch: 0,
            },
            instance_id,
        )
        .ok_or(WindowsHostError::RevisionExhausted)?;
        let leases = SurfaceLeaseRegistry::new(host_identity).map_err(WindowsHostError::Lease)?;
        let class =
            native::WindowClass::register(format!("AsciiWall.WindowsHost.{instance_id:016x}"))
                .map_err(map_native)?;
        let controller = class.create_controller().map_err(map_native)?;
        let instance = wgpu::Instance::default();
        let bootstrap_hwnd = NonZeroIsize::new(controller.raw_handle())
            .ok_or(WindowsHostError::SurfaceCreationFailed)?;
        let bootstrap_window_handle =
            RawWindowHandle::Win32(Win32WindowHandle::new(bootstrap_hwnd));
        let bootstrap_display_handle = RawDisplayHandle::Windows(WindowsDisplayHandle::new());
        // SAFETY: the hidden controller HWND remains owned by `controller` while the temporary
        // WGPU surface is alive. The surface is explicitly dropped before either handle owner.
        let bootstrap_surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: bootstrap_display_handle,
                raw_window_handle: bootstrap_window_handle,
            })
        }
        .map_err(|_| WindowsHostError::SurfaceCreationFailed)?;
        let adapter_result = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&bootstrap_surface),
            })
            .await;
        check_cancellation(&config.cancellation)?;
        let adapter = adapter_result.ok_or(WindowsHostError::AdapterUnavailable)?;
        if std::thread::current().id() != owner_thread {
            return Err(WindowsHostError::WrongThread);
        }
        let mut required_limits = wgpu::Limits::downlevel_defaults();
        required_limits.max_texture_dimension_2d = adapter.limits().max_texture_dimension_2d;
        let device_result = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("AsciiWall Windows wallpaper host"),
                    required_features: wgpu::Features::empty(),
                    required_limits,
                },
                None,
            )
            .await;
        check_cancellation(&config.cancellation)?;
        let (device, queue) = device_result.map_err(|_| WindowsHostError::DeviceRequestFailed)?;
        drop(bootstrap_surface);
        if std::thread::current().id() != owner_thread {
            return Err(WindowsHostError::WrongThread);
        }
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let device_status = ascii_render::DeviceStatus::new();
        let lost_status = device_status.clone();
        device.set_device_lost_callback(move |_reason, _message| lost_status.mark_lost());
        let renderer = ascii_render::Renderer::new(
            device.clone(),
            queue,
            config.glyph_table,
            config.budget,
            config.cancellation.clone(),
            device_status.clone(),
        );
        require_default_input_desktop(native::default_input_desktop().map_err(map_native)?)?;
        let taskbar_created_message = native::register_taskbar_created().map_err(map_native)?;
        native::register_session_notifications(controller.hwnd()).map_err(map_native)?;

        let now = Instant::now();
        let mut host = Self {
            owner_thread,
            started_at: now,
            class,
            controller: Some(controller),
            session_notifications_registered: true,
            taskbar_created_message,
            instance,
            adapter,
            device,
            renderer,
            device_status,
            current_frame: config.initial_frame,
            cancellation: config.cancellation,
            capabilities: CapabilitySnapshot::unknown(host_identity, HostRestrictions::empty()),
            events: HostEventQueue::new(),
            leases,
            surfaces: std::array::from_fn(|_| None),
            desired: [None; MAX_WINDOWS_HOST_DISPLAYS],
            displays: [None; MAX_WINDOWS_HOST_DISPLAYS],
            display_count: 0,
            topology_available: false,
            topology_revision: 0,
            next_surface_identity: 1,
            shell_process: None,
            session_locked: false,
            last_session_check: now,
            last_hierarchy_check: now,
            last_topology_check: now,
            recovery_pending: false,
            recovery_attempts: 0,
            next_recovery_at: now,
            recovery_circuit_open: false,
            _not_send_or_sync: PhantomData,
        };
        host.set_health(HostHealth::Starting);
        host.set_lock_capability();
        host.refresh_topology(false)?;
        host.set_health(HostHealth::Degraded);
        host.last_session_check = Instant::now();
        require_default_input_desktop(native::default_input_desktop().map_err(map_native)?)?;
        check_cancellation(&host.cancellation)?;
        Ok(host)
    }

    /// Re-enumerates the complete physical topology; message dimensions are never treated as
    /// authoritative. A changed topology retires/rebuilds affected owned surfaces.
    pub fn displays(&mut self) -> Result<DisplaySnapshot, WindowsHostError> {
        self.ensure_owner_thread()?;
        self.refresh_topology(true)?;
        Ok(self.display_snapshot())
    }

    /// Creates and leases a rendered surface only after hidden-frame, DPI, parent, Z-order, and
    /// nonactivation checks succeed. A failure destroys only the HWND created for this attempt.
    pub fn attach(&mut self, display: &DisplayInfo) -> Result<SurfaceLease, WindowsHostError> {
        self.ensure_owner_thread()?;
        self.ensure_session_available()?;
        self.attach_internal(display, true)
    }

    /// Presents a current immutable frame against the exact active lease. The `Arc` is retained as
    /// the recovery frame; callers cannot leave Explorer recovery rendering an older scene.
    pub fn present(
        &mut self,
        lease: SurfaceLease,
        frame: Arc<AsciiFrame>,
    ) -> Result<ascii_render::PresentOutcome, WindowsHostError> {
        self.ensure_owner_thread()?;
        self.ensure_session_available()?;
        if self.cancellation.is_cancelled() {
            return Err(WindowsHostError::InvalidFrame(ascii_core::Error::Cancelled));
        }
        if self.device_status.is_lost() {
            self.fail_device();
            return Err(WindowsHostError::DeviceLost);
        }
        let Some(index) = self
            .surfaces
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|active| active.lease == lease))
        else {
            return Err(WindowsHostError::Lease(SurfaceLeaseError::RetiredLease));
        };
        let display_identity = self.surfaces[index]
            .as_ref()
            .ok_or(WindowsHostError::Lease(SurfaceLeaseError::RetiredLease))?
            .hosted
            .display
            .identity;
        self.leases
            .validate_present(lease, display_identity)
            .map_err(WindowsHostError::Lease)?;
        let placement_matches = self.surfaces[index].as_ref().is_some_and(|active| {
            active
                .hosted
                .window
                .verify_present_placement(&active.hosted.shell_hierarchy)
                .is_ok()
        });
        if !placement_matches {
            self.invalidate_shell();
            return Err(WindowsHostError::PlacementVerificationFailed);
        }
        let result = {
            let active = self.surfaces[index]
                .as_ref()
                .expect("active surface was verified above");
            let target = render_target(&active.hosted, None);
            self.renderer.present(&frame, &target)
        };
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                if error == ascii_render::Error::OutOfMemory {
                    self.fail_device();
                }
                return Err(WindowsHostError::Renderer(error));
            }
        };
        self.current_frame = frame;
        match outcome {
            ascii_render::PresentOutcome::Recoverable(
                ascii_render::RecoverableReason::SurfaceLost
                | ascii_render::RecoverableReason::SurfaceOutdated,
            )
            | ascii_render::PresentOutcome::Presented { suboptimal: true } => {
                self.retire_for_recovery(index, SurfaceRetirementReason::SurfaceRecreated);
                self.renderer.invalidate();
                self.schedule_recovery(true);
            }
            ascii_render::PresentOutcome::Recoverable(
                ascii_render::RecoverableReason::DeviceLost,
            ) => self.fail_device(),
            _ => {}
        }
        Ok(outcome)
    }

    /// Retires a lease before destroying its owned WGPU surface and HWND.
    pub fn detach(&mut self, lease: SurfaceLease) -> Result<(), WindowsHostError> {
        self.ensure_owner_thread()?;
        let index = self
            .surfaces
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|active| active.lease == lease))
            .ok_or(WindowsHostError::Lease(SurfaceLeaseError::RetiredLease))?;
        let active = self.surfaces[index].take().expect("located active surface");
        let retired = self.leases.retire(lease).map_err(WindowsHostError::Lease)?;
        self.remove_desired(active.hosted.device_name);
        active.hosted.window.hide();
        self.queue_event(HostEvent::SurfaceRetired {
            lease: retired,
            reason: SurfaceRetirementReason::DisplayDetached,
        })?;
        drop(active);
        if self.surfaces.iter().all(Option::is_none) {
            self.shell_process = None;
        }
        Ok(())
    }

    /// Resets the bounded Explorer recovery circuit breaker after an explicit controller action.
    pub fn retry_recovery(&mut self) -> Result<(), WindowsHostError> {
        self.ensure_owner_thread()?;
        self.ensure_session_available()?;
        self.recovery_attempts = 0;
        self.recovery_circuit_open = false;
        self.schedule_recovery(true);
        Ok(())
    }

    fn attach_internal(
        &mut self,
        display: &DisplayInfo,
        remember: bool,
    ) -> Result<SurfaceLease, WindowsHostError> {
        self.ensure_session_available()?;
        if !self.topology_available {
            return Err(WindowsHostError::DisplayEnumerationFailed);
        }
        if self.recovery_circuit_open {
            return Err(WindowsHostError::RecoveryCircuitOpen);
        }
        if self.device_status.is_lost() {
            self.fail_device();
            return Err(WindowsHostError::DeviceLost);
        }
        if self.cancellation.is_cancelled() {
            return Err(WindowsHostError::InvalidFrame(ascii_core::Error::Cancelled));
        }
        let display_record = self
            .display_records()
            .find(|record| record.info.identity == display.identity)
            .copied()
            .ok_or(WindowsHostError::DisplayNotFound)?;
        if display_record.info != *display {
            return Err(WindowsHostError::StaleDisplay);
        }
        if self
            .surfaces
            .iter()
            .flatten()
            .any(|active| active.hosted.display.identity == display.identity)
        {
            return Err(WindowsHostError::AlreadyAttached);
        }
        let Some(slot) = self.surfaces.iter().position(Option::is_none) else {
            return Err(WindowsHostError::CapacityExhausted);
        };
        if remember
            && self.desired.iter().all(Option::is_some)
            && self
                .desired
                .iter()
                .flatten()
                .all(|desired| desired.device_name != display_record.native.device_name)
        {
            return Err(WindowsHostError::CapacityExhausted);
        }
        let report = crate::probe_host();
        if report.hierarchy.observation.status != crate::ProbeStatus::Experimental
            || report.hierarchy.observation.reason
                != crate::ObservationReason::ShellHierarchyObserved
        {
            return Err(WindowsHostError::UnsupportedHierarchy);
        }

        check_cancellation(&self.cancellation)?;
        require_default_input_desktop(native::default_input_desktop().map_err(map_native)?)?;
        let existing_owned = self.owned_windows(None);
        let target = native::discover_shell_target_for_attach(&existing_owned, &self.cancellation)
            .map_err(map_native)?;
        if !target.process.is_alive().map_err(map_native)? {
            return Err(WindowsHostError::ShellUnavailable);
        }
        if self.shell_process.as_ref().is_some_and(|current| {
            current.process_id() != target.hierarchy.process_id
                || self
                    .surfaces
                    .iter()
                    .flatten()
                    .any(|active| active.hosted.shell_hierarchy != target.hierarchy)
        }) {
            self.invalidate_shell();
            return Err(WindowsHostError::ShellUnavailable);
        }
        let window = self
            .class
            .create_surface_window(
                &target.hierarchy,
                display_record.info.left_px,
                display_record.info.top_px,
                display_record.info.width_px,
                display_record.info.height_px,
            )
            .map_err(map_native)?;
        let nonzero_hwnd = NonZeroIsize::new(window.raw_handle())
            .ok_or(WindowsHostError::SurfaceCreationFailed)?;
        let raw_window_handle = RawWindowHandle::Win32(Win32WindowHandle::new(nonzero_hwnd));
        let raw_display_handle = RawDisplayHandle::Windows(WindowsDisplayHandle::new());
        // SAFETY: `HostedSurface` owns this HWND and explicitly drops the WGPU surface before the
        // HWND. The raw handles remain valid until after surface destruction; no handle escapes.
        let surface: wgpu::Surface<'static> = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle,
                    raw_window_handle,
                })
        }
        .map_err(|_| WindowsHostError::SurfaceCreationFailed)?;
        let configuration = surface_configuration(&surface, &self.adapter, &display_record.info)?;
        surface.configure(&self.device, &configuration);
        let surface_identity = self.allocate_surface_identity()?;
        let mut hosted = HostedSurface {
            surface: Some(surface),
            window,
            configuration,
            identity: surface_identity,
            revision: 1,
            display: display_record.info,
            device_name: display_record.native.device_name,
            shell_hierarchy: target.hierarchy,
        };
        let first_target = render_target(&hosted, None);
        let first_outcome = self.renderer.present(&self.current_frame, &first_target);
        if let Err(error) = check_cancellation(&self.cancellation) {
            hosted.window.hide();
            return Err(error);
        }
        let first_outcome = first_outcome.map_err(WindowsHostError::Renderer)?;
        if !matches!(
            first_outcome,
            ascii_render::PresentOutcome::Presented { .. }
        ) {
            return Err(WindowsHostError::FirstFrameNotPresented);
        }

        if !target.process.is_alive().map_err(map_native)? {
            return Err(WindowsHostError::ShellUnavailable);
        }
        hosted
            .window
            .attach_to(
                &target.hierarchy,
                display_record.info.left_px,
                display_record.info.top_px,
                display_record.info.width_px,
                display_record.info.height_px,
            )
            .map_err(map_native)?;
        let owned_windows = self.owned_windows(Some(&hosted.window));
        hosted
            .window
            .verify_hidden_placement(&target.hierarchy, &owned_windows)
            .map_err(map_native)?;
        let revalidated =
            native::discover_shell_target_read_only(&owned_windows).map_err(map_native)?;
        if revalidated.hierarchy != target.hierarchy
            || !target.process.is_alive().map_err(map_native)?
        {
            return Err(WindowsHostError::PlacementVerificationFailed);
        }
        drop(revalidated);
        if let Err(error) = check_cancellation(&self.cancellation) {
            hosted.window.hide();
            return Err(error);
        }
        self.ensure_session_available()?;
        let foreground_before = native::foreground_window();

        let lease = self
            .leases
            .attach(display_record.info.identity)
            .map_err(WindowsHostError::Lease)?;
        if let Err(error) = check_cancellation(&self.cancellation) {
            hosted.window.hide();
            let _ = self.leases.retire(lease);
            return Err(error);
        }
        let owned_windows = self.owned_windows(Some(&hosted.window));
        let reveal_result =
            hosted
                .window
                .show_no_activate(&target.hierarchy, &owned_windows, foreground_before);
        if let Err(error) = check_cancellation(&self.cancellation) {
            hosted.window.hide();
            let _ = self.leases.retire(lease);
            return Err(error);
        }
        if let Err(error) = reveal_result {
            hosted.window.hide();
            let _ = self.leases.retire(lease);
            return Err(map_native(error));
        }
        // The initial frame was presented while this HWND was hidden. Force a redraw now
        // that it is visible; otherwise the renderer's static-frame cache can skip it.
        self.renderer.invalidate();
        let visible_outcome = self
            .renderer
            .present(&self.current_frame, &render_target(&hosted, None));
        if let Err(error) = check_cancellation(&self.cancellation) {
            hosted.window.hide();
            let _ = self.leases.retire(lease);
            return Err(error);
        }
        match visible_outcome {
            Ok(ascii_render::PresentOutcome::Presented { .. }) => {}
            Ok(_) => {
                hosted.window.hide();
                let _ = self.leases.retire(lease);
                return Err(WindowsHostError::FirstFrameNotPresented);
            }
            Err(error) => {
                hosted.window.hide();
                let _ = self.leases.retire(lease);
                return Err(WindowsHostError::Renderer(error));
            }
        }
        if let Err(error) = self.set_wallpaper_capability(CapabilityAvailability::Available) {
            hosted.window.hide();
            let _ = self.leases.retire(lease);
            return Err(error);
        }
        if self.shell_process.is_none() {
            self.shell_process = Some(target.process);
        }
        self.surfaces[slot] = Some(ActiveSurface { lease, hosted });
        if remember
            && let Err(error) = self.remember_desired(DesiredDisplay {
                device_name: display_record.native.device_name,
                info: display_record.info,
            })
        {
            self.retire_for_recovery(slot, SurfaceRetirementReason::AdapterShutdown);
            return Err(error);
        }
        self.recovery_pending = false;
        self.recovery_attempts = 0;
        self.set_health(HostHealth::Healthy);
        if let Err(error) = self.queue_event(HostEvent::SurfaceAttached(lease)) {
            self.fail_device();
            return Err(error);
        }
        Ok(lease)
    }

    fn allocate_surface_identity(&mut self) -> Result<u64, WindowsHostError> {
        let identity = self.next_surface_identity;
        self.next_surface_identity = identity
            .checked_add(1)
            .ok_or(WindowsHostError::RevisionExhausted)?;
        Ok(identity)
    }

    fn display_records(&self) -> impl Iterator<Item = &DisplayRecord> {
        self.displays[..self.display_count].iter().flatten()
    }
    fn owned_windows<'a>(
        &'a self,
        additional: Option<&'a native::Window>,
    ) -> [Option<&'a native::Window>; MAX_WINDOWS_HOST_DISPLAYS] {
        let mut windows = [None; MAX_WINDOWS_HOST_DISPLAYS];
        let mut count = 0;
        for active in self.surfaces.iter().flatten() {
            windows[count] = Some(&active.hosted.window);
            count += 1;
        }
        if let Some(window) = additional
            && count < MAX_WINDOWS_HOST_DISPLAYS
        {
            windows[count] = Some(window);
        }
        windows
    }

    fn display_snapshot(&self) -> DisplaySnapshot {
        let mut snapshot = DisplaySnapshot::empty(self.topology_revision);
        for record in self.display_records() {
            snapshot.records[usize::from(snapshot.len)] = Some(record.info);
            snapshot.len += 1;
        }
        snapshot
    }

    fn refresh_topology(&mut self, recover: bool) -> Result<bool, WindowsHostError> {
        self.ensure_owner_thread()?;
        self.last_topology_check = Instant::now();
        let native_snapshot = match native::enumerate_monitors() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.topology_available = false;
                return Err(map_native(error));
            }
        };
        let mut next = [None; MAX_WINDOWS_HOST_DISPLAYS];
        for (index, native_monitor) in native_snapshot.iter().enumerate() {
            let old = self
                .display_records()
                .find(|record| record.native.device_name == native_monitor.device_name);
            let monitor_id = if let Some(old) = old {
                old.info.identity.id()
            } else {
                next_monitor_id().ok_or(WindowsHostError::RevisionExhausted)?
            };
            let fallback = MonitorFallbackMatch::new(
                native_monitor.left,
                native_monitor.top,
                native_monitor.width,
                native_monitor.height,
            )
            .map_err(|_| WindowsHostError::InvalidDisplayBounds)?;
            let info = DisplayInfo {
                identity: MonitorIdentity::new(monitor_id, fallback),
                left_px: native_monitor.left,
                top_px: native_monitor.top,
                width_px: native_monitor.width,
                height_px: native_monitor.height,
            };
            next[index] = Some(DisplayRecord {
                native: *native_monitor,
                info,
            });
        }
        self.topology_available = true;
        let changed = !same_topology(
            &self.displays[..self.display_count],
            &next[..native_snapshot.len],
        );
        if !changed && self.display_count == native_snapshot.len {
            return Ok(false);
        }
        self.topology_revision = self
            .topology_revision
            .checked_add(1)
            .ok_or(WindowsHostError::RevisionExhausted)?;
        let old_desired = self.desired;
        self.desired = [None; MAX_WINDOWS_HOST_DISPLAYS];
        for desired in old_desired.into_iter().flatten() {
            if let Some(current) = next[..native_snapshot.len]
                .iter()
                .flatten()
                .find(|record| record.native.device_name == desired.device_name)
            {
                let _ = self.remember_desired(DesiredDisplay {
                    device_name: desired.device_name,
                    info: current.info,
                });
            }
        }
        for index in 0..MAX_WINDOWS_HOST_DISPLAYS {
            let Some(active) = self.surfaces[index].as_ref() else {
                continue;
            };
            let reason = if next[..native_snapshot.len]
                .iter()
                .flatten()
                .any(|record| record.native.device_name == active.hosted.device_name)
            {
                SurfaceRetirementReason::SurfaceRecreated
            } else {
                SurfaceRetirementReason::DisplayDetached
            };
            self.retire_for_recovery(index, reason);
        }
        self.displays = next;
        self.display_count = native_snapshot.len;
        self.renderer.invalidate();
        if recover && !self.session_locked && self.desired.iter().any(Option::is_some) {
            self.schedule_recovery(true);
        }
        Ok(true)
    }

    fn remember_desired(&mut self, desired: DesiredDisplay) -> Result<(), WindowsHostError> {
        if let Some(existing) = self.desired.iter_mut().find(|entry| {
            entry
                .as_ref()
                .is_some_and(|value| value.device_name == desired.device_name)
        }) {
            *existing = Some(desired);
            return Ok(());
        }
        let slot = self
            .desired
            .iter_mut()
            .find(|entry| entry.is_none())
            .ok_or(WindowsHostError::CapacityExhausted)?;
        *slot = Some(desired);
        Ok(())
    }

    fn remove_desired(&mut self, device_name: [u16; 32]) {
        if let Some(slot) = self.desired.iter_mut().find(|entry| {
            entry
                .as_ref()
                .is_some_and(|desired| desired.device_name == device_name)
        }) {
            *slot = None;
        }
    }

    fn retire_for_recovery(&mut self, index: usize, reason: SurfaceRetirementReason) {
        let Some(active) = self.surfaces[index].take() else {
            return;
        };
        active.hosted.window.hide();
        if let Ok(retired) = self.leases.retire(active.lease) {
            let _ = self.queue_event(HostEvent::SurfaceRetired {
                lease: retired,
                reason,
            });
        }
        drop(active);
    }

    fn invalidate_shell(&mut self) {
        self.shell_process = None;
        for index in 0..MAX_WINDOWS_HOST_DISPLAYS {
            self.retire_for_recovery(index, SurfaceRetirementReason::HostLost);
        }
        self.renderer.invalidate();
        let _ = self.set_wallpaper_capability(CapabilityAvailability::TemporarilyUnavailable);
        self.set_health(HostHealth::Unavailable);
        if !self.session_locked && self.desired.iter().any(Option::is_some) {
            self.schedule_recovery(false);
        }
    }

    fn fail_device(&mut self) {
        self.recovery_pending = false;
        self.recovery_circuit_open = true;
        self.shell_process = None;
        for index in 0..MAX_WINDOWS_HOST_DISPLAYS {
            self.retire_for_recovery(index, SurfaceRetirementReason::AdapterShutdown);
        }
        let _ = self.set_wallpaper_capability(CapabilityAvailability::Faulted);
        self.set_health(HostHealth::Faulted);
    }
    fn open_recovery_circuit(&mut self) {
        self.recovery_pending = false;
        self.recovery_circuit_open = true;
        self.shell_process = None;
        for index in 0..MAX_WINDOWS_HOST_DISPLAYS {
            self.retire_for_recovery(index, SurfaceRetirementReason::HostLost);
        }
        let _ = self.set_wallpaper_capability(CapabilityAvailability::Faulted);
        self.set_health(HostHealth::Faulted);
    }

    fn schedule_recovery(&mut self, immediate: bool) {
        if self.session_locked
            || self.recovery_circuit_open
            || !self.desired.iter().any(Option::is_some)
        {
            return;
        }
        self.recovery_pending = true;
        self.next_recovery_at = if immediate {
            Instant::now()
        } else {
            Instant::now() + recovery_delay(self.recovery_attempts)
        };
        self.set_health(HostHealth::Unavailable);
    }

    fn run_recovery_if_due(&mut self) {
        if !self.recovery_pending
            || self.session_locked
            || !self.topology_available
            || Instant::now() < self.next_recovery_at
        {
            return;
        }
        if self.device_status.is_lost() {
            self.fail_device();
            return;
        }
        self.recovery_pending = false;
        let desired = self.desired;
        let mut failed = false;
        for output in desired.into_iter().flatten() {
            if self
                .surfaces
                .iter()
                .flatten()
                .any(|active| active.hosted.device_name == output.device_name)
            {
                continue;
            }
            if self.attach_internal(&output.info, false).is_err() {
                failed = true;
                break;
            }
        }
        if failed {
            self.recovery_attempts = self.recovery_attempts.saturating_add(1);
            if self.recovery_attempts >= MAX_RECOVERY_ATTEMPTS {
                self.open_recovery_circuit();
            } else {
                self.schedule_recovery(false);
            }
        } else {
            self.recovery_attempts = 0;
            self.recovery_pending = false;
            self.recovery_circuit_open = false;
            self.set_health(HostHealth::Healthy);
        }
    }

    fn pump_messages(&mut self) {
        let mut remaining = MAX_MESSAGES_PER_POLL;
        let mut signals = native::NativeSignals::default();
        if let Some(controller) = &self.controller {
            controller.pump_messages(self.taskbar_created_message, &mut remaining, &mut signals);
        }
        for active in self.surfaces.iter().flatten() {
            if remaining == 0 {
                break;
            }
            active.hosted.window.pump_messages(
                self.taskbar_created_message,
                &mut remaining,
                &mut signals,
            );
        }
        if signals.locked == Some(true) {
            self.set_session_locked(true);
        } else if signals.locked == Some(false) {
            self.set_session_locked(false);
        }
        if signals.explorer_restarted || signals.owned_window_destroyed {
            self.invalidate_shell();
        }
        if signals.display_changed && self.refresh_topology(true).is_err() {
            self.invalidate_shell();
        }

        if self.last_topology_check.elapsed() >= TOPOLOGY_CHECK_INTERVAL
            && self.refresh_topology(true).is_err()
        {
            self.invalidate_shell();
        }
        let shell_process_dead = self
            .shell_process
            .as_ref()
            .is_some_and(|process| !matches!(process.is_alive(), Ok(true)));
        if shell_process_dead {
            self.invalidate_shell();
        }
        if self.last_hierarchy_check.elapsed() >= Duration::from_secs(1)
            && self.surfaces.iter().any(Option::is_some)
        {
            self.last_hierarchy_check = Instant::now();
            let hierarchy_matches = {
                let owned_windows = self.owned_windows(None);
                match native::discover_shell_target_read_only(&owned_windows) {
                    Ok(target) => {
                        let matches =
                            target.process.is_alive().unwrap_or(false)
                                && self.surfaces.iter().flatten().all(|active| {
                                    active.hosted.shell_hierarchy == target.hierarchy
                                })
                                && native::verify_owned_windows(
                                    &target.hierarchy,
                                    &owned_windows,
                                    true,
                                )
                                .is_ok();
                        drop(target);
                        matches
                    }
                    Err(_) => false,
                }
            };
            if !hierarchy_matches {
                self.invalidate_shell();
            }
        }
        if self.last_session_check.elapsed() >= SESSION_CHECK_INTERVAL {
            self.last_session_check = Instant::now();
            match native::default_input_desktop() {
                Ok(true) if self.session_locked => self.set_session_locked(false),
                Ok(false) if !self.session_locked => self.set_session_locked(true),
                Err(_) if !self.session_locked => self.set_session_locked(true),
                _ => {}
            }
        }
        self.run_recovery_if_due();
    }

    fn set_session_locked(&mut self, locked: bool) {
        if self.session_locked == locked {
            return;
        }
        if !locked && !matches!(native::default_input_desktop(), Ok(true)) {
            return;
        }
        self.session_locked = locked;
        if locked {
            for active in self.surfaces.iter().flatten() {
                active.hosted.window.hide();
            }
            self.renderer.invalidate();
        }
        let state = if locked {
            SessionLockState::Locked
        } else {
            SessionLockState::Unlocked
        };
        let feed = FeedEvent::SessionLock {
            state,
            observed_at_ms: self.elapsed_ms(),
        };
        if let Ok(revision) = self.capabilities.advance_revision() {
            let _ = self.events.push(HostEvent::Feed(feed), revision);
        } else {
            self.fail_device();
            return;
        }
        if locked {
            let _ = self.set_wallpaper_capability(CapabilityAvailability::TemporarilyUnavailable);
            self.set_health(HostHealth::Unavailable);
        } else {
            self.resume_after_session();
        }
    }

    fn resume_after_session(&mut self) {
        if self.device_status.is_lost() {
            self.fail_device();
            return;
        }
        if self.refresh_topology(true).is_err() {
            self.invalidate_shell();
            return;
        }
        if self.surfaces.iter().all(Option::is_none) {
            self.schedule_recovery(true);
            return;
        }
        if self.cancellation.is_cancelled() {
            return;
        }
        let target = {
            let owned_windows = self.owned_windows(None);
            match crate::probe_host().hierarchy.observation {
                observation
                    if observation.status == crate::ProbeStatus::Experimental
                        && observation.reason
                            == crate::ObservationReason::ShellHierarchyObserved =>
                {
                    native::discover_shell_target_for_attach(&owned_windows, &self.cancellation)
                }
                _ => Err(native::NativeError::UnsupportedHierarchy),
            }
        };
        let Ok(shell_target) = target else {
            self.invalidate_shell();
            return;
        };
        if !shell_target.process.is_alive().unwrap_or(false)
            || self
                .shell_process
                .as_ref()
                .is_some_and(|process| process.process_id() != shell_target.hierarchy.process_id)
            || self
                .surfaces
                .iter()
                .flatten()
                .any(|active| active.hosted.shell_hierarchy != shell_target.hierarchy)
        {
            drop(shell_target);
            self.invalidate_shell();
            return;
        }
        self.renderer.invalidate();
        let mut failed = false;
        for index in 0..MAX_WINDOWS_HOST_DISPLAYS {
            let Some(active) = self.surfaces[index].as_ref() else {
                continue;
            };
            let hidden_placement_valid = {
                let owned_windows = self.owned_windows(None);
                active
                    .hosted
                    .window
                    .verify_hidden_placement(&shell_target.hierarchy, &owned_windows)
                    .is_ok()
            };
            if !hidden_placement_valid {
                failed = true;
                break;
            }
            let presented = {
                let target = render_target(&active.hosted, None);
                self.renderer.present(&self.current_frame, &target)
            };
            if !matches!(
                presented,
                Ok(ascii_render::PresentOutcome::Presented { suboptimal: false })
            ) {
                failed = true;
                break;
            }
            if !matches!(native::default_input_desktop(), Ok(true)) {
                drop(shell_target);
                self.set_session_locked(true);
                return;
            }
            let foreground_before = native::foreground_window();
            let reveal_succeeded = {
                let owned_windows = self.owned_windows(None);
                active
                    .hosted
                    .window
                    .show_no_activate(&shell_target.hierarchy, &owned_windows, foreground_before)
                    .is_ok()
            };
            if !reveal_succeeded {
                failed = true;
                break;
            }
        }
        if failed {
            self.invalidate_shell();
            return;
        }
        if self
            .set_wallpaper_capability(CapabilityAvailability::Available)
            .is_err()
        {
            self.fail_device();
            return;
        }
        self.set_health(HostHealth::Healthy);
    }

    fn ensure_session_available(&mut self) -> Result<(), WindowsHostError> {
        match native::default_input_desktop() {
            Ok(true) if !self.session_locked => Ok(()),
            Ok(true) => Err(WindowsHostError::SessionUnavailable),
            Ok(false) => {
                self.set_session_locked(true);
                Err(WindowsHostError::SecureDesktop)
            }
            Err(error) => {
                self.set_session_locked(true);
                Err(map_native(error))
            }
        }
    }

    fn ensure_owner_thread(&self) -> Result<(), WindowsHostError> {
        if std::thread::current().id() == self.owner_thread {
            Ok(())
        } else {
            Err(WindowsHostError::WrongThread)
        }
    }

    fn set_lock_capability(&mut self) {
        let record = CapabilityRecord::new(
            CapabilityFeature::LockState,
            CapabilityScope::Session,
            CapabilityAvailability::Available,
            None,
            Freshness::observed_at(self.elapsed_ms(), u32::MAX),
            Confidence::High,
            CapabilitySet::empty(),
        );
        if let Ok(record) = record {
            self.replace_capability(record);
        }
    }

    fn set_wallpaper_capability(
        &mut self,
        availability: CapabilityAvailability,
    ) -> Result<(), WindowsHostError> {
        let reason = match availability {
            CapabilityAvailability::Unknown => Some(crate::CapabilityReason::NotObserved),
            CapabilityAvailability::Available => None,
            CapabilityAvailability::PermissionRequired => {
                Some(crate::CapabilityReason::PermissionRequired)
            }
            CapabilityAvailability::Denied => Some(crate::CapabilityReason::PermissionDenied),
            CapabilityAvailability::Unsupported => Some(crate::CapabilityReason::UnsupportedByHost),
            CapabilityAvailability::Unavailable => Some(crate::CapabilityReason::FeedUnavailable),
            CapabilityAvailability::TemporarilyUnavailable => {
                Some(crate::CapabilityReason::TemporaryFailure)
            }
            CapabilityAvailability::Faulted => Some(crate::CapabilityReason::AdapterFault),
        };
        let freshness = if availability == CapabilityAvailability::Available {
            Freshness::observed_at(self.elapsed_ms(), u32::MAX)
        } else {
            Freshness::UNKNOWN
        };
        let record = CapabilityRecord::new(
            CapabilityFeature::WallpaperSurface,
            CapabilityScope::Host,
            availability,
            reason,
            freshness,
            if availability == CapabilityAvailability::Available {
                Confidence::Low
            } else {
                Confidence::Unknown
            },
            CapabilitySet::empty(),
        )
        .map_err(|_| WindowsHostError::RevisionExhausted)?;
        self.replace_capability(record);
        Ok(())
    }

    fn replace_capability(&mut self, record: CapabilityRecord) {
        if let Ok(changes) = self.capabilities.replace(record) {
            let revision = changes.revision();
            for change in changes.iter() {
                let _ = self
                    .events
                    .push(HostEvent::CapabilityChanged(*change), revision);
            }
        }
    }

    fn set_health(&mut self, health: HostHealth) {
        let previous = self.capabilities.health();
        if previous == health {
            return;
        }
        if let Ok(changes) = self.capabilities.set_health(health) {
            let revision = changes.revision();
            for change in changes.iter() {
                let _ = self
                    .events
                    .push(HostEvent::CapabilityChanged(*change), revision);
            }
            let _ = self.events.push(
                HostEvent::HealthChanged {
                    previous,
                    current: health,
                    revision,
                },
                revision,
            );
        }
    }

    fn queue_event(&mut self, event: HostEvent) -> Result<(), WindowsHostError> {
        let revision = self
            .capabilities
            .advance_revision()
            .map_err(|_| WindowsHostError::RevisionExhausted)?;
        let _ = self.events.push(event, revision);
        Ok(())
    }

    fn elapsed_ms(&self) -> u64 {
        self.started_at
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }
}

impl HostAdapter for WindowsHost {
    fn capabilities(&self) -> CapabilitySnapshot {
        self.capabilities
    }

    fn reconciliation_snapshot(&self) -> HostStateSnapshot {
        let mut snapshot = HostStateSnapshot::new(self.capabilities);
        let _ = snapshot.set_feed(FeedEvent::SessionLock {
            state: if self.session_locked {
                SessionLockState::Locked
            } else {
                SessionLockState::Unlocked
            },
            observed_at_ms: self.elapsed_ms(),
        });
        for active in self.surfaces.iter().flatten() {
            let _ = snapshot.set_lease(active.lease);
        }
        snapshot
    }

    fn poll(&mut self) -> HostEventBatch {
        if self.ensure_owner_thread().is_ok() {
            self.pump_messages();
        }
        self.events.drain()
    }
}

impl Drop for WindowsHost {
    fn drop(&mut self) {
        if let Some(controller) = self.controller.as_ref()
            && self.session_notifications_registered
        {
            native::unregister_session_notifications(controller.hwnd());
            self.session_notifications_registered = false;
        }
        for index in 0..MAX_WINDOWS_HOST_DISPLAYS {
            self.retire_for_recovery(index, SurfaceRetirementReason::AdapterShutdown);
        }
        self.shell_process = None;
        self.controller.take();
        // `class` unregisters after surfaces/controller have been dropped. Each surface drops its
        // `wgpu::Surface` before its owned HWND, and no shell-owned HWND is ever destroyed.
    }
}

fn surface_configuration(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    display: &DisplayInfo,
) -> Result<wgpu::SurfaceConfiguration, WindowsHostError> {
    validate_surface_extent(
        display.width_px,
        display.height_px,
        adapter.limits().max_texture_dimension_2d,
    )?;
    let capabilities = surface.get_capabilities(adapter);
    let format = if capabilities
        .formats
        .contains(&wgpu::TextureFormat::Bgra8UnormSrgb)
    {
        wgpu::TextureFormat::Bgra8UnormSrgb
    } else if capabilities
        .formats
        .contains(&wgpu::TextureFormat::Rgba8UnormSrgb)
    {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        return Err(WindowsHostError::SurfaceUnsupported);
    };
    if !capabilities
        .present_modes
        .contains(&wgpu::PresentMode::Fifo)
        || !capabilities
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::Opaque)
    {
        return Err(WindowsHostError::SurfaceUnsupported);
    }
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: display.width_px,
        height: display.height_px,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
        view_formats: Vec::new(),
        desired_maximum_frame_latency: 2,
    })
}
fn validate_surface_extent(
    width: u32,
    height: u32,
    max_texture_dimension: u32,
) -> Result<(), WindowsHostError> {
    if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
        return Err(WindowsHostError::InvalidDisplayBounds);
    }
    if width > max_texture_dimension || height > max_texture_dimension {
        return Err(WindowsHostError::SurfaceUnsupported);
    }
    Ok(())
}

fn render_target(
    hosted: &HostedSurface,
    status: Option<ascii_render::RecoverableReason>,
) -> ascii_render::RenderTarget<'_> {
    ascii_render::RenderTarget::surface(
        hosted.surface.as_ref().expect("surface is alive"),
        hosted.identity,
        hosted.revision,
        hosted.configuration.width,
        hosted.configuration.height,
        hosted.configuration.format,
        status,
    )
}

fn same_topology(old: &[Option<DisplayRecord>], new: &[Option<DisplayRecord>]) -> bool {
    old.len() == new.len()
        && old.iter().flatten().all(|old_record| {
            new.iter().flatten().any(|new_record| {
                old_record.native.device_name == new_record.native.device_name
                    && old_record.info == new_record.info
            })
        })
}

fn next_identifier(counter: &AtomicU64) -> Option<u64> {
    counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            (current != 0).then(|| current.checked_add(1)).flatten()
        })
        .ok()
}

fn next_monitor_id() -> Option<MonitorId> {
    let value = next_identifier(&NEXT_MONITOR_ID)?;
    let mut bytes = [0_u8; ascii_core::ID_BYTES];
    bytes[..8].copy_from_slice(&value.to_le_bytes());
    bytes[8..].copy_from_slice(b"windows!");
    MonitorId::new(bytes).ok()
}

fn recovery_delay(attempt: u8) -> Duration {
    let shift = u32::from(attempt.min(4));
    RECOVERY_BASE_DELAY
        .checked_mul(1_u32 << shift)
        .unwrap_or(RECOVERY_MAX_DELAY)
        .min(RECOVERY_MAX_DELAY)
}

fn map_native(error: native::NativeError) -> WindowsHostError {
    match error {
        native::NativeError::Win32 { operation, code } => {
            WindowsHostError::Win32 { operation, code }
        }
        native::NativeError::StyleMutationFailed {
            operation,
            code,
            hwnd,
            index,
            is_window_before,
            is_window_after,
            current_pid,
            current_tid,
            owner_pid,
            owner_tid,
            observed_style,
            observed_style_readable,
            api_return,
        } => WindowsHostError::StyleMutationFailed {
            operation,
            code,
            hwnd,
            index,
            is_window_before,
            is_window_after,
            current_pid,
            current_tid,
            owner_pid,
            owner_tid,
            observed_style,
            observed_style_readable,
            api_return,
        },
        native::NativeError::CreateWindowFailed {
            kind,
            code,
            parent,
            style,
            extended_style,
            left,
            top,
            width,
            height,
            stage,
        } => WindowsHostError::CreateWindowFailed {
            kind,
            code,
            parent,
            style,
            extended_style,
            left,
            top,
            width,
            height,
            stage,
        },
        native::NativeError::DisplayEnumerationFailed => WindowsHostError::DisplayEnumerationFailed,
        native::NativeError::DisplayCapacityExceeded => WindowsHostError::DisplayCapacityExceeded,
        native::NativeError::InvalidDisplayBounds => WindowsHostError::InvalidDisplayBounds,
        native::NativeError::ShellUnavailable => WindowsHostError::ShellUnavailable,
        native::NativeError::UnsupportedHierarchy => WindowsHostError::UnsupportedHierarchy,
        native::NativeError::AmbiguousHierarchy => WindowsHostError::AmbiguousHierarchy,
        native::NativeError::HierarchyBoundExceeded => WindowsHostError::HierarchyBoundExceeded,
        native::NativeError::DpiContextUnavailable => WindowsHostError::DpiContextUnavailable,
        native::NativeError::DpiContextMismatch => WindowsHostError::DpiContextMismatch,
        native::NativeError::OwnedWindowInvalid => WindowsHostError::OwnedWindowInvalid,
        native::NativeError::PlacementVerificationFailed => {
            WindowsHostError::PlacementVerificationFailed
        }
        native::NativeError::SecureDesktop => WindowsHostError::SecureDesktop,
        native::NativeError::Cancelled => {
            WindowsHostError::InvalidFrame(ascii_core::Error::Cancelled)
        }
        native::NativeError::IdentityExhausted => WindowsHostError::RevisionExhausted,
    }
}
fn require_default_input_desktop(is_default: bool) -> Result<(), WindowsHostError> {
    if is_default {
        Ok(())
    } else {
        Err(WindowsHostError::SecureDesktop)
    }
}

fn check_cancellation(cancellation: &CancellationToken) -> Result<(), WindowsHostError> {
    if cancellation.is_cancelled() {
        Err(WindowsHostError::InvalidFrame(ascii_core::Error::Cancelled))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_backoff_is_finite_and_capped() {
        assert_eq!(recovery_delay(0), Duration::from_millis(100));
        assert_eq!(recovery_delay(1), Duration::from_millis(200));
        assert_eq!(recovery_delay(2), Duration::from_millis(400));
        assert_eq!(recovery_delay(3), Duration::from_millis(800));
        assert_eq!(recovery_delay(4), Duration::from_millis(1_600));
        assert_eq!(recovery_delay(u8::MAX), RECOVERY_MAX_DELAY);
        assert_eq!(MAX_RECOVERY_ATTEMPTS, 5);
    }

    #[test]
    fn monitor_ids_are_nonzero_and_change_without_wrap() {
        let first = next_monitor_id().expect("available process-local identity");
        let second = next_monitor_id().expect("available process-local identity");
        assert_ne!(first, second);
        assert_ne!(first.as_bytes(), &[0; ascii_core::ID_BYTES]);
    }
    #[test]
    fn surface_extent_preflight_rejects_zero_and_unrepresentable_sizes() {
        assert_eq!(
            validate_surface_extent(0, 1080, 8192),
            Err(WindowsHostError::InvalidDisplayBounds)
        );
        assert_eq!(
            validate_surface_extent(u32::MAX, 1080, u32::MAX),
            Err(WindowsHostError::InvalidDisplayBounds)
        );
    }

    #[test]
    fn surface_extent_preflight_respects_device_texture_limits() {
        assert_eq!(validate_surface_extent(3840, 2160, 8192), Ok(()));
        assert_eq!(
            validate_surface_extent(8193, 1080, 8192),
            Err(WindowsHostError::SurfaceUnsupported)
        );
    }

    #[test]
    fn startup_session_gate_fails_closed_without_default_input_desktop() {
        assert_eq!(
            require_default_input_desktop(false),
            Err(WindowsHostError::SecureDesktop)
        );
        assert_eq!(require_default_input_desktop(true), Ok(()));
    }

    #[test]
    fn raised_slot_session_and_cancellation_gates_map_without_native_actions() {
        assert_eq!(
            map_native(native::NativeError::SecureDesktop),
            WindowsHostError::SecureDesktop
        );
        assert_eq!(
            map_native(native::NativeError::Cancelled),
            WindowsHostError::InvalidFrame(ascii_core::Error::Cancelled)
        );
    }

    #[test]
    fn create_window_failure_preserves_diagnostics_and_reports_creation_stages() {
        for (kind, stage, nccreate, create) in
            [("controller", 1, true, false), ("surface", 2, false, true)]
        {
            let error = map_native(native::NativeError::CreateWindowFailed {
                kind,
                code: 0,
                parent: 0x1234,
                style: 0x5678,
                extended_style: 0x9abc,
                left: -12,
                top: 34,
                width: 1920,
                height: 1080,
                stage,
            });
            assert_eq!(
                error,
                WindowsHostError::CreateWindowFailed {
                    kind,
                    code: 0,
                    parent: 0x1234,
                    style: 0x5678,
                    extended_style: 0x9abc,
                    left: -12,
                    top: 34,
                    width: 1920,
                    height: 1080,
                    stage,
                }
            );
            let message = error.to_string();
            assert!(message.contains(&format!("WM_NCCREATE={nccreate}")));
            assert!(message.contains(&format!("WM_CREATE={create}")));
        }
    }

    #[test]
    fn cancellation_check_returns_the_typed_frame_error() {
        let cancellation = CancellationToken::new();
        assert_eq!(check_cancellation(&cancellation), Ok(()));
        cancellation.cancel();
        assert_eq!(
            check_cancellation(&cancellation),
            Err(WindowsHostError::InvalidFrame(ascii_core::Error::Cancelled))
        );
    }
}
