//! Fixed-capacity host event records and overflow/reconciliation behavior.

use ascii_core::{Generation, MonitorIdentity};

use crate::{
    CapabilityChange, CapabilitySnapshot, HostIdentity, SurfaceLease, SurfaceRetirementReason,
};

/// Hard upper bound on events retained by one poll batch or queue.
pub const MAX_HOST_EVENTS: usize = 32;

/// Ephemeral, privacy-minimal identity for a window observation; never a native window handle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WindowIdentity {
    value: u64,
    generation: Generation,
}

impl WindowIdentity {
    /// Creates an adapter-local token whose generation must change when an OS handle is reused.
    #[must_use]
    pub const fn new(value: u64, generation: Generation) -> Self {
        Self { value, generation }
    }

    /// Adapter-local opaque token value, not an OS handle.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.value
    }

    /// Generation distinguishing reuse of an adapter-local token.
    #[must_use]
    pub const fn generation(self) -> Generation {
        self.generation
    }
}

/// Position observation in signed virtual-desktop pixel coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PointerPosition {
    /// Display identity used to interpret the coordinate space.
    pub display: MonitorIdentity,
    /// Signed horizontal virtual-desktop coordinate.
    pub x: i32,
    /// Signed vertical virtual-desktop coordinate.
    pub y: i32,
    /// Monotonic observation time in milliseconds.
    pub observed_at_ms: u64,
}

/// Known passive pointer buttons. Unknown bits are rejected by `new`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PointerButtons(u8);

impl PointerButtons {
    /// Left button is down.
    pub const LEFT: u8 = 1 << 0;
    /// Right button is down.
    pub const RIGHT: u8 = 1 << 1;
    /// Middle button is down.
    pub const MIDDLE: u8 = 1 << 2;
    /// First auxiliary button is down.
    pub const AUXILIARY_1: u8 = 1 << 3;
    /// Second auxiliary button is down.
    pub const AUXILIARY_2: u8 = 1 << 4;
    const KNOWN_BITS: u8 = (1 << 5) - 1;

    /// Creates a button state when all set bits are defined.
    #[must_use]
    pub const fn new(bits: u8) -> Option<Self> {
        if bits & !Self::KNOWN_BITS == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// Returns whether the selected button is down.
    #[must_use]
    pub const fn contains(self, button: u8) -> bool {
        self.0 & button != 0
    }

    /// Returns all currently-down button bits.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

/// One observed pointer-button state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PointerButtonsUpdate {
    /// Current known button state; zero bits is a valid empty state.
    pub buttons: PointerButtons,
    /// Monotonic observation time in milliseconds.
    pub observed_at_ms: u64,
}

/// Coarse window state, intentionally separate from rectangle availability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowState {
    /// The window is in its normal state.
    Normal,
    /// The window is minimized.
    Minimized,
    /// The window is maximized.
    Maximized,
    /// The window is fullscreen.
    Fullscreen,
    /// The window is hidden.
    Hidden,
    /// The host has a state value outside this portable set.
    Other,
}

/// Window rectangle in signed virtual-desktop pixel coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowRectangle {
    /// Ephemeral token; this structure carries no title, process path, or native handle.
    pub window: WindowIdentity,
    /// Display identity when the host can assign one unambiguously.
    pub display: Option<MonitorIdentity>,
    /// Signed left coordinate.
    pub left: i32,
    /// Signed top coordinate.
    pub top: i32,
    /// Width in pixels; zero is representable and is not evidence of unknown geometry.
    pub width: u32,
    /// Height in pixels; zero is representable and is not evidence of unknown geometry.
    pub height: u32,
    /// Monotonic observation time in milliseconds.
    pub observed_at_ms: u64,
}

/// Lock state for the current user session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLockState {
    /// The interactive session is locked.
    Locked,
    /// The interactive session is unlocked.
    Unlocked,
}

/// Battery/power feed values; each optional field remains unknown rather than fabricated as zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatteryUpdate {
    /// Battery charge percentage, if the host reports it (including valid zero).
    pub charge_percent: Option<u8>,
    /// Whether the battery is charging, if known.
    pub charging: Option<bool>,
    /// Whether external power is connected, if known.
    pub external_power: Option<bool>,
    /// Monotonic observation time in milliseconds.
    pub observed_at_ms: u64,
}

/// A feed-specific data observation, bounded to one value per event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeedEvent {
    /// A pointer position; event presence indicates a valid coordinate, including `(0, 0)`.
    PointerPosition(PointerPosition),
    /// Current passive pointer button state; an empty bit set is valid.
    PointerButtons(PointerButtonsUpdate),
    /// A single window geometry observation.
    WindowRectangle(WindowRectangle),
    /// A coarse window-state observation without requiring geometry.
    WindowState {
        /// Ephemeral token; no native handle is exposed.
        window: WindowIdentity,
        /// Observed state.
        state: WindowState,
        /// Monotonic observation time in milliseconds.
        observed_at_ms: u64,
    },
    /// User-session lock state.
    SessionLock {
        /// Current lock state.
        state: SessionLockState,
        /// Monotonic observation time in milliseconds.
        observed_at_ms: u64,
    },
    /// Current battery and external-power observation.
    Battery(BatteryUpdate),
    /// Scale for one display in milli-units (1000 means 1.0 scale).
    OutputScale {
        /// Output whose scale was measured.
        display: MonitorIdentity,
        /// Scale multiplied by 1000; zero is preserved as reported data, not `Unknown`.
        scale_milli: u32,
        /// Monotonic observation time in milliseconds.
        observed_at_ms: u64,
    },
    /// Static-wallpaper setting operation state, never a live-placement claim.
    StaticWallpaper {
        /// Display affected when the host can identify one.
        display: Option<MonitorIdentity>,
        /// Result of the OS-owned static-wallpaper operation.
        state: StaticWallpaperUpdate,
        /// Monotonic observation time in milliseconds.
        observed_at_ms: u64,
    },
    /// Tray integration availability or loss.
    Tray {
        /// Whether the tray facility is currently available.
        available: bool,
        /// Monotonic observation time in milliseconds.
        observed_at_ms: u64,
    },
}

/// Maximum distinct current feed values retained by a reconciliation snapshot.
pub const MAX_RECONCILIATION_FEEDS: usize = MAX_HOST_EVENTS;
/// Maximum active surface leases retained by a reconciliation snapshot.
pub const MAX_RECONCILIATION_LEASES: usize = MAX_HOST_EVENTS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FeedKey {
    PointerPosition,
    PointerButtons,
    WindowRectangle(WindowIdentity),
    WindowState(WindowIdentity),
    SessionLock,
    Battery,
    OutputScale(MonitorIdentity),
    StaticWallpaper(Option<MonitorIdentity>),
    Tray,
}

impl FeedEvent {
    const fn key(self) -> FeedKey {
        match self {
            Self::PointerPosition(_) => FeedKey::PointerPosition,
            Self::PointerButtons(_) => FeedKey::PointerButtons,
            Self::WindowRectangle(value) => FeedKey::WindowRectangle(value.window),
            Self::WindowState { window, .. } => FeedKey::WindowState(window),
            Self::SessionLock { .. } => FeedKey::SessionLock,
            Self::Battery(_) => FeedKey::Battery,
            Self::OutputScale { display, .. } => FeedKey::OutputScale(display),
            Self::StaticWallpaper { display, .. } => FeedKey::StaticWallpaper(display),
            Self::Tray { .. } => FeedKey::Tray,
        }
    }
}

/// Why a bounded authoritative reconciliation snapshot could not retain a supplied value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostStateSnapshotError {
    /// More distinct current feed values were supplied than the fixed capacity permits.
    FeedCapacityExhausted,
    /// More active surface leases were supplied than the fixed capacity permits.
    LeaseCapacityExhausted,
    /// A lease from another host was supplied to this snapshot.
    LeaseOwnerMismatch,
}

/// Allocation-free authoritative current host state at one capability revision.
///
/// Feed values are unique by feed and logical key; setting an existing key replaces its current
/// value. Lease values are unique by display and setting an existing display replaces its lease.
/// Adapters must populate every currently known value and active lease. A failed capacity insert
/// marks that category truncated, and the snapshot must not be treated as complete.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostStateSnapshot {
    capabilities: CapabilitySnapshot,
    feeds: [Option<FeedEvent>; MAX_RECONCILIATION_FEEDS],
    feed_count: u8,
    leases: [Option<SurfaceLease>; MAX_RECONCILIATION_LEASES],
    lease_count: u8,
    feeds_truncated: bool,
    leases_truncated: bool,
}

impl HostStateSnapshot {
    /// Creates an empty snapshot tied to one exact capability revision and host identity.
    #[must_use]
    pub const fn new(capabilities: CapabilitySnapshot) -> Self {
        Self {
            capabilities,
            feeds: [None; MAX_RECONCILIATION_FEEDS],
            feed_count: 0,
            leases: [None; MAX_RECONCILIATION_LEASES],
            lease_count: 0,
            feeds_truncated: false,
            leases_truncated: false,
        }
    }

    /// Capability metadata, host identity, and revision captured with this state.
    #[must_use]
    pub const fn capabilities(&self) -> &CapabilitySnapshot {
        &self.capabilities
    }

    /// Host identity that owns the feed and lease values.
    #[must_use]
    pub const fn host(&self) -> HostIdentity {
        self.capabilities.host()
    }

    /// Revision that covers the capability metadata and every supplied current value.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.capabilities.revision()
    }

    /// Number of distinct retained current feed values.
    #[must_use]
    pub const fn feed_count(&self) -> usize {
        self.feed_count as usize
    }

    /// Number of retained active surface leases.
    #[must_use]
    pub const fn lease_count(&self) -> usize {
        self.lease_count as usize
    }

    /// Whether the current feed set exceeded its fixed capacity.
    #[must_use]
    pub const fn feeds_truncated(&self) -> bool {
        self.feeds_truncated
    }

    /// Whether the active lease set exceeded its fixed capacity.
    #[must_use]
    pub const fn leases_truncated(&self) -> bool {
        self.leases_truncated
    }

    /// Whether every supplied feed and lease fit without truncation.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        !self.feeds_truncated && !self.leases_truncated
    }

    /// Iterates retained current feed values without allocation.
    pub fn feeds(&self) -> impl Iterator<Item = &FeedEvent> {
        self.feeds[..usize::from(self.feed_count)].iter().flatten()
    }

    /// Iterates retained active leases without allocation.
    pub fn leases(&self) -> impl Iterator<Item = &SurfaceLease> {
        self.leases[..usize::from(self.lease_count)]
            .iter()
            .flatten()
    }

    /// Inserts or replaces the current value for a feed key without allocating.
    pub fn set_feed(&mut self, feed: FeedEvent) -> Result<(), HostStateSnapshotError> {
        let key = feed.key();
        let mut index = 0;
        while index < usize::from(self.feed_count) {
            match self.feeds[index] {
                Some(current) if current.key() == key => {
                    self.feeds[index] = Some(feed);
                    return Ok(());
                }
                _ => {}
            }
            index += 1;
        }
        if usize::from(self.feed_count) == MAX_RECONCILIATION_FEEDS {
            self.feeds_truncated = true;
            return Err(HostStateSnapshotError::FeedCapacityExhausted);
        }
        self.feeds[usize::from(self.feed_count)] = Some(feed);
        self.feed_count += 1;
        Ok(())
    }

    /// Inserts or replaces the active lease for one display without allocating.
    pub fn set_lease(&mut self, lease: SurfaceLease) -> Result<(), HostStateSnapshotError> {
        if lease.owner() != self.host() {
            return Err(HostStateSnapshotError::LeaseOwnerMismatch);
        }
        let mut index = 0;
        while index < usize::from(self.lease_count) {
            match self.leases[index] {
                Some(current) if current.display() == lease.display() => {
                    self.leases[index] = Some(lease);
                    return Ok(());
                }
                _ => {}
            }
            index += 1;
        }
        if usize::from(self.lease_count) == MAX_RECONCILIATION_LEASES {
            self.leases_truncated = true;
            return Err(HostStateSnapshotError::LeaseCapacityExhausted);
        }
        self.leases[usize::from(self.lease_count)] = Some(lease);
        self.lease_count += 1;
        Ok(())
    }
}

/// Result of an OS static-wallpaper operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticWallpaperUpdate {
    /// The OS wallpaper setting was applied.
    Applied,
    /// The OS wallpaper setting was cleared.
    Cleared,
    /// The OS rejected or failed the operation.
    Failed,
}

/// Contract-level host event. No variant transfers ownership of an OS resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostEvent {
    /// One capability's effective state or an explicitly declared dependent changed.
    CapabilityChanged(CapabilityChange),
    /// Aggregate host health changed; feed-specific capabilities remain separate.
    HealthChanged {
        /// Previous aggregate host health.
        previous: crate::HostHealth,
        /// Current aggregate host health.
        current: crate::HostHealth,
        /// Snapshot revision associated with the update.
        revision: u64,
    },
    /// A new presentation lease became active.
    SurfaceAttached(SurfaceLease),
    /// A presentation lease was retired before native destruction/replacement.
    SurfaceRetired {
        /// Exact token that has ceased to authorize presentation.
        lease: SurfaceLease,
        /// Lifecycle cause reported by the host.
        reason: SurfaceRetirementReason,
    },
    /// One bounded data observation from a capability feed; revoked feeds must stop emitting.
    Feed(FeedEvent),
}

/// Fixed-capacity event batch returned from `HostAdapter::poll`.
#[derive(Debug, Eq, PartialEq)]
pub struct HostEventBatch {
    events: [Option<HostEvent>; MAX_HOST_EVENTS],
    length: u8,
    reconciliation_revision: Option<u64>,
}

impl HostEventBatch {
    const fn empty() -> Self {
        Self {
            events: [None; MAX_HOST_EVENTS],
            length: 0,
            reconciliation_revision: None,
        }
    }

    /// Number of retained events.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.length as usize
    }

    /// Whether the batch contains no events.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Revision the consumer must reconcile against after an event overflow, if any.
    #[must_use]
    pub const fn reconciliation_revision(&self) -> Option<u64> {
        self.reconciliation_revision
    }

    /// Iterates retained events in FIFO order without allocation.
    pub fn iter(&self) -> impl Iterator<Item = &HostEvent> {
        self.events[..usize::from(self.length)].iter().flatten()
    }
}

/// Fixed-size queue suitable for adapter callbacks that cannot allocate or block on growth.
#[derive(Debug, Eq, PartialEq)]
pub struct HostEventQueue {
    events: [Option<HostEvent>; MAX_HOST_EVENTS],
    head: u8,
    length: u8,
    reconciliation_revision: Option<u64>,
}

impl HostEventQueue {
    /// Creates an empty queue.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            events: [None; MAX_HOST_EVENTS],
            head: 0,
            length: 0,
            reconciliation_revision: None,
        }
    }

    /// Returns the number of queued events.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.length as usize
    }

    /// Whether the queue is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Enqueues without allocating. When full, drops the new event and requests reconciliation.
    ///
    /// `snapshot_revision` must be the latest adapter revision that covers the dropped update.
    /// The caller must publish this revision with its authoritative snapshot before treating the
    /// feed as reconciled.
    pub fn push(&mut self, event: HostEvent, snapshot_revision: u64) -> bool {
        if usize::from(self.length) == MAX_HOST_EVENTS {
            self.reconciliation_revision = Some(
                self.reconciliation_revision
                    .map_or(snapshot_revision, |current| current.max(snapshot_revision)),
            );
            return false;
        }
        let index = (usize::from(self.head) + usize::from(self.length)) % MAX_HOST_EVENTS;
        self.events[index] = Some(event);
        self.length += 1;
        true
    }

    /// Drains the queue into a fixed batch and transfers any reconciliation requirement.
    pub fn drain(&mut self) -> HostEventBatch {
        let mut batch = HostEventBatch::empty();
        while self.length != 0 {
            let index = usize::from(self.head);
            batch.events[usize::from(batch.length)] = self.events[index].take();
            batch.length += 1;
            self.head = ((index + 1) % MAX_HOST_EVENTS) as u8;
            self.length -= 1;
        }
        self.head = 0;
        batch.reconciliation_revision = self.reconciliation_revision.take();
        batch
    }
}

impl Default for HostEventQueue {
    fn default() -> Self {
        Self::new()
    }
}
