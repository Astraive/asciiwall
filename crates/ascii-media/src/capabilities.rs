//! Build-accurate capabilities for the statically linked image decoder.

/// Image formats implemented by this decoder build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageCapabilities {
    /// PNG variants accepted by the static-image decoder.
    pub png: PngCapabilities,
    /// JPEG variants accepted by the static-image decoder.
    pub jpeg: JpegCapabilities,
    /// Whether APNG frame animation is routed to an animation decoder.
    pub apng_animated: bool,
    /// WebP variants accepted by the static-image decoder.
    pub webp: WebpCapabilities,
}

/// Supported PNG decode variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PngCapabilities {
    /// Non-animated PNG, including supported indexed, grayscale, RGB/RGBA, and Adam7 inputs.
    pub still: bool,
    /// Animated PNG frame composition.
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
/// The decoder currently has fixed PNG/JPEG backends. APNG and all WebP variants are disabled;
/// the RIFF/WEBP signature is rejected rather than decoded as a static first frame.
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
        apng_animated: false,
        webp: WebpCapabilities {
            lossy: false,
            lossless: false,
            alpha: false,
            animated: false,
        },
    }
}
