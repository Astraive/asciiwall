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
/// The decoder currently has fixed PNG/JPEG backends. `image` 0.25.10's optional WebP feature is
/// disabled here, and its `image-webp` dependency is absent from the lockfile. There is no pinned
/// local fixture encoder for all lossy/lossless/alpha variants or bounded WebP adapter in this
/// crate, so WebP remains disabled despite upstream decoder APIs. The RIFF/WEBP signature is
/// rejected rather than decoded as a static first frame; no animation handler is wired in.
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
