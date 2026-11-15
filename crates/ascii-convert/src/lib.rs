//! Bounded font rasterization and immutable glyph-table primitives.

mod font;
mod font_preflight;
mod glyphs;

pub use font::{
    GlyphConfig, GlyphPreview, GlyphPreviewStatus, GlyphRecord, GlyphRepertoire,
    GlyphRepertoirePreset, GlyphTable, build_glyph_preview, build_glyph_table,
    is_supported_single_cell_scalar,
};
pub use glyphs::{Centroid, EdgeOrientation, GlyphBounds, GlyphCellClassification, GlyphMetrics};
