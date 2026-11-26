use super::{
    DesktopClickBatch, DesktopClickObservation, DesktopHitClassification, FullscreenCandidate,
    MAX_DESKTOP_CLICK_OBSERVATIONS, MAX_FULLSCREEN_OBSERVATIONS, MAX_WINDOWS_OBSERVED,
    ObservedWindowBounds, WindowBoundsSource, WindowsObserverConsent, WindowsObserverSnapshot,
};
use crate::{
    AdapterVersion, CapabilityAvailability, CapabilityFeature, CapabilityReason, CapabilityRecord,
    CapabilityScope, CapabilitySet, CapabilitySnapshot, Confidence, FeedEvent, Freshness,
    HostAdapter, HostEvent, HostEventQueue, HostHealth, HostIdentity, HostRestrictions,
    HostStateSnapshot, PointerButtons, PointerButtonsUpdate, PointerPosition, WindowIdentity,
    WindowRectangle, WindowState,
};
use ascii_core::{Generation, MonitorFallbackMatch, MonitorId, MonitorIdentity};
use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::{MaybeUninit, size_of};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Instant;

const NATIVE_QUEUE_CAPACITY: usize = 128;
const MAX_ENUMERATED_WINDOWS: usize = 128;
const MAX_NATIVE_OUTPUTS: usize = 8;
const FEATURE_MAX_AGE_MS: u32 = 2_000;
const INPUT_DESKTOP_POLL_MS: u32 = 250;
const OUTPUT_POLL_MS: u64 = 1_000;
const CLICK_MAX_AGE_MS: u32 = 50;
const CLICK_HIT_TEST_TIMEOUT_MS: u32 = 25;
const GA_ROOT: u32 = 2;
const LVM_HITTEST: u32 = 0x1000 + 18;
const SMTO_BLOCK: u32 = 0x0001;
const SMTO_ABORTIFHUNG: u32 = 0x0002;
const WM_NCCREATE: u32 = 0x0081;
const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;
const WM_INPUT: u32 = 0x00FF;
const WM_DISPLAYCHANGE: u32 = 0x007E;
const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
const WM_QUIT: u32 = 0x0012;
const GWLP_USERDATA: i32 = -21;
const RIM_TYPEMOUSE: u32 = 0;
const RID_INPUT: u32 = 0x10000003;
const RIDEV_REMOVE: u32 = 0x00000001;
const RIDEV_INPUTSINK: u32 = 0x00000100;
const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const EVENT_SYSTEM_MINIMIZESTART: u32 = 0x0016;
const EVENT_SYSTEM_MINIMIZEEND: u32 = 0x0017;
const EVENT_OBJECT_SHOW: u32 = 0x8002;
const EVENT_OBJECT_HIDE: u32 = 0x8003;
const EVENT_OBJECT_DESTROY: u32 = 0x8001;
const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;
const EVENT_OBJECT_STATECHANGE: u32 = 0x800A;
const OBJID_WINDOW: i32 = 0;
const CHILDID_SELF: i32 = 0;
const DWM_EXTENDED_FRAME_BOUNDS: u32 = 9;
const DWM_CLOAKED: u32 = 14;
const MONITOR_DEFAULTTONULL: u32 = 0;
const DESKTOP_READOBJECTS: u32 = 0x0001;
const UOI_NAME: i32 = 2;
const WTS_SESSION_LOCK: u32 = 0x7;
const WTS_SESSION_LOGOFF: u32 = 0x6;
const WTS_CONSOLE_DISCONNECT: u32 = 0x2;
const WTS_REMOTE_DISCONNECT: u32 = 0x4;
const NOTIFY_FOR_THIS_SESSION: u32 = 0;
const WAIT_FAILED: u32 = u32::MAX;
const QS_ALLINPUT: u32 = 0x04FF;
const MWMO_INPUTAVAILABLE: u32 = 0x0004;
const PM_REMOVE: u32 = 0x0001;
const RI_MOUSE_LEFT_BUTTON_DOWN: u16 = 0x0001;
const RI_MOUSE_LEFT_BUTTON_UP: u16 = 0x0002;
const RI_MOUSE_RIGHT_BUTTON_DOWN: u16 = 0x0004;
const RI_MOUSE_RIGHT_BUTTON_UP: u16 = 0x0008;
const RI_MOUSE_MIDDLE_BUTTON_DOWN: u16 = 0x0010;
const RI_MOUSE_MIDDLE_BUTTON_UP: u16 = 0x0020;
const RI_MOUSE_BUTTON_4_DOWN: u16 = 0x0040;
const RI_MOUSE_BUTTON_4_UP: u16 = 0x0080;
const RI_MOUSE_BUTTON_5_DOWN: u16 = 0x0100;
const RI_MOUSE_BUTTON_5_UP: u16 = 0x0200;
const CLASS_NAME: [u16; 25] = [
    65, 115, 99, 105, 105, 87, 97, 108, 108, 79, 98, 115, 101, 114, 118, 101, 114, 87, 110, 100,
    67, 108, 115, 0, 0,
];
const WINDOW_TITLE: [u16; 1] = [0];
static NEXT_HOST_INSTANCE: AtomicU64 = AtomicU64::new(1);
static RAW_INPUT_OWNER: AtomicBool = AtomicBool::new(false);

const SESSION_ACTIVE: u8 = 0;
const SESSION_REVOKED: u8 = 1;
const SESSION_SUSPENDED: u8 = 2;
const SESSION_LOCKED: u8 = 3;
const SESSION_NATIVE_FAILURE: u8 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsObserverError {
    /// Desktop-click classification requires both explicit pointer and window consent.
    InvalidConsentCombination,
    /// The observer worker could not be created.
    WorkerSpawnFailed,
    /// A message-only native observer window could not be initialized.
    WorkerInitializationFailed,
    /// The observer worker panicked while being joined during explicit revocation.
    WorkerThreadPanicked,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct NativeRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl NativeRect {
    const EMPTY: Self = Self {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    fn dimensions(self) -> Option<(u32, u32)> {
        let width = i64::from(self.right) - i64::from(self.left);
        let height = i64::from(self.bottom) - i64::from(self.top);
        if width <= 0 || height <= 0 {
            return None;
        }
        Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?))
    }

    fn area(self) -> u64 {
        self.dimensions()
            .map_or(0, |(width, height)| u64::from(width) * u64::from(height))
    }
}

#[derive(Clone, Copy)]
struct OutputRecord {
    handle: usize,
    rect: NativeRect,
    identity: MonitorIdentity,
}

#[derive(Clone, Copy)]
struct WindowRecord {
    handle: usize,
    identity: WindowIdentity,
    state: WindowState,
    bounds: Option<ObservedWindowBounds>,
    fullscreen: [Option<FullscreenCandidate>; MAX_FULLSCREEN_OBSERVATIONS],
}

struct ObserverData {
    capabilities: CapabilitySnapshot,
    events: HostEventQueue,
    position: Option<PointerPosition>,
    buttons: Option<PointerButtonsUpdate>,
    windows: [Option<WindowRecord>; MAX_WINDOWS_OBSERVED],
    outputs: [Option<OutputRecord>; MAX_NATIVE_OUTPUTS],
    click_events: ClickQueue,
    next_window_identity: u64,
    next_output_identity: u64,
    windows_truncated: bool,
    outputs_truncated: bool,
}

impl ObserverData {
    fn new(host: HostIdentity, consent: WindowsObserverConsent) -> Self {
        let mut restrictions = HostRestrictions::from_bits(
            HostRestrictions::NO_NATIVE_SURFACE | HostRestrictions::USER_CONSENT_REQUIRED,
        )
        .unwrap_or_else(HostRestrictions::empty);
        if !consent.pointer_buttons() {
            restrictions = restrictions
                .with(HostRestrictions::GLOBAL_POINTER_UNAVAILABLE)
                .unwrap_or(restrictions);
        }
        if !consent.window_metadata() {
            restrictions = restrictions
                .with(HostRestrictions::WINDOW_GEOMETRY_UNAVAILABLE)
                .unwrap_or(restrictions);
        }
        let mut capabilities = CapabilitySnapshot::unknown(host, restrictions);
        let now_ms = 0;
        if let Ok(changes) = capabilities.set_health(HostHealth::Healthy) {
            let _ = changes;
        }
        set_capability(
            &mut capabilities,
            CapabilityFeature::WallpaperSurface,
            CapabilityScope::Host,
            CapabilityAvailability::Unsupported,
            Some(CapabilityReason::UnsupportedByHost),
            Confidence::High,
            now_ms,
        );
        set_consent_capabilities(&mut capabilities, consent, now_ms);
        Self {
            capabilities,
            events: HostEventQueue::new(),
            position: None,
            buttons: None,
            windows: [None; MAX_WINDOWS_OBSERVED],
            outputs: [None; MAX_NATIVE_OUTPUTS],
            click_events: ClickQueue::new(),
            next_window_identity: 1,
            next_output_identity: 1,
            windows_truncated: false,
            outputs_truncated: false,
        }
    }
}

struct SharedInner {
    data: Mutex<ObserverData>,
    started: Instant,
    stop_requested: AtomicBool,
    session_state: AtomicU8,
    consent: WindowsObserverConsent,
    callback_overflow: AtomicBool,
    input_overflow: AtomicBool,
    window_hooks_available: AtomicBool,
    session_boundary_uncertain: AtomicBool,
}

/// Native observer owner. It has no wallpaper-surface control and cannot stop rendering.
pub struct WindowsObserver {
    shared: Arc<SharedInner>,
    worker: Option<JoinHandle<()>>,
    worker_thread_id: u32,
}

impl WindowsObserver {
    /// Starts only explicitly consented feeds; the default consent starts no worker or hooks.
    pub fn start(consent: WindowsObserverConsent) -> Result<Self, WindowsObserverError> {
        if consent.desktop_click_classification()
            && !(consent.pointer_buttons() && consent.window_metadata())
        {
            return Err(WindowsObserverError::InvalidConsentCombination);
        }
        let instance_id = NEXT_HOST_INSTANCE
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                (value != 0 && value != u64::MAX).then_some(value + 1)
            })
            .map_err(|_| WindowsObserverError::WorkerInitializationFailed)?;
        let host = HostIdentity::new(
            "asciiwall.windows.passive-observer",
            AdapterVersion {
                major: 1,
                minor: 0,
                patch: 0,
            },
            instance_id,
        )
        .ok_or(WindowsObserverError::WorkerInitializationFailed)?;
        let shared = Arc::new(SharedInner {
            data: Mutex::new(ObserverData::new(host, consent)),
            started: Instant::now(),
            stop_requested: AtomicBool::new(false),
            session_state: AtomicU8::new(SESSION_ACTIVE),
            consent,
            callback_overflow: AtomicBool::new(false),
            input_overflow: AtomicBool::new(false),
            window_hooks_available: AtomicBool::new(false),
            session_boundary_uncertain: AtomicBool::new(false),
        });
        let requested = consent.pointer_buttons()
            || consent.window_metadata()
            || consent.desktop_click_classification();
        if !requested {
            return Ok(Self {
                shared,
                worker: None,
                worker_thread_id: 0,
            });
        }
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("asciiwall-windows-observer".to_owned())
            .spawn(move || worker_main(worker_shared, ready_tx))
            .map_err(|_| WindowsObserverError::WorkerSpawnFailed)?;
        let worker_thread_id = match ready_rx.recv() {
            Ok(Ok(id)) => id,
            Ok(Err(error)) => {
                let _ = worker.join();
                return Err(error);
            }
            Err(_) => {
                let _ = worker.join();
                return Err(WindowsObserverError::WorkerInitializationFailed);
            }
        };
        Ok(Self {
            shared,
            worker: Some(worker),
            worker_thread_id,
        })
    }

    /// Permanently revokes this observer instance and synchronously clears retained observations.
    ///
    /// Wallpaper/presentation threads are not owned by this adapter and are not stopped here.
    pub fn revoke(&mut self) -> Result<(), WindowsObserverError> {
        self.shared.stop_requested.store(true, Ordering::Release);
        if self.worker_thread_id != 0 {
            // SAFETY: the worker thread ID came from GetCurrentThreadId after its message queue was
            // created, and WM_QUIT is the documented thread-message termination request.
            unsafe { PostThreadMessageW(self.worker_thread_id, WM_QUIT, 0, 0) };
        }
        let mut worker_panicked = false;
        if let Some(worker) = self.worker.take() {
            worker_panicked = worker.join().is_err();
        }
        self.worker_thread_id = 0;
        self.shared
            .session_state
            .store(SESSION_REVOKED, Ordering::Release);
        let mut data = lock_data(&self.shared);
        clear_observations(&mut data);
        set_consent_capability_status(
            &mut data,
            self.shared.consent,
            CapabilityAvailability::Denied,
            Some(CapabilityReason::PermissionRevoked),
            self.now_ms(),
        );
        if worker_panicked {
            Err(WindowsObserverError::WorkerThreadPanicked)
        } else {
            Ok(())
        }
    }

    /// Returns bounded Windows-specific provenance and confidence metadata.
    #[must_use]
    pub fn windows_snapshot(&self) -> WindowsObserverSnapshot {
        let data = lock_data(&self.shared);
        let mut snapshot = WindowsObserverSnapshot {
            windows: [None; MAX_WINDOWS_OBSERVED],
            states: [None; MAX_WINDOWS_OBSERVED],
            fullscreen: [None; MAX_FULLSCREEN_OBSERVATIONS],
            truncated: data.windows_truncated || data.outputs_truncated,
        };
        let mut fullscreen_len = 0;
        for (index, entry) in data.windows.iter().enumerate() {
            if let Some(entry) = entry {
                snapshot.states[index] = Some((entry.identity, entry.state));
                snapshot.windows[index] = entry.bounds;
                for candidate in entry.fullscreen.iter().flatten() {
                    if fullscreen_len == MAX_FULLSCREEN_OBSERVATIONS {
                        snapshot.truncated = true;
                        break;
                    }
                    snapshot.fullscreen[fullscreen_len] = Some(*candidate);
                    fullscreen_len += 1;
                }
            }
        }
        snapshot
    }

    /// Drains the Windows-local desktop-click classifications without allocating.
    #[must_use]
    pub fn drain_desktop_clicks(&self) -> DesktopClickBatch {
        lock_data(&self.shared).click_events.drain()
    }

    fn now_ms(&self) -> u64 {
        elapsed_ms(self.shared.started)
    }
}

impl Drop for WindowsObserver {
    fn drop(&mut self) {
        let _ = self.revoke();
    }
}

impl HostAdapter for WindowsObserver {
    fn capabilities(&self) -> CapabilitySnapshot {
        lock_data(&self.shared).capabilities
    }

    fn reconciliation_snapshot(&self) -> HostStateSnapshot {
        let data = lock_data(&self.shared);
        let mut snapshot = HostStateSnapshot::new(data.capabilities);
        if let Some(position) = data.position {
            let _ = snapshot.set_feed(FeedEvent::PointerPosition(position));
        }
        if let Some(buttons) = data.buttons {
            let _ = snapshot.set_feed(FeedEvent::PointerButtons(buttons));
        }
        for window in data.windows.iter().flatten() {
            if let Some(bounds) = window.bounds {
                let _ = snapshot.set_feed(FeedEvent::WindowRectangle(bounds.rectangle));
            }
            let _ = snapshot.set_feed(FeedEvent::WindowState {
                window: window.identity,
                state: window.state,
                observed_at_ms: self.now_ms(),
            });
        }
        snapshot
    }

    fn poll(&mut self) -> crate::HostEventBatch {
        lock_data(&self.shared).events.drain()
    }
}

struct ClickQueue {
    entries: [Option<DesktopClickObservation>; MAX_DESKTOP_CLICK_OBSERVATIONS],
    head: usize,
    len: usize,
    overflowed: bool,
}

impl ClickQueue {
    const fn new() -> Self {
        Self {
            entries: [None; MAX_DESKTOP_CLICK_OBSERVATIONS],
            head: 0,
            len: 0,
            overflowed: false,
        }
    }

    fn push(&mut self, click: DesktopClickObservation) {
        if self.len == MAX_DESKTOP_CLICK_OBSERVATIONS {
            self.overflowed = true;
            return;
        }
        let index = (self.head + self.len) % MAX_DESKTOP_CLICK_OBSERVATIONS;
        self.entries[index] = Some(click);
        self.len += 1;
    }

    fn drain(&mut self) -> DesktopClickBatch {
        let mut batch = DesktopClickBatch {
            clicks: [None; MAX_DESKTOP_CLICK_OBSERVATIONS],
            len: 0,
            overflowed: self.overflowed,
        };
        while self.len != 0 {
            batch.clicks[batch.len] = self.entries[self.head].take();
            batch.len += 1;
            self.head = (self.head + 1) % MAX_DESKTOP_CLICK_OBSERVATIONS;
            self.len -= 1;
        }
        self.head = 0;
        self.overflowed = false;
        batch
    }
}

#[derive(Clone, Copy)]
enum NativeEvent {
    Window {
        handle: usize,
        event: u32,
    },
    RawInput {
        button_flags: u16,
        message_time: u32,
        position: Option<Point>,
    },
    SessionChange {
        code: u32,
    },
    DisplayChange,
}

struct NativeQueue {
    entries: [Option<NativeEvent>; NATIVE_QUEUE_CAPACITY],
    head: usize,
    len: usize,
    window_overflow: bool,
    button_state_uncertain: bool,
    position_uncertain: bool,
    session_boundary_uncertain: bool,
}

impl NativeQueue {
    const fn new() -> Self {
        Self {
            entries: [None; NATIVE_QUEUE_CAPACITY],
            head: 0,
            len: 0,
            window_overflow: false,
            button_state_uncertain: false,
            position_uncertain: false,
            session_boundary_uncertain: false,
        }
    }

    fn push(&mut self, event: NativeEvent) {
        let last_index = (self.head + self.len + NATIVE_QUEUE_CAPACITY - 1) % NATIVE_QUEUE_CAPACITY;
        if self.len != 0 {
            match (self.entries[last_index], event) {
                (
                    Some(NativeEvent::Window {
                        handle: queued_handle,
                        event: queued_kind,
                    }),
                    NativeEvent::Window {
                        handle,
                        event: kind,
                    },
                ) if queued_handle == handle && queued_kind == kind => {
                    self.entries[last_index] = Some(event);
                    return;
                }
                (
                    Some(NativeEvent::RawInput {
                        button_flags: 0, ..
                    }),
                    NativeEvent::RawInput {
                        button_flags: 0, ..
                    },
                ) => {
                    self.entries[last_index] = Some(event);
                    return;
                }
                _ => {}
            }
        }
        if self.len == NATIVE_QUEUE_CAPACITY {
            match event {
                NativeEvent::Window { .. } => self.window_overflow = true,
                NativeEvent::RawInput { button_flags, .. } => {
                    self.position_uncertain = true;
                    if button_flags != 0 {
                        self.button_state_uncertain = true;
                    }
                }
                NativeEvent::SessionChange { .. } => {
                    self.session_boundary_uncertain = true;
                }
                NativeEvent::DisplayChange => self.window_overflow = true,
            }
            return;
        }
        let index = (self.head + self.len) % NATIVE_QUEUE_CAPACITY;
        self.entries[index] = Some(event);
        self.len += 1;
    }

    fn pop(&mut self) -> Option<NativeEvent> {
        if self.len == 0 {
            return None;
        }
        let event = self.entries[self.head].take();
        self.head = (self.head + 1) % NATIVE_QUEUE_CAPACITY;
        self.len -= 1;
        event
    }
}

struct CallbackContext {
    shared: Arc<SharedInner>,
    native_events: RefCell<NativeQueue>,
}

thread_local! {
    static CALLBACK_CONTEXT: std::cell::Cell<*const CallbackContext> = const {
        std::cell::Cell::new(null())
    };
}

fn lock_data(shared: &SharedInner) -> MutexGuard<'_, ObserverData> {
    shared
        .data
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn set_consent_capabilities(
    capabilities: &mut CapabilitySnapshot,
    consent: WindowsObserverConsent,
    now_ms: u64,
) {
    let (availability, reason) = if consent.pointer_buttons() {
        (
            CapabilityAvailability::Unknown,
            Some(CapabilityReason::NotObserved),
        )
    } else {
        (
            CapabilityAvailability::PermissionRequired,
            Some(CapabilityReason::PermissionRequired),
        )
    };
    set_capability(
        capabilities,
        CapabilityFeature::PointerPosition,
        CapabilityScope::Desktop,
        availability,
        reason,
        Confidence::Unknown,
        now_ms,
    );
    set_capability(
        capabilities,
        CapabilityFeature::PointerButtons,
        CapabilityScope::Desktop,
        availability,
        reason,
        Confidence::Unknown,
        now_ms,
    );
    let (availability, reason) = if consent.window_metadata() {
        (
            CapabilityAvailability::Unknown,
            Some(CapabilityReason::NotObserved),
        )
    } else {
        (
            CapabilityAvailability::PermissionRequired,
            Some(CapabilityReason::PermissionRequired),
        )
    };
    set_capability(
        capabilities,
        CapabilityFeature::WindowRectangles,
        CapabilityScope::Desktop,
        availability,
        reason,
        Confidence::Unknown,
        now_ms,
    );
    set_capability(
        capabilities,
        CapabilityFeature::WindowState,
        CapabilityScope::Desktop,
        availability,
        reason,
        Confidence::Unknown,
        now_ms,
    );
    set_capability(
        capabilities,
        CapabilityFeature::LockState,
        CapabilityScope::Session,
        CapabilityAvailability::Unknown,
        Some(CapabilityReason::NotObserved),
        Confidence::Unknown,
        now_ms,
    );
}

fn set_consent_capability_status(
    data: &mut ObserverData,
    consent: WindowsObserverConsent,
    availability: CapabilityAvailability,
    reason: Option<CapabilityReason>,
    now_ms: u64,
) {
    if consent.pointer_buttons() {
        set_capability_and_publish(
            data,
            CapabilityFeature::PointerPosition,
            CapabilityScope::Desktop,
            availability,
            reason,
            Confidence::High,
            now_ms,
        );
        set_capability_and_publish(
            data,
            CapabilityFeature::PointerButtons,
            CapabilityScope::Desktop,
            availability,
            reason,
            Confidence::High,
            now_ms,
        );
    }
    if consent.window_metadata() {
        set_capability_and_publish(
            data,
            CapabilityFeature::WindowRectangles,
            CapabilityScope::Desktop,
            availability,
            reason,
            Confidence::Medium,
            now_ms,
        );
        set_capability_and_publish(
            data,
            CapabilityFeature::WindowState,
            CapabilityScope::Desktop,
            availability,
            reason,
            Confidence::Medium,
            now_ms,
        );
    }
}

fn set_capability(
    capabilities: &mut CapabilitySnapshot,
    feature: CapabilityFeature,
    scope: CapabilityScope,
    availability: CapabilityAvailability,
    reason: Option<CapabilityReason>,
    confidence: Confidence,
    now_ms: u64,
) {
    if let Ok(record) = CapabilityRecord::new(
        feature,
        scope,
        availability,
        reason,
        Freshness::observed_at(now_ms, FEATURE_MAX_AGE_MS),
        confidence,
        CapabilitySet::empty(),
    ) {
        let _ = capabilities.replace(record);
    }
}

fn set_capability_and_publish(
    data: &mut ObserverData,
    feature: CapabilityFeature,
    scope: CapabilityScope,
    availability: CapabilityAvailability,
    reason: Option<CapabilityReason>,
    confidence: Confidence,
    now_ms: u64,
) {
    let Ok(record) = CapabilityRecord::new(
        feature,
        scope,
        availability,
        reason,
        Freshness::observed_at(now_ms, FEATURE_MAX_AGE_MS),
        confidence,
        CapabilitySet::empty(),
    ) else {
        return;
    };
    if let Ok(changes) = data.capabilities.replace(record) {
        for change in changes.iter() {
            let _ = data
                .events
                .push(HostEvent::CapabilityChanged(*change), changes.revision());
        }
    }
}

fn refresh_active_capabilities(shared: &SharedInner) {
    let mut data = lock_data(shared);
    let now_ms = elapsed_ms(shared.started);
    if shared.consent.pointer_buttons() {
        refresh_capability(
            &mut data,
            CapabilityFeature::PointerPosition,
            CapabilityScope::Desktop,
            now_ms,
        );
        refresh_capability(
            &mut data,
            CapabilityFeature::PointerButtons,
            CapabilityScope::Desktop,
            now_ms,
        );
    }
    if shared.consent.window_metadata() {
        refresh_capability(
            &mut data,
            CapabilityFeature::WindowRectangles,
            CapabilityScope::Desktop,
            now_ms,
        );
        refresh_capability(
            &mut data,
            CapabilityFeature::WindowState,
            CapabilityScope::Desktop,
            now_ms,
        );
    }
}

fn refresh_capability(
    data: &mut ObserverData,
    feature: CapabilityFeature,
    scope: CapabilityScope,
    now_ms: u64,
) {
    let Some(record) = data.capabilities.record(feature, scope).copied() else {
        return;
    };
    set_capability_and_publish(
        data,
        feature,
        scope,
        record.availability(),
        record.reason(),
        record.confidence(),
        now_ms,
    );
}

fn publish_feed(data: &mut ObserverData, feed: FeedEvent) {
    if let Ok(revision) = data.capabilities.advance_revision() {
        let _ = data.events.push(HostEvent::Feed(feed), revision);
    } else {
        data.events = HostEventQueue::new();
    }
}

fn clear_observations(data: &mut ObserverData) {
    data.events = HostEventQueue::new();
    data.position = None;
    data.buttons = None;
    data.windows = [None; MAX_WINDOWS_OBSERVED];
    data.outputs = [None; MAX_NATIVE_OUTPUTS];
    data.click_events = ClickQueue::new();
    data.windows_truncated = false;
    data.outputs_truncated = false;
}

fn worker_main(
    shared: Arc<SharedInner>,
    ready: mpsc::SyncSender<Result<u32, WindowsObserverError>>,
) {
    let mut context = CallbackContext {
        shared: Arc::clone(&shared),
        native_events: RefCell::new(NativeQueue::new()),
    };
    CALLBACK_CONTEXT.with(|slot| slot.set(&context));
    let result = unsafe { worker_setup_and_loop(&mut context, ready) };
    CALLBACK_CONTEXT.with(|slot| slot.set(null()));
    if let Err(state) = result {
        if state == SESSION_SUSPENDED || state == SESSION_LOCKED {
            mark_session_suspended(&shared);
        } else if state != SESSION_REVOKED {
            let mut data = lock_data(&shared);
            clear_observations(&mut data);
            mark_health(&mut data, HostHealth::Faulted);
            set_consent_capability_status(
                &mut data,
                shared.consent,
                CapabilityAvailability::Faulted,
                Some(CapabilityReason::AdapterFault),
                elapsed_ms(shared.started),
            );
        }
    }
}

unsafe fn worker_setup_and_loop(
    context: &mut CallbackContext,
    ready: mpsc::SyncSender<Result<u32, WindowsObserverError>>,
) -> Result<(), u8> {
    let context_user_data = context as *mut CallbackContext as *mut c_void;
    let shared = &context.shared;
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut message = MaybeUninit::<Message>::uninit();
    // Create the thread message queue before another thread can post WM_QUIT.
    unsafe { PeekMessageW(message.as_mut_ptr(), null_mut(), 0, 0, PM_REMOVE) };
    if input_desktop_state() != InputDesktopState::Default {
        let _ = ready.send(Ok(thread_id));
        return Err(SESSION_SUSPENDED);
    }
    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        let _ = ready.send(Err(WindowsObserverError::WorkerInitializationFailed));
        return Err(SESSION_REVOKED);
    }
    let class = WindowClassExW {
        size: size_of::<WindowClassExW>() as u32,
        style: 0,
        procedure: Some(observer_window_proc),
        class_extra: 0,
        window_extra: 0,
        instance,
        icon: null_mut(),
        cursor: null_mut(),
        background: null_mut(),
        menu_name: null(),
        class_name: CLASS_NAME.as_ptr(),
        small_icon: null_mut(),
    };
    let atom = unsafe { RegisterClassExW(&class) };
    if atom == 0 && unsafe { GetLastError() } != 1410 {
        let _ = ready.send(Err(WindowsObserverError::WorkerInitializationFailed));
        return Err(SESSION_REVOKED);
    }
    let previous_dpi_context = unsafe { SetThreadDpiAwarenessContext((-4isize) as Hwnd) };
    let dpi_context_set = !previous_dpi_context.is_null();
    let window = unsafe {
        CreateWindowExW(
            0x0800_0080,
            CLASS_NAME.as_ptr(),
            WINDOW_TITLE.as_ptr(),
            0x8000_0000,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            context_user_data,
        )
    };
    if window.is_null() {
        if dpi_context_set {
            unsafe { SetThreadDpiAwarenessContext(previous_dpi_context) };
        }
        let _ = ready.send(Err(WindowsObserverError::WorkerInitializationFailed));
        return Err(SESSION_REVOKED);
    }

    let mut raw_input_registered = false;
    let mut raw_owner_claimed = false;
    if shared.consent.pointer_buttons() {
        if RAW_INPUT_OWNER
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            raw_owner_claimed = true;
            raw_input_registered = register_raw_input(window);
        }
        let mut data = lock_data(shared);
        let now_ms = elapsed_ms(shared.started);
        let availability = if raw_input_registered {
            CapabilityAvailability::Available
        } else {
            CapabilityAvailability::Unavailable
        };
        let reason = if raw_input_registered {
            None
        } else {
            Some(CapabilityReason::FeedUnavailable)
        };
        set_capability_and_publish(
            &mut data,
            CapabilityFeature::PointerButtons,
            CapabilityScope::Desktop,
            availability,
            reason,
            if raw_input_registered {
                Confidence::High
            } else {
                Confidence::Unknown
            },
            now_ms,
        );
    }

    let mut hooks = [null_mut(); 3];
    if shared.consent.window_metadata() {
        hooks = install_winevent_hooks();
        if !hooks.iter().all(|hook| !hook.is_null()) {
            for hook in hooks.iter().copied().filter(|hook| !hook.is_null()) {
                unsafe { UnhookWinEvent(hook) };
            }
            hooks = [null_mut(); 3];
        }
        let hooks_available = hooks.iter().all(|hook| !hook.is_null());
        shared
            .window_hooks_available
            .store(hooks_available, Ordering::Release);
        let mut data = lock_data(shared);
        let now_ms = elapsed_ms(shared.started);
        let availability = if hooks_available {
            CapabilityAvailability::Available
        } else {
            CapabilityAvailability::Unavailable
        };
        let reason = if availability == CapabilityAvailability::Available {
            None
        } else {
            Some(CapabilityReason::FeedUnavailable)
        };
        set_capability_and_publish(
            &mut data,
            CapabilityFeature::WindowRectangles,
            CapabilityScope::Desktop,
            availability,
            reason,
            if dpi_context_set {
                Confidence::Medium
            } else {
                Confidence::Low
            },
            now_ms,
        );
        set_capability_and_publish(
            &mut data,
            CapabilityFeature::WindowState,
            CapabilityScope::Desktop,
            availability,
            reason,
            Confidence::Medium,
            now_ms,
        );
    }

    let session_notifications =
        unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) } != 0;
    let mut stop_reason = SESSION_ACTIVE;
    if input_desktop_state() != InputDesktopState::Default {
        mark_session_suspended(shared);
        stop_reason = SESSION_SUSPENDED;
        let _ = ready.send(Ok(thread_id));
    } else {
        update_outputs(shared);

        if shared.consent.window_metadata() {
            reconcile_windows(shared);
        }
        if input_desktop_state() != InputDesktopState::Default {
            mark_session_suspended(shared);
            stop_reason = SESSION_SUSPENDED;
        }
        let _ = ready.send(Ok(thread_id));

        if stop_reason == SESSION_ACTIVE {
            let mut last_boundary = unsafe { GetTickCount() };
            let mut last_output_poll = elapsed_ms(shared.started);
            'message_loop: loop {
                if shared.stop_requested.load(Ordering::Acquire) {
                    stop_reason = SESSION_REVOKED;
                    break;
                }
                let wait = unsafe {
                    MsgWaitForMultipleObjectsEx(
                        0,
                        null(),
                        INPUT_DESKTOP_POLL_MS,
                        QS_ALLINPUT,
                        MWMO_INPUTAVAILABLE,
                    )
                };
                if wait == WAIT_FAILED {
                    stop_reason = SESSION_NATIVE_FAILURE;
                    break;
                }
                let mut processed = 0;
                while processed < 64 {
                    let mut message = MaybeUninit::<Message>::uninit();
                    if unsafe { PeekMessageW(message.as_mut_ptr(), null_mut(), 0, 0, PM_REMOVE) }
                        == 0
                    {
                        break;
                    }
                    let message = unsafe { message.assume_init() };
                    if message.message == WM_QUIT {
                        stop_reason = if shared.stop_requested.load(Ordering::Acquire) {
                            SESSION_REVOKED
                        } else {
                            SESSION_NATIVE_FAILURE
                        };
                        break 'message_loop;
                    }
                    unsafe {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    processed += 1;
                }
                if shared.callback_overflow.swap(false, Ordering::AcqRel) {
                    reconcile_windows(shared);
                }
                if shared.input_overflow.swap(false, Ordering::AcqRel) {
                    mark_click_history_uncertain(shared);
                    refresh_button_state(shared);
                    refresh_pointer_position(shared);
                }
                if let Err(reason) = process_native_queue(shared, context) {
                    stop_reason = reason;
                    break;
                }
                if shared.stop_requested.load(Ordering::Acquire) {
                    stop_reason = SESSION_REVOKED;
                    break;
                }
                let tick = unsafe { GetTickCount() };
                if tick.wrapping_sub(last_boundary) >= INPUT_DESKTOP_POLL_MS {
                    last_boundary = tick;
                    match input_desktop_state() {
                        InputDesktopState::Default => {}
                        InputDesktopState::Other | InputDesktopState::Unknown => {
                            stop_reason = SESSION_SUSPENDED;
                            break;
                        }
                    }
                }
                if elapsed_ms(shared.started).saturating_sub(last_output_poll) >= OUTPUT_POLL_MS {
                    last_output_poll = elapsed_ms(shared.started);
                    if update_outputs(shared) && shared.consent.window_metadata() {
                        reconcile_windows(shared);
                    }
                    refresh_active_capabilities(shared);
                }
            }
        }
    }

    if raw_input_registered {
        unregister_raw_input_if_owned(window);
    }
    if raw_owner_claimed {
        RAW_INPUT_OWNER.store(false, Ordering::Release);
    }
    for hook in hooks.iter().copied().filter(|hook| !hook.is_null()) {
        unsafe { UnhookWinEvent(hook) };
    }
    if session_notifications {
        unsafe { WTSUnRegisterSessionNotification(window) };
    }
    unsafe { DestroyWindow(window) };
    if dpi_context_set {
        unsafe { SetThreadDpiAwarenessContext(previous_dpi_context) };
    }
    if stop_reason == SESSION_SUSPENDED || stop_reason == SESSION_LOCKED {
        return Err(stop_reason);
    }
    if stop_reason == SESSION_NATIVE_FAILURE {
        return Err(stop_reason);
    }
    Err(SESSION_REVOKED)
}

fn mark_health(data: &mut ObserverData, health: HostHealth) {
    if let Ok(changes) = data.capabilities.set_health(health) {
        let revision = changes.revision();
        for change in changes.iter() {
            let _ = data
                .events
                .push(HostEvent::CapabilityChanged(*change), revision);
        }
    }
}

fn mark_session_suspended(shared: &SharedInner) {
    shared
        .session_state
        .store(SESSION_SUSPENDED, Ordering::Release);
    let mut data = lock_data(shared);
    let now_ms = elapsed_ms(shared.started);
    clear_observations(&mut data);
    set_consent_capability_status(
        &mut data,
        shared.consent,
        CapabilityAvailability::TemporarilyUnavailable,
        Some(CapabilityReason::SessionUnavailable),
        now_ms,
    );
}

fn process_native_queue(shared: &SharedInner, context: &CallbackContext) -> Result<(), u8> {
    let (window_overflow, buttons_uncertain, position_uncertain, session_uncertain) = {
        let Ok(mut queue) = context.native_events.try_borrow_mut() else {
            shared.callback_overflow.store(true, Ordering::Release);
            return Ok(());
        };
        let flags = (
            queue.window_overflow,
            queue.button_state_uncertain,
            queue.position_uncertain,
            queue.session_boundary_uncertain,
        );
        queue.window_overflow = false;
        queue.button_state_uncertain = false;
        queue.position_uncertain = false;
        queue.session_boundary_uncertain = false;
        flags
    };
    if session_uncertain
        || shared
            .session_boundary_uncertain
            .swap(false, Ordering::AcqRel)
    {
        return Err(SESSION_SUSPENDED);
    }
    if buttons_uncertain {
        mark_click_history_uncertain(shared);
    }
    if position_uncertain {
        refresh_pointer_position(shared);
    }
    if window_overflow {
        reconcile_windows(shared);
    }
    loop {
        let event = match context.native_events.try_borrow_mut() {
            Ok(mut queue) => queue.pop(),
            Err(_) => {
                shared.callback_overflow.store(true, Ordering::Release);
                return Ok(());
            }
        };
        let Some(event) = event else {
            break;
        };
        match event {
            NativeEvent::Window { handle, event } => {
                if shared.consent.window_metadata() {
                    if event == EVENT_OBJECT_DESTROY {
                        remove_window(shared, handle, elapsed_ms(shared.started));
                    } else {
                        refresh_window(shared, handle, elapsed_ms(shared.started));
                    }
                }
            }
            NativeEvent::RawInput {
                button_flags,
                message_time,
                position,
            } => {
                if shared.consent.pointer_buttons() {
                    process_pointer_event(
                        shared,
                        button_flags_for_processing(buttons_uncertain, button_flags),
                        message_time,
                        position,
                    );
                }
            }
            NativeEvent::SessionChange { code } => match code {
                WTS_SESSION_LOCK => return Err(SESSION_LOCKED),
                WTS_SESSION_LOGOFF | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => {
                    return Err(SESSION_SUSPENDED);
                }
                _ => {}
            },
            NativeEvent::DisplayChange => {
                if update_outputs(shared) && shared.consent.window_metadata() {
                    reconcile_windows(shared);
                }
            }
        }
    }
    if buttons_uncertain {
        refresh_button_state(shared);
    }
    Ok(())
}

fn refresh_button_state(shared: &SharedInner) {
    let mut bits = 0;
    for (virtual_key, bit) in [
        (0x01, PointerButtons::LEFT),
        (0x02, PointerButtons::RIGHT),
        (0x04, PointerButtons::MIDDLE),
        (0x05, PointerButtons::AUXILIARY_1),
        (0x06, PointerButtons::AUXILIARY_2),
    ] {
        if unsafe { GetAsyncKeyState(virtual_key) } < 0 {
            bits |= bit;
        }
    }
    if let Some(buttons) = PointerButtons::new(bits) {
        let now_ms = elapsed_ms(shared.started);
        let update = PointerButtonsUpdate {
            buttons,
            observed_at_ms: now_ms,
        };
        let mut data = lock_data(shared);
        data.buttons = Some(update);
        publish_feed(&mut data, FeedEvent::PointerButtons(update));
    }
}

fn mark_click_history_uncertain(shared: &SharedInner) {
    if shared.consent.desktop_click_classification() {
        let mut data = lock_data(shared);
        data.click_events.overflowed = true;
    }
}

fn button_flags_for_processing(buttons_uncertain: bool, flags: u16) -> u16 {
    if buttons_uncertain { 0 } else { flags }
}

fn apply_button_flags(previous: u8, button_flags: u16) -> (u8, [Option<u8>; 5]) {
    let mut next = previous;
    let mut down = [None; 5];
    let transitions = [
        (
            RI_MOUSE_LEFT_BUTTON_DOWN,
            RI_MOUSE_LEFT_BUTTON_UP,
            PointerButtons::LEFT,
        ),
        (
            RI_MOUSE_RIGHT_BUTTON_DOWN,
            RI_MOUSE_RIGHT_BUTTON_UP,
            PointerButtons::RIGHT,
        ),
        (
            RI_MOUSE_MIDDLE_BUTTON_DOWN,
            RI_MOUSE_MIDDLE_BUTTON_UP,
            PointerButtons::MIDDLE,
        ),
        (
            RI_MOUSE_BUTTON_4_DOWN,
            RI_MOUSE_BUTTON_4_UP,
            PointerButtons::AUXILIARY_1,
        ),
        (
            RI_MOUSE_BUTTON_5_DOWN,
            RI_MOUSE_BUTTON_5_UP,
            PointerButtons::AUXILIARY_2,
        ),
    ];
    for (index, (down_flag, up_flag, bit)) in transitions.into_iter().enumerate() {
        if button_flags & down_flag != 0 {
            if next & bit == 0 {
                down[index] = Some(bit);
            }
            next |= bit;
        }
        if button_flags & up_flag != 0 {
            next &= !bit;
        }
    }
    (next, down)
}

fn process_pointer_event(
    shared: &SharedInner,
    button_flags: u16,
    message_time: u32,
    sampled_point: Option<Point>,
) {
    let now_ms = elapsed_ms(shared.started);
    let point = sampled_point.and_then(|point| pointer_position_at(shared, point, now_ms));
    let mut data = lock_data(shared);
    let previous = data.buttons.map_or(0, |update| update.buttons.bits());
    let (next, down) = apply_button_flags(previous, button_flags);
    if let Some(position) = point {
        data.position = Some(position);
        publish_feed(&mut data, FeedEvent::PointerPosition(position));
    }
    if (previous != next || data.buttons.is_none())
        && let Some(buttons) = PointerButtons::new(next)
    {
        let update = PointerButtonsUpdate {
            buttons,
            observed_at_ms: now_ms,
        };
        data.buttons = Some(update);
        publish_feed(&mut data, FeedEvent::PointerButtons(update));
    }
    if shared.consent.desktop_click_classification() {
        let age = unsafe { GetTickCount() }.wrapping_sub(message_time);
        if let Some(position) = point {
            for bit in down.into_iter().flatten() {
                let hit = if age <= CLICK_MAX_AGE_MS {
                    classify_desktop_hit(position)
                } else {
                    DesktopHitClassification::Unknown
                };
                data.click_events.push(DesktopClickObservation {
                    position,
                    button: bit,
                    hit,
                    observed_at_ms: now_ms,
                });
            }
        } else if down.iter().any(Option::is_some) {
            data.click_events.overflowed = true;
        }
    }
}

fn refresh_pointer_position(shared: &SharedInner) {
    let now_ms = elapsed_ms(shared.started);
    if let Some(position) = current_pointer_position(shared, now_ms) {
        let mut data = lock_data(shared);
        data.position = Some(position);
        publish_feed(&mut data, FeedEvent::PointerPosition(position));
    }
}

fn current_pointer_position(shared: &SharedInner, observed_at_ms: u64) -> Option<PointerPosition> {
    let mut point = Point { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return None;
    }
    pointer_position_at(shared, point, observed_at_ms)
}

fn pointer_position_at(
    shared: &SharedInner,
    point: Point,
    observed_at_ms: u64,
) -> Option<PointerPosition> {
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) };
    if monitor.is_null() {
        return None;
    }
    let data = lock_data(shared);
    let output = data
        .outputs
        .iter()
        .flatten()
        .find(|output| output.handle == monitor as usize)?;
    Some(PointerPosition {
        display: output.identity,
        x: point.x,
        y: point.y,
        observed_at_ms,
    })
}

fn classify_desktop_hit(position: PointerPosition) -> DesktopHitClassification {
    let point = Point {
        x: position.x,
        y: position.y,
    };
    let target = unsafe { WindowFromPoint(point) };
    if target.is_null() {
        return DesktopHitClassification::Unknown;
    }
    let root = unsafe { GetAncestor(target, GA_ROOT) };
    if root.is_null() {
        return DesktopHitClassification::Unknown;
    }
    let mut root_class = [0u16; 128];
    let Some(root_length) = window_class_name(root, &mut root_class) else {
        return DesktopHitClassification::Unknown;
    };
    let mut target_class = [0u16; 128];
    let Some(target_length) = window_class_name(target, &mut target_class) else {
        return DesktopHitClassification::Unknown;
    };
    let shell = unsafe { GetShellWindow() };
    let mut shell_process = 0;
    if shell.is_null() || unsafe { GetWindowThreadProcessId(shell, &mut shell_process) } == 0 {
        return DesktopHitClassification::Unknown;
    }
    let mut root_process = 0;
    let mut target_process = 0;
    if unsafe { GetWindowThreadProcessId(root, &mut root_process) } == 0
        || unsafe { GetWindowThreadProcessId(target, &mut target_process) } == 0
        || root_process == 0
        || target_process == 0
    {
        return DesktopHitClassification::Unknown;
    }
    if root_process != shell_process || target_process != shell_process {
        return DesktopHitClassification::Occupied;
    }

    let root_name = &root_class[..root_length];
    let target_name = &target_class[..target_length];
    let root_is_desktop =
        class_matches(root_name, "Progman") || class_matches(root_name, "WorkerW");
    if target == root {
        return classify_shell_surface(root_is_desktop, true, false, false, None);
    }
    if !class_matches(target_name, "SysListView32") {
        return DesktopHitClassification::Occupied;
    }
    let parent = unsafe { GetParent(target) };
    if parent.is_null() {
        return DesktopHitClassification::Unknown;
    }
    let mut parent_process = 0;
    let mut parent_class = [0u16; 128];
    let Some(parent_length) = window_class_name(parent, &mut parent_class) else {
        return DesktopHitClassification::Unknown;
    };
    if unsafe { GetWindowThreadProcessId(parent, &mut parent_process) } == 0
        || parent_process != shell_process
        || !class_matches(&parent_class[..parent_length], "SHELLDLL_DefView")
    {
        return DesktopHitClassification::Unknown;
    }
    classify_desktop_list_hit(
        root_is_desktop,
        target_process == unsafe { GetCurrentProcessId() },
        || list_view_item_at(target, point),
    )
}

fn window_class_name(window: Hwnd, name: &mut [u16]) -> Option<usize> {
    let length = unsafe { GetClassNameW(window, name.as_mut_ptr(), name.len() as i32) };
    (length > 0).then_some(length as usize)
}

fn list_view_item_at(window: Hwnd, mut point: Point) -> Option<i32> {
    if unsafe { ScreenToClient(window, &mut point) } == 0 {
        return None;
    }
    let mut hit = ListViewHitTestInfo {
        point,
        flags: 0,
        item: -1,
        sub_item: 0,
        group: -1,
    };
    let mut result = 0isize;
    if unsafe {
        SendMessageTimeoutW(
            window,
            LVM_HITTEST,
            0,
            (&mut hit as *mut ListViewHitTestInfo) as isize,
            SMTO_BLOCK | SMTO_ABORTIFHUNG,
            CLICK_HIT_TEST_TIMEOUT_MS,
            &mut result,
        )
    } == 0
    {
        return None;
    }
    Some(hit.item)
}

fn classify_desktop_list_hit(
    verified_shell_desktop: bool,
    target_is_local_process: bool,
    hit_test: impl FnOnce() -> Option<i32>,
) -> DesktopHitClassification {
    if !verified_shell_desktop {
        return DesktopHitClassification::Occupied;
    }
    if !target_is_local_process {
        return DesktopHitClassification::Unknown;
    }
    classify_shell_surface(true, false, true, true, hit_test())
}

fn classify_shell_surface(
    verified_shell_desktop: bool,
    target_is_root: bool,
    target_is_desktop_list: bool,
    parent_is_shell_defview: bool,
    item_hit: Option<i32>,
) -> DesktopHitClassification {
    if !verified_shell_desktop {
        return DesktopHitClassification::Occupied;
    }
    if target_is_root {
        return DesktopHitClassification::EmptyWallpaper;
    }
    if target_is_desktop_list && parent_is_shell_defview {
        return match item_hit {
            Some(-1) => DesktopHitClassification::EmptyWallpaper,
            Some(_) => DesktopHitClassification::Occupied,
            None => DesktopHitClassification::Unknown,
        };
    }
    DesktopHitClassification::Occupied
}

fn class_matches(actual: &[u16], expected: &str) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected.bytes())
            .all(|(actual, expected)| {
                let actual = if (65..=90).contains(actual) {
                    *actual + 32
                } else {
                    *actual
                };
                let expected = u16::from(expected);
                let expected = if (65..=90).contains(&expected) {
                    expected + 32
                } else {
                    expected
                };
                actual == expected
            })
}

fn refresh_window(shared: &SharedInner, handle: usize, now_ms: u64) {
    let hwnd = handle as Hwnd;
    if unsafe { IsWindow(hwnd) } == 0 {
        remove_window(shared, handle, now_ms);
        return;
    }
    let mut process_id = 0;
    let window_thread = unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
    if window_thread == 0 || process_id == 0 {
        update_window(shared, hwnd, WindowState::Other, None, now_ms);
        return;
    }
    let own_process = unsafe { GetCurrentProcessId() };
    let shell_window = unsafe { GetShellWindow() };
    let mut shell_process = 0;
    if !shell_window.is_null() {
        unsafe { GetWindowThreadProcessId(shell_window, &mut shell_process) };
    }
    if process_id == own_process || (shell_process != 0 && process_id == shell_process) {
        remove_window(shared, handle, now_ms);
        return;
    }
    let visible = unsafe { IsWindowVisible(hwnd) } != 0;
    let minimized = unsafe { IsIconic(hwnd) } != 0;
    let mut cloaked = 0u32;
    let cloak_known = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWM_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            size_of::<u32>() as u32,
        )
    } >= 0;
    if !visible {
        update_window(shared, hwnd, WindowState::Hidden, None, now_ms);
        return;
    }
    if minimized {
        update_window(shared, hwnd, WindowState::Minimized, None, now_ms);
        return;
    }
    if cloak_known && cloaked != 0 {
        update_window(shared, hwnd, WindowState::Hidden, None, now_ms);
        return;
    }
    let mut geometry = read_bounds(hwnd, now_ms);
    let state = if !cloak_known {
        WindowState::Other
    } else if unsafe { IsZoomed(hwnd) } != 0 {
        WindowState::Maximized
    } else {
        WindowState::Normal
    };
    if let Some(bounds) = geometry.as_mut() {
        bounds.rectangle.display = display_for_rect(shared, bounds.rectangle);
    }
    update_window(shared, hwnd, state, geometry, now_ms);
}

fn read_bounds(hwnd: Hwnd, now_ms: u64) -> Option<ObservedWindowBounds> {
    let mut rect = NativeRect::EMPTY;
    let mut source = None;
    let status = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWM_EXTENDED_FRAME_BOUNDS,
            (&mut rect as *mut NativeRect).cast(),
            size_of::<NativeRect>() as u32,
        )
    };
    if status >= 0 && rect.dimensions().is_some() {
        source = Some(WindowBoundsSource::DwmExtendedFrame);
    } else {
        let thread_context = unsafe { GetThreadDpiAwarenessContext() };
        if !thread_context.is_null()
            && unsafe { AreDpiAwarenessContextsEqual(thread_context, (-4isize) as Hwnd) } != 0
            && unsafe { GetWindowRect(hwnd, (&mut rect as *mut NativeRect).cast()) } != 0
            && rect.dimensions().is_some()
        {
            source = Some(WindowBoundsSource::GetWindowRectPerMonitorV2);
        }
    }
    let source = source?;
    let (width, height) = rect.dimensions()?;
    Some(ObservedWindowBounds {
        rectangle: WindowRectangle {
            window: WindowIdentity::new(0, Generation::INITIAL),
            display: None,
            left: rect.left,
            top: rect.top,
            width,
            height,
            observed_at_ms: now_ms,
        },
        source,
    })
}

fn display_for_rect(shared: &SharedInner, rectangle: WindowRectangle) -> Option<MonitorIdentity> {
    let window_rect = NativeRect {
        left: rectangle.left,
        top: rectangle.top,
        right: i32::try_from(i64::from(rectangle.left) + i64::from(rectangle.width)).ok()?,
        bottom: i32::try_from(i64::from(rectangle.top) + i64::from(rectangle.height)).ok()?,
    };
    let mut largest_area = 0;
    let mut best = None;
    let mut tied = false;
    let data = lock_data(shared);
    for output in data.outputs.iter().flatten() {
        let area = intersection_area(window_rect, output.rect);
        if area > largest_area {
            largest_area = area;
            best = Some(output.identity);
            tied = false;
        } else if area != 0 && area == largest_area {
            tied = true;
        }
    }
    if tied { None } else { best }
}

fn update_window(
    shared: &SharedInner,
    hwnd: Hwnd,
    state: WindowState,
    mut bounds: Option<ObservedWindowBounds>,
    now_ms: u64,
) {
    let handle = hwnd as usize;
    let mut data = lock_data(shared);
    let slot = data
        .windows
        .iter()
        .position(|entry| entry.is_some_and(|entry| entry.handle == handle))
        .or_else(|| data.windows.iter().position(Option::is_none));
    let Some(slot) = slot else {
        data.windows_truncated = true;
        set_capability_and_publish(
            &mut data,
            CapabilityFeature::WindowRectangles,
            CapabilityScope::Desktop,
            CapabilityAvailability::TemporarilyUnavailable,
            Some(CapabilityReason::TemporaryFailure),
            Confidence::Low,
            now_ms,
        );
        set_capability_and_publish(
            &mut data,
            CapabilityFeature::WindowState,
            CapabilityScope::Desktop,
            CapabilityAvailability::TemporarilyUnavailable,
            Some(CapabilityReason::TemporaryFailure),
            Confidence::Low,
            now_ms,
        );
        return;
    };
    let existing = data.windows[slot];
    let identity = existing.map_or_else(
        || {
            let value = data.next_window_identity;
            data.next_window_identity = data.next_window_identity.saturating_add(1);
            WindowIdentity::new(value, Generation::from_value(1))
        },
        |entry| entry.identity,
    );
    if let Some(bounds) = bounds.as_mut() {
        bounds.rectangle.window = identity;
    }
    let mut fullscreen = [None; MAX_FULLSCREEN_OBSERVATIONS];
    if matches!(state, WindowState::Normal | WindowState::Maximized) {
        if let Some(bounds) = bounds {
            let output_records = data.outputs;
            let mut count = 0;
            for output in output_records.iter().flatten() {
                if fullscreen_overlap(bounds.rectangle, output.rect) {
                    if count < MAX_FULLSCREEN_OBSERVATIONS {
                        fullscreen[count] = Some(FullscreenCandidate {
                            window: identity,
                            display: output.identity,
                            confidence: Confidence::Medium,
                            observed_at_ms: now_ms,
                        });
                        count += 1;
                    }
                }
            }
        }
    }
    let next = WindowRecord {
        handle,
        identity,
        state,
        bounds,
        fullscreen,
    };
    if let Some(old) = existing {
        if old.state != state {
            publish_feed(
                &mut data,
                FeedEvent::WindowState {
                    window: identity,
                    state,
                    observed_at_ms: now_ms,
                },
            );
        }
        if !same_bounds(old.bounds, bounds) {
            if let Some(bounds) = bounds {
                publish_feed(&mut data, FeedEvent::WindowRectangle(bounds.rectangle));
            }
        }
    } else {
        publish_feed(
            &mut data,
            FeedEvent::WindowState {
                window: identity,
                state,
                observed_at_ms: now_ms,
            },
        );
        if let Some(bounds) = bounds {
            publish_feed(&mut data, FeedEvent::WindowRectangle(bounds.rectangle));
        }
    }
    data.windows[slot] = Some(next);
}

fn same_bounds(left: Option<ObservedWindowBounds>, right: Option<ObservedWindowBounds>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.source == right.source
                && left.rectangle.display == right.rectangle.display
                && left.rectangle.left == right.rectangle.left
                && left.rectangle.top == right.rectangle.top
                && left.rectangle.width == right.rectangle.width
                && left.rectangle.height == right.rectangle.height
        }
        _ => false,
    }
}

fn remove_window(shared: &SharedInner, handle: usize, now_ms: u64) {
    let mut data = lock_data(shared);
    if let Some(index) = data
        .windows
        .iter()
        .position(|entry| entry.is_some_and(|entry| entry.handle == handle))
    {
        if let Some(entry) = data.windows[index].take() {
            publish_feed(
                &mut data,
                FeedEvent::WindowState {
                    window: entry.identity,
                    state: WindowState::Hidden,
                    observed_at_ms: now_ms,
                },
            );
        }
    }
}

fn same_output_rect(left: NativeRect, right: NativeRect) -> bool {
    left.left == right.left
        && left.top == right.top
        && left.right == right.right
        && left.bottom == right.bottom
}

fn update_outputs(shared: &SharedInner) -> bool {
    let mut collected = MonitorCollector::new();
    let enumerated = unsafe {
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(monitor_collector_proc),
            (&mut collected as *mut MonitorCollector) as isize,
        )
    } != 0;
    let mut data = lock_data(shared);
    let previous = data.outputs;
    let now_ms = elapsed_ms(shared.started);
    if !enumerated && !collected.truncated {
        data.outputs = [None; MAX_NATIVE_OUTPUTS];
        data.outputs_truncated = false;
        if shared.consent.pointer_buttons() {
            set_pointer_position_status(&mut data, now_ms);
        }
        return previous.iter().any(Option::is_some);
    }

    let mut next = [None; MAX_NATIVE_OUTPUTS];
    let mut changed = collected.truncated || data.outputs_truncated;
    let mut dropped_output = false;
    for (index, monitor) in collected.records[..collected.len].iter().enumerate() {
        let existing = previous.iter().flatten().find(|output| {
            output.handle == monitor.handle && same_output_rect(output.rect, monitor.rect)
        });
        let identity = if let Some(output) = existing {
            output.identity
        } else {
            changed = true;
            let serial = data.next_output_identity;
            let Some(next_serial) = serial.checked_add(1) else {
                dropped_output = true;
                continue;
            };
            data.next_output_identity = next_serial;
            match monitor_identity(data.capabilities.host().instance_id(), serial, monitor.rect) {
                Some(identity) => identity,
                None => {
                    dropped_output = true;
                    continue;
                }
            }
        };
        next[index] = Some(OutputRecord {
            handle: monitor.handle,
            rect: monitor.rect,
            identity,
        });
    }
    if collected.len != previous.iter().flatten().count() {
        changed = true;
    }
    data.outputs = next;
    data.outputs_truncated = collected.truncated || dropped_output;
    if shared.consent.pointer_buttons() {
        set_pointer_position_status(&mut data, now_ms);
    }
    changed
}

fn set_pointer_position_status(data: &mut ObserverData, now_ms: u64) {
    let raw_input_available = data
        .capabilities
        .record(CapabilityFeature::PointerButtons, CapabilityScope::Desktop)
        .is_some_and(|record| record.availability() == CapabilityAvailability::Available);
    let outputs_available = data.outputs.iter().any(Option::is_some);
    let (availability, reason, confidence) = if !raw_input_available {
        (
            CapabilityAvailability::Unavailable,
            Some(CapabilityReason::FeedUnavailable),
            Confidence::Unknown,
        )
    } else if data.outputs_truncated {
        (
            CapabilityAvailability::TemporarilyUnavailable,
            Some(CapabilityReason::TemporaryFailure),
            Confidence::Unknown,
        )
    } else if outputs_available {
        (CapabilityAvailability::Available, None, Confidence::High)
    } else {
        (
            CapabilityAvailability::Unavailable,
            Some(CapabilityReason::OutputUnavailable),
            Confidence::Unknown,
        )
    };
    set_capability_and_publish(
        data,
        CapabilityFeature::PointerPosition,
        CapabilityScope::Desktop,
        availability,
        reason,
        confidence,
        now_ms,
    );
}

fn monitor_identity(instance_id: u64, serial: u64, rect: NativeRect) -> Option<MonitorIdentity> {
    let (width, height) = rect.dimensions()?;
    let fallback = MonitorFallbackMatch::new(rect.left, rect.top, width, height).ok()?;
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&instance_id.to_le_bytes());
    bytes[8..].copy_from_slice(&serial.to_le_bytes());
    Some(MonitorIdentity::new(MonitorId::new(bytes).ok()?, fallback))
}

fn reconcile_windows(shared: &SharedInner) {
    if !shared.consent.window_metadata() || !shared.window_hooks_available.load(Ordering::Acquire) {
        return;
    }
    let mut collector = WindowCollector::new();
    let enumerated = unsafe {
        EnumWindows(
            Some(window_collector_proc),
            (&mut collector as *mut WindowCollector) as isize,
        )
    } != 0;
    if !enumerated {
        collector.truncated = true;
    }
    let now_ms = elapsed_ms(shared.started);
    let mut stale = [0usize; MAX_WINDOWS_OBSERVED];
    let mut stale_len = 0;
    if !collector.truncated {
        let data = lock_data(shared);
        for entry in data.windows.iter().flatten() {
            if !collector.records[..collector.len]
                .iter()
                .any(|handle| *handle == entry.handle)
            {
                stale[stale_len] = entry.handle;
                stale_len += 1;
            }
        }
    }
    for handle in stale[..stale_len].iter().copied() {
        remove_window(shared, handle, now_ms);
    }
    if !collector.truncated {
        let mut data = lock_data(shared);
        data.windows_truncated = false;
    }
    for handle in collector.records[..collector.len].iter().copied() {
        refresh_window(shared, handle, now_ms);
    }
    let mut data = lock_data(shared);
    if collector.truncated {
        data.windows_truncated = true;
    }
    let (availability, reason, confidence) = if data.windows_truncated {
        (
            CapabilityAvailability::TemporarilyUnavailable,
            Some(CapabilityReason::TemporaryFailure),
            Confidence::Low,
        )
    } else {
        (CapabilityAvailability::Available, None, Confidence::Medium)
    };
    set_capability_and_publish(
        &mut data,
        CapabilityFeature::WindowState,
        CapabilityScope::Desktop,
        availability,
        reason,
        confidence,
        now_ms,
    );
    set_capability_and_publish(
        &mut data,
        CapabilityFeature::WindowRectangles,
        CapabilityScope::Desktop,
        availability,
        reason,
        Confidence::Low,
        now_ms,
    );
}

fn input_desktop_state() -> InputDesktopState {
    let desktop = unsafe { OpenInputDesktop(0, 0, DESKTOP_READOBJECTS) };
    if desktop.is_null() {
        return InputDesktopState::Unknown;
    }
    let mut name = [0u16; 128];
    let mut needed = 0u32;
    let result = unsafe {
        GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            (name.len() * size_of::<u16>()) as u32,
            &mut needed,
        )
    };
    unsafe { CloseDesktop(desktop) };
    if result == 0 {
        return InputDesktopState::Unknown;
    }
    let length = name
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(name.len());
    if class_matches(&name[..length], "Default") {
        InputDesktopState::Default
    } else {
        InputDesktopState::Other
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum InputDesktopState {
    Default,
    Other,
    Unknown,
}

fn fullscreen_overlap(rectangle: WindowRectangle, output: NativeRect) -> bool {
    let Some((width, height)) = (NativeRect {
        left: rectangle.left,
        top: rectangle.top,
        right: match i32::try_from(i64::from(rectangle.left) + i64::from(rectangle.width)) {
            Ok(value) => value,
            Err(_) => return false,
        },
        bottom: match i32::try_from(i64::from(rectangle.top) + i64::from(rectangle.height)) {
            Ok(value) => value,
            Err(_) => return false,
        },
    })
    .dimensions() else {
        return false;
    };
    let _ = (width, height);
    let output_area = output.area();
    if output_area == 0 {
        return false;
    }
    let window = NativeRect {
        left: rectangle.left,
        top: rectangle.top,
        right: (i64::from(rectangle.left) + i64::from(rectangle.width)) as i32,
        bottom: (i64::from(rectangle.top) + i64::from(rectangle.height)) as i32,
    };
    let overlap = intersection_area(window, output);
    let required = output_area - output_area / 50;
    overlap >= required
}

fn intersection_area(left: NativeRect, right: NativeRect) -> u64 {
    let width =
        (i64::from(left.right.min(right.right)) - i64::from(left.left.max(right.left))).max(0);
    let height =
        (i64::from(left.bottom.min(right.bottom)) - i64::from(left.top.max(right.top))).max(0);
    (width as u64).saturating_mul(height as u64)
}

fn register_raw_input(window: Hwnd) -> bool {
    let mut bytes = size_of::<RawInputDevice>() as u32;
    let count = unsafe { GetRegisteredRawInputDevices(null_mut(), &mut bytes) };
    if count == u32::MAX || bytes as usize != size_of::<RawInputDevice>() {
        return false;
    }
    if count as usize > 64 {
        return false;
    }
    let mut devices = [RawInputDevice::EMPTY; 64];
    if count != 0 {
        let actual = unsafe { GetRegisteredRawInputDevices(devices.as_mut_ptr(), &mut bytes) };
        if actual == u32::MAX || actual != count {
            return false;
        }
        if devices[..count as usize]
            .iter()
            .any(|device| device.usage_page == 1 && device.usage == 2)
        {
            return false;
        }
    }
    let device = RawInputDevice {
        usage_page: 1,
        usage: 2,
        flags: RIDEV_INPUTSINK,
        target: window,
    };
    unsafe { RegisterRawInputDevices(&device, 1, size_of::<RawInputDevice>() as u32) != 0 }
}

fn unregister_raw_input_if_owned(window: Hwnd) {
    let mut bytes = size_of::<RawInputDevice>() as u32;
    let count = unsafe { GetRegisteredRawInputDevices(null_mut(), &mut bytes) };
    if count == u32::MAX || bytes as usize != size_of::<RawInputDevice>() || count > 64 {
        return;
    }
    let mut devices = [RawInputDevice::EMPTY; 64];
    if count == 0
        || unsafe { GetRegisteredRawInputDevices(devices.as_mut_ptr(), &mut bytes) } != count
    {
        return;
    }
    let ours = devices[..count as usize].iter().any(|device| {
        device.usage_page == 1
            && device.usage == 2
            && device.flags == RIDEV_INPUTSINK
            && device.target == window
    });
    if ours {
        let removal = RawInputDevice {
            usage_page: 1,
            usage: 2,
            flags: RIDEV_REMOVE,
            target: null_mut(),
        };
        unsafe { RegisterRawInputDevices(&removal, 1, size_of::<RawInputDevice>() as u32) };
    }
}

fn install_winevent_hooks() -> [HwinEventHook; 3] {
    let callback: Option<WinEventCallback> = Some(winevent_proc as WinEventCallback);
    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    [
        unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                null_mut(),
                callback,
                0,
                0,
                flags,
            )
        },
        unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_MINIMIZESTART,
                EVENT_SYSTEM_MINIMIZEEND,
                null_mut(),
                callback,
                0,
                0,
                flags,
            )
        },
        unsafe {
            SetWinEventHook(
                EVENT_OBJECT_DESTROY,
                EVENT_OBJECT_LOCATIONCHANGE,
                null_mut(),
                callback,
                0,
                0,
                flags,
            )
        },
    ]
}

unsafe extern "system" fn winevent_proc(
    _hook: HwinEventHook,
    event: u32,
    hwnd: Hwnd,
    object_id: i32,
    child_id: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    if hwnd.is_null() || object_id != OBJID_WINDOW || child_id != CHILDID_SELF {
        return;
    }
    if !matches!(
        event,
        EVENT_SYSTEM_FOREGROUND
            | EVENT_SYSTEM_MINIMIZESTART
            | EVENT_SYSTEM_MINIMIZEEND
            | EVENT_OBJECT_SHOW
            | EVENT_OBJECT_HIDE
            | EVENT_OBJECT_DESTROY
            | EVENT_OBJECT_LOCATIONCHANGE
            | EVENT_OBJECT_STATECHANGE
    ) {
        return;
    }
    CALLBACK_CONTEXT.with(|slot| {
        let context = slot.get();
        if context.is_null() {
            return;
        }
        // SAFETY: callbacks are delivered on the thread that owns the out-of-context hooks; the
        // worker keeps this stack context alive until every hook has been removed.
        let context = unsafe { &*context };
        if context.shared.session_state.load(Ordering::Acquire) != SESSION_ACTIVE {
            return;
        }
        if let Ok(mut queue) = context.native_events.try_borrow_mut() {
            queue.push(NativeEvent::Window {
                handle: hwnd as usize,
                event,
            });
        } else {
            context
                .shared
                .callback_overflow
                .store(true, Ordering::Release);
        }
    });
}

unsafe extern "system" fn observer_window_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if message == WM_NCCREATE {
        let create = lparam as *const CreateStructW;
        if !create.is_null() {
            // SAFETY: CREATESTRUCTW is supplied by CreateWindowExW during WM_NCCREATE.
            let context = unsafe { (*create).create_params };
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, context as isize) };
            return 1;
        }
    }
    let context = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const CallbackContext };
    if !context.is_null() {
        // SAFETY: the observer thread owns the context for the complete HWND lifetime.
        let context = unsafe { &*context };
        match message {
            WM_INPUT => {
                if context.shared.session_state.load(Ordering::Acquire) == SESSION_ACTIVE {
                    match read_raw_mouse(lparam as Hrawinput) {
                        Some(button_flags) => {
                            let message_time = unsafe { GetMessageTime() } as u32;
                            let mut point = Point { x: 0, y: 0 };
                            let position = if unsafe { GetCursorPos(&mut point) } != 0 {
                                Some(point)
                            } else {
                                None
                            };
                            if let Ok(mut queue) = context.native_events.try_borrow_mut() {
                                queue.push(NativeEvent::RawInput {
                                    button_flags,
                                    message_time,
                                    position,
                                });
                            } else {
                                context.shared.input_overflow.store(true, Ordering::Release);
                            }
                        }
                        None => context.shared.input_overflow.store(true, Ordering::Release),
                    }
                }
                return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            }
            WM_WTSSESSION_CHANGE => {
                if let Ok(mut queue) = context.native_events.try_borrow_mut() {
                    queue.push(NativeEvent::SessionChange {
                        code: wparam as u32,
                    });
                } else {
                    context
                        .shared
                        .session_boundary_uncertain
                        .store(true, Ordering::Release);
                }
                return 0;
            }
            WM_DISPLAYCHANGE => {
                if let Ok(mut queue) = context.native_events.try_borrow_mut() {
                    queue.push(NativeEvent::DisplayChange);
                } else {
                    context
                        .shared
                        .callback_overflow
                        .store(true, Ordering::Release);
                }
                return 0;
            }
            WM_CLOSE => {
                unsafe { DestroyWindow(hwnd) };
                return 0;
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                return 0;
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn read_raw_mouse(raw: Hrawinput) -> Option<u16> {
    if raw.is_null() {
        return None;
    }
    let mut native = MaybeUninit::<RawInput>::uninit();
    let mut size = size_of::<RawInput>() as u32;
    let read = unsafe {
        GetRawInputData(
            raw,
            RID_INPUT,
            native.as_mut_ptr().cast(),
            &mut size,
            size_of::<RawInputHeader>() as u32,
        )
    };
    if read == u32::MAX
        || (read as usize) < size_of::<RawInputHeader>() + size_of::<RawMouse>()
        || (read as usize) > size_of::<RawInput>()
    {
        return None;
    }
    // SAFETY: GetRawInputData reported a complete, bounded RAWINPUT structure.
    let native = unsafe { native.assume_init() };
    if native.header.kind != RIM_TYPEMOUSE || native.header.size as usize != read as usize {
        return None;
    }
    // SAFETY: dwType is RIM_TYPEMOUSE and the validated RAWINPUT buffer contains RAWMOUSE.
    Some(unsafe { native.data.mouse.button_flags })
}

struct WindowCollector {
    records: [usize; MAX_ENUMERATED_WINDOWS],
    len: usize,
    truncated: bool,
}

impl WindowCollector {
    const fn new() -> Self {
        Self {
            records: [0; MAX_ENUMERATED_WINDOWS],
            len: 0,
            truncated: false,
        }
    }
}

unsafe extern "system" fn window_collector_proc(hwnd: Hwnd, parameter: isize) -> i32 {
    let collector = parameter as *mut WindowCollector;
    if collector.is_null() {
        return 0;
    }
    // SAFETY: EnumWindows calls synchronously while the stack collector remains live.
    let collector = unsafe { &mut *collector };
    if collector.len == MAX_ENUMERATED_WINDOWS {
        collector.truncated = true;
        return 0;
    }
    collector.records[collector.len] = hwnd as usize;
    collector.len += 1;
    1
}

struct MonitorCollector {
    records: [NativeMonitor; MAX_NATIVE_OUTPUTS],
    len: usize,
    truncated: bool,
}

impl MonitorCollector {
    const fn new() -> Self {
        Self {
            records: [NativeMonitor::EMPTY; MAX_NATIVE_OUTPUTS],
            len: 0,
            truncated: false,
        }
    }
}

#[derive(Clone, Copy)]
struct NativeMonitor {
    handle: usize,
    rect: NativeRect,
}

impl NativeMonitor {
    const EMPTY: Self = Self {
        handle: 0,
        rect: NativeRect::EMPTY,
    };
}

unsafe extern "system" fn monitor_collector_proc(
    monitor: Hmonitor,
    _dc: Hdc,
    rect: *mut NativeRect,
    parameter: isize,
) -> i32 {
    let collector = parameter as *mut MonitorCollector;
    if collector.is_null() || rect.is_null() {
        return 0;
    }
    // SAFETY: EnumDisplayMonitors calls synchronously with a live collector and RECT argument.
    let collector = unsafe { &mut *collector };
    if collector.len == MAX_NATIVE_OUTPUTS {
        collector.truncated = true;
        return 0;
    }
    // SAFETY: the callback RECT is valid for the duration of this synchronous callback.
    let rect = unsafe { *rect };
    collector.records[collector.len] = NativeMonitor {
        handle: monitor as usize,
        rect,
    };
    collector.len += 1;
    1
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct ListViewHitTestInfo {
    point: Point,
    flags: u32,
    item: i32,
    sub_item: i32,
    group: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawInputHeader {
    kind: u32,
    size: u32,
    device: Hwnd,
    wparam: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawMouse {
    flags: u16,
    button_flags: u16,
    button_data: u16,
    raw_buttons: u32,
    last_x: i32,
    last_y: i32,
    extra_information: u32,
}

#[repr(C)]
union RawInputData {
    mouse: RawMouse,
    padding: [u8; 24],
}

#[repr(C)]
struct RawInput {
    header: RawInputHeader,
    data: RawInputData,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawInputDevice {
    usage_page: u16,
    usage: u16,
    flags: u32,
    target: Hwnd,
}

impl RawInputDevice {
    const EMPTY: Self = Self {
        usage_page: 0,
        usage: 0,
        flags: 0,
        target: null_mut(),
    };
}

#[repr(C)]
struct Message {
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    point: Point,
    private: u32,
}

#[repr(C)]
struct WindowClassExW {
    size: u32,
    style: u32,
    procedure: Option<unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize>,
    class_extra: i32,
    window_extra: i32,
    instance: Hwnd,
    icon: Hwnd,
    cursor: Hwnd,
    background: Hwnd,
    menu_name: *const u16,
    class_name: *const u16,
    small_icon: Hwnd,
}

#[repr(C)]
struct CreateStructW {
    create_params: *mut c_void,
    instance: Hwnd,
    menu: Hwnd,
    parent: Hwnd,
    height: i32,
    width: i32,
    y: i32,
    x: i32,
    style: i32,
    name: *const u16,
    class: *const u16,
    extended_style: u32,
}

type Hwnd = *mut c_void;
type Hmonitor = *mut c_void;
type Hdc = *mut c_void;
type Hrawinput = *mut c_void;
type Hdesktop = *mut c_void;
type HwinEventHook = *mut c_void;
type WinEventCallback = unsafe extern "system" fn(HwinEventHook, u32, Hwnd, i32, i32, u32, u32);
type EnumWindowsCallback = unsafe extern "system" fn(Hwnd, isize) -> i32;
type EnumMonitorsCallback = unsafe extern "system" fn(Hmonitor, Hdc, *mut NativeRect, isize) -> i32;

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WindowClassExW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: Hwnd,
        instance: Hwnd,
        parameter: *mut c_void,
    ) -> Hwnd;
    fn DestroyWindow(window: Hwnd) -> i32;
    fn DefWindowProcW(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    fn PostQuitMessage(exit_code: i32);
    fn PeekMessageW(message: *mut Message, window: Hwnd, min: u32, max: u32, remove: u32) -> i32;
    fn TranslateMessage(message: *const Message) -> i32;
    fn DispatchMessageW(message: *const Message) -> isize;
    fn MsgWaitForMultipleObjectsEx(
        count: u32,
        handles: *const Hwnd,
        timeout: u32,
        wake_mask: u32,
        flags: u32,
    ) -> u32;
    fn PostThreadMessageW(thread_id: u32, message: u32, wparam: usize, lparam: isize) -> i32;
    fn GetCurrentThreadId() -> u32;
    fn GetCurrentProcessId() -> u32;
    fn GetLastError() -> u32;
    fn GetMessageTime() -> i32;
    fn GetTickCount() -> u32;
    fn GetCursorPos(point: *mut Point) -> i32;
    fn WindowFromPoint(point: Point) -> Hwnd;
    fn GetAncestor(window: Hwnd, flags: u32) -> Hwnd;
    fn GetParent(window: Hwnd) -> Hwnd;
    fn ScreenToClient(window: Hwnd, point: *mut Point) -> i32;
    fn SendMessageTimeoutW(
        window: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        flags: u32,
        timeout: u32,
        result: *mut isize,
    ) -> isize;
    fn GetWindowThreadProcessId(window: Hwnd, process_id: *mut u32) -> u32;
    fn GetShellWindow() -> Hwnd;
    fn GetClassNameW(window: Hwnd, name: *mut u16, max_count: i32) -> i32;
    fn IsWindow(window: Hwnd) -> i32;
    fn IsWindowVisible(window: Hwnd) -> i32;
    fn IsIconic(window: Hwnd) -> i32;
    fn IsZoomed(window: Hwnd) -> i32;
    fn GetWindowRect(window: Hwnd, rect: *mut NativeRect) -> i32;
    fn EnumWindows(callback: Option<EnumWindowsCallback>, parameter: isize) -> i32;
    fn EnumDisplayMonitors(
        dc: Hdc,
        clip: *const NativeRect,
        callback: Option<EnumMonitorsCallback>,
        parameter: isize,
    ) -> i32;
    fn MonitorFromPoint(point: Point, flags: u32) -> Hmonitor;
    fn GetRegisteredRawInputDevices(devices: *mut RawInputDevice, bytes: *mut u32) -> u32;
    fn RegisterRawInputDevices(devices: *const RawInputDevice, count: u32, bytes: u32) -> i32;
    fn GetRawInputData(
        raw: Hrawinput,
        command: u32,
        data: *mut c_void,
        size: *mut u32,
        header_size: u32,
    ) -> u32;
    fn SetWinEventHook(
        event_min: u32,
        event_max: u32,
        module: Hwnd,
        callback: Option<WinEventCallback>,
        process_id: u32,
        thread_id: u32,
        flags: u32,
    ) -> HwinEventHook;
    fn UnhookWinEvent(hook: HwinEventHook) -> i32;
    fn GetAsyncKeyState(key: i32) -> i16;
    fn OpenInputDesktop(flags: u32, inherit: i32, desired_access: u32) -> Hdesktop;
    fn CloseDesktop(desktop: Hdesktop) -> i32;
    fn GetUserObjectInformationW(
        object: Hdesktop,
        index: i32,
        information: *mut c_void,
        length: u32,
        needed: *mut u32,
    ) -> i32;
    fn SetThreadDpiAwarenessContext(context: Hwnd) -> Hwnd;
    fn GetThreadDpiAwarenessContext() -> Hwnd;
    fn AreDpiAwarenessContextsEqual(first: Hwnd, second: Hwnd) -> i32;
}

#[cfg(target_pointer_width = "64")]
#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowLongPtrW(window: Hwnd, index: i32, value: isize) -> isize;
    fn GetWindowLongPtrW(window: Hwnd, index: i32) -> isize;
}

#[cfg(target_pointer_width = "32")]
#[link(name = "user32")]
unsafe extern "system" {
    #[link_name = "SetWindowLongW"]
    fn SetWindowLongPtrW(window: Hwnd, index: i32, value: isize) -> isize;
    #[link_name = "GetWindowLongW"]
    fn GetWindowLongPtrW(window: Hwnd, index: i32) -> isize;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(module: *const u16) -> Hwnd;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmGetWindowAttribute(window: Hwnd, attribute: u32, value: *mut c_void, size: u32) -> i32;
}

#[link(name = "wtsapi32")]
unsafe extern "system" {
    fn WTSRegisterSessionNotification(window: Hwnd, flags: u32) -> i32;
    fn WTSUnRegisterSessionNotification(window: Hwnd) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_overlap_is_per_output_and_requires_ninety_eight_percent() {
        let output = NativeRect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
        };
        let full = WindowRectangle {
            window: WindowIdentity::new(1, Generation::INITIAL),
            display: None,
            left: -1920,
            top: 0,
            width: 1920,
            height: 1080,
            observed_at_ms: 1,
        };
        assert!(fullscreen_overlap(full, output));

        let partial = WindowRectangle {
            width: 1870,
            ..full
        };
        assert!(!fullscreen_overlap(partial, output));

        let other_output = NativeRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        assert!(!fullscreen_overlap(full, other_output));
    }

    #[test]
    fn desktop_hit_requires_verified_shell_surface_and_positive_blank_hit_test() {
        assert_eq!(
            classify_shell_surface(true, true, false, false, None),
            DesktopHitClassification::EmptyWallpaper
        );
        assert_eq!(
            classify_shell_surface(true, false, true, true, Some(-1)),
            DesktopHitClassification::EmptyWallpaper
        );
        assert_eq!(
            classify_shell_surface(true, false, true, true, Some(0)),
            DesktopHitClassification::Occupied
        );
        assert_eq!(
            classify_shell_surface(true, false, true, true, None),
            DesktopHitClassification::Unknown
        );
        assert_eq!(
            classify_shell_surface(true, false, true, false, Some(-1)),
            DesktopHitClassification::Occupied
        );
        assert_eq!(
            classify_shell_surface(false, true, false, false, None),
            DesktopHitClassification::Occupied
        );
    }

    #[test]
    fn foreign_process_desktop_list_hit_is_unknown_without_sending_pointer_message() {
        let mut sent_pointer_message = false;
        let hit = classify_desktop_list_hit(true, false, || {
            sent_pointer_message = true;
            Some(-1)
        });
        assert_eq!(hit, DesktopHitClassification::Unknown);
        assert!(!sent_pointer_message);
        assert_eq!(
            classify_desktop_list_hit(true, true, || Some(-1)),
            DesktopHitClassification::EmptyWallpaper
        );
    }

    #[test]
    fn click_observation_overflow_is_explicit_and_bounded() {
        let display = monitor_identity(
            1,
            1,
            NativeRect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
        )
        .unwrap();
        let mut queue = ClickQueue::new();
        for index in 0..=MAX_DESKTOP_CLICK_OBSERVATIONS {
            queue.push(DesktopClickObservation {
                position: PointerPosition {
                    display,
                    x: index as i32,
                    y: 0,
                    observed_at_ms: index as u64,
                },
                button: PointerButtons::LEFT,
                hit: DesktopHitClassification::Unknown,
                observed_at_ms: index as u64,
            });
        }
        let batch = queue.drain();
        assert_eq!(batch.len, MAX_DESKTOP_CLICK_OBSERVATIONS);
        assert!(batch.overflowed);
    }

    #[test]
    fn native_callback_queue_is_bounded_and_coalesces_moves() {
        let mut queue = NativeQueue::new();
        queue.push(NativeEvent::RawInput {
            button_flags: 0,
            message_time: 1,
            position: None,
        });
        queue.push(NativeEvent::RawInput {
            button_flags: 0,
            message_time: 2,
            position: None,
        });
        assert_eq!(queue.len, 1);
        assert!(matches!(
            queue.pop(),
            Some(NativeEvent::RawInput {
                message_time: 2,
                ..
            })
        ));

        for index in 0..NATIVE_QUEUE_CAPACITY {
            queue.push(NativeEvent::RawInput {
                button_flags: RI_MOUSE_LEFT_BUTTON_DOWN,
                message_time: index as u32,
                position: None,
            });
        }
        queue.push(NativeEvent::RawInput {
            button_flags: RI_MOUSE_LEFT_BUTTON_UP,
            message_time: 500,
            position: None,
        });
        assert_eq!(queue.len, NATIVE_QUEUE_CAPACITY);
        assert!(queue.button_state_uncertain);
        assert!(queue.position_uncertain);
    }

    #[test]
    fn dropped_release_discards_queued_press_and_stays_released() {
        let mut queue = NativeQueue::new();
        queue.push(NativeEvent::RawInput {
            button_flags: RI_MOUSE_LEFT_BUTTON_DOWN,
            message_time: 1,
            position: None,
        });
        for _ in 1..NATIVE_QUEUE_CAPACITY {
            queue.push(NativeEvent::DisplayChange);
        }
        queue.push(NativeEvent::RawInput {
            button_flags: RI_MOUSE_LEFT_BUTTON_UP,
            message_time: 2,
            position: None,
        });
        assert!(queue.button_state_uncertain);

        let queued_press = match queue.pop() {
            Some(NativeEvent::RawInput { button_flags, .. }) => button_flags,
            _ => panic!("queued press must be first"),
        };
        assert_eq!(queued_press, RI_MOUSE_LEFT_BUTTON_DOWN);
        let sampled_buttons = 0;
        let flags = button_flags_for_processing(queue.button_state_uncertain, queued_press);
        let (final_buttons, down) = apply_button_flags(sampled_buttons, flags);
        assert_eq!(final_buttons, 0);
        assert_eq!(down, [None; 5]);
    }

    #[test]
    fn bounds_require_positive_representable_dimensions() {
        assert_eq!(
            NativeRect {
                left: -1920,
                top: -200,
                right: 0,
                bottom: 880,
            }
            .dimensions(),
            Some((1920, 1080))
        );
        assert!(
            NativeRect {
                left: 5,
                top: 0,
                right: 5,
                bottom: 100,
            }
            .dimensions()
            .is_none()
        );
    }
}
