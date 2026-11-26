use ascii_core::coordinates::{
    CharacterGrid, ClockDomainId, ClockDomainMapping, ClockEpochTracker, ClockMappingError,
    CoordinateError, DesktopLogicalPoint, DesktopLogicalRect, DisplayRoutingMode, EventFreshnessGate,
    FitMode, FixedStepError, FixedStepScheduler, FittedContent, MonotonicEpoch, OutputGeometry,
    OutputRotation, OutputScale, SceneClockGroupId, ScenePoint, SceneRect, SpanGapPolicy, SurfaceRect,
    TopologySnapshot, TopologyState, WindowForceFreshness,
};
use ascii_core::events::{
    ButtonState, CapabilityScope, DesktopPoint, DesktopRect, EventEnvelope, EventOverflowCounters,
    EventPayload, EventPushErrorKind, EventPushOutcome, EventQueue, EventSequence, EventSourceId,
    MonotonicInstant, MissingReason, Observation, PointerBoundary, PointerButton,
    PointerButtonEvent, PointerMotion, TopologyChange, UserCommand, WindowSnapshot,
};
use ascii_core::{Generation, MonitorId};

fn monitor(byte: u8) -> MonitorId {
    MonitorId::new([byte; 16]).unwrap()
}

fn event(sequence: u64, nanos: u64, generation: u64, payload: EventPayload) -> EventEnvelope {
    EventEnvelope::new(
        EventSourceId::new(7),
        MonotonicInstant::new(MonotonicEpoch::new(3), nanos),
        EventSequence::new(sequence),
        Generation::from_value(generation),
        CapabilityScope::Application,
        payload,
    )
}

fn button(state: ButtonState) -> EventPayload {
    EventPayload::PointerButton(PointerButtonEvent {
        button: PointerButton::Primary,
        state,
        position: Observation::missing(MissingReason::TemporarilyUnavailable),
    })
}

fn geometry(
    id: u8,
    desktop: DesktopLogicalRect,
    scale: OutputScale,
    rotation: OutputRotation,
    fitted: FittedContent,
) -> OutputGeometry {
    OutputGeometry::new(
        monitor(id),
        desktop,
        scale,
        rotation,
        fitted,
        CharacterGrid::new(80, 24).unwrap(),
    )
    .unwrap()
}

#[test]
fn envelope_preserves_missing_reasons_and_motion_coalescing_is_barrier_safe() {
    let missing = Observation::<DesktopPoint>::missing(MissingReason::PermissionDenied);
    let mut queue = EventQueue::<3>::new();
    let first_motion = event(
        1,
        10,
        4,
        EventPayload::PointerMotion(PointerMotion { position: missing }),
    );
    let newer_motion = event(
        2,
        11,
        4,
        EventPayload::PointerMotion(PointerMotion {
            position: Observation::known(DesktopPoint::new(-10.5, 20.25).unwrap()),
        }),
    );
    assert_eq!(missing.value(), None);
    assert_eq!(missing.reason(), Some(MissingReason::PermissionDenied));
    assert_eq!(queue.push(first_motion), Ok(EventPushOutcome::Queued));
    assert_eq!(queue.push(newer_motion), Ok(EventPushOutcome::CoalescedMotion));
    assert_eq!(queue.pop(), Some(newer_motion));
    assert_eq!(
        queue.overflow_counters(),
        EventOverflowCounters {
            coalesced_motion: 1,
            ..EventOverflowCounters::default()
        }
    );
    let window = event(
        3,
        12,
        4,
        EventPayload::WindowSnapshot(WindowSnapshot {
            window: Observation::missing(MissingReason::Redacted),
            bounds: Observation::missing(MissingReason::NotObserved),
        }),
    );
    assert_eq!(queue.push(window), Ok(EventPushOutcome::Queued));
    assert_eq!(queue.pop(), Some(window));
    let mut zero_capacity = EventQueue::<0>::new();
    assert_eq!(
        zero_capacity.push(first_motion),
        Ok(EventPushOutcome::DroppedMotion)
    );
    let critical = event(4, 13, 4, button(ButtonState::Released));
    assert_eq!(
        zero_capacity.push(critical).unwrap_err().kind,
        EventPushErrorKind::FullOfCriticalEvents
    );
    assert_eq!(zero_capacity.overflow_counters().dropped_motion, 1);
    assert_eq!(zero_capacity.overflow_counters().critical_backpressure, 1);

    let mut queue = EventQueue::<3>::new();
    let motion = |sequence, nanos| {
        event(
            sequence,
            nanos,
            4,
            EventPayload::PointerMotion(PointerMotion {
                position: Observation::known(DesktopPoint::new(nanos as f64, -4.0).unwrap()),
            }),
        )
    };
    let press = event(2, 2, 4, button(ButtonState::Pressed));
    let stop = event(4, 4, 4, EventPayload::UserCommand(UserCommand::Stop));
    assert_eq!(queue.push(motion(1, 1)), Ok(EventPushOutcome::Queued));
    assert_eq!(queue.push(press), Ok(EventPushOutcome::Queued));
    assert_eq!(queue.push(motion(3, 3)), Ok(EventPushOutcome::Queued));
    assert_eq!(
        queue.push(stop),
        Ok(EventPushOutcome::QueuedAfterEvictingMotion)
    );
    assert_eq!(queue.pop(), Some(press));
    assert_eq!(queue.pop(), Some(motion(3, 3)));
    assert_eq!(queue.pop(), Some(stop));
    assert_eq!(queue.overflow_counters().evicted_motion, 1);
}

#[test]
fn critical_edges_topology_and_stop_return_under_pressure_without_reordering() {
    let mut queue = EventQueue::<3>::new();
    let press = event(1, 10, 1, button(ButtonState::Pressed));
    let topology = event(
        2,
        20,
        1,
        EventPayload::TopologyChanged(
            TopologyChange::new(Generation::from_value(1), Generation::from_value(2)).unwrap(),
        ),
    );
    let release = event(3, 30, 2, button(ButtonState::Released));
    let stop = event(4, 40, 2, EventPayload::UserCommand(UserCommand::Stop));
    assert_eq!(queue.push(press), Ok(EventPushOutcome::Queued));
    assert_eq!(queue.push(topology), Ok(EventPushOutcome::Queued));
    assert_eq!(queue.push(release), Ok(EventPushOutcome::Queued));

    let refused = queue.push(stop).unwrap_err();
    assert_eq!(refused.kind, EventPushErrorKind::FullOfCriticalEvents);
    assert_eq!(refused.event, stop);
    assert_eq!(queue.overflow_counters().critical_backpressure, 1);
    assert_eq!(queue.pop(), Some(press));
    assert_eq!(queue.push(refused.event), Ok(EventPushOutcome::Queued));
    assert_eq!(queue.pop(), Some(topology));
    assert_eq!(queue.pop(), Some(release));
    assert_eq!(queue.pop(), Some(stop));
    assert_eq!(queue.pop(), None);

    let leave = event(
        5,
        50,
        2,
        EventPayload::PointerLeave(PointerBoundary {
            position: Observation::missing(MissingReason::NotObserved),
        }),
    );
    let older = event(6, 49, 2, button(ButtonState::Pressed));
    assert_eq!(queue.push(leave), Ok(EventPushOutcome::Queued));
    let out_of_order = queue.push(older).unwrap_err();
    assert_eq!(out_of_order.event, older);
    assert_eq!(out_of_order.kind, EventPushErrorKind::OutOfOrder);
    assert_eq!(queue.overflow_counters().out_of_order, 1);
}

#[test]
fn negative_portrait_fractional_dpi_rotation_and_letterbox_roundtrip() {
    let desktop = DesktopLogicalRect::new(-1920.0, -300.0, 1080.0, 1920.0).unwrap();
    let fitted = FittedContent::new(
        SurfaceRect::new(0.0, 225.0, 2880.0, 900.0).unwrap(),
        SceneRect::new(0.0, 0.0, 1.0, 1.0).unwrap(),
        FitMode::Letterbox,
    )
    .unwrap();
    let output = geometry(
        1,
        desktop,
        OutputScale::new(1.25, 1.5).unwrap(),
        OutputRotation::Clockwise90,
        fitted,
    );
    assert_eq!(output.surface_dimensions(), (2880.0, 1350.0));
    let topology = TopologySnapshot::<2>::new(
        Generation::from_value(5),
        DisplayRoutingMode::PerDisplay,
        &[output],
    )
    .unwrap();
    let transform = topology
        .output_transform(monitor(1), Generation::from_value(5))
        .unwrap();
    let desktop_point = DesktopLogicalPoint::new(-1600.0, 340.0).unwrap();
    let scene = transform
        .desktop_to_scene(Generation::from_value(5), desktop_point)
        .unwrap();
    let roundtrip = transform
        .scene_to_desktop(Generation::from_value(5), scene)
        .unwrap();
    assert!((roundtrip.x() - desktop_point.x()).abs() <= 1.0e-9);
    assert!((roundtrip.y() - desktop_point.y()).abs() <= 1.0e-9);

    let character = transform
        .scene_to_character(Generation::from_value(5), scene)
        .unwrap();
    let character_roundtrip = transform
        .character_to_scene(Generation::from_value(5), character)
        .unwrap();
    assert!((character_roundtrip.x() - scene.x()).abs() <= 1.0e-12);
    assert!((character_roundtrip.y() - scene.y()).abs() <= 1.0e-12);

    let letterbox_point = DesktopLogicalPoint::new(-1820.0, -250.0).unwrap();
    assert_eq!(
        transform.desktop_to_scene(Generation::from_value(5), letterbox_point),
        Err(CoordinateError::OutsideOutputOrContent)
    );
    assert_eq!(
        transform.desktop_to_scene(Generation::from_value(6), desktop_point),
        Err(CoordinateError::StaleGeneration)
    );
}

#[test]
fn crop_maps_only_its_declared_scene_view() {
    let crop = FittedContent::new(
        SurfaceRect::new(0.0, 0.0, 400.0, 200.0).unwrap(),
        SceneRect::new(0.25, 0.25, 0.5, 0.5).unwrap(),
        FitMode::Crop,
    )
    .unwrap();
    let output = geometry(
        4,
        DesktopLogicalRect::new(0.0, 0.0, 400.0, 200.0).unwrap(),
        OutputScale::new(1.0, 1.0).unwrap(),
        OutputRotation::None,
        crop,
    );
    let transform = TopologySnapshot::<1>::new(
        Generation::from_value(1),
        DisplayRoutingMode::PerDisplay,
        &[output],
    )
    .unwrap()
    .output_transform(monitor(4), Generation::from_value(1))
    .unwrap();
    let center = transform
        .desktop_to_scene(
            Generation::from_value(1),
            DesktopLogicalPoint::new(200.0, 100.0).unwrap(),
        )
        .unwrap();
    assert_eq!(center, ScenePoint::new(0.5, 0.5).unwrap());
    assert_eq!(
        transform.scene_to_surface(
            Generation::from_value(1),
            ScenePoint::new(0.1, 0.5).unwrap(),
        ),
        Err(CoordinateError::OutsideOutputOrContent)
    );
}

#[test]
fn topology_swap_invalidates_old_transforms_routes_gaps_and_handles_mirror_clocks() {
    let full_scene = FittedContent::new(
        SurfaceRect::new(0.0, 0.0, 1000.0, 1000.0).unwrap(),
        SceneRect::new(0.0, 0.0, 1.0, 1.0).unwrap(),
        FitMode::Stretch,
    )
    .unwrap();
    let first = geometry(
        1,
        DesktopLogicalRect::new(-1000.0, 0.0, 1000.0, 1000.0).unwrap(),
        OutputScale::new(1.0, 1.0).unwrap(),
        OutputRotation::None,
        full_scene,
    );
    let next_fit = FittedContent::new(
        SurfaceRect::new(0.0, 0.0, 800.0, 1000.0).unwrap(),
        SceneRect::new(0.0, 0.0, 1.0, 1.0).unwrap(),
        FitMode::Stretch,
    )
    .unwrap();
    let second = geometry(
        2,
        DesktopLogicalRect::new(1200.0, 0.0, 800.0, 1000.0).unwrap(),
        OutputScale::new(1.0, 1.0).unwrap(),
        OutputRotation::None,
        next_fit,
    );
    let old = TopologySnapshot::<2>::new(
        Generation::from_value(1),
        DisplayRoutingMode::PerDisplay,
        &[first],
    )
    .unwrap();
    let old_transform = old
        .output_transform(monitor(1), Generation::from_value(1))
        .unwrap();
    let replacement = TopologySnapshot::<2>::new(
        Generation::from_value(2),
        DisplayRoutingMode::Span(SpanGapPolicy::Preserve),
        &[first, second],
    )
    .unwrap();
    let mut state = TopologyState::new(old.clone());
    assert_eq!(state.replace(replacement), Ok(Generation::from_value(2)));
    assert_eq!(
        state.replace(old),
        Err(CoordinateError::NonMonotonicGeneration)
    );
    assert_eq!(
        old_transform.desktop_to_scene(
            Generation::from_value(2),
            DesktopLogicalPoint::new(-500.0, 500.0).unwrap(),
        ),
        Err(CoordinateError::StaleGeneration)
    );
    assert_eq!(
        state.snapshot().route_desktop_point(
            DesktopLogicalPoint::new(1100.0, 500.0).unwrap(),
            state.generation(),
        ),
        Err(CoordinateError::DisplayGap)
    );

    let collapsed = TopologySnapshot::<2>::new(
        Generation::from_value(3),
        DisplayRoutingMode::Span(SpanGapPolicy::CollapseHorizontal),
        &[first, second],
    )
    .unwrap();
    let routed = collapsed
        .route_desktop_point(
            DesktopLogicalPoint::new(1250.0, 500.0).unwrap(),
            Generation::from_value(3),
        )
        .unwrap();
    assert_eq!(routed.outputs().collect::<Vec<_>>(), vec![monitor(2)]);
    let scene = routed.span_scene_point().unwrap();
    assert!((scene.x() - (1_050.0 / 1_800.0)).abs() <= 1.0e-12);
    assert_eq!(
        collapsed.scene_to_desktop(monitor(1), scene, Generation::from_value(3)),
        Err(CoordinateError::DisplayGap)
    );
    let vertical_first = geometry(
        5,
        DesktopLogicalRect::new(0.0, -500.0, 800.0, 500.0).unwrap(),
        OutputScale::new(1.0, 1.0).unwrap(),
        OutputRotation::None,
        FittedContent::new(
            SurfaceRect::new(0.0, 0.0, 800.0, 500.0).unwrap(),
            SceneRect::new(0.0, 0.0, 1.0, 1.0).unwrap(),
            FitMode::Stretch,
        )
        .unwrap(),
    );
    let vertical_second = geometry(
        6,
        DesktopLogicalRect::new(0.0, 200.0, 800.0, 600.0).unwrap(),
        OutputScale::new(1.0, 1.0).unwrap(),
        OutputRotation::None,
        FittedContent::new(
            SurfaceRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
            SceneRect::new(0.0, 0.0, 1.0, 1.0).unwrap(),
            FitMode::Stretch,
        )
        .unwrap(),
    );
    let vertical = TopologySnapshot::<2>::new(
        Generation::from_value(5),
        DisplayRoutingMode::Span(SpanGapPolicy::CollapseVertical),
        &[vertical_first, vertical_second],
    )
    .unwrap();
    let vertical_route = vertical
        .route_desktop_point(
            DesktopLogicalPoint::new(400.0, 250.0).unwrap(),
            Generation::from_value(5),
        )
        .unwrap();
    let vertical_scene = vertical_route.span_scene_point().unwrap();
    assert!((vertical_scene.y() - (550.0 / 1_100.0)).abs() <= 1.0e-12);
    assert_eq!(
        vertical.scene_to_desktop(
            monitor(5),
            vertical_scene,
            Generation::from_value(5),
        ),
        Err(CoordinateError::DisplayGap)
    );

    let mirrored = geometry(
        3,
        DesktopLogicalRect::new(-1000.0, 0.0, 1000.0, 1000.0).unwrap(),
        OutputScale::new(1.5, 1.25).unwrap(),
        OutputRotation::Clockwise90,
        FittedContent::new(
            SurfaceRect::new(0.0, 0.0, 1250.0, 1500.0).unwrap(),
            SceneRect::new(0.0, 0.0, 1.0, 1.0).unwrap(),
            FitMode::Stretch,
        )
        .unwrap(),
    );
    let mirror = TopologySnapshot::<2>::new(
        Generation::from_value(4),
        DisplayRoutingMode::Mirror {
            clock_group: SceneClockGroupId::new(12),
        },
        &[first, mirrored],
    )
    .unwrap();
    let route = mirror
        .route_desktop_point(
            DesktopLogicalPoint::new(-500.0, 500.0).unwrap(),
            Generation::from_value(4),
        )
        .unwrap();
    assert_eq!(route.len(), 2);
    assert_eq!(
        mirror.mode(),
        DisplayRoutingMode::Mirror {
            clock_group: SceneClockGroupId::new(12),
        }
    );
    assert_eq!(
        mirror.scene_clock_group(monitor(1), Generation::from_value(4)),
        Ok(Some(SceneClockGroupId::new(12)))
    );
    assert_eq!(
        mirror.scene_clock_group(monitor(3), Generation::from_value(4)),
        Ok(Some(SceneClockGroupId::new(12)))
    );
    let local_a = mirror
        .output_transform(monitor(1), Generation::from_value(4))
        .unwrap()
        .desktop_to_output_logical(
            Generation::from_value(4),
            DesktopLogicalPoint::new(-500.0, 500.0).unwrap(),
        )
        .unwrap();
    let local_b = mirror
        .output_transform(monitor(3), Generation::from_value(4))
        .unwrap()
        .desktop_to_output_logical(
            Generation::from_value(4),
            DesktopLogicalPoint::new(-500.0, 500.0).unwrap(),
        )
        .unwrap();
    assert_eq!((local_a.x(), local_a.y()), (500.0, 500.0));
    assert_eq!((local_b.x(), local_b.y()), (500.0, 500.0));
    let transform_a = mirror
        .output_transform(monitor(1), Generation::from_value(4))
        .unwrap();
    let transform_b = mirror
        .output_transform(monitor(3), Generation::from_value(4))
        .unwrap();
    let surface_a = transform_a
        .output_logical_to_surface(Generation::from_value(4), local_a)
        .unwrap();
    let surface_b = transform_b
        .output_logical_to_surface(Generation::from_value(4), local_b)
        .unwrap();
    assert_eq!((surface_a.x(), surface_a.y()), (500.0, 500.0));
    assert_eq!((surface_b.x(), surface_b.y()), (625.0, 750.0));
}

#[test]
fn clock_epochs_fixed_steps_pause_clicks_and_window_force_decay_are_explicit() {
    let domain = ClockDomainId::new(9);
    let mapping = ClockDomainMapping::new(domain, 2, MonotonicEpoch::new(3), 1_000, 500, 1, 2)
        .unwrap();
    assert_eq!(
        mapping.map(domain, 2, 1_020).unwrap(),
        MonotonicInstant::new(MonotonicEpoch::new(3), 510)
    );
    assert_eq!(
        mapping.map(domain, 3, 1_020),
        Err(ClockMappingError::WrongSourceEpoch)
    );
    let mut epochs = ClockEpochTracker::new(MonotonicEpoch::new(3));
    assert_eq!(epochs.advance(), Ok(MonotonicEpoch::new(4)));

    let mut scheduler = FixedStepScheduler::new(100).unwrap();
    assert_eq!(scheduler.step_for(MonotonicInstant::new(MonotonicEpoch::new(3), 500)), Ok(0));
    assert_eq!(scheduler.step_for(MonotonicInstant::new(MonotonicEpoch::new(3), 599)), Ok(0));
    assert_eq!(scheduler.step_for(MonotonicInstant::new(MonotonicEpoch::new(3), 600)), Ok(1));
    assert_eq!(
        scheduler.step_for(MonotonicInstant::new(MonotonicEpoch::new(3), 550)),
        Err(FixedStepError::OutOfOrder)
    );
    assert_eq!(
        scheduler.step_for(MonotonicInstant::new(MonotonicEpoch::new(3), 599)),
        Err(FixedStepError::OutOfOrder)
    );

    let generation = Generation::from_value(4);
    let mut gate = EventFreshnessGate::new(generation, MonotonicEpoch::new(3), 1_000);
    let pause = event(1, 100, 4, EventPayload::UserCommand(UserCommand::Pause));
    let resume = event(2, 200, 4, EventPayload::UserCommand(UserCommand::Resume));
    assert_eq!(gate.accept(pause, MonotonicInstant::new(MonotonicEpoch::new(3), 100)), Ok(()));
    assert_eq!(gate.accept(resume, MonotonicInstant::new(MonotonicEpoch::new(3), 200)), Ok(()));
    let old_click = event(3, 150, 4, button(ButtonState::Pressed));
    assert_eq!(
        gate.accept(old_click, MonotonicInstant::new(MonotonicEpoch::new(3), 210)),
        Err(ascii_core::coordinates::EventRejection::StaleClick)
    );
    let current_click = event(4, 205, 4, button(ButtonState::Pressed));
    assert_eq!(gate.accept(current_click, MonotonicInstant::new(MonotonicEpoch::new(3), 210)), Ok(()));
    assert_eq!(
        gate.replace_generation(Generation::from_value(5)),
        Ok(())
    );
    assert_eq!(
        gate.accept(current_click, MonotonicInstant::new(MonotonicEpoch::new(3), 210)),
        Err(ascii_core::coordinates::EventRejection::StaleGeneration)
    );
    let mut topology_gate =
        EventFreshnessGate::new(Generation::from_value(1), MonotonicEpoch::new(3), 1_000);
    let topology_event = event(
        1,
        220,
        1,
        EventPayload::TopologyChanged(
            TopologyChange::new(Generation::from_value(1), Generation::from_value(2)).unwrap(),
        ),
    );
    assert_eq!(
        topology_gate.accept(topology_event, MonotonicInstant::new(MonotonicEpoch::new(3), 220)),
        Ok(())
    );
    let stale_after_hotplug = event(2, 225, 1, button(ButtonState::Pressed));
    assert_eq!(
        topology_gate.accept(
            stale_after_hotplug,
            MonotonicInstant::new(MonotonicEpoch::new(3), 230),
        ),
        Err(ascii_core::coordinates::EventRejection::StaleGeneration)
    );

    let known_bounds = Observation::known(DesktopRect::new(-10.0, 0.0, 20.0, 30.0).unwrap());
    let missing_bounds = Observation::<DesktopRect>::missing(MissingReason::NotObserved);
    let mut freshness = WindowForceFreshness::new(100, 200).unwrap();
    freshness.observe(MonotonicInstant::new(MonotonicEpoch::new(3), 0), &known_bounds);
    freshness.observe(MonotonicInstant::new(MonotonicEpoch::new(3), 50), &missing_bounds);
    assert_eq!(freshness.weight_at(MonotonicInstant::new(MonotonicEpoch::new(3), 100)), 1.0);
    assert_eq!(freshness.weight_at(MonotonicInstant::new(MonotonicEpoch::new(3), 200)), 0.5);
    assert_eq!(freshness.weight_at(MonotonicInstant::new(MonotonicEpoch::new(3), 300)), 0.0);
    assert_eq!(freshness.weight_at(MonotonicInstant::new(MonotonicEpoch::new(4), 150)), 0.0);
}
