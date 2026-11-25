#[allow(dead_code)]
mod synthetic_font {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ascii-convert/tests/support/synthetic_font.rs"
    ));
}

use std::time::Duration;

use ascii_convert::{GlyphConfig, GlyphRepertoire, build_glyph_table};
use ascii_core::{Budget, CancellationToken};
use ascii_scene::{
    MAX_APPROVED_BUTTON_EVENTS, STARFIELD_DESCRIPTOR, STARFIELD_PRESETS, SceneDimensions,
    SceneParameters, SceneResourceBudget, SceneRuntime, StarfieldConfig, StarfieldGenerator,
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
    count_cap: u32,
    config: StarfieldConfig,
    glyphs: &'a ascii_convert::GlyphTable,
) -> SceneRuntime<StarfieldGenerator<'a>> {
    let parameters = SceneParameters::new(seed, 1.0, count_cap, 60, true).unwrap();
    SceneRuntime::try_new(
        parameters,
        SceneDimensions::new(80, 40).unwrap(),
        StarfieldGenerator::new("starfield", config, glyphs),
        &SceneResourceBudget::default(),
    )
    .unwrap()
}

fn hash(frame: &[char]) -> u64 {
    frame.iter().fold(0xcbf2_9ce4_8422_2325, |hash, cell| {
        (hash ^ u64::from(*cell as u32)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[test]
fn bounds_presets_and_requested_effective_counts_are_explicit() {
    assert!(StarfieldConfig::new(0, 1.0, 20.0, 0).is_err());
    assert!(StarfieldConfig::new(20_001, 1.0, 20.0, 0).is_err());
    assert!(StarfieldConfig::new(1, f32::NAN, 20.0, 0).is_err());
    assert!(StarfieldConfig::new(1, 1.0, 0.09, 0).is_err());
    assert!(StarfieldConfig::from_hex_rgba(10, 1.0, 20.0, "#bad").is_err());
    assert_eq!(
        StarfieldConfig::ambient(),
        StarfieldConfig::from_hex_rgba(500, 0.5, 20.0, "#d8e8ffff").unwrap()
    );
    assert_eq!(
        StarfieldConfig::warp(),
        StarfieldConfig::from_hex_rgba(2_000, 3.0, 50.0, "#ffffffff").unwrap()
    );
    assert_eq!(StarfieldConfig::still().speed_q16(), 0);
    assert_eq!(STARFIELD_DESCRIPTOR.id, "builtin.starfield");
    assert_eq!(STARFIELD_DESCRIPTOR.parameters.len(), 4);
    assert_eq!(STARFIELD_PRESETS.len(), 3);
    assert!(
        STARFIELD_PRESETS
            .iter()
            .all(|preset| preset.cost_is_estimate)
    );

    let glyphs = table();
    let config = StarfieldConfig::new(20_000, 1.0, 20.0, 0xffff_ffff).unwrap();
    let scene = runtime(7, 2_000, config, &glyphs);
    assert_eq!(scene.node_id(), "starfield");
    assert_eq!(config.requested_count(), 20_000);
    assert_eq!(
        config.effective_count(scene.parameters().max_particles()),
        2_000
    );
    assert!(scene.resource_estimate().generator_scratch_bytes() >= 2_000);
}

#[test]
fn button_event_scan_work_is_admitted_for_each_catch_up_step() {
    let glyphs = table();
    let scene = runtime(0x154, 2_000, StarfieldConfig::ambient(), &glyphs);
    let particle_and_grid_work = 80_u64 * 40 + 2_000 * 5;
    let button_scan_work = (MAX_APPROVED_BUTTON_EVENTS as u64) * 4;
    assert_eq!(
        scene.resource_estimate().work_units(),
        particle_and_grid_work + button_scan_work
    );
}

#[test]
fn seeded_timeline_reproduces_depth_projection_and_cell_occupancy() {
    let glyphs = table();
    let config = StarfieldConfig::ambient();
    let mut first = runtime(0x154, 2_000, config, &glyphs);
    let mut second = runtime(0x154, 2_000, config, &glyphs);
    for _ in 0..600 {
        first
            .advance(Duration::from_nanos(16_666_667), &Default::default())
            .unwrap();
        second
            .advance(Duration::from_nanos(16_666_667), &Default::default())
            .unwrap();
    }
    assert_eq!(first.frame(), second.frame());
    assert_eq!(hash(first.frame()), hash(second.frame()));
    assert_eq!(first.simulation_step_count(), 600);
    assert_eq!(
        first.simulation_step_count(),
        second.simulation_step_count()
    );
    let fingerprint = glyphs
        .fingerprint()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        fingerprint,
        "4ddc3a1585b233278e7050a05eb95b777916375d3e619efb8c76cbd85db648e0"
    );
    assert_eq!(hash(first.frame()), 0x0f70_90f8_c4d3_4773);
}

#[test]
fn still_and_reduced_motion_do_not_evolve_after_initial_render() {
    let glyphs = table();
    let config = StarfieldConfig::still();
    let parameters = SceneParameters::new(9, 1.0, 500, 60, false).unwrap();
    let mut scene = SceneRuntime::try_new(
        parameters,
        SceneDimensions::new(80, 40).unwrap(),
        StarfieldGenerator::new("still", config, &glyphs),
        &SceneResourceBudget::default(),
    )
    .unwrap();
    let initial_hash = hash(scene.frame());
    for _ in 0..600 {
        scene
            .advance(Duration::from_secs(1), &Default::default())
            .unwrap();
    }
    assert_eq!(hash(scene.frame()), initial_hash);
    assert_eq!(scene.simulation_step_count(), 0);
}

#[test]
fn near_plane_particles_recycle_without_zero_division() {
    let glyphs = table();
    let config = StarfieldConfig::new(128, 10.0, 0.1, 0xffff_ffff).unwrap();
    let mut scene = runtime(0x154, 128, config, &glyphs);
    for _ in 0..600 {
        scene
            .advance(Duration::from_nanos(16_666_667), &Default::default())
            .unwrap();
    }
    assert_eq!(scene.simulation_step_count(), 600);
    assert_eq!(
        scene.frame().len(),
        SceneDimensions::new(80, 40).unwrap().cell_count()
    );
}

#[test]
fn benchmark_27_cases_for_sixty_second_timelines() {
    use std::time::Instant;

    fn percentile(values: &mut [u128], percent: usize) -> u128 {
        values.sort_unstable();
        values[(values.len() - 1) * percent / 100]
    }

    let glyphs = table();
    let presets = [
        ("ambient", StarfieldConfig::ambient()),
        ("warp", StarfieldConfig::warp()),
        ("still", StarfieldConfig::still()),
    ];
    for (preset, config) in presets {
        for columns in [160, 240, 320] {
            for fps in [20_u64, 30, 60] {
                let dimensions = SceneDimensions::new(columns, 120).unwrap();
                let parameters = SceneParameters::new(0x154, 1.0, 2_000, 60, true).unwrap();
                let mut scene = SceneRuntime::try_new(
                    parameters,
                    dimensions,
                    StarfieldGenerator::new("starfield-bench", config, &glyphs),
                    &SceneResourceBudget::default(),
                )
                .unwrap();
                let calls = fps * 60;
                let elapsed = Duration::from_nanos(1_000_000_000 / fps);
                let mut samples = Vec::with_capacity(calls as usize);
                for _ in 0..calls {
                    let start = Instant::now();
                    scene.advance(elapsed, &Default::default()).unwrap();
                    samples.push(start.elapsed().as_nanos());
                }
                let p50 = percentile(&mut samples, 50);
                let p95 = percentile(&mut samples, 95);
                let p99 = percentile(&mut samples, 99);
                println!(
                    "SCN003_BENCH preset={preset} columns={columns} fps={fps} simulation_ns_p50={p50} simulation_ns_p95={p95} simulation_ns_p99={p99} peak_state_bytes={}",
                    scene.resource_estimate().total_bytes()
                );
            }
        }
    }
}
