// The upstream fixture is generated from source constants and is used only by tests.
#[path = "../../../crates/ascii-convert/tests/support/synthetic_font.rs"]
mod synthetic_font;

use std::num::NonZeroU32;

use ascii_convert::{
    ConversionSettings, DensityConversionError, GlyphConfig, GridSizing, LinearRgb,
    build_glyph_table, convert_density_detailed,
};
use ascii_core::{
    AlphaMode, Budget, CancellationToken, ColorDescription, Error, FrameMetadata, GlyphTableId,
    PixelFormat, RasterFrameView, RasterOrientation,
};
use asciiwall_cli::convert::monochrome_density;

#[test]
fn cli_density_stage_is_the_shared_engine_output() {
    let budget = Budget::new(8_000_000, 64_000_000, 16_000_000);
    let cancellation = CancellationToken::new();
    let table = build_glyph_table(
        &synthetic_font::synthetic_font(false),
        " AB",
        &GlyphConfig::new(8.0, 8, 12, 9).unwrap(),
        &budget,
        &cancellation,
    ).unwrap();
    let pixels = [0_u8, 255, 128, 64, 220, 180];
    let raster = RasterFrameView::new(
        2, 3, 2, PixelFormat::gray8(), ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown, &pixels,
    );
    let actual = monochrome_density(raster, &table, NonZeroU32::new(2).unwrap(), &budget, &cancellation)
        .unwrap();
    let metadata = FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 0, 0, 0, 0);
    let settings = ConversionSettings::new(
        GridSizing::columns(2), metadata, &cancellation, LinearRgb::WHITE, LinearRgb::BLACK,
    ).unwrap();
    let expected = convert_density_detailed(raster, &table, &settings, &budget).unwrap();
    assert_eq!(actual, expected);

    cancellation.cancel();
    let error = monochrome_density(raster, &table, NonZeroU32::new(2).unwrap(), &budget, &cancellation)
        .unwrap_err();
    assert_eq!(error, DensityConversionError::Core(Error::Cancelled));
}
