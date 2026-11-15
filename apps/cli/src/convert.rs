//! The available headless density stage. Decoding and export remain separate engine contracts.

use std::num::NonZeroU32;

use ascii_convert::{
    ConversionSettings, DensityConversion, DensityConversionError, GlyphTable, GridSizing,
    LinearRgb, convert_density_detailed,
};
use ascii_core::{Budget, CancellationToken, FrameMetadata, GlyphTableId, RasterFrameView};

/// Converts a decoded raster using an already-measured glyph table and the shared density engine.
///
/// The caller owns decoding, font selection/table construction, output encoding, and publication.
/// This stage is deliberately monochrome: the shared density engine currently stores a fixed
/// foreground and background in every cell, not sampled source colors.
pub fn monochrome_density(
    input: RasterFrameView<'_>,
    glyphs: &GlyphTable,
    columns: NonZeroU32,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<DensityConversion, DensityConversionError> {
    let metadata = FrameMetadata::new(GlyphTableId::new(*glyphs.fingerprint()), 0, 0, 0, 0);
    let settings = ConversionSettings::new(
        GridSizing::columns(columns.get()),
        metadata,
        cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )?;
    convert_density_detailed(input, glyphs, &settings, budget)
}
