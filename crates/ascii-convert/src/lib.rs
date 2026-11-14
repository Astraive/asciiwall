//! Bounded CPU density conversion, font masks, shared geometry, and color normalization.

mod color;
mod density;

mod font;
mod font_preflight;
mod glyphs;

mod sampling;
pub use density::{
    ConversionSettings, DensityConversion, DensityConversionError, GridSizing, convert_density,
    convert_density_detailed,
};
pub use color::{
    Alpha, ColorNormalizationError, ColorNormalizationWarning, EncodedOutputColor, EncodedSrgb,
    InvalidColorSample, LinearPremultipliedRgba, LinearRgb, Luminance, NormalizedRaster,
    OutputEncoding, SRGB_DECODE_BREAKPOINT, SRGB_ENCODE_BREAKPOINT,
    SRGB_TRANSFER_CONTINUITY_TOLERANCE, UnsupportedColorReason, decode_srgb, encode_output,
    encode_srgb, normalize_raster,
};

pub use font::{GlyphConfig, GlyphRecord, GlyphTable, build_glyph_table};
pub use glyphs::{Centroid, EdgeOrientation, GlyphBounds, GlyphMetrics};
pub use sampling::{
    CropRect, FitMode, FitTransform, PatchDescriptors, PatchDimensions, QualityPatches,
    SamplingFilter, SamplingIdentity, SamplingOptions, SamplingRect,
};
