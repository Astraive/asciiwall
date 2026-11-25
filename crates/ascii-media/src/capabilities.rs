//! Build-accurate capabilities for the statically linked image decoder.

/// Image formats and variants reported by this decoder build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageCapabilities {
    /// PNG variants accepted by the still and animation decoders.
    pub png: PngCapabilities,
    /// JPEG variants accepted by the static-image decoder.
    pub jpeg: JpegCapabilities,
    /// GIF variants accepted by this build.
    pub gif: GifCapabilities,
    /// WebP variants accepted by this build.
    pub webp: WebpCapabilities,
}

/// Supported PNG decode variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PngCapabilities {
    /// Non-animated PNG, including supported indexed, grayscale, RGB/RGBA, and Adam7 inputs.
    pub still: bool,
    /// Whether the supported APNG subset is decoded and composed.
    pub animated: bool,
    /// APNG frames with 8-bit source samples.
    pub apng_8bit: bool,
    /// APNG frames with 16-bit source samples.
    pub apng_16bit: bool,
    /// APNG whose default PNG image is separate from its animation frames.
    pub apng_separate_default_image: bool,
}

/// Supported JPEG decode variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JpegCapabilities {
    /// Baseline sequential JPEG.
    pub baseline: bool,
    /// Progressive JPEG.
    pub progressive: bool,
}

/// Supported GIF decode variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GifCapabilities {
    /// Whether a static GIF image can be decoded.
    pub still: bool,
    /// Whether animated GIF frames are composed.
    pub animated: bool,
}

/// Supported WebP decode variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebpCapabilities {
    /// Lossy still WebP.
    pub lossy: bool,
    /// Lossless still WebP.
    pub lossless: bool,
    /// Still WebP with alpha.
    pub alpha: bool,
    /// Animated WebP frame composition.
    pub animated: bool,
}

/// Returns the capabilities of this exact `ascii-media` build.
///
/// APNG supports the documented 8-bit participating-default subset, and GIF frames are decoded
/// with bounded full-canvas composition. 16-bit/separate-default APNG and animated WebP remain
/// unavailable. WebP is recognized only for safe format/animation classification; all pixel
/// decoding remains disabled because pinned `image-webp` lossless Huffman groups exceed the memory
/// ledger.
#[must_use]
pub const fn image_capabilities() -> ImageCapabilities {
    ImageCapabilities {
        png: PngCapabilities {
            still: true,
            animated: true,
            apng_8bit: true,
            apng_16bit: false,
            apng_separate_default_image: false,
        },
        jpeg: JpegCapabilities {
            baseline: true,
            progressive: true,
        },
        gif: GifCapabilities {
            still: false,
            animated: true,
        },
        webp: WebpCapabilities {
            lossy: false,
            lossless: false,
            alpha: false,
            animated: false,
        },
    }
}
