//! Bounded font rasterization and immutable glyph-table primitives.

mod font;
mod font_preflight;
mod glyphs;

pub use font::{
    GlyphConfig, GlyphRecord, GlyphRepertoire, GlyphTable, build_glyph_table,
    is_supported_single_cell_scalar,
};
