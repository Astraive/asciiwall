//! Bounded CPU density conversion, font rasterization, immutable glyph-table primitives, and
//! shared geometry with typed color normalization and alpha compositing.

mod color;
mod density;
mod font;
mod font_preflight;
mod glyphs;

mod sampling;
pub use color::{
    Alpha, ColorNormalizationError, ColorNormalizationWarning, EncodedOutputColor, EncodedSrgb,
    InvalidColorSample, LinearPremultipliedRgba, LinearRgb, Luminance, NormalizedRaster,
    OutputEncoding, SRGB_DECODE_BREAKPOINT, SRGB_ENCODE_BREAKPOINT,
    SRGB_TRANSFER_CONTINUITY_TOLERANCE, UnsupportedColorReason, decode_srgb, encode_output,
    encode_srgb, normalize_raster,
};
pub use density::{
    ConversionSettings, DensityConversion, DensityConversionError, GridSizing, convert_density,
    convert_density_detailed,
};
pub use font::{
    GlyphConfig, GlyphPreview, GlyphPreviewStatus, GlyphRecord, GlyphRepertoire,
    GlyphRepertoirePreset, GlyphTable, build_glyph_preview, build_glyph_table,
    is_supported_single_cell_scalar,
};

pub use glyphs::{Centroid, EdgeOrientation, GlyphBounds, GlyphCellClassification, GlyphMetrics};
pub use sampling::{
    CropRect, FitMode, FitTransform, PatchDescriptors, PatchDimensions, QualityPatches,
    SamplingFilter, SamplingIdentity, SamplingOptions, SamplingRect,
};
