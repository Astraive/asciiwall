#![cfg(windows)]

use ascii_core::{Generation, MonitorFallbackMatch, MonitorId, MonitorIdentity};
use ascii_platform::{
    AdapterVersion, CapabilityAvailability, CapabilityFeature, CapabilityReason, CapabilityRecord,
    CapabilityScope, CapabilitySet, CapabilitySnapshot, Confidence, DowngradeReason, FeedEvent,
    Freshness, HostEvent, HostEventQueue, HostIdentity, HostRestrictions, PointerButtons,
    PointerButtonsUpdate, PresentationMode, PresentationRequest, SurfaceLeaseError,
    SurfaceLeaseRegistry, resolve_capability, resolve_presentation,
};

fn host(instance_id: u64) -> HostIdentity {
    HostIdentity::new(
        "fixture.contract-only",
        AdapterVersion {
            major: 1,
            minor: 0,
            patch: 0,
        },
        instance_id,
    )
    .unwrap()
}

fn display(id: u8) -> MonitorIdentity {
    let monitor_id = MonitorId::new([id; 16]).unwrap();
    let fallback = MonitorFallbackMatch::new(0, 0, 1920, 1080).unwrap();
    MonitorIdentity::new(monitor_id, fallback)
}

fn record(
    feature: CapabilityFeature,
    scope: CapabilityScope,
    availability: CapabilityAvailability,
    reason: Option<CapabilityReason>,
    dependencies: CapabilitySet,
) -> CapabilityRecord {
    CapabilityRecord::new(
        feature,
        scope,
        availability,
        reason,
        Freshness::observed_at(100, 1_000),
        Confidence::High,
        dependencies,
    )
    .unwrap()
}

#[test]
fn denied_pointer_and_state_only_window_feed_do_not_disable_wallpaper() {
    let mut snapshot = CapabilitySnapshot::unknown(host(1), HostRestrictions::empty());
    snapshot
        .replace(record(
            CapabilityFeature::WallpaperSurface,
            CapabilityScope::Host,
            CapabilityAvailability::Available,
            None,
            CapabilitySet::empty(),
        ))
        .unwrap();
    snapshot
        .replace(record(
            CapabilityFeature::PointerPosition,
            CapabilityScope::Desktop,
            CapabilityAvailability::Denied,
            Some(CapabilityReason::PermissionRevoked),
            CapabilitySet::empty(),
        ))
        .unwrap();
    snapshot
        .replace(record(
            CapabilityFeature::WindowState,
            CapabilityScope::Desktop,
            CapabilityAvailability::Available,
            None,
            CapabilitySet::empty(),
        ))
        .unwrap();
    snapshot
        .replace(record(
            CapabilityFeature::WindowRectangles,
            CapabilityScope::Desktop,
            CapabilityAvailability::Unsupported,
            Some(CapabilityReason::UnsupportedByHost),
            CapabilitySet::empty(),
        ))
        .unwrap();

    assert!(
        snapshot
            .effective_at(
                CapabilityFeature::WallpaperSurface,
                CapabilityScope::Host,
                101,
            )
            .is_available()
    );
    assert_eq!(
        snapshot
            .effective_at(
                CapabilityFeature::PointerPosition,
                CapabilityScope::Desktop,
                101,
            )
            .availability(),
        CapabilityAvailability::Denied
    );
    assert!(
        snapshot
            .effective_at(
                CapabilityFeature::WindowState,
                CapabilityScope::Desktop,
                101,
            )
            .is_available()
    );
    assert_eq!(
        snapshot
            .effective_at(
                CapabilityFeature::WindowRectangles,
                CapabilityScope::Desktop,
                101,
            )
            .availability(),
        CapabilityAvailability::Unsupported
    );

    let requested_pointer = resolve_capability(
        &snapshot,
        CapabilityFeature::PointerPosition,
        CapabilityScope::Desktop,
        true,
        101,
    );
    assert!(requested_pointer.requested());
    assert!(!requested_pointer.effective());
}

#[test]
fn denied_unknown_and_valid_empty_data_are_distinct() {
    let snapshot = CapabilitySnapshot::unknown(host(2), HostRestrictions::empty());
    assert_eq!(
        snapshot
            .effective_at(
                CapabilityFeature::PointerButtons,
                CapabilityScope::Desktop,
                100,
            )
            .availability(),
        CapabilityAvailability::Unknown
    );
    let empty_buttons = PointerButtons::new(0).unwrap();
    assert_eq!(empty_buttons.bits(), 0);
    let empty_update = PointerButtonsUpdate {
        buttons: empty_buttons,
        observed_at_ms: 100,
    };
    assert_eq!(empty_update.buttons.bits(), 0);
    assert_ne!(
        snapshot
            .effective_at(
                CapabilityFeature::PointerButtons,
                CapabilityScope::Desktop,
                100,
            )
            .availability(),
        CapabilityAvailability::Available
    );
}

#[test]
fn permission_loss_revokes_only_declared_dependents() {
    let mut snapshot = CapabilitySnapshot::unknown(host(3), HostRestrictions::empty());
    snapshot
        .replace(record(
            CapabilityFeature::PointerPosition,
            CapabilityScope::Desktop,
            CapabilityAvailability::Available,
            None,
            CapabilitySet::empty(),
        ))
        .unwrap();
    snapshot
        .replace(record(
            CapabilityFeature::PointerButtons,
            CapabilityScope::Desktop,
            CapabilityAvailability::Available,
            None,
            CapabilitySet::one(CapabilityFeature::PointerPosition),
        ))
        .unwrap();
    snapshot
        .replace(record(
            CapabilityFeature::WallpaperSurface,
            CapabilityScope::Host,
            CapabilityAvailability::Available,
            None,
            CapabilitySet::empty(),
        ))
        .unwrap();
    snapshot
        .replace(record(
            CapabilityFeature::WindowState,
            CapabilityScope::Desktop,
            CapabilityAvailability::Available,
            None,
            CapabilitySet::empty(),
        ))
        .unwrap();

    let changes = snapshot
        .replace(record(
            CapabilityFeature::PointerPosition,
            CapabilityScope::Desktop,
            CapabilityAvailability::Denied,
            Some(CapabilityReason::PermissionRevoked),
            CapabilitySet::empty(),
        ))
        .unwrap();

    assert!(changes.iter().any(|change| {
        change.feature() == CapabilityFeature::PointerButtons
            && change.current().availability() == CapabilityAvailability::Unavailable
            && change.current().blocked_by() == Some(CapabilityFeature::PointerPosition)
    }));
    assert!(
        !changes
            .iter()
            .any(|change| change.feature() == CapabilityFeature::WallpaperSurface)
    );
    assert!(
        !changes
            .iter()
            .any(|change| change.feature() == CapabilityFeature::WindowState)
    );
    assert!(
        snapshot
            .effective_at(
                CapabilityFeature::WallpaperSurface,
                CapabilityScope::Host,
                101,
            )
            .is_available()
    );
}

#[test]
fn output_scale_records_are_scoped_per_display() {
    let first = display(1);
    let second = display(2);
    let mut snapshot = CapabilitySnapshot::unknown(host(4), HostRestrictions::empty());
    snapshot
        .replace(record(
            CapabilityFeature::OutputScale,
            CapabilityScope::Output(first),
            CapabilityAvailability::Available,
            None,
            CapabilitySet::empty(),
        ))
        .unwrap();
    snapshot
        .replace(record(
            CapabilityFeature::OutputScale,
            CapabilityScope::Output(second),
            CapabilityAvailability::Denied,
            Some(CapabilityReason::PermissionDenied),
            CapabilitySet::empty(),
        ))
        .unwrap();

    assert!(
        snapshot
            .effective_at(
                CapabilityFeature::OutputScale,
                CapabilityScope::Output(first),
                101,
            )
            .is_available()
    );
    assert_eq!(
        snapshot
            .effective_at(
                CapabilityFeature::OutputScale,
                CapabilityScope::Output(second),
                101,
            )
            .availability(),
        CapabilityAvailability::Denied
    );
}

#[test]
fn bounded_event_overflow_requests_snapshot_reconciliation() {
    let mut queue = HostEventQueue::new();
    let mut snapshot = CapabilitySnapshot::unknown(host(5), HostRestrictions::empty());
    let event = HostEvent::Feed(FeedEvent::PointerButtons(PointerButtonsUpdate {
        buttons: PointerButtons::new(0).unwrap(),
        observed_at_ms: 100,
    }));
    for _ in 0..32 {
        let revision = snapshot.advance_revision().unwrap();
        assert!(queue.push(event, revision));
    }
    let lost_revision = snapshot.advance_revision().unwrap();
    assert!(!queue.push(event, lost_revision));

    let batch = queue.drain();
    assert_eq!(batch.len(), 32);
    assert_eq!(batch.iter().count(), 32);
    assert_eq!(batch.reconciliation_revision(), Some(33));
    assert!(queue.is_empty());

    let next_revision = snapshot.advance_revision().unwrap();
    assert!(queue.push(event, next_revision));
    let next = queue.drain();
    assert_eq!(next.len(), 1);
    assert_eq!(next.reconciliation_revision(), None);
}

#[test]
fn detach_reattach_rejects_stale_generation_even_when_native_handle_is_reused() {
    let monitor = display(9);
    let owner = host(9);
    let mut registry = SurfaceLeaseRegistry::<2>::new(owner);
    // The numeric token belongs only to this synthetic test fixture, never to SurfaceLease.
    let recycled_native_handle = 0x1234_u64;
    let old = registry.attach(monitor).unwrap();
    assert!(registry.validate_present(old, monitor).is_ok());
    assert_eq!(
        registry.validate_present(old, display(10)),
        Err(SurfaceLeaseError::DisplayMismatch)
    );
    registry.retire(old).unwrap();

    // Simulate the OS assigning the same opaque handle value to the replacement native surface.
    let reused_native_handle = recycled_native_handle;
    assert_eq!(reused_native_handle, recycled_native_handle);
    let current = registry.attach(monitor).unwrap();
    assert_ne!(current.generation(), old.generation());
    assert_eq!(
        registry.validate_present(old, monitor),
        Err(SurfaceLeaseError::StaleGeneration)
    );
    assert!(registry.validate_present(current, monitor).is_ok());
    assert_eq!(registry.active_count(), 1);
}

#[test]
fn lease_retirement_and_capacity_are_explicit_errors() {
    let first = display(10);
    let second = display(11);
    let mut registry = SurfaceLeaseRegistry::<1>::new(host(10));
    let lease = registry.attach(first).unwrap();
    assert_eq!(
        registry.attach(second),
        Err(SurfaceLeaseError::CapacityExhausted)
    );
    registry.retire(lease).unwrap();
    assert_eq!(
        registry.validate_present(lease, first),
        Err(SurfaceLeaseError::RetiredLease)
    );
}

#[test]
fn host_identity_restrictions_and_consent_boundary_are_reported() {
    let restrictions = HostRestrictions::empty()
        .with(HostRestrictions::WINDOW_GEOMETRY_UNAVAILABLE)
        .unwrap();
    assert!(restrictions.contains(HostRestrictions::WINDOW_GEOMETRY_UNAVAILABLE));
    assert!(
        HostIdentity::new(
            "fixture.contract-only",
            AdapterVersion {
                major: 1,
                minor: 0,
                patch: 0
            },
            21,
        )
        .is_some()
    );

    let mut snapshot = CapabilitySnapshot::unknown(host(21), restrictions);
    snapshot
        .replace(record(
            CapabilityFeature::StaticWallpaper,
            CapabilityScope::Host,
            CapabilityAvailability::Available,
            None,
            CapabilitySet::empty(),
        ))
        .unwrap();
    let requested = PresentationRequest {
        mode: PresentationMode::LiveWallpaper,
        consent_static_fallback: false,
    };
    let result = resolve_presentation(&snapshot, requested, 101);
    assert_eq!(result.requested(), PresentationMode::LiveWallpaper);
    assert_eq!(result.effective(), PresentationMode::PreviewOnly);
    assert!(matches!(
        result.downgrade(),
        Some(DowngradeReason::StaticFallbackRequiresConsent(_))
    ));
    assert_eq!(snapshot.host().adapter_id(), "fixture.contract-only");
    assert_eq!(snapshot.health(), ascii_platform::HostHealth::Unknown);
}

#[test]
fn adapter_fault_requires_safe_preview_or_consented_static_fallback() {
    let mut snapshot = CapabilitySnapshot::unknown(host(22), HostRestrictions::empty());
    snapshot
        .replace(record(
            CapabilityFeature::WallpaperSurface,
            CapabilityScope::Host,
            CapabilityAvailability::Available,
            None,
            CapabilitySet::empty(),
        ))
        .unwrap();
    snapshot
        .replace(record(
            CapabilityFeature::StaticWallpaper,
            CapabilityScope::Host,
            CapabilityAvailability::Available,
            None,
            CapabilitySet::empty(),
        ))
        .unwrap();
    let health_changes = snapshot
        .set_health(ascii_platform::HostHealth::Faulted)
        .unwrap();
    assert!(health_changes.iter().any(|change| {
        change.feature() == CapabilityFeature::WallpaperSurface
            && change.current().availability() == CapabilityAvailability::Faulted
    }));
    assert!(
        !health_changes
            .iter()
            .any(|change| change.feature() == CapabilityFeature::StaticWallpaper)
    );

    assert_eq!(
        snapshot
            .effective_at(
                CapabilityFeature::WallpaperSurface,
                CapabilityScope::Host,
                101,
            )
            .availability(),
        CapabilityAvailability::Faulted
    );
    let safe_preview = resolve_presentation(
        &snapshot,
        PresentationRequest {
            mode: PresentationMode::LiveWallpaper,
            consent_static_fallback: false,
        },
        101,
    );
    assert_eq!(safe_preview.effective(), PresentationMode::PreviewOnly);
    assert!(safe_preview.downgrade().is_some());

    let consented_static = resolve_presentation(
        &snapshot,
        PresentationRequest {
            mode: PresentationMode::LiveWallpaper,
            consent_static_fallback: true,
        },
        101,
    );
    assert_eq!(
        consented_static.effective(),
        PresentationMode::StaticWallpaper
    );
    let directly_requested_static = resolve_presentation(
        &snapshot,
        PresentationRequest {
            mode: PresentationMode::StaticWallpaper,
            consent_static_fallback: false,
        },
        101,
    );
    assert_eq!(
        directly_requested_static.effective(),
        PresentationMode::StaticWallpaper
    );
}

#[test]
fn window_tokens_include_core_generation_identity() {
    let old = ascii_platform::WindowIdentity::new(7, Generation::INITIAL);
    let replacement =
        ascii_platform::WindowIdentity::new(7, Generation::INITIAL.checked_next().unwrap());
    assert_ne!(old, replacement);
}
