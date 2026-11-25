//! Build-accurate capabilities for the statically linked image decoder.

/// Image formats implemented by this decoder build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageCapabilities {
    /// PNG variants accepted by the static-image decoder.
    pub png: PngCapabilities,
    /// JPEG variants accepted by the static-image decoder.
    pub jpeg: JpegCapabilities,
    /// WebP variants accepted by the static-image decoder.
    pub webp: WebpCapabilities,
}

/// Supported PNG decode variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PngCapabilities {
    /// Non-animated PNG, including supported indexed, grayscale, RGB/RGBA, and Adam7 inputs.
    pub still: bool,
    /// Whether APNG frame animation is decoded and composed.
    pub animated: bool,
}

/// Supported JPEG decode variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JpegCapabilities {
    /// Baseline sequential JPEG.
    pub baseline: bool,
    /// Progressive JPEG.
    pub progressive: bool,
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
/// WebP is recognized for safe format/animation classification, but all pixel-decoding variants
/// are disabled because the pinned decoder has unbounded lossless Huffman-group allocations. No
/// animation timeline handler is available here.
#[must_use]
pub const fn image_capabilities() -> ImageCapabilities {
    ImageCapabilities {
        png: PngCapabilities {
            still: true,
            animated: false,
        },
        jpeg: JpegCapabilities {
            baseline: true,
            progressive: true,
        },
        webp: WebpCapabilities {
            lossy: false,
            lossless: false,
            alpha: false,
            animated: false,
        },
    }
}
