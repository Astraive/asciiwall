//! Content-sniffed PNG/JPEG still and bounded APNG/GIF animation decoding into owned rasters.
//!
//! Input is copied once into a size-bounded snapshot so codec metadata inspection and decoding
//! cannot observe different source bytes. PNGs are checked through IEND and every chunk CRC;
//! JPEG uses strict pure-Rust `zune-jpeg`. APNG and GIF subframes are composed into budgeted
//! full-canvas frames on a shared normalized timeline. WebP is restricted to safe RIFF/animation
//! classification and is not pixel-decoded.

mod animation;
mod capabilities;
mod decoded_frame;
mod image;
mod resources;

pub use animation::{
    Animation, AnimationFrame, AnimationLoops, MAX_ANIMATION_DURATION, MAX_ANIMATION_FRAMES,
    MIN_FRAME_DURATION,
};
pub use capabilities::{
    GifCapabilities, ImageCapabilities, JpegCapabilities, PngCapabilities, WebpCapabilities,
    image_capabilities,
};
pub use decoded_frame::DecodedFrame;
pub use image::{DecodeLimits, ReadSeek, decode_animation, decode_image};
pub use resources::DecodeResources;
