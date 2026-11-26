//! Typed portable events and an allocation-free bounded event queue.
//!
//! Event timestamps are expressed in the receiving process's monotonic epoch. Platform adapters
//! map foreign clocks before constructing an envelope; this module performs no OS calls.

use crate::{Generation, MonitorId};

/// Opaque identity for one event-producing source.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EventSourceId(u64);

impl EventSourceId {
    /// Creates an ID from a caller-owned source counter.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying source counter.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Opaque ID for one monotonic epoch in the receiving process.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MonotonicEpoch(u64);

impl MonotonicEpoch {
    /// Creates an epoch value supplied by the receiving process.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the epoch counter.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Advances the epoch without wrapping and revalidating stale timestamps.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

/// Receiving-process monotonic time in nanoseconds.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MonotonicInstant {
    epoch: MonotonicEpoch,
    nanos: u64,
}

impl MonotonicInstant {
    /// Creates an instant in a caller-selected receiving-clock epoch.
    #[must_use]
    pub const fn new(epoch: MonotonicEpoch, nanos: u64) -> Self {
        Self { epoch, nanos }
    }

    /// Returns the clock epoch.
    #[must_use]
    pub const fn epoch(self) -> MonotonicEpoch {
        self.epoch
    }

    /// Returns elapsed nanoseconds, only when both instants belong to the same epoch and are ordered.
    #[must_use]
    pub const fn duration_since(self, earlier: Self) -> Option<u64> {
        if self.epoch.0 == earlier.epoch.0 {
            self.nanos.checked_sub(earlier.nanos)
        } else {
            None
        }
    }

    /// Returns the raw nanosecond count in this epoch.
    #[must_use]
    pub const fn nanos(self) -> u64 {
        self.nanos
    }
}

/// Sequence number assigned by one source; values must not wrap during that source's lifetime.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EventSequence(u64);

impl EventSequence {
    /// Creates a sequence value from a caller-owned monotonic counter.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the sequence value.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
    /// Returns the next source sequence, or `None` rather than wrapping.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

/// Authorization scope attached to an event, independent of the source that emitted it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CapabilityScope {
    /// The event is authorized at the whole-application boundary.
    Application,
    /// The event is authorized for one caller-assigned session.
    Session(u64),
    /// The event is authorized for one local output identity.
    Output(MonitorId),
    /// The event is authorized only for one event source.
    Source(EventSourceId),
}

/// Why an observation has no value; absence must never be interpreted as origin or empty geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MissingReason {
    /// The platform or user denied access to the value.
    PermissionDenied,
    /// The host does not implement this observation.
    Unsupported,
    /// The source has not reported a value yet.
    NotObserved,
    /// The source temporarily cannot provide a value.
    TemporarilyUnavailable,
    /// Privacy policy intentionally omitted the value.
    Redacted,
    /// The source supplied a malformed value.
    Invalid,
}

/// A value or an explicit reason it is absent.
///
/// Constructors enforce the invariant that a known value has no absence reason and a missing
/// value always has one. Callers can inspect the `Option` without ever receiving fabricated data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation<T> {
    value: Option<T>,
    reason: Option<MissingReason>,
}

impl<T> Observation<T> {
    /// Records a known value.
    #[must_use]
    pub const fn known(value: T) -> Self {
        Self {
            value: Some(value),
            reason: None,
        }
    }

    /// Records absence with a required reason.
    #[must_use]
    pub const fn missing(reason: MissingReason) -> Self {
        Self {
            value: None,
            reason: Some(reason),
        }
    }

    /// Returns the observed value, or `None` when it is unavailable.
    #[must_use]
    pub const fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// Returns the absence reason, or `None` for a known value.
    #[must_use]
    pub const fn reason(&self) -> Option<MissingReason> {
        self.reason
    }

    /// Returns whether this observation contains a known value.
    #[must_use]
    pub const fn is_known(&self) -> bool {
        self.value.is_some()
    }
}

/// Signed global desktop coordinates in the host's documented logical-pixel space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesktopPoint {
    x: f64,
    y: f64,
}

impl DesktopPoint {
    /// Creates a finite desktop position; negative coordinates are valid.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Option<Self> {
        if x.is_finite() && y.is_finite() {
            Some(Self { x, y })
        } else {
            None
        }
    }

    /// Returns the signed horizontal coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Returns the signed vertical coordinate.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.y
    }
}

/// Signed, positive-area rectangle in the host's desktop logical coordinate space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesktopRect {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

impl DesktopRect {
    /// Creates a finite positive-area rectangle without overflowing its right or bottom edge.
    #[must_use]
    pub const fn new(left: f64, top: f64, width: f64, height: f64) -> Option<Self> {
        let right = left + width;
        let bottom = top + height;
        if left.is_finite()
            && top.is_finite()
            && width.is_finite()
            && height.is_finite()
            && width > 0.0
            && height > 0.0
            && right.is_finite()
            && bottom.is_finite()
        {
            Some(Self {
                left,
                top,
                width,
                height,
            })
        } else {
            None
        }
    }

    /// Returns the signed left edge.
    #[must_use]
    pub const fn left(self) -> f64 {
        self.left
    }

    /// Returns the signed top edge.
    #[must_use]
    pub const fn top(self) -> f64 {
        self.top
    }

    /// Returns the positive width.
    #[must_use]
    pub const fn width(self) -> f64 {
        self.width
    }

    /// Returns the positive height.
    #[must_use]
    pub const fn height(self) -> f64 {
        self.height
    }

    /// Returns whether a desktop point lies in this half-open rectangle.
    #[must_use]
    pub const fn contains(self, point: DesktopPoint) -> bool {
        point.x >= self.left
            && point.y >= self.top
            && point.x < self.left + self.width
            && point.y < self.top + self.height
    }
}

/// Pointer motion with an explicit unavailable-position reason when the host cannot provide it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerMotion {
    /// Last observed pointer position in signed desktop logical coordinates.
    pub position: Observation<DesktopPoint>,
}

/// A supported pointer button identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PointerButton {
    /// Primary/left button.
    Primary,
    /// Secondary/right button.
    Secondary,
    /// Middle button.
    Middle,
    /// Adapter-defined auxiliary button identifier.
    Auxiliary(u8),
}

/// A physical button edge; both press and release are non-coalescible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerButtonEvent {
    /// Button whose state changed.
    pub button: PointerButton,
    /// New edge state.
    pub state: ButtonState,
    /// Position at the edge, or an explicit absence reason.
    pub position: Observation<DesktopPoint>,
}

/// State represented by a pointer-button edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonState {
    /// The button was pressed.
    Pressed,
    /// The button was released.
    Released,
}

/// Enter/leave boundary with an explicit position observation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerBoundary {
    /// Position at the boundary, when available.
    pub position: Observation<DesktopPoint>,
}

/// Opaque ephemeral identity for one privacy-minimal window observation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WindowId(u64);

impl WindowId {
    /// Creates an adapter-local window identity; it is not a native window handle.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the adapter-local identity value.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// One last-good window observation; unavailable bounds retain their reason.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowSnapshot {
    /// Ephemeral window identity, if the host is allowed to report one.
    pub window: Observation<WindowId>,
    /// Window rectangle, if the host can observe it.
    pub bounds: Observation<DesktopRect>,
}

/// A validated transition between complete topology generations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TopologyChange {
    previous: Generation,
    current: Generation,
}

impl TopologyChange {
    /// Creates a strictly forward topology transition.
    #[must_use]
    pub const fn new(previous: Generation, current: Generation) -> Option<Self> {
        if current.value() > previous.value() {
            Some(Self { previous, current })
        } else {
            None
        }
    }

    /// Returns the previously committed generation.
    #[must_use]
    pub const fn previous(self) -> Generation {
        self.previous
    }

    /// Returns the newly committed generation.
    #[must_use]
    pub const fn current(self) -> Generation {
        self.current
    }
}

/// Host power policy whose state changed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerPolicy {
    /// Display power-saving policy.
    DisplaySleep,
    /// System sleep policy.
    SystemSleep,
    /// Battery-saver policy.
    BatterySaver,
}

/// A typed power-policy transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowerPolicyEvent {
    /// Policy that changed.
    pub policy: PowerPolicy,
    /// Whether that policy is currently active.
    pub enabled: bool,
}

/// Why a source can no longer produce trusted events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceInvalidationReason {
    /// Its permission or capability was revoked.
    CapabilityRevoked,
    /// The source was explicitly removed.
    Removed,
    /// The source restarted and its old timestamps are invalid.
    Restarted,
    /// The source clock discontinuity began a new epoch.
    ClockDiscontinuity,
}

/// Explicit command from a user or session controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserCommand {
    /// Begin or resume active processing.
    Start,
    /// Pause processing; subsequent resume must reject pre-resume clicks.
    Pause,
    /// Resume processing at this event's monotonic instant.
    Resume,
    /// Stop processing; this command is never coalesced.
    Stop,
}

/// Event data; only pointer motion may be coalesced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventPayload {
    /// Latest pointer position sample.
    PointerMotion(PointerMotion),
    /// Non-coalescible button edge.
    PointerButton(PointerButtonEvent),
    /// Pointer entered the tracked region.
    PointerEnter(PointerBoundary),
    /// Pointer left the tracked region.
    PointerLeave(PointerBoundary),
    /// Window geometry/state observation.
    WindowSnapshot(WindowSnapshot),
    /// Complete topology replacement boundary.
    TopologyChanged(TopologyChange),
    /// Power policy transition.
    PowerPolicy(PowerPolicyEvent),
    /// Source no longer produces trusted events.
    SourceInvalidated(SourceInvalidationReason),
    /// Explicit user/session command.
    UserCommand(UserCommand),
}

impl EventPayload {
    /// Returns whether this event can be safely replaced by an adjacent newer motion sample.
    #[must_use]
    pub const fn is_coalescible_motion(self) -> bool {
        matches!(self, Self::PointerMotion(_))
    }

    /// Returns whether this is a button press that needs click-freshness checks.
    #[must_use]
    pub const fn is_button_press(self) -> bool {
        matches!(
            self,
            Self::PointerButton(PointerButtonEvent {
                state: ButtonState::Pressed,
                ..
            })
        )
    }
}

/// Immutable event identity, time, authorization scope, generation and typed payload.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventEnvelope {
    /// Source that observed or requested the event.
    pub source: EventSourceId,
    /// Receiving-process monotonic timestamp and epoch.
    pub timestamp: MonotonicInstant,
    /// Monotonic sequence assigned by this source.
    pub sequence: EventSequence,
    /// Source/assignment generation captured when the event was created.
    pub generation: Generation,
    /// Capability scope under which this event was observed.
    pub scope: CapabilityScope,
    /// Typed event payload.
    pub payload: EventPayload,
}

impl EventEnvelope {
    /// Constructs a complete event envelope.
    #[must_use]
    pub const fn new(
        source: EventSourceId,
        timestamp: MonotonicInstant,
        sequence: EventSequence,
        generation: Generation,
        scope: CapabilityScope,
        payload: EventPayload,
    ) -> Self {
        Self {
            source,
            timestamp,
            sequence,
            generation,
            scope,
            payload,
        }
    }
}

/// Counts queue pressure without silently hiding lost/coalesced motion or refused critical events.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EventOverflowCounters {
    /// Adjacent motion samples replaced by a newer sample from the same source context.
    pub coalesced_motion: u64,
    /// Incoming motion samples dropped because no free slot or adjacent coalescing slot is available.
    pub dropped_motion: u64,
    /// Older queued motion samples evicted to admit a non-coalescible event.
    pub evicted_motion: u64,
    /// Critical events returned to the caller because no motion could be evicted.
    pub critical_backpressure: u64,
    /// Events returned to the caller because their timestamp precedes the queue tail.
    pub out_of_order: u64,
}

impl EventOverflowCounters {
    fn increment(counter: &mut u64) {
        *counter = counter.saturating_add(1);
    }
}

/// Outcome of inserting or coalescing an event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventPushOutcome {
    /// Event appended without dropping any queued data.
    Queued,
    /// Newer adjacent motion replaced the queue tail's same-source motion.
    CoalescedMotion,
    /// New critical event was admitted after evicting one queued motion.
    QueuedAfterEvictingMotion,
    /// New motion was dropped due to capacity; the overflow counter records it.
    DroppedMotion,
}

/// Why a critical event was returned instead of being admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventPushErrorKind {
    /// No slot is available and no queued motion can be evicted; caller must drain/retry.
    FullOfCriticalEvents,
    /// Event time precedes the current queue tail; caller must preserve order and retry/drop by policy.
    OutOfOrder,
}

/// Refused event; ownership is returned so a critical edge/stop/topology transition is never lost.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventPushError {
    /// Event that was not admitted.
    pub event: EventEnvelope,
    /// Reason insertion was refused.
    pub kind: EventPushErrorKind,
}

/// Fixed-capacity FIFO for already-clock-mapped events.
///
/// Adjacent same-source motion is replaced in place. A full queue may discard motion to preserve
/// critical-event capacity, but never silently discards a button edge, topology change, or Stop:
/// if no motion can be evicted, `push` returns the original event for caller backpressure/retry.
/// Events must be submitted in nondecreasing monotonic timestamp order. After retrying a returned
/// critical event, the producer must not submit later events first.
#[derive(Clone, Debug, PartialEq)]
pub struct EventQueue<const CAPACITY: usize> {
    entries: [Option<EventEnvelope>; CAPACITY],
    head: usize,
    len: usize,
    overflow: EventOverflowCounters,
}

impl<const CAPACITY: usize> EventQueue<CAPACITY> {
    /// Creates an empty event queue with compile-time fixed capacity.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [None; CAPACITY],
            head: 0,
            len: 0,
            overflow: EventOverflowCounters {
                coalesced_motion: 0,
                dropped_motion: 0,
                evicted_motion: 0,
                critical_backpressure: 0,
                out_of_order: 0,
            },
        }
    }

    /// Returns the number of queued events.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns whether no events are queued.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the fixed queue capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        CAPACITY
    }

    /// Returns a snapshot of saturating overflow/coalescing counters.
    #[must_use]
    pub const fn overflow_counters(&self) -> EventOverflowCounters {
        self.overflow
    }

    /// Enqueues an event, coalescing adjacent motion and returning unadmitted critical events.
    pub fn push(&mut self, event: EventEnvelope) -> Result<EventPushOutcome, EventPushError> {
        if CAPACITY == 0 {
            if event.payload.is_coalescible_motion() {
                EventOverflowCounters::increment(&mut self.overflow.dropped_motion);
                return Ok(EventPushOutcome::DroppedMotion);
            }
            return self.reject_full(event);
        }

        if let Some(tail) = self.tail() {
            if event.timestamp < tail.timestamp {
                EventOverflowCounters::increment(&mut self.overflow.out_of_order);
                return Err(EventPushError {
                    event,
                    kind: EventPushErrorKind::OutOfOrder,
                });
            }
            if event.payload.is_coalescible_motion()
                && tail.payload.is_coalescible_motion()
                && event.source == tail.source
                && event.generation == tail.generation
                && event.scope == tail.scope
            {
                let tail_index = self.physical_index(self.len - 1);
                self.entries[tail_index] = Some(event);
                EventOverflowCounters::increment(&mut self.overflow.coalesced_motion);
                return Ok(EventPushOutcome::CoalescedMotion);
            }
        }

        if self.len < CAPACITY {
            self.append(event);
            return Ok(EventPushOutcome::Queued);
        }

        if event.payload.is_coalescible_motion() {
            EventOverflowCounters::increment(&mut self.overflow.dropped_motion);
            return Ok(EventPushOutcome::DroppedMotion);
        }

        if let Some(motion_index) = self.oldest_motion_index() {
            self.remove_at(motion_index);
            EventOverflowCounters::increment(&mut self.overflow.evicted_motion);
            self.append(event);
            return Ok(EventPushOutcome::QueuedAfterEvictingMotion);
        }

        self.reject_full(event)
    }

    /// Removes and returns the oldest event.
    pub fn pop(&mut self) -> Option<EventEnvelope> {
        if self.len == 0 {
            return None;
        }
        let event = self.entries[self.head].take();
        self.len -= 1;
        let next_head = self.head + 1;
        self.head = if next_head == CAPACITY {
            0
        } else {
            next_head
        };
        event
    }

    /// Returns the oldest event without removing it.
    #[must_use]
    pub fn front(&self) -> Option<&EventEnvelope> {
        if self.len == 0 {
            None
        } else {
            self.entries[self.head].as_ref()
        }
    }

    fn reject_full(
        &mut self,
        event: EventEnvelope,
    ) -> Result<EventPushOutcome, EventPushError> {
        EventOverflowCounters::increment(&mut self.overflow.critical_backpressure);
        Err(EventPushError {
            event,
            kind: EventPushErrorKind::FullOfCriticalEvents,
        })
    }

    fn tail(&self) -> Option<EventEnvelope> {
        if self.len == 0 {
            None
        } else {
            self.entries[self.physical_index(self.len - 1)]
        }
    }

    fn append(&mut self, event: EventEnvelope) {
        let index = self.physical_index(self.len);
        self.entries[index] = Some(event);
        self.len += 1;
    }

    fn physical_index(&self, logical_index: usize) -> usize {
        let index = self.head + logical_index;
        if index < CAPACITY {
            index
        } else {
            index - CAPACITY
        }
    }
    fn oldest_motion_index(&self) -> Option<usize> {
        let mut logical_index = 0;
        while logical_index < self.len {
            let event = self.entries[self.physical_index(logical_index)]?;
            if event.payload.is_coalescible_motion() {
                return Some(logical_index);
            }
            logical_index += 1;
        }
        None
    }

    fn remove_at(&mut self, logical_index: usize) {
        let mut index = logical_index;
        while index + 1 < self.len {
            let current = self.physical_index(index);
            let next = self.physical_index(index + 1);
            self.entries[current] = self.entries[next].take();
            index += 1;
        }
        let last = self.physical_index(self.len - 1);
        self.entries[last] = None;
        self.len -= 1;
    }
}

impl<const CAPACITY: usize> Default for EventQueue<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}
