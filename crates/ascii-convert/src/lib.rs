//! Bounded conversion primitives for fonts, glyph descriptors, and color normalization.

mod color;

mod font;
mod font_preflight;
mod glyphs;

pub use color::{
    Alpha, ColorNormalizationError, ColorNormalizationWarning, EncodedOutputColor, EncodedSrgb,
    InvalidColorSample, LinearPremultipliedRgba, LinearRgb, Luminance, NormalizedRaster,
    OutputEncoding, SRGB_DECODE_BREAKPOINT, SRGB_ENCODE_BREAKPOINT,
    SRGB_TRANSFER_CONTINUITY_TOLERANCE, UnsupportedColorReason, decode_srgb, encode_output,
    encode_srgb, normalize_raster,
};

pub use font::{GlyphConfig, GlyphRecord, GlyphTable, build_glyph_table};
pub use glyphs::{Centroid, EdgeOrientation, GlyphBounds, GlyphMetrics};
