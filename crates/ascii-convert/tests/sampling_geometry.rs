#[allow(dead_code)]
mod support;

use std::mem::size_of;

use ascii_convert::{
    ConversionSettings, CropRect, FitMode, FitTransform, GlyphConfig, GlyphRepertoire, GlyphTable,
    LinearPremultipliedRgba, LinearRgb, PatchDescriptors, PatchDimensions, QualityPatches,
    SamplingFilter, SamplingOptions, build_glyph_table, convert_density_detailed,
};
use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, ColorPrimaries, Error,
    FrameMetadata, GlyphTableId, GridSize, MalformedReason, PixelFormat, RasterFrameView,
    RasterOrientation, TransferFunction,
};
use support::synthetic_font::synthetic_font;

const CELL_WIDTH: u32 = 8;
const CELL_HEIGHT: u32 = 12;
const LINEAR_TOLERANCE: f32 = 1.0e-6;

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
            ColorPrimaries::Srgb,
            TransferFunction::Linear,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        pixels,
    )
}

fn settings<'a>(
    table: &GlyphTable,
    cancellation: &'a CancellationToken,
    sizing: ascii_convert::GridSizing,
    sampling: SamplingOptions,
) -> ConversionSettings<'a> {
    ConversionSettings::new(
        sizing,
        FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 1, 0, 0, 1),
        cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .expect("fixture conversion settings are valid")
    .with_sampling(sampling)
}

fn one_cell_settings<'a>(
    table: &GlyphTable,
    cancellation: &'a CancellationToken,
    fit: FitMode,
    filter: SamplingFilter,
    patch: Option<PatchDimensions>,
) -> ConversionSettings<'a> {
    settings(
        table,
        cancellation,
        ascii_convert::GridSizing::fixed_pixel_surface(CELL_WIDTH, CELL_HEIGHT),
        SamplingOptions::new(fit, filter, patch),
    )
}

fn average(values: &[f32]) -> f64 {
    values.iter().map(|value| f64::from(*value)).sum::<f64>() / values.len() as f64
}

fn assert_round_trip(transform: FitTransform, source_x: f64, source_y: f64) {
    let (display_x, display_y) = transform
        .map_source_to_display(source_x, source_y)
        .expect("source point belongs to the transformed region");
    let (round_trip_x, round_trip_y) = transform
        .map_display_to_source(display_x, display_y)
        .expect("mapped display point belongs to the transformed region");
    assert!((round_trip_x - source_x).abs() < 1.0e-12);
    assert!((round_trip_y - source_y).abs() < 1.0e-12);
}

#[test]
fn contain_cover_and_custom_crop_share_invertible_pixel_edge_regions() {
    let contain =
        FitTransform::new(16, 8, 16, 16, FitMode::Contain).expect("contain geometry is valid");
    assert_eq!(contain.source_rect().x(), 0.0);
    assert_eq!(contain.source_rect().y(), 0.0);
    assert_eq!(contain.source_rect().width(), 16.0);
    assert_eq!(contain.source_rect().height(), 8.0);
    assert_eq!(contain.display_rect().x(), 0.0);
    assert_eq!(contain.display_rect().y(), 4.0);
    assert_eq!(contain.display_rect().width(), 16.0);
    assert_eq!(contain.display_rect().height(), 8.0);
    assert_eq!(contain.map_display_to_source(8.0, 3.99), None);
    assert_eq!(contain.map_display_to_source(8.0, 12.01), None);
    assert_eq!(contain.map_source_to_display(8.0, -0.01), None);
    assert_eq!(contain.map_display_to_source(0.0, 4.0), Some((0.0, 0.0)));
    assert_eq!(contain.map_display_to_source(16.0, 12.0), Some((16.0, 8.0)));
    assert_round_trip(contain, 5.25, 2.75);

    let cover = FitTransform::new(16, 8, 8, 8, FitMode::Cover).expect("cover geometry is valid");
    assert_eq!(cover.source_rect().x(), 4.0);
    assert_eq!(cover.source_rect().y(), 0.0);
    assert_eq!(cover.source_rect().width(), 8.0);
    assert_eq!(cover.source_rect().height(), 8.0);
    assert_eq!(cover.map_source_to_display(3.99, 4.0), None);
    assert_eq!(cover.map_source_to_display(12.01, 4.0), None);
    assert_eq!(cover.map_display_to_source(0.0, 0.0), Some((4.0, 0.0)));
    assert_eq!(cover.map_display_to_source(8.0, 8.0), Some((12.0, 8.0)));
    assert_round_trip(cover, 9.25, 3.5);

    let crop = CropRect::new(0.125, 0.25, 0.875, 0.75).expect("normalized crop is valid");
    let custom = FitTransform::new(13, 7, 32, 24, FitMode::CustomCrop(crop))
        .expect("custom-crop geometry is valid");
    assert_eq!(custom.source_rect().x(), 1.625);
    assert_eq!(custom.source_rect().y(), 1.75);
    assert_eq!(custom.source_rect().width(), 9.75);
    assert_eq!(custom.source_rect().height(), 3.5);
    assert_eq!(custom.map_source_to_display(1.624, 2.0), None);
    assert_eq!(custom.map_source_to_display(11.376, 2.0), None);
    assert_eq!(custom.map_source_to_display(1.625, 1.75), Some((0.0, 0.0)));
    assert_eq!(
        custom.map_source_to_display(11.375, 5.25),
        Some((32.0, 24.0))
    );
    assert_round_trip(custom, 6.5, 3.0);
}

#[test]
fn detailed_conversion_returns_sampling_transform_for_its_floor_sized_surface() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let pixels = [128; 6];
    let fit = FitMode::Contain;
    let conversion = convert_density_detailed(
        linear_gray_raster(&pixels, 3, 2, 3),
        &table,
        &settings(
            &table,
            &cancellation,
            ascii_convert::GridSizing::fixed_pixel_surface(19, 29),
            SamplingOptions::new(fit, SamplingFilter::Box, None),
        ),
        &budget(),
    )
    .expect("fixed surface converts");

    assert_eq!(conversion.frame().size(), GridSize::new(2, 2));
    assert_eq!(conversion.transform().display_dimensions(), (16, 24));
    assert_eq!(
        conversion.transform(),
        FitTransform::new(3, 2, 16, 24, fit).expect("expected transform is valid")
    );
    assert_eq!(
        ascii_convert::GridSizing::fixed_pixel_surface(19, 29)
            .fixed_surface_leftovers(CELL_WIDTH, CELL_HEIGHT),
        Some((3, 5))
    );
}

#[test]
fn contain_letterboxes_reject_pointer_mapping_and_integrate_as_background() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let patch = PatchDimensions::new(1, 1).expect("one-sample patch is valid");
    let settings = settings(
        &table,
        &cancellation,
        ascii_convert::GridSizing::fixed_pixel_surface(16, 12),
        SamplingOptions::new(FitMode::Contain, SamplingFilter::Box, Some(patch)),
    );
    let conversion = convert_density_detailed(
        linear_gray_raster(&[u8::MAX], 1, 1, 1),
        &table,
        &settings,
        &budget(),
    )
    .expect("letterboxed surface converts");

    assert_eq!(conversion.frame().size(), GridSize::new(2, 1));
    assert_eq!(conversion.transform().display_rect().x(), 2.0);
    assert_eq!(conversion.transform().map_display_to_source(0.0, 6.0), None);
    let patches = conversion.patches().expect("quality patches are retained");
    for index in 0..patches.cell_count() {
        let values = patches
            .luminance_for_cell(index)
            .expect("each output cell has a patch");
        assert!((f64::from(values[0]) - 0.75).abs() < f64::from(LINEAR_TOLERANCE));
    }
}

#[test]
fn box_area_patches_conserve_fractional_crop_coverage_and_ignore_stride_padding() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let crop = FitMode::CustomCrop(
        CropRect::new(0.125, 0.25, 0.625, 0.75).expect("fractional crop is valid"),
    );
    let coarse_dimensions = PatchDimensions::new(2, 2).expect("small patch is valid");
    let fine_dimensions = PatchDimensions::new(4, 3).expect("fine patch is valid");
    let direct_dimensions = PatchDimensions::new(1, 1).expect("direct integral patch is valid");
    let direct_settings = one_cell_settings(
        &table,
        &cancellation,
        crop,
        SamplingFilter::Box,
        Some(direct_dimensions),
    );
    let coarse_settings = one_cell_settings(
        &table,
        &cancellation,
        crop,
        SamplingFilter::Box,
        Some(coarse_dimensions),
    );
    let fine_settings = one_cell_settings(
        &table,
        &cancellation,
        crop,
        SamplingFilter::Box,
        Some(fine_dimensions),
    );
    let packed_pixels = [0, 64, 128, 192, 255, 192, 128, 64];
    let expected_mean = 127.875_f64 / 255.0;

    let direct = convert_density_detailed(
        linear_gray_raster(&packed_pixels, 4, 2, 4),
        &table,
        &direct_settings,
        &budget(),
    )
    .expect("whole-cell box integration succeeds");
    let direct_patch = direct
        .patches()
        .expect("direct integration retains its patch")
        .luminance_for_cell(0)
        .expect("one-cell patch exists");
    assert!((f64::from(direct_patch[0]) - expected_mean).abs() < 1.0e-6);

    let coarse = convert_density_detailed(
        linear_gray_raster(&packed_pixels, 4, 2, 4),
        &table,
        &coarse_settings,
        &budget(),
    )
    .expect("fractional area integration succeeds");
    let coarse_patch = coarse
        .patches()
        .expect("coarse quality patch is retained")
        .luminance_for_cell(0)
        .expect("one-cell patch exists");
    let expected_values = [32.0_f32 / 255.0, 96.0 / 255.0, 223.5 / 255.0, 160.0 / 255.0];
    for (actual, expected) in coarse_patch.iter().zip(expected_values) {
        assert!((actual - expected).abs() < LINEAR_TOLERANCE);
    }
    assert!((average(coarse_patch) - expected_mean).abs() < f64::from(LINEAR_TOLERANCE));
    assert!((average(coarse_patch) - f64::from(direct_patch[0])).abs() < 1.0e-6);
    let descriptor: PatchDescriptors = coarse
        .patches()
        .expect("coarse patch collection remains available")
        .descriptor(0)
        .expect("one-cell descriptor exists");
    assert!((f64::from(descriptor.mean_luminance()) - expected_mean).abs() < 1.0e-6);
    assert!((f64::from(descriptor.mean_absolute_dx()) - 0.25).abs() < 1.0e-6);
    assert!((f64::from(descriptor.mean_absolute_dy()) - 0.500_980_4).abs() < 1.0e-6);

    let fine = convert_density_detailed(
        linear_gray_raster(&packed_pixels, 4, 2, 4),
        &table,
        &fine_settings,
        &budget(),
    )
    .expect("fractional subcell integration succeeds");
    let fine_patch = fine
        .patches()
        .expect("fine quality patch is retained")
        .luminance_for_cell(0)
        .expect("one-cell patch exists");
    assert!((average(fine_patch) - expected_mean).abs() < f64::from(LINEAR_TOLERANCE));
    assert!((average(fine_patch) - f64::from(direct_patch[0])).abs() < 1.0e-6);

    let padded_pixels = [0, 64, 128, 192, 19, 23, 29, 255, 192, 128, 64, 31, 37, 41];
    let padded = convert_density_detailed(
        linear_gray_raster(&padded_pixels, 4, 2, 7),
        &table,
        &coarse_settings,
        &budget(),
    )
    .expect("padded-stride raster converts");
    let padded_patch = padded
        .patches()
        .expect("padded raster retains its patch")
        .luminance_for_cell(0)
        .expect("one-cell patch exists");
    assert_eq!(padded_patch, coarse_patch);
    assert_eq!(padded.frame().cells(), coarse.frame().cells());
}

#[test]
fn extreme_valid_crop_does_not_underflow_box_footprint_area() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let smallest_positive = f64::from_bits(1);
    let crop = CropRect::new(0.0, 0.0, smallest_positive * 2.0, smallest_positive * 3.0)
        .expect("positive subnormal crop is valid");
    let patch = PatchDimensions::new(1, 1).expect("one-sample patch is valid");
    let settings = settings(
        &table,
        &cancellation,
        ascii_convert::GridSizing::columns(1),
        SamplingOptions::new(FitMode::CustomCrop(crop), SamplingFilter::Box, Some(patch)),
    );

    let conversion = convert_density_detailed(
        linear_gray_raster(&[128], 1, 1, 1),
        &table,
        &settings,
        &budget(),
    )
    .expect("area integration over a valid tiny crop succeeds");
    assert_eq!(conversion.frame().size(), GridSize::new(1, 1));
    assert_eq!(
        conversion.transform().source_rect().width(),
        smallest_positive * 2.0
    );
    assert_eq!(
        conversion.transform().source_rect().height(),
        smallest_positive * 3.0
    );
    let sample = conversion
        .patches()
        .expect("tiny crop retains its quality patch")
        .luminance_for_cell(0)
        .expect("one-cell patch exists")[0];
    assert!((f64::from(sample) - 128.0 / 255.0).abs() < 1.0e-6);
}

#[test]
fn sampling_identity_changes_with_filter_patch_size_and_resolved_geometry() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let pixels = [0, 64, 128, 192, 255, 192, 128, 64];
    let raster = || linear_gray_raster(&pixels, 4, 2, 4);
    let base_crop = CropRect::new(0.0, 0.0, 1.0, 1.0).expect("full crop is valid");
    let narrower_crop = CropRect::new(0.0, 0.0, 0.5, 1.0).expect("narrow crop is valid");
    let patch_2x2 = PatchDimensions::new(2, 2).expect("patch size is valid");
    let patch_2x3 = PatchDimensions::new(2, 3).expect("alternate patch size is valid");
    let convert = |fit, filter, patch| {
        convert_density_detailed(
            raster(),
            &table,
            &one_cell_settings(&table, &cancellation, fit, filter, Some(patch)),
            &budget(),
        )
        .expect("sampling configuration converts")
        .sampling_identity()
    };

    let base = convert(
        FitMode::CustomCrop(base_crop),
        SamplingFilter::Box,
        patch_2x2,
    );
    assert_eq!(
        base,
        convert(
            FitMode::CustomCrop(base_crop),
            SamplingFilter::Box,
            patch_2x2,
        )
    );
    assert_ne!(
        base,
        convert(
            FitMode::CustomCrop(base_crop),
            SamplingFilter::Nearest,
            patch_2x2,
        )
    );
    assert_ne!(
        base,
        convert(
            FitMode::CustomCrop(base_crop),
            SamplingFilter::Box,
            patch_2x3,
        )
    );
    assert_ne!(
        base,
        convert(
            FitMode::CustomCrop(narrower_crop),
            SamplingFilter::Box,
            patch_2x2,
        )
    );
}

#[test]
fn patch_size_and_aggregate_budgets_fail_at_exact_thresholds_before_allocation() {
    assert_eq!(
        PatchDimensions::new(65, 1),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    );
    assert_eq!(
        PatchDimensions::new(64, 64)
            .expect("maximum bounded patch is valid")
            .sample_count(),
        4096
    );

    let table = build_table();
    let cancellation = CancellationToken::new();
    let patch = PatchDimensions::new(2, 3).expect("bounded patch is valid");
    let settings = one_cell_settings(
        &table,
        &cancellation,
        FitMode::Contain,
        SamplingFilter::Nearest,
        Some(patch),
    );
    let source = [128];
    let input = || linear_gray_raster(&source, 1, 1, 1);
    let cells = 1_u64;
    let sample_values = patch.sample_count();
    let exact_work = 1 + 3 * cells + sample_values + 3 * sample_values + sample_values + cells;
    assert_eq!(exact_work, 35);

    let below_work = Budget::new(1_000_000, u64::MAX, exact_work - 1);
    assert_eq!(
        convert_density_detailed(input(), &table, &settings, &below_work),
        Err(ascii_convert::DensityConversionError::Core(
            Error::BudgetExceeded(BudgetKind::WorkUnits)
        ))
    );

    let readable_input = 1_u64;
    let normalized_raster =
        u64::try_from(size_of::<LinearPremultipliedRgba>()).expect("normalized pixel size fits");
    let patch_storage = sample_values * size_of::<f32>() as u64
        + size_of::<PatchDescriptors>() as u64
        + size_of::<QualityPatches>() as u64;
    let exact_raster_bytes = readable_input + normalized_raster + patch_storage;
    let below_raster = Budget::new(1_000_000, exact_raster_bytes - 1, exact_work);
    assert_eq!(
        convert_density_detailed(input(), &table, &settings, &below_raster),
        Err(ascii_convert::DensityConversionError::Core(
            Error::BudgetExceeded(BudgetKind::RasterBytes)
        ))
    );

    let exact = Budget::new(1_000_000, exact_raster_bytes, exact_work);
    assert!(convert_density_detailed(input(), &table, &settings, &exact).is_ok());
}

#[test]
fn cancelled_quality_conversion_returns_no_partial_result() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let patch = PatchDimensions::new(8, 16).expect("quality patch is within limits");
    let settings = one_cell_settings(
        &table,
        &cancellation,
        FitMode::Contain,
        SamplingFilter::Box,
        Some(patch),
    );
    cancellation.cancel();

    assert_eq!(
        convert_density_detailed(
            linear_gray_raster(&[128], 1, 1, 1),
            &table,
            &settings,
            &budget(),
        ),
        Err(ascii_convert::DensityConversionError::Core(
            Error::Cancelled
        ))
    );
}
