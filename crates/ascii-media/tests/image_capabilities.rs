use std::io::Cursor;

#[cfg(feature = "webp")]
use ascii_core::{AlphaMode, ColorRange, SourceProfile};
use ascii_core::{Budget, CancellationToken, Error, MalformedReason, UnsupportedReason};
use ascii_media::{DecodeLimits, DecodeResources, decode_image, image_capabilities};
#[cfg(feature = "webp")]
use sha2::{Digest, Sha256};

#[cfg(feature = "webp")]
const WEBP_LOSSY_RGB_HEX: &str = concat!(
    "5249464652000000574542505650382046000000b001009d012a030002000200",
    "3425a80274010efe02ec00fede3fbbb403cf6a53533d23944ac7ff26dfa6afc9",
    "dfc9bffff20cff9abe9abf277f26ffaa976b48591b180b640000",
);
#[cfg(feature = "webp")]
const WEBP_LOSSLESS_RGB_HEX: &str = concat!(
    "524946462e000000574542505650384c210000002f024000001008105a943ab7",
    "2110200488f23f452040c892ef4cff8319e0bf520100",
);
#[cfg(feature = "webp")]
const WEBP_LOSSY_RGBA_HEX: &str = concat!(
    "524946467400000057454250565038580a00000010000000020000010000414c",
    "50480700000000004080c0ff64005650382046000000d001009d012a03000200",
    "02003425a80274010efe038e0000fede3fbbb403cf6a53533d23944ac7ff43cc",
    "a67e7ffc9bffff380fff80a6c7cfff937fe2bdeeb7e4e6f474abdc00",
);
#[cfg(feature = "webp")]
const WEBP_LOSSLESS_RGBA_HEX: &str = concat!(
    "5249464638000000574542505650384c2b0000002f024000101008105a943ab7",
    "2110200488f23f452040c892ef0c41004968c10c32da6607c320dc7fd5d50000",
);
#[cfg(feature = "webp")]
const WEBP_ANIMATED_HEX: &str = concat!(
    "524946468200000057454250565038580a00000012000000020000010000414e",
    "494d06000000000000000000414e4d4626000000000000000000020000010000",
    "640000025650384c0e0000002f02400000b98ce87f20e63f2202414e4d462800",
    "0000000000000000020000010000640000025650384c0f0000002f0240001000",
    "08c2ffb689e87f015300",
);
#[cfg(feature = "webp")]
const WEBP_ORIENTATION_HEX: &str = concat!(
    "524946466200000057454250565038580a000000080000000200000100005650",
    "384c210000002f024000001008105a943ab72110200488f23f452040c892ef4c",
    "ff8319e0bf520100455849461a0000004d4d002a000000080001011200030000",
    "00010006000000000000",
);

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

#[cfg(feature = "webp")]
fn bytes_from_hex(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[cfg(feature = "webp")]
fn checked_fixture(name: &str, hex: &str, expected_sha256: &str) -> Vec<u8> {
    let bytes = bytes_from_hex(hex);
    let digest = format!("{:x}", Sha256::digest(&bytes));
    println!("WebP fixture {name} sha256={digest}");
    assert_eq!(digest, expected_sha256, "{name} fixture bytes changed");
    bytes
}

fn decode(bytes: &[u8]) -> Result<ascii_media::DecodedFrame, Error> {
    decode_image(
        &mut Cursor::new(bytes),
        &limits(),
        &CancellationToken::new(),
    )
}

#[test]
fn malformed_webp_container_is_rejected() {
    let expected = if cfg!(feature = "webp") {
        Error::Malformed(MalformedReason::InvalidEncodedData)
    } else {
        Error::Unsupported(UnsupportedReason::UnsupportedFormat)
    };
    match decode(b"RIFF\0\0\0\0WEBP") {
        Err(error) => assert_eq!(error, expected),
        Ok(_) => panic!("invalid RIFF extent unexpectedly published an image"),
    }
}

#[cfg(feature = "webp")]
#[test]
fn decodes_pinned_lossy_lossless_and_alpha_webp_fixtures() {
    const SETTINGS: &str = "pillow=12.2.0;libwebp=1.6.0;size=3x2;rgb=255,0,0|0,255,0|0,0,255|255,255,255|64,128,192|12,34,56;rgba=255,0,0,0|0,255,0,64|0,0,255,128|255,255,255,192|64,128,192,255|12,34,56,100;lossy=quality75,method6;lossless=quality100,method6;lossy_alpha=exact;animation=2frames,100ms,loop0";
    println!(
        "WebP fixture settings_sha256={:x} settings={SETTINGS}",
        Sha256::digest(SETTINGS.as_bytes())
    );
    let lossy_rgb = checked_fixture(
        "lossy_rgb",
        WEBP_LOSSY_RGB_HEX,
        "6572694ee1d61d75c576ac2a06814aad5d22bc764dbad28dbf80cf9d4c52a14e",
    );
    let lossless_rgb = checked_fixture(
        "lossless_rgb",
        WEBP_LOSSLESS_RGB_HEX,
        "c5328c391d6e687347f7700507725d74544f36dadadf4042dcb2bb2d2133bda2",
    );
    let lossy_rgba = checked_fixture(
        "lossy_rgba",
        WEBP_LOSSY_RGBA_HEX,
        "531d0a9bd04a054844397ec748604127c91e45545a8f32aff13331a9591e75e3",
    );
    let lossless_rgba = checked_fixture(
        "lossless_rgba",
        WEBP_LOSSLESS_RGBA_HEX,
        "7e0d46bb4497d2c65804e12947b5fb2ac0d621108979fccd14bc8ce91d4e52c9",
    );

    let lossy_rgb = decode(&lossy_rgb).unwrap();
    let lossless_rgb = decode(&lossless_rgb).unwrap();
    let lossy_rgba = decode(&lossy_rgba).unwrap();
    let lossless_rgba = decode(&lossless_rgba).unwrap();
    for frame in [&lossy_rgb, &lossless_rgb, &lossy_rgba, &lossless_rgba] {
        assert_eq!((frame.width(), frame.height()), (3, 2));
        assert_eq!(frame.color().source_profile(), SourceProfile::AssumedSrgb);
        assert_eq!(frame.color().range(), ColorRange::Unspecified);
    }
    assert_eq!(lossless_rgb.color().alpha_mode(), AlphaMode::Opaque);
    assert_eq!(
        lossless_rgb.pixels(),
        &[
            255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 64, 128, 192, 12, 34, 56,
        ]
    );
    assert_eq!(lossy_rgb.pixels().len(), 3 * 2 * 3);
    assert!(lossy_rgb.pixels().iter().any(|sample| *sample != 0));

    for frame in [&lossy_rgba, &lossless_rgba] {
        assert_eq!(frame.pixels().len() % 4, 0);
        assert_eq!(
            frame
                .pixels()
                .chunks(4)
                .map(|pixel| pixel[3])
                .collect::<Vec<_>>(),
            [0, 64, 128, 192, 255, 100]
        );
    }
    assert_eq!(
        lossless_rgba.pixels(),
        &[
            255, 0, 0, 0, 0, 255, 0, 64, 0, 0, 255, 128, 255, 255, 255, 192, 64, 128, 192, 255, 12,
            34, 56, 100,
        ]
    );
}

#[cfg(feature = "webp")]
#[test]
fn webp_exif_orientation_is_applied_once() {
    const SETTINGS: &str = "pillow=12.2.0;libwebp=1.6.0;webp=lossless-rgb-3x2;exif=TIFF-MM;IFD0.Orientation=SHORT,count1,value6";
    println!(
        "WebP orientation settings_sha256={:x} settings={SETTINGS}",
        Sha256::digest(SETTINGS.as_bytes())
    );
    let bytes = checked_fixture(
        "orientation_6",
        WEBP_ORIENTATION_HEX,
        "9de5e8fc73091d2ade011874871beb6c6a37110faf7e839269e3a0a8d83eb6e5",
    );
    let frame = decode(&bytes).unwrap();
    assert_eq!((frame.width(), frame.height()), (2, 3));
    assert_eq!(
        frame.pixels(),
        &[
            255, 255, 255, 255, 0, 0, 64, 128, 192, 0, 255, 0, 12, 34, 56, 0, 0, 255,
        ]
    );
}

#[cfg(feature = "webp")]
#[test]
fn animated_webp_is_identified_without_publishing_a_static_frame() {
    let bytes = checked_fixture(
        "animated_two_frame",
        WEBP_ANIMATED_HEX,
        "441f56c14aa8f83a84bf7f57c6624834b22de219cc6f18b363c8590e24de5280",
    );
    assert!(matches!(
        decode(&bytes),
        Err(Error::Unsupported(UnsupportedReason::AnimatedImage))
    ));
}

#[test]
fn reports_only_variants_implemented_by_this_build() {
    let capabilities = image_capabilities();

    assert!(capabilities.png.still);
    assert!(!capabilities.png.animated);
    assert!(capabilities.jpeg.baseline);
    assert!(capabilities.jpeg.progressive);
    assert_eq!(capabilities.webp.lossy, cfg!(feature = "webp"));
    assert_eq!(capabilities.webp.lossless, cfg!(feature = "webp"));
    assert_eq!(capabilities.webp.alpha, cfg!(feature = "webp"));
    assert!(!capabilities.webp.animated);
}
