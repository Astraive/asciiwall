//! Content-sniffed PNG/JPEG decoding into bounded, validated owned raster frames.
//!
//! Input is copied once into a size-bounded snapshot so codec metadata inspection and decoding
//! cannot observe different source bytes. PNGs are checked through IEND and every chunk CRC;
//! JPEG uses strict pure-Rust `zune-jpeg`. WebP is limited to bounded RIFF/animation classification
//! and is never pixel-decoded until an allocation-safe codec is available.

mod capabilities;
mod decoded_frame;
mod image;
mod resources;

pub use capabilities::{
    ImageCapabilities, JpegCapabilities, PngCapabilities, WebpCapabilities, image_capabilities,
};
pub use decoded_frame::DecodedFrame;
pub use image::{DecodeLimits, ReadSeek, decode_image};
pub use resources::DecodeResources;
