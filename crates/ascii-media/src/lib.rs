//! Bounded media import for validated ASCII clips and PNG/JPEG raster frames.
//!
//! Clip parsing validates the full uncompressed v1 container into complete owned frames. Image
//! input is copied once into a size-bounded snapshot so codec metadata inspection and decoding
//! cannot observe different source bytes. PNGs are checked through IEND and every chunk CRC;
//! JPEG uses the pure-Rust strict `zune-jpeg` backend, avoiding native callback/panic paths.
//! Codec crate limits are defense in depth; decoder scratch, input, work, and retained output
//! are also preflighted and charged to local and shared resource ledgers.

mod clip;
mod clip_reader;
mod decoded_frame;
mod image;
mod resources;
pub use clip::{ClipGlyphTable, ClipGlyphTableError, MAX_CLIP_GLYPH_COUNT, parse_clip_glyph_table};
pub use clip_reader::{
    AsciiClip, ClipCell, ClipFrame, ClipFrameError, ClipReadError, parse_ascii_clip,
};
pub use decoded_frame::DecodedFrame;
pub use image::{DecodeLimits, ReadSeek, decode_image};
pub use resources::DecodeResources;
