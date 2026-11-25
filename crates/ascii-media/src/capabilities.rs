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
/// APNG composition supports 8-bit samples when the default PNG image is the first animation
/// frame. It composes all APNG blend and disposal operations. 16-bit APNG, GIF, and animated WebP
/// remain unavailable. WebP decoding is disabled until the pinned decoder's allocation behavior
/// can be bounded by the resource ledger.
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
        webp: WebpCapabilities {
            lossy: false,
            lossless: false,
            alpha: false,
            animated: false,
        },
    }
}
