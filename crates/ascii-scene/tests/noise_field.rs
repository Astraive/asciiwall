#[allow(dead_code)]
mod synthetic_font {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ascii-convert/tests/support/synthetic_font.rs"
    ));
}
use ascii_convert::{GlyphConfig, GlyphRepertoire, build_glyph_table};
use ascii_core::{Budget, CancellationToken};
use ascii_scene::{
    ApprovedSnapshots, NOISE_ALGORITHM, NOISE_FIELD_DESCRIPTOR, NOISE_FIELD_PRESETS,
    NoiseFieldConfig, NoiseFieldGenerator, SceneDimensions, SceneParameters, SceneResourceBudget,
    SceneRuntime, TerrainControls,
};
fn fixture() -> ascii_convert::GlyphTable {
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
fn hash(cells: &[char]) -> u64 {
    cells.iter().fold(0xcbf2_9ce4_8422_2325, |h, c| {
        (h ^ u64::from(*c as u32)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn config_hash(config: NoiseFieldConfig) -> u64 {
    [config.scale_q16(), config.speed_q16(), config.color_rgba()]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}
fn render(
    seed: u32,
    config: NoiseFieldConfig,
    terrain: Option<TerrainControls>,
    dimensions: SceneDimensions,
) -> Vec<char> {
    let table = fixture();
    let params = SceneParameters::new(seed, 1.0, 0, 60, true).unwrap();
    let mut generator = NoiseFieldGenerator::new(&table, config);
    if let Some(t) = terrain {
        generator = generator.with_terrain_controls(t);
    }
    let mut runtime = SceneRuntime::try_new(
        params,
        dimensions,
        generator,
        &SceneResourceBudget::default(),
    )
    .unwrap();
    let snapshots = ApprovedSnapshots::empty();
    for _ in 0..600 {
        runtime
            .advance(std::time::Duration::from_nanos(16_666_667), &snapshots)
            .unwrap();
    }
    runtime.frame().to_vec()
}
#[test]
fn seeded_presets_repeat_and_parameters_stay_exact_v1() {
    assert!(NOISE_ALGORITHM.contains("v1"));
    assert_eq!(
        NOISE_FIELD_DESCRIPTOR
            .parameters
            .iter()
            .map(|p| p.name)
            .collect::<Vec<_>>(),
        ["scale", "speed", "color"]
    );
    assert_eq!(
        NOISE_FIELD_PRESETS
            .iter()
            .map(|p| p.name)
            .collect::<Vec<_>>(),
        ["cloud", "fine-noise", "terrain"]
    );
    for preset in NOISE_FIELD_PRESETS {
        let terrain = if preset.name == "terrain" {
            preset.terrain
        } else {
            None
        };
        let first = render(
            1,
            preset.config,
            terrain,
            SceneDimensions::new(32, 16).unwrap(),
        );
        let second = render(
            1,
            preset.config,
            terrain,
            SceneDimensions::new(32, 16).unwrap(),
        );
        assert_eq!(first, second);
        eprintln!(
            "noise preset={} config={:016x} hash={:016x}",
            preset.name,
            config_hash(preset.config),
            hash(&first)
        );
    }
    assert_ne!(
        render(
            1,
            NoiseFieldConfig::cloud(),
            None,
            SceneDimensions::new(32, 16).unwrap()
        ),
        render(
            2,
            NoiseFieldConfig::cloud(),
            None,
            SceneDimensions::new(32, 16).unwrap()
        )
    );
}
#[test]
fn configuration_and_internal_terrain_bounds_are_checked() {
    for scale in [0.009, 101.0, f32::NAN] {
        assert!(NoiseFieldConfig::new(scale, 0.0, 0).is_err());
    }
    assert!(NoiseFieldConfig::new(0.01, 0.0, u32::MAX).is_ok());
    assert!(NoiseFieldConfig::new(100.0, 10.0, 0).is_ok());
    assert!(TerrainControls::new(0.0, 1, false).is_err());
    assert!(TerrainControls::new(0.0001, 1, false).is_err());
    assert!(TerrainControls::new(0.01, 1, false).is_ok());
    assert!(NoiseFieldConfig::from_hex_rgba(2.0, 0.2, "#ccccffff").is_ok());
    assert!(NoiseFieldConfig::from_hex_rgba(2.0, 0.2, "ccccffff").is_err());
}
#[test]
fn tiny_large_scale_zero_speed_and_resize_remain_bounded() {
    let small = render(
        1,
        NoiseFieldConfig::new(0.01, 0.0, 0).unwrap(),
        None,
        SceneDimensions::new(1, 1).unwrap(),
    );
    assert_eq!(small.len(), 1);
    let large = render(
        1,
        NoiseFieldConfig::new(100.0, 0.0, 0).unwrap(),
        None,
        SceneDimensions::new(128, 64).unwrap(),
    );
    assert_eq!(large.len(), 8192);
    assert_eq!(
        large,
        render(
            1,
            NoiseFieldConfig::new(100.0, 0.0, 0).unwrap(),
            None,
            SceneDimensions::new(128, 64).unwrap()
        )
    );
}
