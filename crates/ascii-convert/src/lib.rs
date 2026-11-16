//! Bounded CPU density conversion, glyph-color fitting, explicit palettes, and color normalization.

mod color;
mod color_fit;
mod density;
mod font;
mod font_preflight;
mod glyphs;
mod palette;

mod sampling;
pub use color::{
    Alpha, ColorNormalizationError, ColorNormalizationWarning, EncodedOutputColor, EncodedSrgb,
    InvalidColorSample, LinearPremultipliedRgba, LinearRgb, Luminance, NormalizedRaster,
    OutputEncoding, SRGB_DECODE_BREAKPOINT, SRGB_ENCODE_BREAKPOINT,
    SRGB_TRANSFER_CONTINUITY_TOLERANCE, UnsupportedColorReason, decode_srgb, encode_output,
    encode_srgb, normalize_raster,
};
pub use color_fit::{
    COLOR_FIT_ALGORITHM_VERSION, ColorFit, ColorFitError, ColorFitStrategy, MAX_COLOR_FIT_SAMPLES,
    MAX_COLOR_FIT_WORK_UNITS, estimate_color_fit_work, fit_color_patch,
};
pub use density::{
    ConversionSettings, DensityConversion, DensityConversionError, GridSizing, convert_density,
    convert_density_detailed,
};
pub use font::{
    GlyphConfig, GlyphRecord, GlyphRepertoire, GlyphTable, build_glyph_table,
    is_supported_single_cell_scalar,
};
pub use palette::{
    GradientStop, MAX_GRADIENT_STOPS, MAX_PALETTE_COLORS, MAX_PALETTE_PAIR_RESOLUTION_WORK_UNITS,
    MAX_PALETTE_RESOLUTION_WORK_UNITS, PALETTE_ALGORITHM_VERSION, Palette, PaletteColor,
    PaletteColorPair, PaletteDefinition, PaletteDistanceSpace, PaletteError, PaletteKind,
};

pub use glyphs::{Centroid, EdgeOrientation, GlyphBounds, GlyphMetrics};
pub use sampling::{
    CropRect, FitMode, FitTransform, PatchDescriptors, PatchDimensions, QualityPatches,
    SamplingFilter, SamplingIdentity, SamplingOptions, SamplingRect,
};
