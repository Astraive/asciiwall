#[allow(dead_code)]
mod synthetic_font {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ascii-convert/tests/support/synthetic_font.rs"
    ));
}

use std::time::{Duration, Instant};

use ascii_convert::{GlyphConfig, GlyphRepertoire, build_glyph_table};
use ascii_core::{Budget, CancellationToken};
use ascii_scene::{
    ApprovedSnapshots, NormalizedPoint, SceneDimensions, SceneParameters, SceneResourceBudget,
    SceneRuntime, SnapshotCandidates, SnapshotCapabilities, WaveFieldConfig, WaveFieldGenerator,
    WAVE_FIELD_DESCRIPTOR, WAVE_FIELD_PRESETS,
};

fn table() -> ascii_convert::GlyphTable {
    let font = synthetic_font::synthetic_font(false);
    let repertoire = GlyphRepertoire::new("BA ").unwrap();
    let geometry = GlyphConfig::new(8.0, 8, 12, 9).unwrap();
    build_glyph_table(
        &font,
        &repertoire,
        &geometry,
        &Budget::new(1_000_000, 1_000_000, 1_000_000),
        &CancellationToken::new(),
    )
    .unwrap()
}

fn runtime<'a>(
    config: WaveFieldConfig,
    glyphs: &'a ascii_convert::GlyphTable,
    columns: u32,
    rows: u32,
    simulation_hz: u16,
) -> SceneRuntime<WaveFieldGenerator<'a>> {
    let dimensions = SceneDimensions::new(columns, rows).unwrap();
    let parameters = SceneParameters::new(0x0155, 1.0, 2_000, simulation_hz, true).unwrap();
    SceneRuntime::try_new(
        parameters,
        dimensions,
        WaveFieldGenerator::new(glyphs, config),
        &SceneResourceBudget::default(),
    )
    .unwrap()
}

fn hash(frame: &[char]) -> u64 {
    frame.iter().fold(0xcbf2_9ce4_8422_2325, |hash, character| {
        (hash ^ u64::from(*character as u32)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn advance_for(runtime: &mut SceneRuntime<WaveFieldGenerator<'_>>, frames_per_second: u32) {
    let elapsed = Duration::from_nanos(1_000_000_000 / u64::from(frames_per_second));
    let snapshots = ApprovedSnapshots::empty();
    for _ in 0..frames_per_second * 60 {
        runtime.advance(elapsed, &snapshots).unwrap();
    }
}

#[test]
fn preset_bounds_and_exact_values_are_explicit() {
    let glyphs = table();
    assert_eq!(
        glyphs.fingerprint(),
        &[
            0x4d, 0xdc, 0x3a, 0x15, 0x85, 0xb2, 0x33, 0x27, 0x8e, 0x70, 0x50, 0xa0, 0x5e, 0xb9,
            0x5b, 0x77, 0x79, 0x16, 0x37, 0x5d, 0x3e, 0x61, 0x9e, 0xfb, 0x8c, 0x76, 0xcb, 0xd8,
            0x5d, 0xb6, 0x48, 0xe0,
        ]
    );
    assert_eq!(WAVE_FIELD_DESCRIPTOR.id, "builtin.wave-field");
    assert!(WAVE_FIELD_DESCRIPTOR.supported_signals.is_empty());
    assert!(WAVE_FIELD_DESCRIPTOR.presets.is_empty());
    assert!(!WAVE_FIELD_DESCRIPTOR.deterministic_export);
    assert_eq!(WAVE_FIELD_DESCRIPTOR.parameters.len(), 4);
    assert_eq!(WAVE_FIELD_PRESETS.len(), 3);
    let calm = WAVE_FIELD_PRESETS[0];
    assert_eq!(calm.name, "calm");
    assert_eq!(calm.config.amplitude(), 0.5);
    assert_eq!(calm.config.frequency(), 2.0);
    assert_eq!(calm.config.speed(), 0.5);
    assert_eq!(calm.config.color_rgba(), 0x66cc_ffff);
    let contour = WAVE_FIELD_PRESETS[1];
    assert_eq!(contour.name, "contour");
    assert_eq!(contour.config.amplitude(), 2.0);
    assert_eq!(contour.config.frequency(), 5.0);
    assert_eq!(contour.config.speed(), 1.0);
    assert_eq!(contour.config.color_rgba(), 0xffff_ffff);
    let still = WAVE_FIELD_PRESETS[2];
    assert_eq!(still.name, "still");
    assert_eq!(still.config.speed(), 0.0);
    assert!(WaveFieldConfig::new(10.0, 20.0, 10.0, 0).is_ok());
    assert!(WaveFieldConfig::new(-0.01, 2.0, 0.5, 0).is_err());
    assert!(WaveFieldConfig::new(0.5, 20.01, 0.5, 0).is_err());
    assert!(WaveFieldConfig::new(0.5, 2.0, f32::INFINITY, 0).is_err());
    assert!(WaveFieldConfig::from_hex_rgba(1.0, 2.0, 1.0, "66ccffff").is_err());
    assert_eq!(WaveFieldConfig::from_hex_rgba(1.0, 2.0, 1.0, "#66ccffff").unwrap().color_rgba(), 0x66cc_ffff);
}

#[test]
fn simulation_hash_matches_at_20_30_and_60_presentations() {
    let glyphs = table();
    let mut hashes = [0; 3];
    for (index, fps) in [20, 30, 60].into_iter().enumerate() {
        let mut runtime = runtime(WaveFieldConfig::calm(), &glyphs, 80, 40, 60);
        advance_for(&mut runtime, fps);
        hashes[index] = hash(runtime.frame());
        assert_eq!(runtime.simulation_step_count(), 3_600);
        eprintln!(
            "wave-fixture fps={fps} glyph_table={} cell_hash={:016x}",
            glyphs.fingerprint().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            hashes[index],
        );
    }
    assert_eq!(hashes[0], hashes[1]);
    assert_eq!(hashes[1], hashes[2]);
}

#[test]
fn zero_frequency_and_amplitude_remain_finite_and_bounded() {
    let glyphs = table();
    for config in [
        WaveFieldConfig::new(0.0, 0.0, 0.0, 0).unwrap(),
        WaveFieldConfig::new(10.0, 0.0, 10.0, 0xffff_ffff).unwrap(),
        WaveFieldConfig::new(0.0, 20.0, 10.0, 0).unwrap(),
        WaveFieldConfig::new(10.0, 20.0, 0.0, 0).unwrap(),
    ] {
        let mut runtime = runtime(config, &glyphs, 160, 90, 60);
        let initial = hash(runtime.frame());
        let snapshots = ApprovedSnapshots::empty();
        for _ in 0..600 {
            runtime.advance(Duration::from_millis(10), &snapshots).unwrap();
            assert!(runtime.frame().iter().all(|cell| glyphs.glyphs().iter().any(|g| g.character() == *cell)));
        }
        if config.amplitude_q16() == 0 {
            assert!(runtime.frame().iter().all(|cell| *cell == runtime.frame()[0]));
        }
        if config.frequency_q16() == 0 {
            assert!(runtime.frame().iter().all(|cell| *cell == runtime.frame()[0]));
        } else if config.speed_q16() == 0 {
            assert_eq!(hash(runtime.frame()), initial);
        }
    }
}

#[test]
fn deterministic_seed_and_unsupported_inputs_do_not_create_ripples() {
    let glyphs = table();
    let mut first = runtime(WaveFieldConfig::contour(), &glyphs, 64, 36, 60);
    let mut second = runtime(WaveFieldConfig::contour(), &glyphs, 64, 36, 60);
    let pointer = NormalizedPoint::new(32_768, 32_768);
    let snapshots = ApprovedSnapshots::filter(
        SnapshotCapabilities::new(true, false, false, false),
        SnapshotCandidates::new(Some(pointer), None, None, None),
    );
    for _ in 0..300 {
        first.advance(Duration::from_millis(16), &snapshots).unwrap();
        second.advance(Duration::from_millis(16), &ApprovedSnapshots::empty()).unwrap();
    }
    assert_eq!(hash(first.frame()), hash(second.frame()));
}

#[test]
fn still_and_reduced_motion_do_not_evolve() {
    let glyphs = table();
    let mut still = runtime(WaveFieldConfig::still(), &glyphs, 80, 40, 60);
    let before = hash(still.frame());
    still.advance(Duration::from_secs(60), &ApprovedSnapshots::empty()).unwrap();
    assert_eq!(hash(still.frame()), before);
    let dimensions = SceneDimensions::new(80, 40).unwrap();
    let parameters = SceneParameters::new(0x0155, 1.0, 2_000, 60, false).unwrap();
    let mut reduced = SceneRuntime::try_new(
        parameters,
        dimensions,
        WaveFieldGenerator::new(&glyphs, WaveFieldConfig::calm()),
        &SceneResourceBudget::default(),
    )
    .unwrap();
    let before = hash(reduced.frame());
    reduced.advance(Duration::from_secs(60), &ApprovedSnapshots::empty()).unwrap();
    assert_eq!(hash(reduced.frame()), before);
    assert_eq!(reduced.simulation_step_count(), 0);
}

#[test]
fn benchmark_27_sixty_second_timelines() {
    let glyphs = table();
    let cases = [
        ("calm", WaveFieldConfig::calm()),
        ("contour", WaveFieldConfig::contour()),
        ("still", WaveFieldConfig::still()),
    ];
    for (name, config) in cases {
        for columns in [160, 240, 320] {
            for fps in [20, 30, 60] {
                let rows = columns * 9 / 16;
                let mut runtime = runtime(config, &glyphs, columns, rows, 60);
                let elapsed = Duration::from_nanos(1_000_000_000 / fps);
                let snapshots = ApprovedSnapshots::empty();
                let mut timings = Vec::with_capacity((fps * 60) as usize);
                for _ in 0..fps * 60 {
                    let start = Instant::now();
                    runtime.advance(elapsed, &snapshots).unwrap();
                    timings.push(start.elapsed().as_nanos());
                }
                timings.sort_unstable();
                let percentile = |p: usize| timings[(timings.len() - 1) * p / 100];
                eprintln!(
                    "wave-bench preset={name} columns={columns} fps={fps} p50_ns={} p95_ns={} p99_ns={} logical_state_bytes={}",
                    percentile(50), percentile(95), percentile(99), runtime.resource_estimate().total_bytes(),
                );
            }
        }
    }
}

#[test]
fn config_and_grid_reject_before_runtime_reservation() {
    let glyphs = table();
    let budget = SceneResourceBudget::new(0);
    assert!(SceneDimensions::new(0, 10).is_err());
    assert!(SceneDimensions::new(1_025, 10).is_err());
    let parameters = SceneParameters::DEFAULT;
    assert!(WaveFieldConfig::new(f32::NAN, 2.0, 0.5, 0).is_err());
    assert_eq!(budget.reserved_bytes(), 0);
    assert!(SceneRuntime::try_new(
        parameters,
        SceneDimensions::new(10, 10).unwrap(),
        WaveFieldGenerator::new(&glyphs, WaveFieldConfig::calm()),
        &budget,
    ).is_err());
    assert_eq!(budget.reserved_bytes(), 0);
}
