//! Bounded font rasterization and immutable glyph-table primitives.

mod font;
mod font_preflight;
mod glyphs;

pub use font::{GlyphConfig, GlyphRecord, GlyphTable, build_glyph_table};
pub use glyphs::{Centroid, EdgeOrientation, GlyphBounds, GlyphMetrics};
