mod support;

use ascii_convert::{
    ColorNormalizationError, ColorNormalizationWarning, ConversionSettings, DensityConversionError,
    FitMode, GlyphConfig, GlyphTable, GridSizing, LinearRgb, PatchDimensions, SamplingFilter,
    SamplingOptions, UnsupportedColorReason, build_glyph_table, convert_density,
    convert_density_detailed,
};
use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, Error, FrameMetadata,
    GlyphTableId, GridSize, MalformedReason, PixelChannels, PixelFormat, RasterFrameView,
    RasterOrientation, TransferFunction,
};
use support::synthetic_font::synthetic_font;

const CELL_WIDTH: u32 = 8;
const CELL_HEIGHT: u32 = 12;

fn build_table() -> GlyphTable {
    build_glyph_table(
        &synthetic_font(false),
        "BA ",
        &GlyphConfig::new(8.0, CELL_WIDTH, CELL_HEIGHT, 9)
            .expect("fixture glyph geometry is valid"),
        &Budget::new(1_000_000, 2_000_000, 2_000_000),
        &CancellationToken::new(),
    )
    .expect("synthetic font builds a measured table")
}

fn budget() -> Budget {
    Budget::new(8_000_000, 64_000_000, 16_000_000)
}

fn raster(
    pixels: &[u8],
    width: u32,
    height: u32,
    row_stride: usize,
) -> RasterFrameView<'_> {
    RasterFrameView::new(
        width,
        height,
        row_stride,
        PixelFormat::gray8(),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
        pixels,
    )
}

fn linear_gray_raster(
    pixels: &[u8],
    width: u32,
    height: u32,
    row_stride: usize,
) -> RasterFrameView<'_> {
    RasterFrameView::new(
        width,
        height,
        row_stride,
        PixelFormat::gray8(),
        ColorDescription::new(
            ascii_core::ColorPrimaries::Srgb,
            TransferFunction::Linear,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        pixels,
    )
}

fn metadata(table: &GlyphTable) -> FrameMetadata {
    FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 19, 7, -123, 41_000)
}

fn settings<'a>(
    table: &GlyphTable,
    cancellation: &'a CancellationToken,
    sizing: GridSizing,
    foreground: LinearRgb,
    background: LinearRgb,
) -> ConversionSettings<'a> {
    ConversionSettings::new(
        sizing,
        metadata(table),
        cancellation,
        foreground,
        background,
    )
    .expect("fixture conversion settings are valid")
}

fn slot(table: &GlyphTable, character: char) -> u32 {
    u32::try_from(
        table
            .glyphs()
            .iter()
            .position(|glyph| glyph.character() == character)
            .expect("fixture repertoire contains the character"),
    )
    .expect("fixture table position fits u32")
}

#[test]
fn column_geometry_rounds_half_rows_and_accounts_for_glyph_aspect() {
    let landscape = GridSizing::columns(240)
        .resolve(1920, 1080, 8, 16)
        .expect("landscape geometry is valid");
    assert_eq!(landscape, GridSize::new(240, 68));

    let portrait = GridSizing::columns(240)
        .resolve(1080, 1920, 8, 16)
        .expect("portrait geometry is valid");
    assert_eq!(portrait, GridSize::new(240, 213));

    let one_cell = GridSizing::columns(1)
        .resolve(8, 12, 8, 12)
        .expect("one-cell geometry is valid");
    assert_eq!(one_cell, GridSize::new(1, 1));
    assert_eq!(
        GridSizing::columns(1).resolve(1, 1, 8, 12),
        Err(Error::Malformed(MalformedReason::EmptyDimensions))
    );
}

#[test]
fn fixed_surface_floors_grid_and_reports_right_bottom_leftovers() {
    let sizing = GridSizing::fixed_pixel_surface(19, 29);
    assert_eq!(
        sizing.resolve(1920, 1080, CELL_WIDTH, CELL_HEIGHT),
        Ok(GridSize::new(2, 2))
    );
    assert_eq!(sizing.fixed_surface_leftovers(CELL_WIDTH, CELL_HEIGHT), Some((3, 5)));
    assert_eq!(GridSizing::columns(10).fixed_surface_leftovers(8, 12), None);
}

#[test]
fn endpoints_and_both_luminance_polarities_match_space_and_ink() {
    let table = build_table();
    let black = [0_u8; 6];
    let white = [u8::MAX; 6];
    let normal = CancellationToken::new();
    let dark_space = convert_density_detailed(raster(&black, 2, 3, 2),
    &table,
    &settings(
        &table,
        &normal,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    ),
    &budget(),)
    .expect("black sample converts");
    let light_ink = convert_density_detailed(raster(&white, 2, 3, 2),
    &table,
    &settings(
        &table,
        &normal,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    ),
    &budget(),)
    .expect("white sample converts");
    assert_eq!(dark_space.frame().cells()[0].glyph(), slot(&table, ' '));
    assert_eq!(light_ink.frame().cells()[0].glyph(), slot(&table, 'A'));
    assert_eq!(dark_space.frame().cells()[0].fg_srgba8(), [255, 255, 255, 255]);
    assert_eq!(dark_space.frame().cells()[0].bg_srgba8(), [0, 0, 0, 255]);

    let reversed_black = convert_density_detailed(raster(&black, 2, 3, 2),
    &table,
    &settings(
        &table,
        &normal,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::BLACK,
        LinearRgb::WHITE,
    ),
    &budget(),)
    .expect("dark-on-light sample converts");
    let reversed_white = convert_density_detailed(raster(&white, 2, 3, 2),
    &table,
    &settings(
        &table,
        &normal,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::BLACK,
        LinearRgb::WHITE,
    ),
    &budget(),)
    .expect("light background sample converts");
    assert_eq!(reversed_black.frame().cells()[0].glyph(), slot(&table, 'A'));
    assert_eq!(reversed_white.frame().cells()[0].glyph(), slot(&table, ' '));
    assert_eq!(reversed_black.frame().cells()[0].fg_srgba8(), [0, 0, 0, 255]);
    assert_eq!(reversed_black.frame().cells()[0].bg_srgba8(), [255, 255, 255, 255]);
}

#[test]
fn equal_coverage_ties_use_font_index_and_repeat_deterministically() {
    let table = build_table();
    let source = [u8::MAX];
    let cancellation = CancellationToken::new();
    let settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );

    let first = convert_density_detailed(raster(&source, 1, 1, 1), &table, &settings, &budget())
        .expect("first conversion succeeds");
    let second = convert_density_detailed(raster(&source, 1, 1, 1), &table, &settings, &budget())
        .expect("repeat conversion succeeds");

    assert_eq!(table.glyphs()[slot(&table, 'A') as usize].mean_coverage(),
        table.glyphs()[slot(&table, 'B') as usize].mean_coverage());
    assert_eq!(first.frame().cells()[0].glyph(), slot(&table, 'A'));
    assert_eq!(first.frame().cells(), second.frame().cells());
}

#[test]
fn box_sampling_integrates_cell_footprint_and_never_reads_stride_padding() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let sizing = GridSizing::fixed_pixel_surface(8, 12);
    let conversion_settings = settings(
        &table,
        &cancellation,
        sizing,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );

    let area_settings = conversion_settings.with_sampling(SamplingOptions::new(
        FitMode::Contain,
        SamplingFilter::Box,
        Some(PatchDimensions::new(1, 1).expect("one-sample patch is valid")),
    ));
    let mixed = [0, 255, 0, 255, 0, 255];
    let mixed_result = convert_density_detailed(linear_gray_raster(&mixed, 2, 3, 2),
    &table,
    &area_settings,
    &budget(),)
    .expect("fractional-area conversion succeeds");
    let patch = mixed_result
        .patches()
        .expect("quality mode retains its patch")
        .luminance_for_cell(0)
        .expect("one-cell patch exists");
    assert!((patch[0] - 0.5).abs() < 1.0e-7);

    let padded_rows = [255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0];
    let tightly_packed = [255, 255, 255, 255, 255, 255];
    let padded_result = convert_density_detailed(raster(&padded_rows, 2, 3, 4),
    &table,
    &conversion_settings,
    &budget(),)
    .expect("padded rows normalize successfully");
    let packed_result = convert_density_detailed(raster(&tightly_packed, 2, 3, 2),
    &table,
    &conversion_settings,
    &budget(),)
    .expect("packed rows normalize successfully");
    assert_eq!(padded_result.frame().cells(), packed_result.frame().cells());
    assert_eq!(padded_result.frame().cells()[0].glyph(), slot(&table, 'A'));
}

#[test]
fn source_metadata_is_propagated_exactly_and_mismatched_identity_is_rejected() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let source = [128];
    let expected_metadata = metadata(&table);
    let frame = convert_density(raster(&source, 1, 1, 1),
    &table,
    &settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    ),
    &budget(),)
    .expect("matching glyph identity converts");
    assert_eq!(frame.metadata(), expected_metadata);
    assert_eq!(frame.generation(), expected_metadata.generation());
    assert_eq!(frame.frame_id(), expected_metadata.frame_id());
    assert_eq!(frame.pts_us(), expected_metadata.pts_us());
    assert_eq!(frame.duration_us(), expected_metadata.duration_us());

    let wrong_metadata = FrameMetadata::new(GlyphTableId::new([0xA5; 32]), 19, 7, -123, 41_000);
    let mismatched = ConversionSettings::new(
        GridSizing::fixed_pixel_surface(8, 12),
        wrong_metadata,
        &cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .expect("the mismatch is detected against the actual table during conversion");
    assert_eq!(
        convert_density_detailed(raster(&source, 1, 1, 1), &table, &mismatched, &budget()),
        Err(DensityConversionError::Core(Error::Malformed(
            MalformedReason::GlyphTableMismatch
        )))
    );
}

#[test]
fn cancellation_and_output_budgets_fail_before_publishing_a_frame() {
    let table = build_table();
    let source = [255];
    let cancelled = CancellationToken::new();
    let cancelled_settings = settings(
        &table,
        &cancelled,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );
    cancelled.cancel();
    assert_eq!(
        convert_density_detailed(raster(&source, 1, 1, 1), &table, &cancelled_settings, &budget()),
        Err(DensityConversionError::Core(Error::Cancelled))
    );

    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );
    let unsupported_format = RasterFrameView::new(
        1,
        1,
        1,
        PixelFormat::new(PixelChannels::Other(5), 8),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
        &source,
    );
    let no_frame_budget = Budget::new(0, 64_000_000, 16_000_000);
    assert_eq!(
        convert_density_detailed(unsupported_format, &table, &conversion_settings, &no_frame_budget),
        Err(DensityConversionError::Core(Error::BudgetExceeded(
            BudgetKind::FrameBytes
        )))
    );

    let insufficient_work = Budget::new(8_000_000, 64_000_000, 3);
    assert_eq!(
        convert_density_detailed(
            raster(&source, 1, 1, 1),
            &table,
            &conversion_settings,
            &insufficient_work,
        ),
        Err(DensityConversionError::Core(Error::BudgetExceeded(
            BudgetKind::WorkUnits
        )))
    );
}

#[test]
fn shared_normalizer_failures_keep_their_typed_color_reason() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );
    let hdr = RasterFrameView::new(
        1,
        1,
        1,
        PixelFormat::gray8(),
        ColorDescription::new(
            ascii_core::ColorPrimaries::Srgb,
            TransferFunction::Pq,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        &[128],
    );

    assert_eq!(
        convert_density_detailed(hdr, &table, &conversion_settings, &budget()),
        Err(DensityConversionError::ColorNormalization(
            ColorNormalizationError::UnsupportedColor(UnsupportedColorReason::HdrTransfer(
                TransferFunction::Pq
            ))
        ))
    );
    let hdr_core = RasterFrameView::new(
        1,
        1,
        1,
        PixelFormat::gray8(),
        ColorDescription::new(
            ascii_core::ColorPrimaries::Srgb,
            TransferFunction::Pq,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        &[128],
    );
    assert_eq!(
        convert_density(hdr_core, &table, &conversion_settings, &budget()),
        Err(Error::Unsupported(
            ascii_core::UnsupportedReason::ColorDescription
        ))
    );
}

#[test]
fn conversion_preserves_unknown_profile_warning() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );
    let raster = RasterFrameView::new(
        1,
        1,
        1,
        PixelFormat::gray8(),
        ColorDescription::new(
            ascii_core::ColorPrimaries::Other(7),
            TransferFunction::Other(9),
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        &[128],
    );
    let result = convert_density_detailed(raster, &table, &conversion_settings, &budget())
        .expect("unknown profile is converted with an explicit warning");

    assert_eq!(
        result.normalization_warning(),
        Some(ColorNormalizationWarning::AssumedSrgbForUnknownProfile)
    );
}

#[test]
fn settings_reject_empty_geometry_and_indistinguishable_luminance() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    assert_eq!(
        ConversionSettings::new(
            GridSizing::columns(0),
            metadata(&table),
            &cancellation,
            LinearRgb::WHITE,
            LinearRgb::BLACK,
        )
        .err(),
        Some(Error::Malformed(MalformedReason::InvalidConfiguration))
    );
    assert_eq!(
        ConversionSettings::new(
            GridSizing::fixed_pixel_surface(8, 12),
            metadata(&table),
            &cancellation,
            LinearRgb::WHITE,
            LinearRgb::WHITE,
        )
        .err(),
        Some(Error::Malformed(MalformedReason::InvalidConfiguration))
    );
}
