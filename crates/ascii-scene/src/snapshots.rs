/// Portable signal categories; none grants observation or acquisition rights.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SignalKind {
    PointerPosition,
    PointerButtons,
    WindowRectangles,
    BatteryState,
}

/// Maximum pointer-button events retained in one approved snapshot.
pub const MAX_APPROVED_BUTTON_EVENTS: usize = 256;

/// Normalized point in unsigned Q0.16 coordinates, independent of desktop pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalizedPoint {
    x_q16: u16,
    y_q16: u16,
}

impl NormalizedPoint {
    #[must_use]
    pub const fn new(x_q16: u16, y_q16: u16) -> Self {
        Self { x_q16, y_q16 }
    }

    #[must_use]
    pub const fn x_q16(self) -> u16 {
        self.x_q16
    }

    #[must_use]
    pub const fn y_q16(self) -> u16 {
        self.y_q16
    }
}

/// Normalized rectangle in unsigned Q0.16 coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalizedRect {
    x_q16: u16,
    y_q16: u16,
    width_q16: u16,
    height_q16: u16,
}

impl NormalizedRect {
    pub fn new(x_q16: u16, y_q16: u16, width_q16: u16, height_q16: u16) -> Option<Self> {
        if width_q16 == 0
            || height_q16 == 0
            || u32::from(x_q16) + u32::from(width_q16) > u32::from(u16::MAX)
            || u32::from(y_q16) + u32::from(height_q16) > u32::from(u16::MAX)
        {
            return None;
        }
        Some(Self {
            x_q16,
            y_q16,
            width_q16,
            height_q16,
        })
    }

    #[must_use]
    pub const fn x_q16(self) -> u16 {
        self.x_q16
    }

    #[must_use]
    pub const fn y_q16(self) -> u16 {
        self.y_q16
    }

    #[must_use]
    pub const fn width_q16(self) -> u16 {
        self.width_q16
    }

    #[must_use]
    pub const fn height_q16(self) -> u16 {
        self.height_q16
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatterySnapshot {
    percent: u8,
    charging: bool,
}

impl BatterySnapshot {
    pub fn new(percent: u8, charging: bool) -> Option<Self> {
        (percent <= 100).then_some(Self { percent, charging })
    }

    #[must_use]
    pub const fn percent(self) -> u8 {
        self.percent
    }

    #[must_use]
    pub const fn charging(self) -> bool {
        self.charging
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalizedButtonEvent {
    button: u8,
    pressed: bool,
}

impl NormalizedButtonEvent {
    #[must_use]
    pub const fn new(button: u8, pressed: bool) -> Self {
        Self { button, pressed }
    }

    #[must_use]
    pub const fn button(self) -> u8 {
        self.button
    }

    #[must_use]
    pub const fn pressed(self) -> bool {
        self.pressed
    }
}

/// Host-normalized candidate values before the runtime capability gate.
#[derive(Clone, Copy, Debug, Default)]
pub struct SnapshotCandidates<'a> {
    pointer_position: Option<NormalizedPoint>,
    pointer_buttons: Option<&'a [NormalizedButtonEvent]>,
    window_rectangles: Option<&'a [NormalizedRect]>,
    battery_state: Option<BatterySnapshot>,
}

impl<'a> SnapshotCandidates<'a> {
    #[must_use]
    pub const fn new(
        pointer_position: Option<NormalizedPoint>,
        pointer_buttons: Option<&'a [NormalizedButtonEvent]>,
        window_rectangles: Option<&'a [NormalizedRect]>,
        battery_state: Option<BatterySnapshot>,
    ) -> Self {
        Self {
            pointer_position,
            pointer_buttons,
            window_rectangles,
            battery_state,
        }
    }
}

/// Explicit host-approved capabilities; this crate never queries for them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SnapshotCapabilities {
    pointer_position: bool,
    pointer_buttons: bool,
    window_rectangles: bool,
    battery_state: bool,
}

impl SnapshotCapabilities {
    #[must_use]
    pub const fn new(
        pointer_position: bool,
        pointer_buttons: bool,
        window_rectangles: bool,
        battery_state: bool,
    ) -> Self {
        Self {
            pointer_position,
            pointer_buttons,
            window_rectangles,
            battery_state,
        }
    }
}

/// Values retained only when the corresponding host capability was approved.
#[derive(Clone, Copy, Debug, Default)]
pub struct ApprovedSnapshots<'a> {
    pointer_position: Option<NormalizedPoint>,
    pointer_buttons: Option<&'a [NormalizedButtonEvent]>,
    window_rectangles: Option<&'a [NormalizedRect]>,
    battery_state: Option<BatterySnapshot>,
    pointer_buttons_rejected: bool,
}

impl<'a> ApprovedSnapshots<'a> {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            pointer_position: None,
            pointer_buttons: None,
            window_rectangles: None,
            battery_state: None,
            pointer_buttons_rejected: false,
        }
    }

    /// Applies host capabilities and rejects an over-limit button track in its entirety.
    ///
    /// Oversized input is not truncated or retained. Callers can inspect
    /// `pointer_buttons_rejected()` to distinguish rejection from absent or denied input.
    #[must_use]
    pub const fn filter(
        capabilities: SnapshotCapabilities,
        candidates: SnapshotCandidates<'a>,
    ) -> Self {
        let pointer_buttons_rejected = if capabilities.pointer_buttons {
            match candidates.pointer_buttons {
                Some(events) => events.len() > MAX_APPROVED_BUTTON_EVENTS,
                _ => false,
            }
        } else {
            false
        };
        let pointer_buttons = if capabilities.pointer_buttons && !pointer_buttons_rejected {
            candidates.pointer_buttons
        } else {
            None
        };
        Self {
            pointer_position: if capabilities.pointer_position {
                candidates.pointer_position
            } else {
                None
            },
            pointer_buttons,
            pointer_buttons_rejected,
            window_rectangles: if capabilities.window_rectangles {
                candidates.window_rectangles
            } else {
                None
            },
            battery_state: if capabilities.battery_state {
                candidates.battery_state
            } else {
                None
            },
        }
    }

    #[must_use]
    pub const fn pointer_position(self) -> Option<NormalizedPoint> {
        self.pointer_position
    }

    #[must_use]
    pub const fn pointer_buttons(self) -> Option<&'a [NormalizedButtonEvent]> {
        self.pointer_buttons
    }

    #[must_use]
    pub const fn pointer_buttons_rejected(self) -> bool {
        self.pointer_buttons_rejected
    }

    #[must_use]
    pub const fn window_rectangles(self) -> Option<&'a [NormalizedRect]> {
        self.window_rectangles
    }

    #[must_use]
    pub const fn battery_state(self) -> Option<BatterySnapshot> {
        self.battery_state
    }
}
