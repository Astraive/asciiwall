#[allow(dead_code)]
mod support;

use core::num::NonZeroUsize;

use ascii_convert::{
    ConversionResources, ConversionSettings, GlyphConfig, GlyphRepertoire, GlyphTable, GridSizing,
    LinearRgb, NormalizedRasterResources, build_glyph_table, convert_density_detailed,
    convert_structural,
};
use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, Error, FrameMetadata,
    GlyphTableId, PixelFormat, RasterFrameView, RasterOrientation,
};
use support::synthetic_font::synthetic_font;

const CELL_WIDTH: u32 = 8;
const CELL_HEIGHT: u32 = 12;
const SURFACE_WIDTH: u32 = 16;
const SURFACE_HEIGHT: u32 = CELL_HEIGHT * 9;
const BUDGET: Budget = Budget::new(8_000_000, 64_000_000, 16_000_000);

fn build_table() -> GlyphTable {
    build_glyph_table(
        &synthetic_font(false),
        &GlyphRepertoire::new("BA ").expect("fixture repertoire is supported"),
        &GlyphConfig::new(8.0, CELL_WIDTH, CELL_HEIGHT, 9)
            .expect("fixture glyph geometry is valid"),
        &Budget::new(1_000_000, 2_000_000, 2_000_000),
        &CancellationToken::new(),
    )
    .expect("source-generated fixture font builds")
}

fn settings<'a>(
    table: &GlyphTable,
    cancellation: &'a CancellationToken,
    resources: &'a ConversionResources,
    workers: NonZeroUsize,
) -> ConversionSettings<'a> {
    ConversionSettings::new(
        GridSizing::fixed_pixel_surface(SURFACE_WIDTH, SURFACE_HEIGHT),
        FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 41, 3, -9, 17_000),
        cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .expect("fixture conversion settings are valid")
    .with_worker_pool(resources, workers)
}

fn raster(pixels: &[u8]) -> RasterFrameView<'_> {
    RasterFrameView::new(
        SURFACE_WIDTH,
        SURFACE_HEIGHT,
        SURFACE_WIDTH as usize,
        PixelFormat::gray8(),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
        pixels,
    )
}

fn fixture_pixels() -> Vec<u8> {
    (0..SURFACE_WIDTH as usize * SURFACE_HEIGHT as usize)
        .map(|index| ((index * 37 + index / 3) % 256) as u8)
        .collect()
}

#[test]
fn density_and_structural_matching_are_identical_across_worker_counts() {
    let table = build_table();
    let pixels = fixture_pixels();
    let one_worker_resources = ConversionResources::new(64_000_000, NonZeroUsize::new(3).unwrap());
    let three_worker_resources = one_worker_resources.clone();
    let serial_cancellation = CancellationToken::new();
    let parallel_cancellation = CancellationToken::new();
    let serial_settings = settings(
        &table,
        &serial_cancellation,
        &one_worker_resources,
        NonZeroUsize::new(1).unwrap(),
    );
    let parallel_settings = settings(
        &table,
        &parallel_cancellation,
        &three_worker_resources,
        NonZeroUsize::new(3).unwrap(),
    );

    let serial_density = convert_density_detailed(
        raster(&pixels),
        &table,
        &serial_settings,
        &BUDGET,
        &NormalizedRasterResources::new(64_000_000),
    )
    .expect("single-worker density conversion completes");
    let parallel_density = convert_density_detailed(
        raster(&pixels),
        &table,
        &parallel_settings,
        &BUDGET,
        &NormalizedRasterResources::new(64_000_000),
    )
    .expect("three-worker density conversion completes");
    assert_eq!(
        serial_density.frame().cells(),
        parallel_density.frame().cells()
    );

    let serial_structural =
        convert_structural(raster(&pixels), &table, &serial_settings, None, &BUDGET)
            .expect("single-worker structural conversion completes");
    let parallel_structural =
        convert_structural(raster(&pixels), &table, &parallel_settings, None, &BUDGET)
            .expect("three-worker structural conversion completes");
    assert_eq!(serial_structural.cells(), parallel_structural.cells());
    assert_eq!(one_worker_resources.active_bytes(), 0);
    assert_eq!(one_worker_resources.active_workers(), 0);
}

#[test]
fn failed_aggregate_admission_releases_all_worker_slots() {
    let table = build_table();
    let pixels = fixture_pixels();
    let resources = ConversionResources::new(1, NonZeroUsize::new(2).unwrap());
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        &resources,
        NonZeroUsize::new(2).unwrap(),
    );

    assert!(matches!(
        convert_density_detailed(
            raster(&pixels),
            &table,
            &conversion_settings,
            &BUDGET,
            &NormalizedRasterResources::new(64_000_000),
        ),
        Err(ascii_convert::DensityConversionError::Core(
            Error::BudgetExceeded(BudgetKind::RasterBytes)
        ))
    ));
    assert_eq!(resources.active_bytes(), 0);
    assert_eq!(resources.active_workers(), 0);
}
