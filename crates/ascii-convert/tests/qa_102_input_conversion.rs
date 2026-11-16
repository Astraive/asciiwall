#[allow(dead_code)]
mod support;

use ascii_convert::{
    ColorNormalizationError, ConversionSettings, CropRect, DensityConversionError, FitMode,
    GlyphConfig, GlyphRepertoire, GlyphTable, GridSizing, LinearRgb, PatchDimensions,
    SamplingFilter, SamplingOptions, UnsupportedColorReason, build_glyph_table,
    convert_density, convert_density_detailed, normalize_raster,
};
use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, ColorPrimaries, Error,
    FrameMetadata, GlyphTableId, MalformedReason, PixelFormat, RasterFrameView, RasterOrientation,
    TransferFunction,
};
use support::synthetic_font::synthetic_font;

const CELL_WIDTH: u32 = 8;
const CELL_HEIGHT: u32 = 12;

fn glyph_table() -> GlyphTable {
    let repertoire = GlyphRepertoire::new("BA ").expect("fixture repertoire is supported");
    build_glyph_table(
        &synthetic_font(false),
        &repertoire,
        &GlyphConfig::new(8.0, CELL_WIDTH, CELL_HEIGHT, 9)
            .expect("synthetic glyph geometry is valid"),
        &Budget::new(1_000_000, 2_000_000, 2_000_000),
        &CancellationToken::new(),
    )
    .expect("synthetic font builds a measured glyph table")
}

fn conversion_settings<'a>(
    table: &GlyphTable,
    cancellation: &'a CancellationToken,
    sizing: GridSizing,
) -> ConversionSettings<'a> {
    ConversionSettings::new(
        sizing,
        FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 1, 0, 0, 0),
        cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .expect("fixture conversion settings are valid")
}

fn conversion_budget() -> Budget {
    Budget::new(1_000_000, 2_000_000, 2_000_000)
}

fn gray_raster(pixels: &[u8], width: u32, height: u32, row_stride: usize) -> RasterFrameView<'_> {
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

#[test]
fn ec_006_unsupported_hdr_and_profile_return_typed_reasons() {
    let table = glyph_table();
    let cancellation = CancellationToken::new();
    let settings = conversion_settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(CELL_WIDTH, CELL_HEIGHT),
    );
    let scenarios = [
        (
            ColorDescription::new(
                ColorPrimaries::Srgb,
                TransferFunction::Pq,
                AlphaMode::Opaque,
            ),
            UnsupportedColorReason::HdrTransfer(TransferFunction::Pq),
            "density conversion color normalization failed: HDR transfer Pq is unsupported; provide SDR input or use an explicitly supported tone-mapping path",
            "unsupported HDR transfer Pq: provide SDR input or use an explicitly supported tone-mapping path",
        ),
        (
            ColorDescription::new(
                ColorPrimaries::Rec2020,
                TransferFunction::Srgb,
                AlphaMode::Opaque,
            ),
            UnsupportedColorReason::WideGamutPrimaries(ColorPrimaries::Rec2020),
            "density conversion color normalization failed: wide-gamut primaries Rec2020 are unsupported; convert to sRGB with tested color-managed tooling",
            "unsupported wide-gamut primaries Rec2020: convert to sRGB with tested color-managed tooling",
        ),
    ];
    let source = [40, 96, 180];

    for (color, expected_reason, expected_message, expected_convenience_message) in scenarios {
        let input = RasterFrameView::new(
            1,
            1,
            source.len(),
            PixelFormat::rgb8(),
            color,
            RasterOrientation::TopDown,
            &source,
        );
        let error = convert_density_detailed(input, &table, &settings, &conversion_budget())
            .expect_err("unsupported color metadata must fail rather than emit a wrong frame");

        assert_eq!(
            error,
            DensityConversionError::ColorNormalization(ColorNormalizationError::UnsupportedColor(
                expected_reason
            ))
        );
        assert_eq!(error.to_string(), expected_message);
        let convenience_error = convert_density(input, &table, &settings, &conversion_budget())
            .expect_err("convenience conversion must preserve the unsupported-color cause");
        let convenience_reason = match expected_reason {
            UnsupportedColorReason::HdrTransfer(transfer) => {
                ascii_core::UnsupportedReason::HdrTransfer(transfer)
            }
            UnsupportedColorReason::WideGamutPrimaries(primaries) => {
                ascii_core::UnsupportedReason::WideGamutPrimaries(primaries)
            }
        };
        assert_eq!(convenience_error, Error::Unsupported(convenience_reason));
        assert_eq!(
            convenience_error.to_string(),
            expected_convenience_message
        );
    }
}

#[test]
fn ec_007_black_white_and_flat_images_map_stably_by_measured_glyph_coverage() {
    let table = glyph_table();
    let glyphs = table.glyphs();
    let blank = glyphs
        .iter()
        .find(|glyph| glyph.character() == ' ')
        .expect("fixture repertoire contains blank space");
    let ink_a = glyphs
        .iter()
        .find(|glyph| glyph.character() == 'A')
        .expect("fixture repertoire contains A");
    let ink_b = glyphs
        .iter()
        .find(|glyph| glyph.character() == 'B')
        .expect("fixture repertoire contains B");
    assert_eq!(blank.mean_coverage(), 0.0);
    assert_eq!(ink_a.mean_coverage(), ink_b.mean_coverage());
    assert!(ink_a.mean_coverage() > blank.mean_coverage());

    let cancellation = CancellationToken::new();
    let settings = conversion_settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(2 * CELL_WIDTH, 2 * CELL_HEIGHT),
    )
    .with_sampling(SamplingOptions::new(
        FitMode::Contain,
        SamplingFilter::Box,
        Some(PatchDimensions::new(2, 2).expect("small quality patch is valid")),
    ));
    let mut black_output_slot = None;
    let mut white_output_slot = None;

    for sample in [0_u8, 128, 255] {
        let source = [sample; 8 * 12];
        let input = gray_raster(&source, 8, 12, 8);
        let first = convert_density_detailed(input, &table, &settings, &conversion_budget())
            .expect("a constant gray raster converts");
        let repeated = convert_density_detailed(input, &table, &settings, &conversion_budget())
            .expect("repeating the flat-raster conversion remains valid");
        let first_glyph = first.frame().cells()[0].glyph();

        assert_eq!(first.frame().cells(), repeated.frame().cells());
        assert!(
            first
                .frame()
                .cells()
                .iter()
                .all(|cell| cell.glyph() == first_glyph)
        );
        match sample {
            0 => black_output_slot = Some(first_glyph),
            255 => white_output_slot = Some(first_glyph),
            _ => {}
        }

        let patches = first
            .patches()
            .expect("quality sampling retains its patches");
        assert_eq!(patches.cell_count(), 4);
        for cell_index in 0..patches.cell_count() {
            let descriptor = patches
                .descriptor(cell_index)
                .expect("each output cell has a descriptor");
            assert!(descriptor.mean_luminance().is_finite());
            assert_eq!(descriptor.mean_absolute_dx(), 0.0);
            assert_eq!(descriptor.mean_absolute_dy(), 0.0);
            let luminance = patches
                .luminance_for_cell(cell_index)
                .expect("each output cell has patch samples");
            assert!(luminance.iter().all(|sample| sample.is_finite()));

            let endpoint_luminance = match sample {
                0 => Some(0.0),
                255 => Some(1.0),
                _ => None,
            };
            if let Some(expected) = endpoint_luminance {
                assert_eq!(descriptor.mean_luminance(), expected);
                assert!(luminance.iter().all(|sample| *sample == expected));
            }
        }
    }

    // This fixture uses white glyph ink over a black background.
    let black_glyph = table
        .glyphs()
        .get(black_output_slot.expect("black image was converted") as usize)
        .expect("black image selected a glyph in the fixture table");
    let white_glyph = table
        .glyphs()
        .get(white_output_slot.expect("white image was converted") as usize)
        .expect("white image selected a glyph in the fixture table");
    assert_eq!(black_glyph.character(), ' ');
    assert_eq!(white_glyph.character(), 'A');
    assert_eq!(black_glyph.mean_coverage(), blank.mean_coverage());
    assert_eq!(white_glyph.mean_coverage(), ink_a.mean_coverage());
    assert!(white_glyph.mean_coverage() > black_glyph.mean_coverage());
}

#[test]
fn ec_008_odd_row_stride_ignores_padding_through_density_conversion() {
    // Three gray pixels plus two poison padding bytes per row; the final row ends at its last pixel.
    let padded_odd_stride = [0, 0, 0, 0xE1, 0x7C, 255, 255, 255];
    let different_padding = [0, 0, 0, 0x7C, 0xE1, 255, 255, 255];
    let normalized = normalize_raster(
        gray_raster(&padded_odd_stride, 3, 2, 5),
        &conversion_budget(),
        &CancellationToken::new(),
    )
    .expect("odd stride with the exact required extent is valid");
    let normalized_with_different_padding = normalize_raster(
        gray_raster(&different_padding, 3, 2, 5),
        &conversion_budget(),
        &CancellationToken::new(),
    )
    .expect("changing padding bytes does not change the active raster");

    assert_eq!(normalized, normalized_with_different_padding);
    assert_eq!((normalized.width(), normalized.height()), (3, 2));
    assert_eq!(normalized.pixels().len(), 6);
    assert!(
        normalized.pixels()[..3]
            .iter()
            .all(|pixel| pixel.straight_rgb() == LinearRgb::BLACK)
    );
    assert!(
        normalized.pixels()[3..]
            .iter()
            .all(|pixel| pixel.straight_rgb() == LinearRgb::WHITE)
    );

    let table = glyph_table();
    let cancellation = CancellationToken::new();
    let settings = conversion_settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(3 * CELL_WIDTH, 2 * CELL_HEIGHT),
    )
    .with_sampling(SamplingOptions::new(
        FitMode::CustomCrop(
            CropRect::new(0.0, 0.0, 1.0, 1.0).expect("full-frame fixture crop is valid"),
        ),
        SamplingFilter::Nearest,
        None,
    ));
    let converted = convert_density_detailed(
        gray_raster(&padded_odd_stride, 3, 2, 5),
        &table,
        &settings,
        &conversion_budget(),
    )
    .expect("the padded raster converts");
    let converted_with_different_padding = convert_density_detailed(
        gray_raster(&different_padding, 3, 2, 5),
        &table,
        &settings,
        &conversion_budget(),
    )
    .expect("conversion ignores padding contents");
    assert_eq!(
        converted.frame().cells(),
        converted_with_different_padding.frame().cells()
    );
    assert_eq!(converted.frame().cells().len(), 6);
    assert_eq!(
        (
            converted.frame().size().columns(),
            converted.frame().size().rows()
        ),
        (3, 2)
    );
    for (cell, expected_character) in converted
        .frame()
        .cells()
        .iter()
        .zip([' ', ' ', ' ', 'A', 'A', 'A'])
    {
        let glyph = table
            .glyphs()
            .get(cell.glyph() as usize)
            .expect("converted glyph index is in the measured table");
        assert_eq!(glyph.character(), expected_character);
    }
}

#[test]
fn density_conversion_rejects_short_raster_backing_with_a_typed_error() {
    let table = glyph_table();
    let cancellation = CancellationToken::new();
    let settings = conversion_settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(CELL_WIDTH, CELL_HEIGHT),
    );
    let truncated_pixels = [10, 20, 30];
    let input = gray_raster(&truncated_pixels, 2, 2, 2);

    assert_eq!(
        convert_density_detailed(input, &table, &settings, &conversion_budget()),
        Err(DensityConversionError::ColorNormalization(
            ColorNormalizationError::Raster(Error::Malformed(
                MalformedReason::RasterBackingTooShort
            ))
        ))
    );
}

#[test]
fn normalizer_rejects_raster_budgets_before_backing_validation() {
    // A decoder is not part of this crate; this synthetic view probes the actual raster preflight
    // boundary with enormous dimensions and no backing allocation.
    let input = gray_raster(&[], 4096, 4096, 4096);
    assert_eq!(
        normalize_raster(
            input,
            &Budget::new(u64::MAX, 1024, u64::MAX),
            &CancellationToken::new(),
        ),
        Err(ColorNormalizationError::Raster(Error::BudgetExceeded(
            BudgetKind::RasterBytes
        )))
    );
    assert_eq!(
        normalize_raster(
            input,
            &Budget::new(u64::MAX, 32 * 1024 * 1024, 4096),
            &CancellationToken::new(),
        ),
        Err(ColorNormalizationError::Raster(Error::BudgetExceeded(
            BudgetKind::WorkUnits
        )))
    );
}
