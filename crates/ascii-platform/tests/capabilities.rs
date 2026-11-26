use ascii_core::{Generation, MonitorFallbackMatch, MonitorId, MonitorIdentity};
use ascii_platform::{
    AdapterVersion, CapabilityAvailability, CapabilityFeature, CapabilityReason, CapabilityRecord,
    CapabilityScope, CapabilitySet, CapabilitySnapshot, Confidence, DowngradeReason, FeedEvent,
    Freshness, HostAdapter, HostEvent, HostEventBatch, HostEventQueue, HostIdentity,
    HostRestrictions, HostStateSnapshot, HostStateSnapshotError, MAX_RECONCILIATION_FEEDS,
    MAX_RECONCILIATION_LEASES, PointerButtons, PointerButtonsUpdate, PointerPosition,
    PresentationMode, PresentationRequest, SurfaceLeaseError, SurfaceLeaseRegistry,
    resolve_capability, resolve_presentation,
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

struct FixtureAdapter {
    capabilities: CapabilitySnapshot,
    reconciliation: HostStateSnapshot,
}

impl HostAdapter for FixtureAdapter {
    fn capabilities(&self) -> CapabilitySnapshot {
        self.capabilities
    }

    fn reconciliation_snapshot(&self) -> HostStateSnapshot {
        self.reconciliation
    }

    fn poll(&mut self) -> HostEventBatch {
        HostEventQueue::new().drain()
    }
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
fn dependency_cycle_reason_survives_parent_resolution() {
    let mut snapshot = CapabilitySnapshot::unknown(host(31), HostRestrictions::empty());
    snapshot
        .replace(record(
            CapabilityFeature::PointerPosition,
            CapabilityScope::Desktop,
            CapabilityAvailability::Available,
            None,
            CapabilitySet::one(CapabilityFeature::PointerButtons),
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

    let resolved = snapshot.effective_at(
        CapabilityFeature::PointerPosition,
        CapabilityScope::Desktop,
        101,
    );
    assert_eq!(resolved.availability(), CapabilityAvailability::Unavailable);
    assert_eq!(resolved.reason(), Some(CapabilityReason::DependencyCycle));
}

#[test]
fn bounded_event_overflow_reconciles_current_feed_and_lease_values() {
    let owner = host(5);
    let mut queue = HostEventQueue::new();
    let mut capabilities = CapabilitySnapshot::unknown(owner, HostRestrictions::empty());
    let retained_event = HostEvent::Feed(FeedEvent::PointerButtons(PointerButtonsUpdate {
        buttons: PointerButtons::new(0).unwrap(),
        observed_at_ms: 100,
    }));
    for _ in 0..32 {
        let revision = capabilities.advance_revision().unwrap();
        assert!(queue.push(retained_event, revision));
    }
    let latest_position = PointerPosition {
        display: display(12),
        x: -81,
        y: 44,
        observed_at_ms: 133,
    };
    let dropped_event = HostEvent::Feed(FeedEvent::PointerPosition(latest_position));
    let lost_revision = capabilities.advance_revision().unwrap();
    assert!(!queue.push(dropped_event, lost_revision));

    let batch = queue.drain();
    assert_eq!(batch.len(), 32);
    assert_eq!(batch.iter().count(), 32);
    assert_eq!(batch.reconciliation_revision(), Some(lost_revision));
    assert!(
        !batch
            .iter()
            .any(|event| matches!(event, HostEvent::Feed(FeedEvent::PointerPosition(_))))
    );
    assert!(queue.is_empty());

    let mut registry = SurfaceLeaseRegistry::<1>::new(owner).unwrap();
    let lease = registry.attach(display(12)).unwrap();
    let mut current_state = HostStateSnapshot::new(capabilities);
    current_state
        .set_feed(FeedEvent::PointerPosition(latest_position))
        .unwrap();
    for active_lease in registry.active_leases() {
        current_state.set_lease(*active_lease).unwrap();
    }
    assert_eq!(registry.active_count(), 1);
    let adapter = FixtureAdapter {
        capabilities,
        reconciliation: current_state,
    };
    let reconciled = adapter.reconcile(lost_revision).unwrap();
    assert_eq!(reconciled.revision(), lost_revision);
    assert!(reconciled.is_complete());
    assert!(matches!(
        reconciled.feeds().next(),
        Some(FeedEvent::PointerPosition(position)) if *position == latest_position
    ));
    assert_eq!(reconciled.leases().copied().next(), Some(lease));
    assert_eq!(reconciled.lease_count(), 1);
    assert_eq!(
        adapter.reconcile(lost_revision + 1),
        Err(ascii_platform::ReconciliationError::SnapshotBehind {
            requested_revision: lost_revision + 1,
            snapshot_revision: lost_revision,
        })
    );

    let next_revision = capabilities.advance_revision().unwrap();
    assert!(queue.push(retained_event, next_revision));
    let next = queue.drain();
    assert_eq!(next.len(), 1);
    assert_eq!(next.reconciliation_revision(), None);
}

#[test]
fn bounded_reconciliation_snapshot_reports_feed_and_lease_truncation() {
    let owner = host(30);
    let capabilities = CapabilitySnapshot::unknown(owner, HostRestrictions::empty());
    let mut state = HostStateSnapshot::new(capabilities);
    for id in 1..=MAX_RECONCILIATION_FEEDS as u8 {
        state
            .set_feed(FeedEvent::OutputScale {
                display: display(id),
                scale_milli: u32::from(id) * 1_000,
                observed_at_ms: 100,
            })
            .unwrap();
    }
    assert_eq!(
        state.set_feed(FeedEvent::OutputScale {
            display: display(33),
            scale_milli: 1_000,
            observed_at_ms: 101,
        }),
        Err(HostStateSnapshotError::FeedCapacityExhausted)
    );

    let mut registry = SurfaceLeaseRegistry::<33>::new(owner).unwrap();
    for id in 1..=MAX_RECONCILIATION_LEASES as u8 {
        let lease = registry.attach(display(id)).unwrap();
        state.set_lease(lease).unwrap();
    }
    let overflow_lease = registry.attach(display(33)).unwrap();
    assert_eq!(
        state.set_lease(overflow_lease),
        Err(HostStateSnapshotError::LeaseCapacityExhausted)
    );
    assert_eq!(state.feed_count(), MAX_RECONCILIATION_FEEDS);
    assert_eq!(state.lease_count(), MAX_RECONCILIATION_LEASES);
    assert!(state.feeds_truncated());
    assert!(state.leases_truncated());
    assert!(!state.is_complete());
    let snapshot_revision = state.revision();
    let adapter = FixtureAdapter {
        capabilities,
        reconciliation: state,
    };
    assert_eq!(
        adapter.reconcile(snapshot_revision + 1),
        Err(ascii_platform::ReconciliationError::SnapshotBehind {
            requested_revision: snapshot_revision + 1,
            snapshot_revision,
        })
    );
    let ascii_platform::ReconciliationError::IncompleteSnapshot { snapshot } =
        adapter.reconcile(snapshot_revision).unwrap_err()
    else {
        panic!("a truncated snapshot must not report successful reconciliation");
    };
    assert_eq!(snapshot.revision(), snapshot_revision);
    assert_eq!(snapshot.feed_count(), MAX_RECONCILIATION_FEEDS);
    assert_eq!(snapshot.lease_count(), MAX_RECONCILIATION_LEASES);
    assert!(snapshot.feeds_truncated());
    assert!(snapshot.leases_truncated());
    assert!(!snapshot.is_complete());
    assert!(snapshot.feeds().next().is_some());
    assert!(snapshot.leases().next().is_some());
}

#[test]
fn detach_reattach_rejects_stale_generation_even_when_native_handle_is_reused() {
    let monitor = display(9);
    let owner = host(9);
    let mut registry = SurfaceLeaseRegistry::<2>::new(owner).unwrap();
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
fn registry_recreation_rejects_old_lease_with_same_owner_display_and_generation() {
    let owner = host(32);
    let monitor = display(12);
    let old = {
        let mut old_registry = SurfaceLeaseRegistry::<1>::new(owner).unwrap();
        old_registry.attach(monitor).unwrap()
    };

    let mut replacement = SurfaceLeaseRegistry::<1>::new(owner).unwrap();
    let current = replacement.attach(monitor).unwrap();
    assert_eq!(old.owner(), current.owner());
    assert_eq!(old.display(), current.display());
    assert_eq!(old.generation(), current.generation());
    assert_ne!(old.registry_epoch(), current.registry_epoch());
    assert_eq!(
        replacement.validate_present(old, monitor),
        Err(SurfaceLeaseError::WrongRegistry)
    );
    assert_eq!(
        replacement.retire(old),
        Err(SurfaceLeaseError::WrongRegistry)
    );
    assert!(replacement.validate_present(current, monitor).is_ok());
}

#[test]
fn lease_retirement_and_capacity_are_explicit_errors() {
    let first = display(10);
    let second = display(11);
    let mut registry = SurfaceLeaseRegistry::<1>::new(host(10)).unwrap();
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
