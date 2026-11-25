use std::io::Cursor;

use ascii_core::{Budget, CancellationToken, Error, UnsupportedReason};
use ascii_media::{DecodeLimits, DecodeResources, decode_image, image_capabilities};

fn limits() -> DecodeLimits {
    DecodeLimits::new(
        Budget::new(1 << 20, 1 << 20, 1 << 20),
        DecodeResources::new(1, 1 << 20),
        16,
        16,
        1 << 20,
        1 << 20,
    )
}

#[test]
fn webp_signature_is_rejected_by_the_static_decoder() {
    let mut bytes = Cursor::new(b"RIFF\0\0\0\0WEBP");
    let error = decode_image(&mut bytes, &limits(), &CancellationToken::new())
        .expect_err("WebP must not be exposed as a decoded still frame");

    assert_eq!(
        error,
        Error::Unsupported(UnsupportedReason::UnsupportedFormat)
    );
}

#[test]
fn reports_only_variants_implemented_by_this_build() {
    let capabilities = image_capabilities();

    assert!(capabilities.png.still);
    assert!(!capabilities.png.animated);
    assert!(capabilities.jpeg.baseline);
    assert!(capabilities.jpeg.progressive);
    assert!(!capabilities.webp.lossy);
    assert!(!capabilities.webp.lossless);
    assert!(!capabilities.webp.alpha);
    assert!(!capabilities.webp.animated);
}
