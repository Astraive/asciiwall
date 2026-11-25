use std::time::Duration;

use ascii_core::{BudgetKind, MalformedReason};
use ascii_scene::Error;
use ascii_scene::{
    ApprovedSnapshots, ExportContext, ExportSignalTrack, GENERATOR_DESCRIPTOR,
    MAX_APPROVED_BUTTON_EVENTS, MAX_CATCH_UP_STEPS, MAX_GRID_CELLS, MAX_NODE_ID_BYTES,
    MAX_PARTICLES, MAX_RECORDED_EVENTS, MAX_RUNTIME_RESOURCE_BYTES, NormalizedPoint,
    NormalizedRect, ParameterDefault, ParameterKind, ParticleFieldGenerator, PresetMotion,
    RandomStream, RecordedEvent, RecordedEventKind, SceneDimensions, SceneParameters,
    SceneResourceBudget, SceneRuntime, SimulationLimits, SnapshotCandidates, SnapshotCapabilities,
    derive_effect_seed, derive_node_seed,
};

fn parameters(
    seed: u32,
    time_scale: f32,
    max_particles: u32,
    background_motion: bool,
) -> SceneParameters {
    SceneParameters::new(seed, time_scale, max_particles, 60, background_motion).unwrap()
}

fn runtime(
    parameters: SceneParameters,
    dimensions: SceneDimensions,
    budget: &SceneResourceBudget,
) -> SceneRuntime<ParticleFieldGenerator<'static>> {
    SceneRuntime::try_new(
        parameters,
        dimensions,
        ParticleFieldGenerator::default(),
        budget,
    )
    .unwrap()
}

fn recorded_context<'a>(
    parameters: SceneParameters,
    events: &'a [RecordedEvent],
) -> ExportContext<'a> {
    ExportContext::new(
        parameters,
        1_735_689_600_000,
        [0; 32],
        [1; 32],
        ExportSignalTrack::Recorded(events),
    )
    .unwrap()
}

#[test]
fn same_seed_and_fixed_track_produce_exact_character_cells() {
    let dimensions = SceneDimensions::new(32, 16).unwrap();
    let budget = SceneResourceBudget::default();
    let snapshots = ApprovedSnapshots::empty();

    for seed in [0, u32::MAX] {
        let parameters = parameters(seed, 1.0, 64, true);
        let mut first = runtime(parameters, dimensions, &budget);
        let mut second = runtime(parameters, dimensions, &budget);

        for _ in 0..600 {
            first
                .advance(Duration::from_nanos(16_666_667), &snapshots)
                .unwrap();
            second
                .advance(Duration::from_nanos(16_666_667), &snapshots)
                .unwrap();
        }

        assert_eq!(first.simulation_step_count(), 600);
        assert_eq!(first.frame(), second.frame());
    }
}

#[test]
fn independent_node_streams_do_not_perturb_existing_sequences() {
    let mut baseline = RandomStream::for_node(0, "node.existing").unwrap();
    let expected: Vec<u64> = (0..8).map(|_| baseline.next_u64()).collect();

    let mut with_added_node = RandomStream::for_node(0, "node.existing").unwrap();
    for value in &expected[..4] {
        assert_eq!(with_added_node.next_u64(), *value);
    }
    let mut added_node = RandomStream::for_node(0, "node.added").unwrap();
    for _ in 0..8 {
        let _ = added_node.next_u64();
    }
    for value in &expected[4..] {
        assert_eq!(with_added_node.next_u64(), *value);
    }
    assert_ne!(
        derive_node_seed(u32::MAX, "node.existing").unwrap(),
        derive_node_seed(u32::MAX, "node.added").unwrap()
    );
    assert_eq!(
        derive_node_seed(0, "node.existing").unwrap(),
        0xedc4_3e93_cd78_0788
    );
    assert_eq!(
        derive_node_seed(u32::MAX, "node.existing").unwrap(),
        0x45d1_f985_7c9f_a5de
    );
}

#[test]
fn effect_streams_are_stable_when_another_effect_is_added() {
    let mut baseline = RandomStream::for_effect(1, "node", "existing-effect").unwrap();
    let expected: Vec<u64> = (0..8).map(|_| baseline.next_u64()).collect();
    let mut with_added_effect = RandomStream::for_effect(1, "node", "existing-effect").unwrap();
    for value in &expected[..4] {
        assert_eq!(with_added_effect.next_u64(), *value);
    }
    let mut added_effect = RandomStream::for_effect(1, "node", "new-effect").unwrap();
    for _ in 0..8 {
        let _ = added_effect.next_u64();
    }
    for value in &expected[4..] {
        assert_eq!(with_added_effect.next_u64(), *value);
    }
    assert_ne!(
        derive_effect_seed(1, "node", "existing-effect").unwrap(),
        derive_effect_seed(1, "node", "new-effect").unwrap()
    );
    assert_eq!(
        derive_effect_seed(1, "node", "existing-effect").unwrap(),
        0x8575_0a7c_9e60_2002
    );
}

#[test]
fn splitmix64_reference_outputs_pin_the_prng_transition() {
    let mut stream = RandomStream::new(0);
    assert_eq!(stream.next_u64(), 0xe220_a839_7b1d_cdaf);
    assert_eq!(stream.next_u64(), 0x6e78_9e6a_a1b9_65f4);
}
#[test]
fn descriptor_advertises_versioned_parameters_limits_and_estimated_presets() {
    assert_eq!(GENERATOR_DESCRIPTOR.id, "builtin.particle-field");
    assert_eq!(GENERATOR_DESCRIPTOR.version, 1);
    assert_eq!(GENERATOR_DESCRIPTOR.parameter_schema_version, 1);
    assert_eq!(GENERATOR_DESCRIPTOR.parameters.len(), 5);
    assert_eq!(GENERATOR_DESCRIPTOR.limits.max_columns, 1_024);
    assert_eq!(GENERATOR_DESCRIPTOR.limits.max_rows, 1_024);
    assert_eq!(GENERATOR_DESCRIPTOR.limits.max_cells, MAX_GRID_CELLS);
    assert_eq!(GENERATOR_DESCRIPTOR.limits.max_particles, MAX_PARTICLES);
    assert_eq!(
        GENERATOR_DESCRIPTOR.limits.max_runtime_resource_bytes,
        MAX_RUNTIME_RESOURCE_BYTES
    );
    assert_eq!(
        GENERATOR_DESCRIPTOR.limits.max_work_units_per_advance,
        1_000_000
    );
    assert_eq!(GENERATOR_DESCRIPTOR.limits.max_lifetime_ticks, 7_200);
    assert_eq!(
        GENERATOR_DESCRIPTOR.reduced_motion,
        ascii_scene::MotionPolicy::FreezeWhenBackgroundMotionDisabled
    );
    assert_eq!(
        GENERATOR_DESCRIPTOR
            .resource_estimate
            .character_bytes_per_cell,
        core::mem::size_of::<char>() as u64
    );
    assert_eq!(
        GENERATOR_DESCRIPTOR.resource_estimate.particle_record_bytes,
        32
    );
    const {
        assert!(
            GENERATOR_DESCRIPTOR
                .resource_estimate
                .excludes_allocator_overhead
        );
    };
    assert_eq!(
        GENERATOR_DESCRIPTOR.limits.max_node_id_bytes,
        MAX_NODE_ID_BYTES
    );
    assert_eq!(GENERATOR_DESCRIPTOR.limits.default_catch_up_steps, 4);
    assert_eq!(GENERATOR_DESCRIPTOR.limits.max_catch_up_steps, 8);
    assert_eq!(GENERATOR_DESCRIPTOR.limits.max_velocity_q16, 64 << 16);
    assert_eq!(GENERATOR_DESCRIPTOR.limits.max_acceleration_q16, 16 << 16);
    const {
        assert!(GENERATOR_DESCRIPTOR.deterministic_export);
    };
    assert_eq!(
        GENERATOR_DESCRIPTOR.supported_signals,
        &[ascii_scene::SignalKind::PointerPosition]
    );
    assert_eq!(GENERATOR_DESCRIPTOR.presets.len(), 2);
    assert!(
        GENERATOR_DESCRIPTOR
            .presets
            .iter()
            .all(|preset| preset.cost_is_estimate)
    );
    let seed = GENERATOR_DESCRIPTOR
        .parameters
        .iter()
        .find(|parameter| parameter.name == "seed")
        .unwrap();
    assert_eq!(seed.group, "Randomness");
    assert_eq!(seed.kind, ParameterKind::U32);
    assert_eq!(seed.default, ParameterDefault::U32(1));
    let simulation_hz = GENERATOR_DESCRIPTOR
        .parameters
        .iter()
        .find(|parameter| parameter.name == "simulation_hz")
        .unwrap();
    assert_eq!(simulation_hz.kind, ParameterKind::U16);
    assert_eq!(simulation_hz.default, ParameterDefault::U16(60));
    assert_eq!(GENERATOR_DESCRIPTOR.presets[0].seed, 1);
    assert_eq!(GENERATOR_DESCRIPTOR.presets[0].simulation_hz, 60);
    assert_eq!(GENERATOR_DESCRIPTOR.presets[1].motion, PresetMotion::Static);
    assert!(!GENERATOR_DESCRIPTOR.presets[1].background_motion);
}

#[test]
fn typed_scene_defaults_match_the_published_parameter_contract() {
    let defaults = SceneParameters::default();
    assert_eq!(defaults.seed(), 1);
    assert_eq!(defaults.time_scale(), 1.0);
    assert_eq!(defaults.time_scale_q16(), 1 << 16);
    assert_eq!(
        SceneParameters::new(1, 0.5, 2_000, 60, true)
            .unwrap()
            .time_scale_q16(),
        1 << 15
    );
    assert_eq!(defaults.max_particles(), 2_000);
    assert_eq!(defaults.simulation_hz(), 60);
    assert!(defaults.background_motion());
}

#[test]
fn node_identifiers_and_export_event_tracks_have_explicit_work_caps() {
    let oversized_node = "n".repeat(MAX_NODE_ID_BYTES + 1);
    assert!(matches!(
        derive_node_seed(1, &oversized_node),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    ));
    assert!(matches!(
        RandomStream::for_node(1, ""),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    ));
    assert!(derive_node_seed(1, &"n".repeat(MAX_NODE_ID_BYTES)).is_ok());
    assert!(matches!(
        derive_effect_seed(1, "node", &oversized_node),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    ));
    assert!(matches!(
        derive_effect_seed(1, "node", ""),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    ));

    let events = vec![
        RecordedEvent::new(
            0,
            RecordedEventKind::PointerButton {
                button: 1,
                pressed: true
            }
        );
        MAX_RECORDED_EVENTS + 1
    ];
    assert!(matches!(
        ExportContext::new(
            parameters(1, 1.0, 0, true),
            0,
            [0; 32],
            [0; 32],
            ExportSignalTrack::Recorded(&events),
        ),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    ));
}

#[test]
fn unsigned_seed_extremes_round_trip_in_export_context() {
    for seed in [0, u32::MAX] {
        let parameters = parameters(seed, 1.0, 0, true);
        let export = ExportContext::new(
            parameters,
            1_735_689_600_000,
            [0x11; 32],
            [0x22; 32],
            ExportSignalTrack::IdleFallback,
        )
        .unwrap();
        assert_eq!(u32::from_le_bytes(export.seed_le_bytes()), seed);
        assert_eq!(export.parameters().seed(), seed);
        assert_eq!(export.simulation_hz(), 60);
        assert_eq!(export.selected_start_timestamp_unix_ms(), 1_735_689_600_000);
    }
}

#[test]
fn configured_catch_up_limit_is_respected() {
    assert_eq!(MAX_CATCH_UP_STEPS, 8);
    let mut scene = SceneRuntime::try_new_with_limits(
        parameters(1, 1.0, 16, true),
        SceneDimensions::new(16, 8).unwrap(),
        ParticleFieldGenerator::default(),
        &SceneResourceBudget::default(),
        SimulationLimits::new(8).unwrap(),
    )
    .unwrap();
    let report = scene
        .advance(Duration::from_secs(60), &ApprovedSnapshots::empty())
        .unwrap();
    assert_eq!(report.simulation_steps(), 8);
    assert!(report.elapsed_clamped());
}

#[test]
fn resume_gap_is_clamped_to_the_four_step_default() {
    let mut scene = runtime(
        parameters(1, 1.0, 32, true),
        SceneDimensions::new(16, 8).unwrap(),
        &SceneResourceBudget::default(),
    );
    let report = scene
        .advance(Duration::from_secs(60), &ApprovedSnapshots::empty())
        .unwrap();

    assert_eq!(report.simulation_steps(), 4);
    assert!(report.elapsed_clamped());
    assert_eq!(scene.simulation_step_count(), 4);
}

#[test]
fn zero_scale_and_static_motion_perform_no_simulation_or_render_work() {
    for parameters in [parameters(1, 0.0, 32, true), parameters(1, 1.0, 32, false)] {
        let mut scene = runtime(
            parameters,
            SceneDimensions::new(16, 8).unwrap(),
            &SceneResourceBudget::default(),
        );
        let frame_pointer = scene.frame().as_ptr();
        let frame_before = scene.frame().to_vec();
        let report = scene
            .advance(Duration::from_secs(60), &ApprovedSnapshots::empty())
            .unwrap();
        assert_eq!(scene.resource_estimate().work_units(), 0);

        assert_eq!(report.simulation_steps(), 0);
        assert!(!report.output_refreshed());
        assert_eq!(scene.simulation_step_count(), 0);
        assert_eq!(scene.frame().as_ptr(), frame_pointer);
        assert_eq!(scene.frame(), frame_before);
    }
}

#[test]
fn runtime_reuses_its_character_buffer() {
    let mut scene = runtime(
        parameters(1, 1.0, 32, true),
        SceneDimensions::new(16, 8).unwrap(),
        &SceneResourceBudget::default(),
    );
    let frame_pointer = scene.frame().as_ptr();
    let capacity = scene.frame_storage_capacity();
    scene
        .advance(Duration::from_millis(17), &ApprovedSnapshots::empty())
        .unwrap();

    assert_eq!(scene.frame().as_ptr(), frame_pointer);
    assert_eq!(scene.frame_storage_capacity(), capacity);
}

#[test]
fn dimensions_nonfinite_parameters_and_caps_fail_before_reservation() {
    assert_eq!(
        SceneDimensions::new(0, 10).unwrap_err(),
        Error::Malformed(MalformedReason::EmptyDimensions)
    );
    assert_eq!(
        SceneDimensions::new(1_024, 1_024).unwrap_err(),
        Error::BudgetExceeded(BudgetKind::WorkUnits)
    );
    assert_eq!(MAX_GRID_CELLS, 65_536);
    assert_eq!(MAX_PARTICLES, 20_000);
    for time_scale in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 4.01] {
        assert_eq!(
            SceneParameters::new(1, time_scale, 10, 60, true).unwrap_err(),
            Error::Malformed(MalformedReason::InvalidConfiguration)
        );
    }
    assert_eq!(
        SceneParameters::new(1, 1.0, MAX_PARTICLES + 1, 60, true).unwrap_err(),
        Error::Malformed(MalformedReason::InvalidConfiguration)
    );
    for simulation_hz in [14, 121] {
        assert_eq!(
            SceneParameters::new(1, 1.0, 10, simulation_hz, true).unwrap_err(),
            Error::Malformed(MalformedReason::InvalidConfiguration)
        );
    }

    let budget = SceneResourceBudget::new(100);
    let result = SceneRuntime::try_new(
        parameters(1, 1.0, 10, true),
        SceneDimensions::new(4, 4).unwrap(),
        ParticleFieldGenerator::default(),
        &budget,
    );
    assert!(matches!(
        result,
        Err(Error::BudgetExceeded(BudgetKind::FrameBytes))
    ));
    assert_eq!(budget.reserved_bytes(), 0);
}

#[test]
fn four_maximal_preview_estimates_stay_within_shared_aggregate_budget() {
    let parameters = parameters(1, 1.0, MAX_PARTICLES, true);
    let dimensions = SceneDimensions::new(256, 256).unwrap();
    let budget = SceneResourceBudget::default();
    let mut previews = Vec::new();

    for _ in 0..4 {
        previews.push(runtime(parameters, dimensions, &budget));
    }

    let per_preview = previews[0].resource_estimate().total_bytes();
    assert_eq!(per_preview, 902_144);
    assert_eq!(budget.maximum_bytes(), MAX_RUNTIME_RESOURCE_BYTES * 4);
    assert!(per_preview <= MAX_RUNTIME_RESOURCE_BYTES);
    assert_eq!(budget.reserved_bytes(), per_preview * 4);
    assert!(budget.reserved_bytes() <= budget.maximum_bytes());
    drop(previews.pop());
    assert_eq!(budget.reserved_bytes(), per_preview * 3);
}

#[test]
fn snapshots_are_filtered_by_explicit_host_capabilities() {
    let rectangle = NormalizedRect::new(1, 2, 100, 200).unwrap();
    let button = ascii_scene::NormalizedButtonEvent::new(1, true);
    let battery = ascii_scene::BatterySnapshot::new(80, true).unwrap();
    let candidates = SnapshotCandidates::new(
        Some(NormalizedPoint::new(300, 400)),
        Some(std::slice::from_ref(&button)),
        Some(std::slice::from_ref(&rectangle)),
        Some(battery),
    );
    let denied = ApprovedSnapshots::filter(SnapshotCapabilities::default(), candidates);
    assert_eq!(denied.pointer_position(), None);
    assert_eq!(denied.pointer_buttons(), None);
    assert_eq!(denied.window_rectangles(), None);
    assert_eq!(denied.battery_state(), None);

    let pointer_only = ApprovedSnapshots::filter(
        SnapshotCapabilities::new(true, true, false, false),
        candidates,
    );
    assert_eq!(
        pointer_only.pointer_position(),
        Some(NormalizedPoint::new(300, 400))
    );
    assert_eq!(
        pointer_only.pointer_buttons(),
        Some(std::slice::from_ref(&button))
    );
    assert_eq!(pointer_only.window_rectangles(), None);
    assert_eq!(pointer_only.battery_state(), None);
    let battery_only = ApprovedSnapshots::filter(
        SnapshotCapabilities::new(false, false, false, true),
        candidates,
    );
    assert_eq!(battery_only.battery_state(), Some(battery));
}

#[test]
fn over_limit_button_tracks_are_rejected_without_retaining_a_prefix() {
    let event = ascii_scene::NormalizedButtonEvent::new(0, true);
    let events = vec![event; MAX_APPROVED_BUTTON_EVENTS + 1];
    let capabilities = SnapshotCapabilities::new(false, true, false, false);

    let accepted = ApprovedSnapshots::filter(
        capabilities,
        SnapshotCandidates::new(
            None,
            Some(&events[..MAX_APPROVED_BUTTON_EVENTS]),
            None,
            None,
        ),
    );
    assert_eq!(
        accepted.pointer_buttons().map(|track| track.len()),
        Some(MAX_APPROVED_BUTTON_EVENTS)
    );
    assert!(!accepted.pointer_buttons_rejected());

    let rejected = ApprovedSnapshots::filter(
        capabilities,
        SnapshotCandidates::new(None, Some(&events), None, None),
    );
    assert_eq!(rejected.pointer_buttons(), None);
    assert!(rejected.pointer_buttons_rejected());

    let denied = ApprovedSnapshots::filter(
        SnapshotCapabilities::default(),
        SnapshotCandidates::new(None, Some(&events), None, None),
    );
    assert!(!denied.pointer_buttons_rejected());
}
#[test]
fn synthetic_pointer_snapshots_drive_deterministic_local_interaction() {
    let dimensions = SceneDimensions::new(32, 16).unwrap();
    let parameters = parameters(0, 1.0, 64, true);
    let budget = SceneResourceBudget::default();
    let mut idle = runtime(parameters, dimensions, &budget);
    let mut first = runtime(parameters, dimensions, &budget);
    let mut second = runtime(parameters, dimensions, &budget);
    let snapshots = ApprovedSnapshots::filter(
        SnapshotCapabilities::new(true, false, false, false),
        SnapshotCandidates::new(Some(NormalizedPoint::new(0, 0)), None, None, None),
    );
    let no_input = ApprovedSnapshots::empty();

    for _ in 0..600 {
        idle.advance(Duration::from_nanos(16_666_667), &no_input)
            .unwrap();
        first
            .advance(Duration::from_nanos(16_666_667), &snapshots)
            .unwrap();
        second
            .advance(Duration::from_nanos(16_666_667), &snapshots)
            .unwrap();
    }

    assert_eq!(first.frame(), second.frame());
    assert_ne!(idle.frame(), first.frame());
}

#[test]
fn export_records_normalized_events_or_requires_an_explicit_idle_fallback() {
    let parameters = parameters(1, 1.0, 0, true);
    let events = [
        RecordedEvent::new(
            1,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(1, 2)),
        ),
        RecordedEvent::new(
            1,
            RecordedEventKind::PointerButton {
                button: 1,
                pressed: true,
            },
        ),
        RecordedEvent::new(
            3,
            RecordedEventKind::PointerButton {
                button: 1,
                pressed: false,
            },
        ),
    ];
    let export = ExportContext::new(
        parameters,
        1_735_689_600_000,
        [0; 32],
        [1; 32],
        ExportSignalTrack::Recorded(&events),
    )
    .unwrap();
    assert_eq!(export.parameters(), parameters);
    assert_eq!(export.seed_le_bytes(), [1, 0, 0, 0]);
    assert_eq!(export.selected_start_timestamp_unix_ms(), 1_735_689_600_000);
    assert_eq!(export.simulation_hz(), 60);
    assert_eq!(export.settings_fingerprint(), [0; 32]);
    assert_eq!(export.font_identity(), [1; 32]);
    assert!(matches!(export.signal_track(), ExportSignalTrack::Recorded(track) if track == events));

    let event_budget = SceneResourceBudget::default();
    assert!(matches!(
        export.execute_frame(SceneDimensions::new(16, 8).unwrap(), 3, &event_budget),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    ));
    assert_eq!(event_budget.reserved_bytes(), 0);

    let unordered = [events[2], events[0]];
    assert_eq!(
        ExportContext::new(
            parameters,
            1_735_689_600_000,
            [0; 32],
            [1; 32],
            ExportSignalTrack::Recorded(&unordered),
        )
        .unwrap_err(),
        Error::Malformed(MalformedReason::InvalidConfiguration)
    );
    let idle = ExportContext::new(
        parameters,
        1_735_689_600_000,
        [0; 32],
        [1; 32],
        ExportSignalTrack::IdleFallback,
    )
    .unwrap();
    assert!(matches!(
        idle.signal_track(),
        ExportSignalTrack::IdleFallback
    ));
    let budget = SceneResourceBudget::default();
    assert!(matches!(
        idle.execute_frame(SceneDimensions::new(1, 1).unwrap(), 1_000_001, &budget),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    ));
    assert_eq!(budget.reserved_bytes(), 0);
}
#[test]
fn identical_export_contexts_execute_exact_cells_for_seed_extremes() {
    let dimensions = SceneDimensions::new(32, 16).unwrap();
    let budget = SceneResourceBudget::default();

    for seed in [0, u32::MAX] {
        let context = || {
            ExportContext::new(
                parameters(seed, 1.0, 64, true),
                1_735_689_600_000,
                [0xA5; 32],
                [0x5A; 32],
                ExportSignalTrack::IdleFallback,
            )
            .unwrap()
        };
        let first = context().execute_frame(dimensions, 600, &budget).unwrap();
        let second = context().execute_frame(dimensions, 600, &budget).unwrap();

        assert_eq!(first.cells(), second.cells());
        assert_eq!(first.step_index(), 600);
        assert_eq!(first.parameters().seed(), seed);
        assert_eq!(u32::from_le_bytes(first.seed_le_bytes()), seed);
        assert_eq!(first.selected_start_timestamp_unix_ms(), 1_735_689_600_000);
        assert_eq!(first.settings_fingerprint(), [0xA5; 32]);
        assert_eq!(first.font_identity(), [0x5A; 32]);
        assert!(matches!(
            first.signal_track(),
            ExportSignalTrack::IdleFallback
        ));
    }
}
#[test]
fn recorded_pointer_tracks_replay_exactly_and_future_events_are_ignored() {
    let dimensions = SceneDimensions::new(32, 16).unwrap();
    let parameters = parameters(0, 1.0, 64, true);
    let track = [
        RecordedEvent::new(
            0,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(0, 0)),
        ),
        RecordedEvent::new(
            20,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(u16::MAX, u16::MAX)),
        ),
        RecordedEvent::new(
            20,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(0, u16::MAX)),
        ),
        RecordedEvent::new(
            90,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(0, 0)),
        ),
    ];
    let changed_track = [
        RecordedEvent::new(
            0,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(32_768, 32_768)),
        ),
        RecordedEvent::new(
            20,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(32_768, 32_768)),
        ),
        RecordedEvent::new(
            20,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(32_768, 32_768)),
        ),
        RecordedEvent::new(
            90,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(32_768, 32_768)),
        ),
    ];
    let budget = SceneResourceBudget::default();
    let first = recorded_context(parameters, &track)
        .execute_frame(dimensions, 600, &budget)
        .unwrap();
    let repeated = recorded_context(parameters, &track)
        .execute_frame(dimensions, 600, &budget)
        .unwrap();
    let changed = recorded_context(parameters, &changed_track)
        .execute_frame(dimensions, 600, &budget)
        .unwrap();
    assert_eq!(first.cells(), repeated.cells());
    assert_ne!(first.cells(), changed.cells());

    let duplicate_step = [
        RecordedEvent::new(
            1,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(0, 0)),
        ),
        RecordedEvent::new(
            1,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(32_768, 32_768)),
        ),
    ];
    let only_last = [duplicate_step[1]];
    assert_eq!(
        recorded_context(parameters, &duplicate_step)
            .execute_frame(dimensions, 600, &budget)
            .unwrap()
            .cells(),
        recorded_context(parameters, &only_last)
            .execute_frame(dimensions, 600, &budget)
            .unwrap()
            .cells()
    );

    let initial = [track[0]];
    let with_future = [
        track[0],
        RecordedEvent::new(
            601,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(32_768, 32_768)),
        ),
    ];
    assert_eq!(
        recorded_context(parameters, &initial)
            .execute_frame(dimensions, 600, &budget)
            .unwrap()
            .cells(),
        recorded_context(parameters, &with_future)
            .execute_frame(dimensions, 600, &budget)
            .unwrap()
            .cells()
    );
}
#[test]
fn export_charges_validation_and_replay_scans_before_reserving_resources() {
    let events = vec![
        RecordedEvent::new(
            0,
            RecordedEventKind::PointerPosition(NormalizedPoint::new(0, 0)),
        );
        MAX_RECORDED_EVENTS
    ];
    let context = recorded_context(parameters(1, 1.0, 0, true), &events);
    let budget = SceneResourceBudget::default();

    assert_eq!(events.len(), MAX_RECORDED_EVENTS);
    assert!(matches!(
        context.execute_frame(SceneDimensions::new(256, 256).unwrap(), 803_392, &budget,),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    ));
    assert_eq!(budget.reserved_bytes(), 0);
}

#[test]
fn aggregate_budget_rejects_the_next_runtime_without_leaking_a_reservation() {
    let parameters = parameters(1, 1.0, 2_000, true);
    let dimensions = SceneDimensions::new(32, 16).unwrap();
    let candidate = runtime(parameters, dimensions, &SceneResourceBudget::default());
    let budget = SceneResourceBudget::new(candidate.resource_estimate().total_bytes());
    let first = runtime(parameters, dimensions, &budget);

    let second = SceneRuntime::try_new(
        parameters,
        dimensions,
        ParticleFieldGenerator::default(),
        &budget,
    );
    assert!(matches!(
        second,
        Err(Error::BudgetExceeded(BudgetKind::FrameBytes))
    ));
    assert_eq!(
        budget.reserved_bytes(),
        first.resource_estimate().total_bytes()
    );
}
