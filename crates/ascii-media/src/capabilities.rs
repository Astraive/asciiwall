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
/// `webp` enables the direct `image-webp` still decoder; this build's fixtures cover lossy,
/// lossless, and alpha inputs. Animated WebP is classified as `Unsupported(AnimatedImage)` before
/// static pixel decoding, but no animation/timeline handler is available here.
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
            lossy: cfg!(feature = "webp"),
            lossless: cfg!(feature = "webp"),
            alpha: cfg!(feature = "webp"),
            animated: false,
        },
    }
}
