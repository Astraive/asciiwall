//! Content-sniffed PNG/JPEG and bounded APNG animation decoding into owned normalized rasters.
//!
//! Input is copied once into a size-bounded snapshot so codec metadata inspection and decoding
//! cannot observe different source bytes. PNGs are checked through IEND and every chunk CRC;
//! JPEG uses strict pure-Rust `zune-jpeg`. APNG subframes are composed into budgeted full-canvas
//! frames with a shared normalized timeline. WebP and GIF animation are unavailable until their
//! decoders have bounded allocation and cancellation behavior.

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
    ImageCapabilities, JpegCapabilities, PngCapabilities, WebpCapabilities, image_capabilities,
};
pub use decoded_frame::DecodedFrame;
pub use image::{DecodeLimits, ReadSeek, decode_animation, decode_image};
pub use resources::DecodeResources;
