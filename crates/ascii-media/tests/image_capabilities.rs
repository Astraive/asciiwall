use std::io::Cursor;

use ascii_core::{Budget, CancellationToken, Error, MalformedReason, UnsupportedReason};
use ascii_media::{DecodeLimits, DecodeResources, decode_image, image_capabilities};
use sha2::{Digest, Sha256};

const WEBP_ANIMATED_HEX: &str = concat!(
    "524946468200000057454250565038580a00000012000000020000010000414e",
    "494d06000000000000000000414e4d4626000000000000000000020000010000",
    "640000025650384c0e0000002f02400000b98ce87f20e63f2202414e4d462800",
    "0000000000000000020000010000640000025650384c0f0000002f0240001000",
    "08c2ffb689e87f015300",
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

fn bytes_from_hex(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

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

fn webp_with_chunks(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
    for (fourcc, data) in chunks {
        bytes.extend_from_slice(*fourcc);
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data);
        if data.len() & 1 != 0 {
            bytes.push(0);
        }
    }
    let riff_size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
    bytes
}

fn assert_malformed_webp_chunks(chunks: &[(&[u8; 4], &[u8])]) {
    assert!(matches!(
        decode(&webp_with_chunks(chunks)),
        Err(Error::Malformed(MalformedReason::InvalidEncodedData))
    ));
}

#[test]
fn empty_animation_chunk_is_malformed() {
    let vp8x = *b"\x02\0\0\0\0\0\0\0\0\0";
    assert_malformed_webp_chunks(&[(b"VP8X", &vp8x), (b"ANIM", &[])]);
}

#[test]
fn short_animation_chunk_is_malformed() {
    let vp8x = *b"\x02\0\0\0\0\0\0\0\0\0";
    assert_malformed_webp_chunks(&[(b"VP8X", &vp8x), (b"ANIM", &[0; 5])]);
}

#[test]
fn animation_frame_before_animation_control_is_malformed() {
    let vp8x = *b"\x02\0\0\0\0\0\0\0\0\0";
    assert_malformed_webp_chunks(&[(b"VP8X", &vp8x), (b"ANMF", &[0; 24]), (b"ANIM", &[0; 6])]);
}

#[test]
fn short_animation_frame_chunk_is_malformed() {
    let vp8x = *b"\x02\0\0\0\0\0\0\0\0\0";
    assert_malformed_webp_chunks(&[(b"VP8X", &vp8x), (b"ANIM", &[0; 6]), (b"ANMF", &[0; 16])]);
}

#[test]
fn malformed_webp_container_is_rejected() {
    assert!(matches!(
        decode(b"RIFF\0\0\0\0WEBP"),
        Err(Error::Malformed(MalformedReason::InvalidEncodedData))
    ));
}

#[test]
fn empty_size_consistent_webp_container_is_malformed() {
    let fixture = b"RIFF\x04\0\0\0WEBP";
    assert!(matches!(
        decode(fixture),
        Err(Error::Malformed(MalformedReason::InvalidEncodedData))
    ));
}

#[test]
fn nonzero_odd_riff_chunk_padding_is_rejected() {
    let fixture = b"RIFF\x0e\0\0\0WEBPJUNK\x01\0\0\0\0\x01";
    assert!(matches!(
        decode(fixture),
        Err(Error::Malformed(MalformedReason::InvalidEncodedData))
    ));
}

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
    assert!(!capabilities.webp.lossy);
    assert!(!capabilities.webp.lossless);
    assert!(!capabilities.webp.alpha);
    assert!(!capabilities.webp.animated);
}
