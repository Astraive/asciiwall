#[allow(dead_code)]
mod support;

use ascii_convert::{
    ColorNormalizationError, ColorNormalizationWarning, ConversionSettings, CropRect,
    DensityConversionError, FitMode, FitTransform, GlyphConfig, GlyphRepertoire, GlyphTable,
    GridSizing, LinearRgb, PatchDimensions, SamplingFilter, SamplingOptions,
    UnsupportedColorReason, build_glyph_table, convert_density, convert_density_detailed,
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
        &GlyphRepertoire::new("BA ").expect("fixture repertoire is supported"),
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

fn raster(pixels: &[u8], width: u32, height: u32, row_stride: usize) -> RasterFrameView<'_> {
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
        GridSizing::columns(1).resolve(1, 1, 8, 24),
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
    assert_eq!(
        sizing.fixed_surface_leftovers(CELL_WIDTH, CELL_HEIGHT),
        Some((3, 5))
    );
    assert_eq!(GridSizing::columns(10).fixed_surface_leftovers(8, 12), None);
}

#[test]
fn custom_crop_column_sizing_uses_selected_source_geometry() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let source = vec![128; 32 * 12];
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::columns(4),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::CustomCrop(CropRect::new(0.25, 0.0, 0.5, 1.0).expect("crop is valid")),
        SamplingFilter::Box,
        None,
    ));
    let raster = RasterFrameView::new(
        32,
        12,
        32,
        PixelFormat::gray8(),
        ColorDescription::new(
            ascii_core::ColorPrimaries::Other(7),
            TransferFunction::Other(9),
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        &source,
    );
    let converted = convert_density_detailed(raster, &table, &conversion_settings, &budget())
        .expect("custom-crop conversion succeeds with an explicit profile warning");

    assert_eq!(converted.frame().size(), GridSize::new(4, 4));
    assert_eq!(converted.transform().source_rect().width(), 8.0);
    assert_eq!(converted.transform().source_rect().height(), 12.0);
    assert_eq!(converted.transform().display_dimensions(), (32, 48));
    assert_eq!(
        converted.normalization_warning(),
        Some(ColorNormalizationWarning::AssumedSrgbForUnknownProfile)
    );
}

#[test]
fn custom_crop_row_limit_is_checked_after_half_up_rounding() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let pixels = [128_u8];
    let convert_for_requested_rows = |requested_rows: f64| {
        let crop_width = 2.0 / (3.0 * requested_rows);
        let crop = CropRect::new(0.0, 0.0, crop_width, 1.0).expect("crop is valid");
        let conversion_settings = settings(
            &table,
            &cancellation,
            GridSizing::columns(1),
            LinearRgb::WHITE,
            LinearRgb::BLACK,
        )
        .with_sampling(SamplingOptions::new(
            FitMode::CustomCrop(crop),
            SamplingFilter::Box,
            None,
        ));
        convert_density_detailed(
            raster(&pixels, 1, 1, 1),
            &table,
            &conversion_settings,
            &budget(),
        )
    };

    assert!(matches!(
        convert_for_requested_rows(f64::from(u32::MAX) + 0.25),
        Err(DensityConversionError::Core(Error::BudgetExceeded(_)))
    ));
    assert_eq!(
        convert_for_requested_rows(f64::from(u32::MAX) + 0.75),
        Err(DensityConversionError::Core(Error::SizeOverflow))
    );
}

#[test]
fn unsupported_hdr_precedes_minimum_positive_crop_sizing() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::columns(1),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::CustomCrop(
            CropRect::new(0.0, 0.0, f64::MIN_POSITIVE, 1.0).expect("crop is valid"),
        ),
        SamplingFilter::Box,
        None,
    ));
    let pixels = [128_u8];
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
        &pixels,
    );
    let valid_profile = RasterFrameView::new(
        1,
        1,
        1,
        PixelFormat::gray8(),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
        &pixels,
    );

    assert_eq!(
        convert_density_detailed(hdr, &table, &conversion_settings, &budget()),
        Err(DensityConversionError::ColorNormalization(
            ColorNormalizationError::UnsupportedColor(UnsupportedColorReason::HdrTransfer(
                TransferFunction::Pq
            ))
        ))
    );
    assert_eq!(
        convert_density_detailed(valid_profile, &table, &conversion_settings, &budget()),
        Err(DensityConversionError::Core(Error::SizeOverflow))
    );
}

#[test]
fn custom_crop_preserves_narrow_extent_at_large_source_origin() {
    let transform = FitTransform::new(
        10_000_000,
        1,
        8,
        12,
        FitMode::CustomCrop(CropRect::new(0.5, 0.0, 0.5000001, 1.0).expect("crop is valid")),
    )
    .expect("representable crop edges tolerate rounding at their large origin");

    assert_eq!(transform.source_rect().x(), 5_000_000.0);
    assert!(transform.source_rect().width() > 0.0);
    assert!(transform.map_display_to_source(4.0, 6.0).is_some());
}

#[test]
fn endpoints_and_both_luminance_polarities_match_space_and_ink() {
    let table = build_table();
    let black = [0_u8; 6];
    let white = [u8::MAX; 6];
    let normal = CancellationToken::new();
    let dark_space = convert_density_detailed(
        raster(&black, 2, 3, 2),
        &table,
        &settings(
            &table,
            &normal,
            GridSizing::fixed_pixel_surface(8, 12),
            LinearRgb::WHITE,
            LinearRgb::BLACK,
        ),
        &budget(),
    )
    .expect("black sample converts");
    let light_ink = convert_density_detailed(
        raster(&white, 2, 3, 2),
        &table,
        &settings(
            &table,
            &normal,
            GridSizing::fixed_pixel_surface(8, 12),
            LinearRgb::WHITE,
            LinearRgb::BLACK,
        ),
        &budget(),
    )
    .expect("white sample converts");
    assert_eq!(dark_space.frame().cells()[0].glyph(), slot(&table, ' '));
    assert_eq!(light_ink.frame().cells()[0].glyph(), slot(&table, 'A'));
    assert_eq!(
        dark_space.frame().cells()[0].fg_srgba8(),
        [255, 255, 255, 255]
    );
    assert_eq!(dark_space.frame().cells()[0].bg_srgba8(), [0, 0, 0, 255]);

    let reversed_black = convert_density_detailed(
        raster(&black, 2, 3, 2),
        &table,
        &settings(
            &table,
            &normal,
            GridSizing::fixed_pixel_surface(8, 12),
            LinearRgb::BLACK,
            LinearRgb::WHITE,
        ),
        &budget(),
    )
    .expect("dark-on-light sample converts");
    let reversed_white = convert_density_detailed(
        raster(&white, 2, 3, 2),
        &table,
        &settings(
            &table,
            &normal,
            GridSizing::fixed_pixel_surface(8, 12),
            LinearRgb::BLACK,
            LinearRgb::WHITE,
        ),
        &budget(),
    )
    .expect("light background sample converts");
    assert_eq!(reversed_black.frame().cells()[0].glyph(), slot(&table, 'A'));
    assert_eq!(reversed_white.frame().cells()[0].glyph(), slot(&table, ' '));
    assert_eq!(
        reversed_black.frame().cells()[0].fg_srgba8(),
        [0, 0, 0, 255]
    );
    assert_eq!(
        reversed_black.frame().cells()[0].bg_srgba8(),
        [255, 255, 255, 255]
    );
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

    assert_eq!(
        table.glyphs()[slot(&table, 'A') as usize].mean_coverage(),
        table.glyphs()[slot(&table, 'B') as usize].mean_coverage()
    );
    assert_eq!(first.frame().cells()[0].glyph(), slot(&table, 'A'));
    assert_eq!(first.frame().cells(), second.frame().cells());
}

#[test]
fn box_sampling_converts_subnormal_crop_with_normalized_weights() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let pixel = [128_u8];
    let crop_edge = f64::from_bits(1);
    let crop = CropRect::new(0.0, 0.0, crop_edge, crop_edge).expect("subnormal crop is valid");
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::CustomCrop(crop),
        SamplingFilter::Box,
        Some(PatchDimensions::new(1, 1).expect("one-sample patch is valid")),
    ));

    let converted = convert_density_detailed(
        linear_gray_raster(&pixel, 1, 1, 1),
        &table,
        &conversion_settings,
        &budget(),
    )
    .expect("box sampling converts a valid subnormal crop");
    let luminance = converted
        .patches()
        .expect("quality mode retains its sampled luminance")
        .luminance_for_cell(0)
        .expect("one-cell patch exists");

    assert_eq!(converted.frame().size(), GridSize::new(1, 1));
    assert!((f64::from(luminance[0]) - 128.0 / 255.0).abs() < 1.0e-7);
}

#[test]
fn box_quality_patch_subdivision_preserves_narrow_crop_at_large_origin() {
    const SOURCE_WIDTH: u32 = 1_000_000;
    let source_width = usize::try_from(SOURCE_WIDTH).expect("source width fits usize");
    let table = build_table();
    let cancellation = CancellationToken::new();
    let pixels = vec![128_u8; source_width];

    for (left, right) in [
        (0.5, 0.5000000000000001),
        (0.5000000000000001, 0.5000000000000002),
    ] {
        let conversion_settings = settings(
            &table,
            &cancellation,
            GridSizing::fixed_pixel_surface(8, 12),
            LinearRgb::WHITE,
            LinearRgb::BLACK,
        )
        .with_sampling(SamplingOptions::new(
            FitMode::CustomCrop(
                CropRect::new(left, 0.0, right, 1.0).expect("narrow crop is valid"),
            ),
            SamplingFilter::Box,
            Some(PatchDimensions::new(4, 1).expect("four-sample patch is valid")),
        ));

        let converted = convert_density_detailed(
            linear_gray_raster(&pixels, SOURCE_WIDTH, 1, source_width),
            &table,
            &conversion_settings,
            &budget(),
        )
        .expect("box quality-patch conversion preserves each subinterval");
        let patch = converted
            .patches()
            .expect("quality mode retains its sampled luminance")
            .luminance_for_cell(0)
            .expect("one-cell patch exists");

        assert_eq!(
            converted.transform().source_rect().x(),
            left * f64::from(SOURCE_WIDTH)
        );
        assert!(converted.transform().source_rect().width() > 0.0);
        assert_eq!(patch.len(), 4);
        assert!(
            patch
                .iter()
                .all(|value| (f64::from(*value) - 128.0 / 255.0).abs() < 1.0e-7)
        );
    }
}

#[test]
fn box_quality_patch_subdivision_preserves_fractional_source_origin() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let pixels = [128_u8; 8];
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::CustomCrop(
            CropRect::new(0.1875, 0.0, 0.18750000000000003, 1.0)
                .expect("narrow fractional-origin crop is valid"),
        ),
        SamplingFilter::Box,
        Some(PatchDimensions::new(4, 1).expect("four-sample patch is valid")),
    ));

    let converted = convert_density_detailed(
        linear_gray_raster(&pixels, 8, 1, pixels.len()),
        &table,
        &conversion_settings,
        &budget(),
    )
    .expect("box quality-patch conversion preserves positive subintervals");
    let patch = converted
        .patches()
        .expect("quality mode retains its sampled luminance")
        .luminance_for_cell(0)
        .expect("one-cell patch exists");

    assert_eq!(converted.transform().source_rect().x(), 1.5);
    assert!(converted.transform().source_rect().width() > 0.0);
    assert_eq!(patch.len(), 4);
    assert!(
        patch
            .iter()
            .all(|value| (f64::from(*value) - 128.0 / 255.0).abs() < 1.0e-7)
    );
}

#[test]
fn box_quality_patch_subdivision_integrates_fractional_pixel_edges() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let pixels = [0_u8, u8::MAX];
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::CustomCrop(
            CropRect::new(0.25, 0.0, 0.875, 1.0).expect("fractional crop is valid"),
        ),
        SamplingFilter::Box,
        Some(PatchDimensions::new(4, 1).expect("four-sample patch is valid")),
    ));

    let converted = convert_density_detailed(
        linear_gray_raster(&pixels, 2, 1, pixels.len()),
        &table,
        &conversion_settings,
        &budget(),
    )
    .expect("box quality patches integrate across source pixel edges");
    let patch = converted
        .patches()
        .expect("quality mode retains its sampled luminance")
        .luminance_for_cell(0)
        .expect("one-cell patch exists");

    assert_eq!(patch.len(), 4);
    for (actual, expected) in patch.iter().zip([0.0, 0.4, 1.0, 1.0]) {
        assert!((f64::from(*actual) - expected).abs() < 1.0e-7);
    }
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
    let mixed_result = convert_density_detailed(
        linear_gray_raster(&mixed, 2, 3, 2),
        &table,
        &area_settings,
        &budget(),
    )
    .expect("fractional-area conversion succeeds");
    let patch = mixed_result
        .patches()
        .expect("quality mode retains its patch")
        .luminance_for_cell(0)
        .expect("one-cell patch exists");
    assert!((patch[0] - 0.5).abs() < 1.0e-7);

    let padded_rows = [255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0];
    let tightly_packed = [255, 255, 255, 255, 255, 255];
    let padded_result = convert_density_detailed(
        raster(&padded_rows, 2, 3, 4),
        &table,
        &conversion_settings,
        &budget(),
    )
    .expect("padded rows normalize successfully");
    let packed_result = convert_density_detailed(
        raster(&tightly_packed, 2, 3, 2),
        &table,
        &conversion_settings,
        &budget(),
    )
    .expect("packed rows normalize successfully");
    assert_eq!(padded_result.frame().cells(), packed_result.frame().cells());
    assert_eq!(padded_result.frame().cells()[0].glyph(), slot(&table, 'A'));
}

#[test]
fn circle_density_tracks_cell_aspect_end_to_end() {
    let table = build_table();
    let circle = (0_i32..16)
        .flat_map(|y| {
            (0_i32..16).map(move |x| {
                let dx = 2 * x + 1 - 16;
                let dy = 2 * y + 1 - 16;
                if dx * dx + dy * dy <= 14 * 14 {
                    u8::MAX
                } else {
                    0
                }
            })
        })
        .collect::<Vec<_>>();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::columns(2),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );

    let converted = convert_density_detailed(
        linear_gray_raster(&circle, 16, 16, 16),
        &table,
        &conversion_settings,
        &budget(),
    )
    .expect("circular raster converts using glyph-cell aspect");

    assert_eq!(converted.frame().size(), GridSize::new(2, 1));
    assert_eq!(
        converted
            .frame()
            .cells()
            .iter()
            .map(|cell| cell.glyph())
            .collect::<Vec<_>>(),
        [slot(&table, 'A'), slot(&table, 'A')]
    );
}

#[test]
fn transparent_edge_pixel_composites_over_configured_nonblack_background() {
    let table = build_table();
    // A half-covered white edge pixel occupies the upper-left corner; transparent pixels expose
    // the configured background rather than black.
    let pixels = [
        255, 255, 255, 128, 0, 0, 0, 0, // top row
        0, 0, 0, 0, 0, 0, 0, 0, // middle row
        0, 0, 0, 0, 0, 0, 0, 0, // bottom row
    ];
    let input = RasterFrameView::new(
        2,
        3,
        8,
        PixelFormat::rgba8(),
        ColorDescription::srgb(AlphaMode::Straight),
        RasterOrientation::TopDown,
        &pixels,
    );
    let background = LinearRgb::new(0.2, 0.2, 0.2).expect("gray background is in range");
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        background,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::Contain,
        SamplingFilter::Box,
        Some(PatchDimensions::new(1, 1).expect("one-sample patch is valid")),
    ));

    let converted = convert_density_detailed(input, &table, &conversion_settings, &budget())
        .expect("alpha edge converts over configured background");
    let patch = converted
        .patches()
        .expect("quality mode retains its sampled luminance")
        .luminance_for_cell(0)
        .expect("one-cell patch exists");
    let alpha = f64::from(128_u8) / f64::from(u8::MAX);
    let expected = 0.2 + alpha * (1.0 - 0.2) / 6.0;

    assert!((f64::from(patch[0]) - expected).abs() < 1.0e-7);
    assert_eq!(
        converted.frame().cells()[0].bg_srgba8(),
        [124, 124, 124, 255]
    );
}

#[test]
fn nearest_sampling_returns_pixel_at_inverse_mapped_cell_center() {
    let table = build_table();
    let mut pixels = [0_u8; 16];
    // Contain maps this cell center to source edge coordinate (2, 2); floor selects pixel (2, 2).
    pixels[10] = u8::MAX;
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::Contain,
        SamplingFilter::Nearest,
        Some(PatchDimensions::new(1, 1).expect("one-sample patch is valid")),
    ));

    let converted = convert_density_detailed(
        linear_gray_raster(&pixels, 4, 4, 4),
        &table,
        &conversion_settings,
        &budget(),
    )
    .expect("nearest-sampled two-tone raster converts");
    let patch = converted
        .patches()
        .expect("quality mode retains its sampled luminance")
        .luminance_for_cell(0)
        .expect("one-cell patch exists");

    assert_eq!(patch, [1.0]);
}

#[test]
fn source_metadata_is_propagated_exactly_and_mismatched_identity_is_rejected() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let source = [128];
    let expected_metadata = metadata(&table);
    let frame = convert_density(
        raster(&source, 1, 1, 1),
        &table,
        &settings(
            &table,
            &cancellation,
            GridSizing::fixed_pixel_surface(8, 12),
            LinearRgb::WHITE,
            LinearRgb::BLACK,
        ),
        &budget(),
    )
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
fn pre_cancelled_no_patch_conversion_precedes_frame_budget() {
    let table = build_table();
    let source = [255];
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::Contain,
        SamplingFilter::Box,
        None,
    ));
    cancellation.cancel();
    let insufficient_frame_budget = Budget::new(0, 64_000_000, 16_000_000);

    assert_eq!(
        convert_density_detailed(
            raster(&source, 1, 1, 1),
            &table,
            &conversion_settings,
            &insufficient_frame_budget,
        ),
        Err(DensityConversionError::Core(Error::Cancelled))
    );
}

#[test]
fn orientation_precedes_unsupported_sample_depth_through_both_apis() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );
    let channels = [PixelChannels::Gray, PixelChannels::Rgb, PixelChannels::Rgba];

    for channels in channels {
        let bottom_up = RasterFrameView::new(
            1,
            1,
            0,
            PixelFormat::new(channels, 16),
            ColorDescription::srgb(AlphaMode::Opaque),
            RasterOrientation::BottomUp,
            &[],
        );
        assert_eq!(
            convert_density_detailed(bottom_up, &table, &conversion_settings, &budget()),
            Err(DensityConversionError::Core(Error::Unsupported(
                ascii_core::UnsupportedReason::Orientation
            )))
        );
        assert_eq!(
            convert_density(bottom_up, &table, &conversion_settings, &budget()),
            Err(Error::Unsupported(
                ascii_core::UnsupportedReason::Orientation
            ))
        );

        let top_down = RasterFrameView::new(
            1,
            1,
            0,
            PixelFormat::new(channels, 16),
            ColorDescription::srgb(AlphaMode::Opaque),
            RasterOrientation::TopDown,
            &[],
        );
        assert_eq!(
            convert_density_detailed(top_down, &table, &conversion_settings, &budget()),
            Err(DensityConversionError::Core(Error::Unsupported(
                ascii_core::UnsupportedReason::SampleDepth
            )))
        );
        assert_eq!(
            convert_density(top_down, &table, &conversion_settings, &budget()),
            Err(Error::Unsupported(
                ascii_core::UnsupportedReason::SampleDepth
            ))
        );
    }
}

#[test]
fn output_budgets_fail_before_publishing_a_frame() {
    let table = build_table();
    let source = [255];

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
        convert_density_detailed(
            unsupported_format,
            &table,
            &conversion_settings,
            &no_frame_budget
        ),
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
fn patch_zero_initialization_is_charged_to_the_work_budget() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::Contain,
        SamplingFilter::Nearest,
        Some(PatchDimensions::new(1, 1).expect("one-sample patch is valid")),
    ));
    let budget_excluding_initialization = Budget::new(8_000_000, 64_000_000, 8);

    assert_eq!(
        convert_density_detailed(
            raster(&[128], 1, 1, 1),
            &table,
            &conversion_settings,
            &budget_excluding_initialization,
        ),
        Err(DensityConversionError::Core(Error::BudgetExceeded(
            BudgetKind::WorkUnits
        )))
    );
}

#[test]
fn premultiplied_validation_pass_is_charged_to_the_work_budget() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::Contain,
        SamplingFilter::Nearest,
        None,
    ));
    let premultiplied = RasterFrameView::new(
        1,
        1,
        4,
        PixelFormat::rgba8(),
        ColorDescription::srgb(AlphaMode::Premultiplied),
        RasterOrientation::TopDown,
        &[128, 128, 128, 255],
    );
    let opaque = raster(&[128], 1, 1, 1);
    let below_exact_work = Budget::new(8_000_000, 64_000_000, 5);
    let exact_work = Budget::new(8_000_000, 64_000_000, 6);

    assert_eq!(
        convert_density_detailed(opaque, &table, &conversion_settings, &below_exact_work),
        Err(DensityConversionError::Core(Error::BudgetExceeded(
            BudgetKind::WorkUnits
        )))
    );
    assert_eq!(
        convert_density_detailed(premultiplied, &table, &conversion_settings, &exact_work),
        Err(DensityConversionError::Core(Error::BudgetExceeded(
            BudgetKind::WorkUnits
        )))
    );
    assert!(convert_density_detailed(opaque, &table, &conversion_settings, &exact_work).is_ok());
}

#[test]
fn patch_memory_budget_includes_readable_input_extent_but_not_trailing_padding() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::Contain,
        SamplingFilter::Nearest,
        Some(PatchDimensions::new(1, 1).expect("one-sample patch is valid")),
    ));
    // Two readable rows: 64 bytes per row including inter-row padding, plus 32 trailing bytes.
    let mut pixels = vec![128; 160];
    pixels[64] = 129;
    let padded = raster(&pixels, 1, 2, 64);
    let normalized_and_patch_only = Budget::new(8_000_000, 136, 16_000_000);

    assert_eq!(
        convert_density_detailed(
            padded,
            &table,
            &conversion_settings,
            &normalized_and_patch_only
        ),
        Err(DensityConversionError::Core(Error::BudgetExceeded(
            BudgetKind::RasterBytes
        )))
    );
    let exact_extent = Budget::new(8_000_000, 201, 16_000_000);
    assert!(convert_density_detailed(padded, &table, &conversion_settings, &exact_extent).is_ok());
}

#[test]
fn unsupported_pixel_channels_precede_size_overflow() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );
    let unsupported_format = RasterFrameView::new(
        u32::MAX,
        1,
        0,
        PixelFormat::new(PixelChannels::Other(5), 16),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
        &[],
    );
    let sufficient_budget = Budget::new(u64::MAX, u64::MAX, u64::MAX);

    assert_eq!(
        convert_density_detailed(
            unsupported_format,
            &table,
            &conversion_settings,
            &sufficient_budget,
        ),
        Err(DensityConversionError::Core(Error::Unsupported(
            ascii_core::UnsupportedReason::PixelFormat
        )))
    );
}

#[test]
fn unsupported_sample_depth_precedes_short_stride() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );

    for channels in [PixelChannels::Gray, PixelChannels::Rgb, PixelChannels::Rgba] {
        let unsupported_depth = RasterFrameView::new(
            1,
            1,
            0,
            PixelFormat::new(channels, 16),
            ColorDescription::srgb(AlphaMode::Opaque),
            RasterOrientation::TopDown,
            &[],
        );
        assert_eq!(
            convert_density_detailed(unsupported_depth, &table, &conversion_settings, &budget(),),
            Err(DensityConversionError::Core(Error::Unsupported(
                ascii_core::UnsupportedReason::SampleDepth
            )))
        );
    }
}

#[test]
fn unsupported_alpha_and_profiles_precede_sampling_work_and_stride_through_both_apis() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(8, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );
    let unsupported_alpha = RasterFrameView::new(
        1,
        1,
        0,
        PixelFormat::gray8(),
        ColorDescription::srgb(AlphaMode::Straight),
        RasterOrientation::TopDown,
        &[],
    );
    let hdr = RasterFrameView::new(
        1,
        1,
        0,
        PixelFormat::gray8(),
        ColorDescription::new(
            ascii_core::ColorPrimaries::Srgb,
            TransferFunction::Pq,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        &[],
    );
    let wide_gamut = RasterFrameView::new(
        1,
        1,
        0,
        PixelFormat::gray8(),
        ColorDescription::new(
            ascii_core::ColorPrimaries::DisplayP3,
            TransferFunction::Srgb,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        &[],
    );
    let cases = [
        (
            unsupported_alpha,
            DensityConversionError::ColorNormalization(ColorNormalizationError::Raster(
                Error::Unsupported(ascii_core::UnsupportedReason::AlphaMode),
            )),
            Error::Unsupported(ascii_core::UnsupportedReason::AlphaMode),
        ),
        (
            hdr,
            DensityConversionError::ColorNormalization(ColorNormalizationError::UnsupportedColor(
                UnsupportedColorReason::HdrTransfer(TransferFunction::Pq),
            )),
            Error::Unsupported(ascii_core::UnsupportedReason::ColorDescription),
        ),
        (
            wide_gamut,
            DensityConversionError::ColorNormalization(ColorNormalizationError::UnsupportedColor(
                UnsupportedColorReason::WideGamutPrimaries(ascii_core::ColorPrimaries::DisplayP3),
            )),
            Error::Unsupported(ascii_core::UnsupportedReason::ColorDescription),
        ),
    ];
    let insufficient_sampling_budget = Budget::new(12, u64::MAX, 1);

    for (source, detailed_error, core_error) in cases {
        assert_eq!(
            convert_density_detailed(
                source,
                &table,
                &conversion_settings,
                &insufficient_sampling_budget,
            ),
            Err(detailed_error)
        );
        let error = convert_density(
            source,
            &table,
            &conversion_settings,
            &insufficient_sampling_budget,
        )
        .expect_err("unsupported profile must not produce a frame");
        assert_eq!(error, core_error);
        if core_error == Error::Unsupported(ascii_core::UnsupportedReason::ColorDescription) {
            assert_eq!(
                error.to_string(),
                "unsupported color profile: provide SDR sRGB input; HDR tone mapping and wide-gamut conversion are not supported"
            );
        }
    }
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
    let near_equal = LinearRgb::new(0.5001, 0.5001, 0.5001).expect("normalized linear gray");
    let mid_gray = LinearRgb::new(0.5, 0.5, 0.5).expect("normalized linear gray");
    assert_eq!(
        ConversionSettings::new(
            GridSizing::fixed_pixel_surface(8, 12),
            metadata(&table),
            &cancellation,
            mid_gray,
            near_equal,
        )
        .err(),
        Some(Error::Malformed(MalformedReason::InvalidConfiguration))
    );
    let distinguishable = LinearRgb::new(0.51, 0.51, 0.51).expect("normalized linear gray");
    assert!(
        ConversionSettings::new(
            GridSizing::fixed_pixel_surface(8, 12),
            metadata(&table),
            &cancellation,
            mid_gray,
            distinguishable,
        )
        .is_ok()
    );
}
