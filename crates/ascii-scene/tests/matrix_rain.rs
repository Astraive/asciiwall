#[allow(dead_code)]
mod synthetic_font {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ascii-convert/tests/support/synthetic_font.rs"
    ));
}

use std::time::{Duration, Instant};

use ascii_convert::{GlyphConfig, GlyphRepertoire, build_glyph_table};
use ascii_core::{Budget, CancellationToken, MalformedReason};
use ascii_scene::{
    MATRIX_RAIN_DESCRIPTOR, MATRIX_RAIN_PRESETS, MatrixRainConfig, MatrixRainGenerator,
    SceneDimensions, SceneParameters, SceneResourceBudget, SceneRuntime,
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
    seed: u32,
    dimensions: SceneDimensions,
    config: MatrixRainConfig,
    table: &'a ascii_convert::GlyphTable,
) -> SceneRuntime<MatrixRainGenerator<'a>> {
    let parameters = SceneParameters::new(seed, 1.0, 0, 60, true).unwrap();
    SceneRuntime::try_new(
        parameters,
        dimensions,
        MatrixRainGenerator::new("matrix", config, table),
        &SceneResourceBudget::default(),
    )
    .unwrap()
}

fn frame_hash(frame: &[char]) -> u64 {
    frame.iter().fold(0xcbf2_9ce4_8422_2325, |hash, cell| {
        (hash ^ u64::from(*cell as u32)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn percentile(samples: &mut [u128], percentile: usize) -> u128 {
    samples.sort_unstable();
    samples[(samples.len() - 1) * percentile / 100]
}

#[test]
fn exact_parameters_presets_and_rejection_bounds_are_pinned() {
    let ambient = MatrixRainConfig::from_hex_rgba(0.2, 0.5, 20, "#00ff66ff").unwrap();
    assert_eq!(ambient, MatrixRainConfig::ambient());
    assert_eq!(MatrixRainConfig::dense().density_q16(), 52_429);
    assert_eq!(MatrixRainConfig::dense().speed_q16(), 131_072);
    assert_eq!(MatrixRainConfig::static_motion().speed_q16(), 0);
    assert_eq!(MATRIX_RAIN_DESCRIPTOR.id, "builtin.matrix-rain");
    assert_eq!(MATRIX_RAIN_DESCRIPTOR.parameters.len(), 4);
    assert!(MATRIX_RAIN_DESCRIPTOR.presets.is_empty());
    assert_eq!(MATRIX_RAIN_PRESETS.len(), 3);
    let ambient_preset = MATRIX_RAIN_PRESETS[0];
    let dense_preset = MATRIX_RAIN_PRESETS[1];
    let static_preset = MATRIX_RAIN_PRESETS[2];
    assert!(
        MATRIX_RAIN_PRESETS
            .iter()
            .all(|preset| preset.cost_is_estimate)
    );
    assert_eq!(ambient_preset.name, "ambient");
    assert_eq!(ambient_preset.config, ambient);
    assert_eq!(
        (
            dense_preset.config.density_q16(),
            dense_preset.config.speed_q16(),
            dense_preset.config.trail_cells(),
            dense_preset.config.color_rgba(),
        ),
        (52_429, 131_072, 60, 0x00ff_66ff)
    );
    assert_eq!(static_preset.name, "static");
    assert_eq!(static_preset.motion, ascii_scene::PresetMotion::Static);
    assert_eq!(
        (
            static_preset.config.speed_q16(),
            static_preset.config.trail_cells(),
            static_preset.config.color_rgba(),
        ),
        (0, 20, 0x00ff_66ff)
    );

    for invalid in [
        MatrixRainConfig::new(f32::NAN, 1.0, 20, 0),
        MatrixRainConfig::new(0.2, f32::INFINITY, 20, 0),
        MatrixRainConfig::new(-0.01, 1.0, 20, 0),
        MatrixRainConfig::new(1.01, 1.0, 20, 0),
        MatrixRainConfig::new(0.2, -0.01, 20, 0),
        MatrixRainConfig::new(0.2, 10.01, 20, 0),
        MatrixRainConfig::new(0.2, 1.0, 0, 0),
        MatrixRainConfig::new(0.2, 1.0, 201, 0),
        MatrixRainConfig::from_hex_rgba(0.2, 1.0, 20, "#00ff66"),
        MatrixRainConfig::from_hex_rgba(0.2, 1.0, 20, "#00ff66gg"),
    ] {
        assert!(matches!(
            invalid,
            Err(ascii_scene::Error::Malformed(
                MalformedReason::InvalidConfiguration
            ))
        ));
    }
}

#[test]
fn fixed_seed_and_timeline_reproduce_matrix_cells_and_stay_in_grid() {
    let glyphs = table();
    let dimensions = SceneDimensions::new(32, 16).unwrap();
    let config = MatrixRainConfig::ambient();
    let mut first = runtime(0x153, dimensions, config, &glyphs);
    let mut second = runtime(0x153, dimensions, config, &glyphs);
    let snapshots = ascii_scene::ApprovedSnapshots::empty();

    for _ in 0..600 {
        first
            .advance(Duration::from_nanos(16_666_667), &snapshots)
            .unwrap();
        second
            .advance(Duration::from_nanos(16_666_667), &snapshots)
            .unwrap();
    }

    assert_eq!(first.frame().len(), dimensions.cell_count());
    assert_eq!(first.frame(), second.frame());
    assert_eq!(frame_hash(first.frame()), frame_hash(second.frame()));
    assert_eq!(first.simulation_step_count(), 600);
    assert_eq!(
        *glyphs.fingerprint(),
        [
            0x4d, 0xdc, 0x3a, 0x15, 0x85, 0xb2, 0x33, 0x27, 0x8e, 0x70, 0x50, 0xa0, 0x5e, 0xb9,
            0x5b, 0x77, 0x79, 0x16, 0x37, 0x5d, 0x3e, 0x61, 0x9e, 0xfb, 0x8c, 0x76, 0xcb, 0xd8,
            0x5d, 0xb6, 0x48, 0xe0,
        ]
    );
    assert_eq!(frame_hash(first.frame()), 0x5e53_6b47_a9c9_df32);
    eprintln!(
        "matrix-fixture seed=0x0153 steps=600 font_fingerprint={:02x?} cell_fnv1a={:016x}",
        glyphs.fingerprint(),
        frame_hash(first.frame())
    );

    let edge_dimensions = SceneDimensions::new(1, 1).unwrap();
    let mut bounded = runtime(
        1,
        edge_dimensions,
        MatrixRainConfig::new(1.0, 10.0, 200, 0).unwrap(),
        &glyphs,
    );
    for _ in 0..120 {
        bounded
            .advance(Duration::from_nanos(16_666_667), &snapshots)
            .unwrap();
        assert_eq!(bounded.frame().len(), 1);
    }
}

#[test]
fn density_zero_and_static_speed_have_no_stream_or_evolution() {
    let glyphs = table();
    let dimensions = SceneDimensions::new(16, 8).unwrap();
    let snapshots = ascii_scene::ApprovedSnapshots::empty();
    let empty = runtime(
        99,
        dimensions,
        MatrixRainConfig::new(0.0, 5.0, 20, 0).unwrap(),
        &glyphs,
    );
    assert!(empty.frame().iter().all(|cell| *cell == ' '));

    let mut static_scene = runtime(99, dimensions, MatrixRainConfig::static_motion(), &glyphs);
    let initial = static_scene.frame().to_vec();
    let report = static_scene
        .advance(Duration::from_secs(60), &snapshots)
        .unwrap();
    assert_eq!(report.simulation_steps(), 0);
    assert!(!report.output_refreshed());
    assert_eq!(static_scene.simulation_step_count(), 0);
    assert_eq!(static_scene.frame(), initial);
}

#[test]
fn benchmark_nine_dimension_rate_cases_for_each_preset() {
    let glyphs = table();
    let snapshots = ascii_scene::ApprovedSnapshots::empty();
    let presets = [
        ("ambient", MatrixRainConfig::ambient()),
        ("dense", MatrixRainConfig::dense()),
        ("static", MatrixRainConfig::static_motion()),
    ];
    eprintln!(
        "preset,columns,fps,sim_p50_ns,sim_p95_ns,sim_p99_ns,peak_state_bytes,work_units,sim_steps"
    );

    for columns in [160, 240, 320] {
        for fps in [20_u32, 30, 60] {
            let frames = fps * 60;
            let total_ns = 60_000_000_000_u64;
            let base_ns = total_ns / u64::from(frames);
            let extra_ns = total_ns % u64::from(frames);
            for (preset_name, config) in presets {
                let dimensions = SceneDimensions::new(columns, 48).unwrap();
                let mut scene = runtime(0x153, dimensions, config, &glyphs);
                let estimate = scene.resource_estimate();
                let peak_state_bytes = estimate.total_bytes();
                let work_units = estimate.work_units();
                assert!(peak_state_bytes <= 1_048_576);
                assert!(work_units <= 1_000_000);
                assert!(scene.frame().len() <= dimensions.cell_count());
                let mut samples = Vec::new();
                samples.try_reserve_exact(frames as usize).unwrap();
                for frame_index in 0..frames {
                    let elapsed_ns = base_ns + u64::from(u64::from(frame_index) < extra_ns);
                    let started = Instant::now();
                    let report = scene
                        .advance(Duration::from_nanos(elapsed_ns), &snapshots)
                        .unwrap();
                    samples.push(started.elapsed().as_nanos());
                    assert!(report.simulation_steps() <= 8);
                    assert!(scene.frame().len() <= dimensions.cell_count());
                }
                let sim_steps = scene.simulation_step_count();
                let p50 = percentile(&mut samples, 50);
                let p95 = percentile(&mut samples, 95);
                let p99 = percentile(&mut samples, 99);
                eprintln!(
                    "{preset_name},{columns},{fps},{p50},{p95},{p99},{peak_state_bytes},{work_units},{sim_steps}"
                );
            }
        }
    }
}
