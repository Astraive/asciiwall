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
use sha2::{Digest, Sha256};
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

const FILTER_FIXTURE_WIDTH: usize = 4;
const FILTER_FIXTURE_HEIGHT: usize = 6;
const FILTER_FIXTURE_PIXEL_COUNT: usize = FILTER_FIXTURE_WIDTH * FILTER_FIXTURE_HEIGHT;
const FILTER_PATCH_WIDTH: u32 = 2;
const FILTER_PATCH_HEIGHT: u32 = 3;
const AREA_COVERAGE_TOLERANCE: f32 = 1.0e-6;

#[derive(Clone, Copy)]
struct FilterComparisonFixture {
    name: &'static str,
    generator: &'static str,
    pixels: [u8; FILTER_FIXTURE_PIXEL_COUNT],
    expects_aliasing: bool,
}

fn filter_comparison_fixtures() -> [FilterComparisonFixture; 4] {
    // Generator parameters are deliberately simple and source-local: a 1 px vertical line at
    // x=1; a 1 px, phase-zero checkerboard; a fixed 4x6 A mask; and a fixed gradient formula.
    let mut thin_line = [0; FILTER_FIXTURE_PIXEL_COUNT];
    for y in 0..FILTER_FIXTURE_HEIGHT {
        thin_line[y * FILTER_FIXTURE_WIDTH + 1] = u8::MAX;
    }

    let mut checkerboard = [0; FILTER_FIXTURE_PIXEL_COUNT];
    for y in 0..FILTER_FIXTURE_HEIGHT {
        for x in 0..FILTER_FIXTURE_WIDTH {
            checkerboard[y * FILTER_FIXTURE_WIDTH + x] = if (x + y).is_multiple_of(2) {
                u8::MAX
            } else {
                0
            };
        }
    }

    let mut text_like = [0; FILTER_FIXTURE_PIXEL_COUNT];
    let text_rows = [0b0110_u8, 0b1001, 0b1111, 0b1001, 0b1001, 0b1001];
    for (y, mask) in text_rows.into_iter().enumerate() {
        for x in 0..FILTER_FIXTURE_WIDTH {
            let bit = 1_u8 << (FILTER_FIXTURE_WIDTH - 1 - x);
            if mask & bit != 0 {
                text_like[y * FILTER_FIXTURE_WIDTH + x] = u8::MAX;
            }
        }
    }

    let mut photo_like = [0; FILTER_FIXTURE_PIXEL_COUNT];
    for y in 0..FILTER_FIXTURE_HEIGHT {
        for x in 0..FILTER_FIXTURE_WIDTH {
            let x = i16::try_from(x).expect("fixture x coordinate fits i16");
            let y = i16::try_from(y).expect("fixture y coordinate fits i16");
            let texture = ((x * 5 + y * 3) % 5 - 2) * 4;
            let value = (18 + 23 * x + 31 * y + texture).clamp(0, 255);
            let index = usize::try_from(y).expect("fixture y coordinate fits usize")
                * FILTER_FIXTURE_WIDTH
                + usize::try_from(x).expect("fixture x coordinate fits usize");
            photo_like[index] = u8::try_from(value).expect("fixture sample is in the byte range");
        }
    }

    [
        FilterComparisonFixture {
            name: "thin-line",
            generator: "4x6 gray8; x=1 is 255 for every row; all other pixels are 0",
            pixels: thin_line,
            expects_aliasing: true,
        },
        FilterComparisonFixture {
            name: "checkerboard",
            generator: "4x6 gray8; (x+y)%2==0 is 255, otherwise 0",
            pixels: checkerboard,
            expects_aliasing: true,
        },
        FilterComparisonFixture {
            name: "text-like",
            generator: "4x6 gray8; A mask rows 0110,1001,1111,1001,1001,1001; ink=255",
            pixels: text_like,
            expects_aliasing: false,
        },
        FilterComparisonFixture {
            name: "photo-like",
            generator: "4x6 gray8; clamp(18+23*x+31*y+4*((5*x+3*y)%5-2),0,255)",
            pixels: photo_like,
            expects_aliasing: false,
        },
    ]
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes.iter().copied() {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    hex
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_bytes(&Sha256::digest(bytes))
}

fn filter_comparison_settings_record(
    fixture: &FilterComparisonFixture,
    filter: SamplingFilter,
    fixture_sha256: &str,
    font_sha256: &str,
    table_fingerprint: &str,
) -> String {
    let filter_name = match filter {
        SamplingFilter::Box => "box",
        SamplingFilter::Nearest => "nearest",
    };
    format!(
        concat!(
            "record=cnv-003-filter-comparison-v1\n",
            "fixture={}\n",
            "generator={}\n",
            "fixture_sha256={}\n",
            "raster_dimensions={}x{}\n",
            "row_stride_bytes={}\n",
            "pixel_format=gray8\n",
            "primaries=srgb\n",
            "transfer=linear\n",
            "alpha=opaque\n",
            "orientation=top_down\n",
            "fit=contain\n",
            "crop=none; source=full-frame\n",
            "grid_sizing=fixed_pixel_surface(8,12)\n",
            "grid_dimensions=1x1\n",
            "cell_dimensions=8x12\n",
            "filter={}\n",
            "patch=2x3\n",
            "foreground=linear_rgb(1,1,1)\n",
            "background=linear_rgb(0,0,0)\n",
            "metadata=table_fingerprint:{},frame_id:1,generation:0,pts_us:0,duration_us:1\n",
            "budget=max_frame_bytes:8000000,max_raster_bytes:64000000,max_work_units:16000000\n",
            "font_generator=synthetic_font(false)\n",
            "font_sha256={}\n",
            "glyph_repertoire=BA<space>\n",
            "glyph_config=point_size:8.0,cell_width:8,cell_height:12,baseline:9\n",
        ),
        fixture.name,
        fixture.generator,
        fixture_sha256,
        FILTER_FIXTURE_WIDTH,
        FILTER_FIXTURE_HEIGHT,
        FILTER_FIXTURE_WIDTH,
        filter_name,
        table_fingerprint,
        font_sha256,
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
fn collapsed_inverse_mapped_boundaries_preserve_64_wide_box_patches() {
    let table = build_table();
    let cancellation = CancellationToken::new();
    let smallest_positive = f64::from_bits(1);
    let wide_patch = PatchDimensions::new(64, 64).expect("maximum patch size is valid");
    let tiny_crop = CropRect::new(0.0, 0.0, smallest_positive * 2.0, smallest_positive * 3.0)
        .expect("positive subnormal crop is valid");
    let tiny_settings = one_cell_settings(
        &table,
        &cancellation,
        FitMode::CustomCrop(tiny_crop),
        SamplingFilter::Box,
        Some(wide_patch),
    );
    let tiny_conversion = convert_density_detailed(
        linear_gray_raster(&[128], 1, 1, 1),
        &table,
        &tiny_settings,
        &budget(),
    )
    .expect("subnormal crop converts across collapsed patch boundaries");
    assert_eq!(tiny_conversion.frame().size(), GridSize::new(1, 1));
    let tiny_patches = tiny_conversion
        .patches()
        .expect("subnormal crop retains its quality patch");
    assert_eq!(tiny_patches.dimensions(), wide_patch);
    assert_eq!(tiny_patches.cell_count(), 1);
    let tiny_values = tiny_patches
        .luminance_for_cell(0)
        .expect("one-cell patch exists");
    assert_eq!(tiny_values.len(), 64 * 64);
    let tiny_expected = 128.0_f32 / 255.0;
    assert!(tiny_values.iter().all(|value| {
        value.is_finite() && (0.0..=1.0).contains(value) && (value - tiny_expected).abs() < 1.0e-6
    }));
    let tiny_descriptor = tiny_patches
        .descriptor(0)
        .expect("one-cell descriptor exists");
    assert!(tiny_descriptor.mean_luminance().is_finite());
    assert!((tiny_descriptor.mean_luminance() - tiny_expected).abs() < 1.0e-6);
    assert_eq!(tiny_descriptor.mean_absolute_dx(), 0.0);
    assert_eq!(tiny_descriptor.mean_absolute_dy(), 0.0);

    let lower_crop_edge = f64::from_bits(0.5_f64.to_bits() - 1);
    let upper_crop_edge = f64::from_bits(0.5_f64.to_bits() + 1);
    let shifted_crop = CropRect::new(lower_crop_edge, 0.0, upper_crop_edge, 1.0)
        .expect("adjacent normalized crop edges around a source boundary are valid");
    let narrow_patch = PatchDimensions::new(64, 1).expect("wide one-row patch is valid");
    let shifted_settings = one_cell_settings(
        &table,
        &cancellation,
        FitMode::CustomCrop(shifted_crop),
        SamplingFilter::Box,
        Some(narrow_patch),
    );
    let shifted_conversion = convert_density_detailed(
        linear_gray_raster(&[64, 192], 2, 1, 2),
        &table,
        &shifted_settings,
        &budget(),
    )
    .expect("subpixel crop crossing a source boundary converts");
    assert_eq!(shifted_conversion.frame().size(), GridSize::new(1, 1));
    let source_rect = shifted_conversion.transform().source_rect();
    assert!(source_rect.x() < 1.0);
    assert!(source_rect.x() + source_rect.width() > 1.0);
    let shifted_patches = shifted_conversion
        .patches()
        .expect("shifted crop retains its quality patch");
    assert_eq!(shifted_patches.dimensions(), narrow_patch);
    let shifted_values = shifted_patches
        .luminance_for_cell(0)
        .expect("one-cell patch exists");
    let left_pixel = 64.0_f32 / 255.0;
    let right_pixel = 192.0_f32 / 255.0;
    assert!(
        shifted_values
            .iter()
            .all(|value| { value.is_finite() && (0.0..=1.0).contains(value) })
    );
    assert!((shifted_values[0] - left_pixel).abs() < 1.0e-6);
    assert!((shifted_values[63] - right_pixel).abs() < 1.0e-6);
    let expected_mean = 448.0_f32 / 765.0;
    let shifted_descriptor = shifted_patches
        .descriptor(0)
        .expect("one-cell descriptor exists");
    assert!(shifted_descriptor.mean_luminance().is_finite());
    assert!((shifted_descriptor.mean_luminance() - expected_mean).abs() < 1.0e-6);
    assert!(shifted_descriptor.mean_absolute_dx().is_finite());
    assert!(shifted_descriptor.mean_absolute_dx() > 0.0);
    assert!(shifted_descriptor.mean_absolute_dx() <= 1.0);
    assert_eq!(shifted_descriptor.mean_absolute_dy(), 0.0);
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

#[test]
fn quality_conversion_recovers_after_cancelled_attempt() {
    let table = build_table();
    let pixels = [128];
    let cancelled = CancellationToken::new();
    let cancelled_settings = one_cell_settings(
        &table,
        &cancelled,
        FitMode::Contain,
        SamplingFilter::Box,
        Some(PatchDimensions::new(1, 1).expect("one-sample patch is valid")),
    );
    cancelled.cancel();
    assert_eq!(
        convert_density_detailed(
            linear_gray_raster(&pixels, 1, 1, 1),
            &table,
            &cancelled_settings,
            &budget(),
        ),
        Err(ascii_convert::DensityConversionError::Core(
            Error::Cancelled
        ))
    );

    let recovery_cancellation = CancellationToken::new();
    let recovery_settings = one_cell_settings(
        &table,
        &recovery_cancellation,
        FitMode::Contain,
        SamplingFilter::Box,
        Some(PatchDimensions::new(1, 1).expect("one-sample patch is valid")),
    );
    assert!(
        convert_density_detailed(
            linear_gray_raster(&pixels, 1, 1, 1),
            &table,
            &recovery_settings,
            &budget(),
        )
        .is_ok()
    );
}

#[test]
fn box_and_nearest_filters_compare_area_coverage_on_synthetic_patterns() {
    // The SFNT is assembled exclusively from tests/support/synthetic_font.rs source constants.
    let font_bytes = synthetic_font(false);
    let font_sha256 = sha256_hex(&font_bytes);
    let table = build_glyph_table(
        &font_bytes,
        &GlyphRepertoire::new("BA ").expect("fixture repertoire is supported"),
        &GlyphConfig::new(8.0, CELL_WIDTH, CELL_HEIGHT, 9)
            .expect("fixture glyph geometry is valid"),
        &Budget::new(1_000_000, 2_000_000, 2_000_000),
        &CancellationToken::new(),
    )
    .expect("synthetic font builds a measured table");
    let table_fingerprint = hex_bytes(table.fingerprint());
    let patch = PatchDimensions::new(FILTER_PATCH_WIDTH, FILTER_PATCH_HEIGHT)
        .expect("comparison patch dimensions are valid");
    let cancellation = CancellationToken::new();
    let raster_width =
        u32::try_from(FILTER_FIXTURE_WIDTH).expect("fixture width fits a raster dimension");
    let raster_height =
        u32::try_from(FILTER_FIXTURE_HEIGHT).expect("fixture height fits a raster dimension");

    for fixture in filter_comparison_fixtures() {
        let fixture_sha256 = sha256_hex(&fixture.pixels);
        println!("fixture={} sha256={fixture_sha256}", fixture.name);
        let convert = |filter| {
            let settings = settings(
                &table,
                &cancellation,
                ascii_convert::GridSizing::fixed_pixel_surface(CELL_WIDTH, CELL_HEIGHT),
                SamplingOptions::new(FitMode::Contain, filter, Some(patch)),
            );
            convert_density_detailed(
                linear_gray_raster(
                    &fixture.pixels,
                    raster_width,
                    raster_height,
                    FILTER_FIXTURE_WIDTH,
                ),
                &table,
                &settings,
                &budget(),
            )
            .expect("synthetic filter-comparison fixture converts")
        };
        let box_conversion = convert(SamplingFilter::Box);
        let nearest_conversion = convert(SamplingFilter::Nearest);
        assert_eq!(box_conversion.frame().size(), GridSize::new(1, 1));
        assert_eq!(
            box_conversion.frame().metadata(),
            nearest_conversion.frame().metadata(),
            "fixture={}",
            fixture.name
        );
        assert_eq!(
            box_conversion.transform(),
            nearest_conversion.transform(),
            "fixture={}",
            fixture.name
        );

        let box_values = box_conversion
            .patches()
            .expect("Box conversion retains its patch")
            .luminance_for_cell(0)
            .expect("Box conversion has one output cell");
        let nearest_values = nearest_conversion
            .patches()
            .expect("Nearest conversion retains its patch")
            .luminance_for_cell(0)
            .expect("Nearest conversion has one output cell");
        assert_eq!(box_values.len(), 6);
        assert_eq!(nearest_values.len(), 6);

        let patch_width = usize::try_from(FILTER_PATCH_WIDTH).expect("patch width fits usize");
        let patch_height = usize::try_from(FILTER_PATCH_HEIGHT).expect("patch height fits usize");
        let mut largest_filter_difference = 0.0_f32;
        for patch_y in 0..patch_height {
            for patch_x in 0..patch_width {
                let patch_index = patch_y * patch_width + patch_x;
                let source_x = patch_x * 2;
                let source_y = patch_y * 2;
                let area_sum =
                    u16::from(fixture.pixels[source_y * FILTER_FIXTURE_WIDTH + source_x])
                        + u16::from(fixture.pixels[source_y * FILTER_FIXTURE_WIDTH + source_x + 1])
                        + u16::from(
                            fixture.pixels[(source_y + 1) * FILTER_FIXTURE_WIDTH + source_x],
                        )
                        + u16::from(
                            fixture.pixels[(source_y + 1) * FILTER_FIXTURE_WIDTH + source_x + 1],
                        );
                let area_reference = f32::from(area_sum) / 4.0 / 255.0;
                assert!(
                    (box_values[patch_index] - area_reference).abs() <= AREA_COVERAGE_TOLERANCE,
                    "fixture={} patch=({}, {}) Box={} area_reference={} tolerance={}",
                    fixture.name,
                    patch_x,
                    patch_y,
                    box_values[patch_index],
                    area_reference,
                    AREA_COVERAGE_TOLERANCE
                );

                // With a 4x6 source, 8x12 destination cell, and 2x3 patch, each nearest
                // footprint's center maps to the lower-right pixel of its 2x2 source block.
                let nearest_source_index = (source_y + 1) * FILTER_FIXTURE_WIDTH + source_x + 1;
                let nearest_reference = f32::from(fixture.pixels[nearest_source_index]) / 255.0;
                assert!(
                    (nearest_values[patch_index] - nearest_reference).abs()
                        <= AREA_COVERAGE_TOLERANCE,
                    "fixture={} patch=({}, {}) Nearest={} center_reference={} tolerance={}",
                    fixture.name,
                    patch_x,
                    patch_y,
                    nearest_values[patch_index],
                    nearest_reference,
                    AREA_COVERAGE_TOLERANCE
                );
                largest_filter_difference = largest_filter_difference
                    .max((box_values[patch_index] - nearest_values[patch_index]).abs());
            }
        }
        if fixture.expects_aliasing {
            assert!(
                largest_filter_difference >= 0.49,
                "fixture={} did not expose thin-feature/checkerboard aliasing: max difference {}",
                fixture.name,
                largest_filter_difference
            );
        }

        for filter in [SamplingFilter::Box, SamplingFilter::Nearest] {
            let record = filter_comparison_settings_record(
                &fixture,
                filter,
                &fixture_sha256,
                &font_sha256,
                &table_fingerprint,
            );
            println!(
                "settings_record_sha256 fixture={} filter={} sha256={}\n{}",
                fixture.name,
                match filter {
                    SamplingFilter::Box => "box",
                    SamplingFilter::Nearest => "nearest",
                },
                sha256_hex(record.as_bytes()),
                record
            );
        }
    }
}
