//! Content-sniffed PNG/JPEG decoding into bounded, validated owned raster frames.
//!
//! Input is copied once into a size-bounded snapshot so codec metadata inspection and decoding
//! cannot observe different source bytes. PNGs are checked through IEND and every chunk CRC;
//! JPEG uses the pure-Rust strict `zune-jpeg` backend, avoiding native callback/panic paths.
//! Codec crate limits are defense in depth; decoder scratch, input, work, and retained output
//! are also preflighted and charged to local and shared resource ledgers.

mod decoded_frame;
mod image;
mod resources;

pub use decoded_frame::DecodedFrame;
pub use image::{DecodeLimits, ReadSeek, decode_image};
pub use resources::DecodeResources;
