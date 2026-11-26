//! Consent-gated Windows-only passive pointer and window observations.
//!
//! All observer-specific extensions remain Windows-local until a shared host contract is approved.

use ascii_core::MonitorIdentity;

use crate::{Confidence, PointerPosition, WindowIdentity, WindowRectangle, WindowState};

#[allow(unsafe_code)]
mod windows;
pub use windows::{WindowsObserver, WindowsObserverError};

/// Hard limit for current window metadata exposed by the Windows observer.
pub const MAX_WINDOWS_OBSERVED: usize = 10;
/// Hard limit for per-output fullscreen candidates exposed by the Windows observer.
pub const MAX_FULLSCREEN_OBSERVATIONS: usize = 8;
/// Hard limit for queued desktop-click classifications.
pub const MAX_DESKTOP_CLICK_OBSERVATIONS: usize = 32;

/// Consent is deliberately opt-in; set a grant only after application-level user consent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowsObserverConsent {
    pointer_buttons: bool,
    window_metadata: bool,
    desktop_click_classification: bool,
}

impl WindowsObserverConsent {
    /// No observer feed is enabled.
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            pointer_buttons: false,
            window_metadata: false,
            desktop_click_classification: false,
        }
    }

    /// Approves passive pointer position and mouse-button observation.
    ///
    /// Call only as the result of an explicit application-level consent action.
    #[must_use]
    pub const fn approve_pointer_buttons(mut self) -> Self {
        self.pointer_buttons = true;
        self
    }

    /// Approves passive title-free top-level window metadata.
    ///
    /// Call only as the result of an explicit application-level consent action.
    #[must_use]
    pub const fn approve_window_metadata(mut self) -> Self {
        self.window_metadata = true;
        self
    }

    /// Approves classification of passive button-down observations as empty wallpaper,
    /// occupied, or unknown. This requires both pointer and window consent.
    #[must_use]
    pub const fn approve_desktop_click_classification(mut self) -> Self {
        self.desktop_click_classification = true;
        self
    }

    /// Whether pointer observation was explicitly approved.
    #[must_use]
    pub const fn pointer_buttons(self) -> bool {
        self.pointer_buttons
    }

    /// Whether window metadata was explicitly approved.
    #[must_use]
    pub const fn window_metadata(self) -> bool {
        self.window_metadata
    }

    /// Whether desktop-click target classification was explicitly approved.
    #[must_use]
    pub const fn desktop_click_classification(self) -> bool {
        self.desktop_click_classification
    }
}

/// Source used for a Windows window rectangle normalized to physical desktop pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowBoundsSource {
    /// DWM extended-frame bounds, expressed by DWM in physical screen coordinates.
    DwmExtendedFrame,
    /// `GetWindowRect` queried after setting per-monitor-v2 thread DPI awareness.
    GetWindowRectPerMonitorV2,
}

/// Windows-only geometry record retaining the bounds source omitted by the shared contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservedWindowBounds {
    /// Portable, handle-free geometry published through `HostAdapter`.
    pub rectangle: WindowRectangle,
    /// Provenance used for normalization and mixed-DPI interpretation.
    pub source: WindowBoundsSource,
}

/// Confidence-bearing fullscreen overlap candidate, scoped to one output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FullscreenCandidate {
    /// Ephemeral window identity, not an HWND.
    pub window: WindowIdentity,
    /// Output covered by the candidate.
    pub display: MonitorIdentity,
    /// The overlap heuristic never claims a universal or certain fullscreen result.
    pub confidence: Confidence,
    /// Monotonic observation time in milliseconds.
    pub observed_at_ms: u64,
}

/// Result of conservative passive desktop target classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopHitClassification {
    /// Hit testing identified the shell's wallpaper background window itself.
    EmptyWallpaper,
    /// Hit testing identified a non-wallpaper target or an interactive shell surface.
    Occupied,
    /// The target, timing, or shell relationship was ambiguous; suppress reactions.
    Unknown,
}

/// A passive button-down observation, never an input delivery or replay instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopClickObservation {
    /// Position queried separately from Raw Input relative motion.
    pub position: PointerPosition,
    /// Button bit from the shared pointer-button contract.
    pub button: u8,
    /// Unknown classification requires the consumer to suppress any wallpaper reaction.
    pub hit: DesktopHitClassification,
    /// Monotonic observation time in milliseconds.
    pub observed_at_ms: u64,
}

/// Fixed-capacity Windows-only metadata snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsObserverSnapshot {
    /// Current normalized window rectangles and their provenance.
    pub windows: [Option<ObservedWindowBounds>; MAX_WINDOWS_OBSERVED],
    /// Current coarse state for each retained window, including state-only records.
    pub states: [Option<(WindowIdentity, WindowState)>; MAX_WINDOWS_OBSERVED],
    /// Current fullscreen candidates, each scoped to one output.
    pub fullscreen: [Option<FullscreenCandidate>; MAX_FULLSCREEN_OBSERVATIONS],
    /// True if native enumeration found more records than this fixed snapshot can retain.
    pub truncated: bool,
}

/// Fixed-capacity batch of Windows-local click classifications.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopClickBatch {
    /// Retained button-down observations in arrival order.
    pub clicks: [Option<DesktopClickObservation>; MAX_DESKTOP_CLICK_OBSERVATIONS],
    /// Number of retained entries.
    pub len: usize,
    /// At least one observation was dropped; callers must discard hit-sensitive history.
    pub overflowed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_is_disabled_by_default_and_grants_are_independent() {
        let disabled = WindowsObserverConsent::default();
        assert_eq!(disabled, WindowsObserverConsent::disabled());
        assert!(!disabled.pointer_buttons());
        assert!(!disabled.window_metadata());
        assert!(!disabled.desktop_click_classification());

        let pointer_only = WindowsObserverConsent::disabled().approve_pointer_buttons();
        assert!(pointer_only.pointer_buttons());
        assert!(!pointer_only.window_metadata());
        assert!(!pointer_only.desktop_click_classification());
    }

    #[test]
    fn unknown_click_classification_is_not_a_positive_hit() {
        assert_ne!(
            DesktopHitClassification::Unknown,
            DesktopHitClassification::EmptyWallpaper
        );
    }

    #[test]
    fn default_observer_does_not_start_native_observation() {
        use crate::{CapabilityAvailability, CapabilityFeature, CapabilityScope, HostAdapter};

        let observer = WindowsObserver::start(WindowsObserverConsent::default()).unwrap();
        let capabilities = observer.capabilities();
        assert_eq!(
            capabilities
                .record(
                    CapabilityFeature::PointerButtons,
                    CapabilityScope::Desktop,
                )
                .unwrap()
                .availability(),
            CapabilityAvailability::PermissionRequired
        );
        assert_eq!(
            capabilities
                .record(
                    CapabilityFeature::WindowRectangles,
                    CapabilityScope::Desktop,
                )
                .unwrap()
                .availability(),
            CapabilityAvailability::PermissionRequired
        );
    }

    #[test]
    fn desktop_classification_consent_requires_pointer_and_window_grants() {
        assert!(matches!(
            WindowsObserver::start(
                WindowsObserverConsent::disabled().approve_desktop_click_classification()
            ),
            Err(WindowsObserverError::InvalidConsentCombination)
        ));
    }
}
