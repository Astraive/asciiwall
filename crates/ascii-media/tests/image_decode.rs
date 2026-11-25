use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::sync::{Arc, Barrier};
use std::thread;

use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorPrimaries, ColorRange, SourceProfile,
    TransferFunction,
};
use ascii_media::{DecodeLimits, DecodeResources, decode_image};
use image::ImageEncoder;
use sha2::{Digest, Sha256};

fn limits() -> DecodeLimits {
    DecodeLimits::new(
        Budget::new(1 << 20, 1 << 20, 1 << 20),
        DecodeResources::new(4, 16 << 20),
        1024,
        1024,
        1 << 20,
        1 << 20,
    )
}

fn png(width: u32, height: u32, color: image::ColorType, samples: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(samples, width, height, color.into())
        .unwrap();
    bytes
}

// Derive a synthetic chunk while retaining the encoder's IHDR and image payload.
// PNG uses the IEEE CRC-32 of the chunk type and payload.
fn png_with_chunk_after_ihdr(png: &[u8], chunk_type: [u8; 4], payload: &[u8]) -> Vec<u8> {
    assert_eq!(&png[12..16], b"IHDR");
    assert_eq!(&png[8..12], &13_u32.to_be_bytes());
    let mut crc = u32::MAX;
    for &byte in chunk_type.iter().chain(payload) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 0 { 0 } else { 0xedb8_8320 };
        }
    }
    let mut tagged = Vec::with_capacity(png.len() + payload.len() + 12);
    tagged.extend_from_slice(&png[..33]);
    tagged.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_be_bytes());
    tagged.extend_from_slice(&chunk_type);
    tagged.extend_from_slice(payload);
    tagged.extend_from_slice(&(!crc).to_be_bytes());
    tagged.extend_from_slice(&png[33..]);
    tagged
}
fn png_gray_depth(depth: png::BitDepth, width: u32, height: u32, samples: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(depth);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(samples)
            .unwrap();
    }
    bytes
}

fn png_with_dimensions(bytes: &[u8], width: u32, height: u32) -> Vec<u8> {
    assert_eq!(&bytes[12..16], b"IHDR");
    let mut patched = bytes.to_vec();
    patched[16..20].copy_from_slice(&width.to_be_bytes());
    patched[20..24].copy_from_slice(&height.to_be_bytes());
    let mut crc = u32::MAX;
    for byte in patched[12..29].iter().copied() {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 0 { 0 } else { 0xedb8_8320 };
        }
    }
    patched[29..33].copy_from_slice(&(!crc).to_be_bytes());
    patched
}

fn jpeg(width: u32, height: u32, samples: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 100)
        .encode(samples, width, height, image::ColorType::Rgb8.into())
        .unwrap();
    bytes
}

// The SOF advertises a large raster while the rights-cleared 2x1 JPEG entropy payload stays tiny.

fn jpeg_with_dimensions(bytes: &[u8], width: u16, height: u16) -> Vec<u8> {
    let mut patched = bytes.to_vec();
    let mut offset = 2;
    while offset < patched.len() {
        assert_eq!(patched[offset], 0xff);
        while offset < patched.len() && patched[offset] == 0xff {
            offset += 1;
        }
        let marker = *patched.get(offset).expect("JPEG marker must exist");
        offset += 1;
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            continue;
        }

        let length_end = offset.checked_add(2).expect("segment offset fits");
        let segment_length = usize::from(u16::from_be_bytes(
            patched
                .get(offset..length_end)
                .expect("JPEG segment length must exist")
                .try_into()
                .expect("JPEG segment length has two bytes"),
        ));
        let segment_end = offset
            .checked_add(segment_length)
            .expect("JPEG segment extent fits");
        assert!(segment_end <= patched.len());
        if marker == 0xc0 {
            let height_start = offset.checked_add(3).expect("SOF offset fits");
            let width_start = offset.checked_add(5).expect("SOF offset fits");
            patched[height_start..height_start + 2].copy_from_slice(&height.to_be_bytes());
            patched[width_start..width_start + 2].copy_from_slice(&width.to_be_bytes());
            return patched;
        }
        offset = segment_end;
    }
    panic!("baseline JPEG must contain a SOF0 marker");
}

fn decode(bytes: &[u8]) -> Result<ascii_media::DecodedFrame, ascii_core::Error> {
    decode_image(
        &mut Cursor::new(bytes),
        &limits(),
        &CancellationToken::new(),
    )
}

#[test]
fn codec_matrix_fixture_configuration_and_hashes() {
    const SETTINGS: &str = "image=0.25.10;png=0.18.0;zune-jpeg=0.5.5;rgb=10,20,30;rgba=11,22,33,44;gray8=77;gray16=0x1234;jpeg_encoder=quality100;jpeg_rgb=255,0,0,0,255,0;color_metadata=untagged-and-sRGB;range=PNG-full-JPEG-unspecified";
    println!(
        "environment os={} arch={} settings_sha256={:x} settings={SETTINGS}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        Sha256::digest(SETTINGS.as_bytes())
    );
    let untagged_png = png(1, 1, image::ColorType::Rgb8, &[10, 20, 30]);
    let srgb_png = png_with_chunk_after_ihdr(&untagged_png, *b"sRGB", &[0]);
    let fixtures = [
        ("png_rgb", untagged_png.clone()),
        ("png_srgb_metadata", srgb_png),
        (
            "png_rgba",
            png(1, 1, image::ColorType::Rgba8, &[11, 22, 33, 44]),
        ),
        ("png_gray", png(1, 1, image::ColorType::L8, &[77])),
        (
            "png_gray16",
            png(1, 1, image::ColorType::L16, &0x1234u16.to_ne_bytes()),
        ),
        ("jpeg_baseline", jpeg(2, 1, &[255, 0, 0, 0, 255, 0])),
    ];
    for (name, bytes) in fixtures {
        println!("fixture={name} sha256={:x}", Sha256::digest(&bytes));
    }
}

#[test]
fn codec_matrix_png_rgb_rgba_grayscale_and_sixteen_bit_normalize() {
    let rgb = decode(&png(1, 1, image::ColorType::Rgb8, &[10, 20, 30])).unwrap();
    assert_eq!((rgb.width(), rgb.height(), rgb.row_stride()), (1, 1, 3));
    assert_eq!(rgb.pixels(), &[10, 20, 30]);

    let rgba = decode(&png(1, 1, image::ColorType::Rgba8, &[11, 22, 33, 44])).unwrap();
    assert_eq!(rgba.pixels(), &[11, 22, 33, 44]);
    assert_eq!(rgba.view().pixels(), rgba.pixels());

    let gray = decode(&png(1, 1, image::ColorType::L8, &[77])).unwrap();
    assert_eq!(gray.pixels(), &[77, 77, 77]);
    let gray16 = decode(&png(1, 1, image::ColorType::L16, &0x1234u16.to_ne_bytes())).unwrap();
    assert_eq!(gray16.pixels(), &[0x12, 0x12, 0x12]);
}

#[test]
fn decoded_color_description_preserves_source_metadata_and_assumptions() {
    let untagged = decode(&png(1, 1, image::ColorType::Rgb8, &[10, 20, 30])).unwrap();
    assert_eq!(untagged.color().primaries(), ColorPrimaries::Srgb);
    assert_eq!(untagged.color().transfer(), TransferFunction::Srgb);
    assert_eq!(
        untagged.color().source_profile(),
        SourceProfile::AssumedSrgb
    );
    assert_eq!(untagged.color().range(), ColorRange::Full);
    assert_eq!(untagged.color().alpha_mode(), AlphaMode::Opaque);

    let encoded = png(1, 1, image::ColorType::Rgb8, &[10, 20, 30]);
    let tagged = png_with_chunk_after_ihdr(&encoded, *b"sRGB", &[0]);
    let tagged = decode(&tagged).unwrap();
    assert_eq!(tagged.color().source_profile(), SourceProfile::Srgb);
    assert_eq!(tagged.color().range(), ColorRange::Full);

    let rgba = decode(&png(1, 1, image::ColorType::Rgba8, &[11, 22, 33, 44])).unwrap();
    assert_eq!(rgba.color().alpha_mode(), AlphaMode::Straight);

    let jpeg = decode(&jpeg(1, 1, &[30, 20, 10])).unwrap();
    assert_eq!(jpeg.color().source_profile(), SourceProfile::AssumedSrgb);
    assert_eq!(jpeg.color().range(), ColorRange::Unspecified);
    assert_eq!(jpeg.color().alpha_mode(), AlphaMode::Opaque);
}

#[test]
fn sixteen_bit_png_normalization_rounds_instead_of_dropping_low_bytes() {
    // Values straddle boundaries where taking the high byte differs from
    // rounding the full 16-bit sample to 8 bits.
    let samples = [0x00ff_u16, 0x7fff, 0xff00]
        .into_iter()
        .flat_map(u16::to_ne_bytes)
        .collect::<Vec<_>>();
    let fixture = png(1, 1, image::ColorType::Rgb16, &samples);
    println!("rounding PNG fixture sha256={:x}", Sha256::digest(&fixture));
    assert_eq!(decode(&fixture).unwrap().pixels(), &[1, 127, 254]);
}

#[test]
fn low_bit_depth_grayscale_expands_to_full_range_rgb() {
    let gray1 = decode(&png_gray_depth(png::BitDepth::One, 2, 1, &[0b0100_0000])).unwrap();
    assert_eq!(gray1.pixels(), &[0, 0, 0, 255, 255, 255]);

    let gray2 = decode(&png_gray_depth(png::BitDepth::Two, 4, 1, &[0b0001_1011])).unwrap();
    assert_eq!(
        gray2.pixels(),
        &[0, 0, 0, 85, 85, 85, 170, 170, 170, 255, 255, 255]
    );

    let gray4 = decode(&png_gray_depth(png::BitDepth::Four, 2, 1, &[0x3f])).unwrap();
    assert_eq!(gray4.pixels(), &[51, 51, 51, 255, 255, 255]);
}

#[test]
fn png_dimensions_must_be_positive_and_checked_arithmetic_must_not_wrap() {
    let valid = png(1, 1, image::ColorType::Rgb8, &[1, 2, 3]);
    let zero_width = png_with_dimensions(&valid, 0, 1);
    assert!(matches!(
        decode(&zero_width),
        Err(ascii_core::Error::Malformed(
            ascii_core::MalformedReason::EmptyDimensions
        ))
    ));

    let rgba16 = png(1, 1, image::ColorType::Rgba16, &[0; 8]);
    let maximum_dimension = 0x7fff_ffff;
    let overflowing = png_with_dimensions(&rgba16, maximum_dimension, maximum_dimension);
    let mut decode_limits = limits();
    decode_limits.max_width = maximum_dimension;
    decode_limits.max_height = maximum_dimension;
    decode_limits.budget = Budget::new(u64::MAX, u64::MAX, u64::MAX);
    assert_eq!(
        decode_image(
            &mut Cursor::new(overflowing),
            &decode_limits,
            &CancellationToken::new()
        )
        .unwrap_err(),
        ascii_core::Error::SizeOverflow
    );
}

#[test]
fn codec_matrix_baseline_jpeg_and_signature_not_suffix() {
    let bytes = jpeg(2, 1, &[255, 0, 0, 0, 255, 0]);
    let mut input = Cursor::new(bytes);
    let frame = decode_image(&mut input, &limits(), &CancellationToken::new()).unwrap();
    assert_eq!((frame.width(), frame.height()), (2, 1));
    assert_eq!(frame.pixels().len(), 6);
}

#[test]
fn codec_matrix_png_orientation_checks_oriented_dimensions() {
    // Synthetic 3x2 grayscale image, TIFF IFD0 orientation 6 (rotate clockwise).
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    encoder
        .set_exif_metadata(vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0,
            0,
        ])
        .unwrap();
    encoder
        .write_image(&[10, 20, 30, 40, 50, 60], 3, 2, image::ColorType::L8.into())
        .unwrap();
    let mut decode_limits = limits();
    decode_limits.max_width = 2;
    decode_limits.max_height = 3;
    let frame = decode_image(
        &mut Cursor::new(&bytes),
        &decode_limits,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(
        (frame.width(), frame.height(), frame.row_stride()),
        (2, 3, 6)
    );
    assert_eq!(
        frame.pixels(),
        &[
            40, 40, 40, 10, 10, 10, 50, 50, 50, 20, 20, 20, 60, 60, 60, 30, 30, 30
        ]
    );
}

#[test]
fn current_position_and_encoded_extent_are_bounded() {
    let bytes = png(1, 1, image::ColorType::Rgb8, &[1, 2, 3]);
    let mut stream = Cursor::new([b"prefix".as_slice(), bytes.as_slice()].concat());
    stream.seek(SeekFrom::Start(6)).unwrap();
    let frame = decode_image(&mut stream, &limits(), &CancellationToken::new()).unwrap();
    assert_eq!(frame.pixels(), &[1, 2, 3]);

    let with_trailing_bytes = [bytes.as_slice(), b"trailing"].concat();
    assert!(matches!(
        decode(&with_trailing_bytes),
        Err(ascii_core::Error::Malformed(_))
    ));

    let mut bounded = limits();
    bounded.max_encoded_bytes = 8;
    let error =
        decode_image(&mut Cursor::new(bytes), &bounded, &CancellationToken::new()).unwrap_err();
    assert!(matches!(error, ascii_core::Error::BudgetExceeded(_)));
}

#[test]
fn png_large_ancillary_and_compressed_payloads_are_not_double_counted_as_scratch() {
    let resources = DecodeResources::new(1, 4 << 20);
    let limits_with_scratch_budget = |max_decoder_bytes| {
        DecodeLimits::new(
            Budget::new(4 << 20, 4 << 20, 16 << 20),
            resources.clone(),
            1024,
            1024,
            2 << 20,
            max_decoder_bytes,
        )
    };
    let ancillary_limits = limits_with_scratch_budget(768 << 10);
    let compressed_limits = limits_with_scratch_budget(256 << 10);
    let tiny = png(1, 1, image::ColorType::Rgb8, &[10, 20, 30]);
    let mut text_payload = vec![b'x'; 512 << 10];
    text_payload[..8].copy_from_slice(b"comment\0");
    let with_large_ancillary = png_with_chunk_after_ihdr(&tiny, *b"tEXt", &text_payload);
    assert!(with_large_ancillary.len() > 256 << 10);
    assert!(with_large_ancillary.len() < 2 << 20);

    let ancillary_frame = decode_image(
        &mut Cursor::new(&with_large_ancillary),
        &ancillary_limits,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(ancillary_frame.pixels(), &[10, 20, 30]);
    drop(ancillary_frame);
    assert_eq!(resources.cpu_bytes_in_use(), 0);

    let mut state = 0x2c1b_3c6d_u32;
    let mut samples = Vec::with_capacity(512 * 512 * 3);
    for _ in 0..512 * 512 * 3 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        samples.push(state.to_be_bytes()[0]);
    }
    let with_large_idat = png(512, 512, image::ColorType::Rgb8, &samples);
    assert!(with_large_idat.len() > 256 << 10);
    assert!(with_large_idat.len() < 2 << 20);

    let idat_frame = decode_image(
        &mut Cursor::new(&with_large_idat),
        &compressed_limits,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(idat_frame.width(), 512);
    assert_eq!(idat_frame.height(), 512);
    drop(idat_frame);
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}

#[test]
fn malformed_unsupported_and_truncated_are_distinct() {
    assert!(matches!(
        decode(b"not an image"),
        Err(ascii_core::Error::Unsupported(_))
    ));
    assert!(matches!(
        decode(b"\x89PNG\r\n\x1a\n"),
        Err(ascii_core::Error::Malformed(_))
    ));
    assert!(matches!(
        decode(b"\x89PNG\r\n\x1a\n\0\0\0\0iCCP\0\0\0\0"),
        Err(ascii_core::Error::Malformed(_))
    ));
    let valid = png(1, 1, image::ColorType::Rgb8, &[10, 20, 30]);
    let mut chunk_offset = 8;
    let (idat_start, idat_length) = loop {
        let length = u32::from_be_bytes([
            valid[chunk_offset],
            valid[chunk_offset + 1],
            valid[chunk_offset + 2],
            valid[chunk_offset + 3],
        ]) as usize;
        let chunk_type = &valid[chunk_offset + 4..chunk_offset + 8];
        if chunk_type == b"IDAT" {
            break (chunk_offset + 8, length);
        }
        chunk_offset += 12 + length;
    };
    assert!(idat_length > 2);
    // Keep only the zlib header, omitting the required compressed image data.
    assert!(matches!(
        decode(&valid[..idat_start + 2]),
        Err(ascii_core::Error::Malformed(_))
    ));
}

#[test]
fn png_requires_a_complete_valid_iend_before_publishing() {
    // The encoder and its inputs are the same locally generated 1x1 RGB fixture as above.
    // Derive one fixture by corrupting the final IEND CRC, truncating that CRC, or appending data.
    let valid = png(1, 1, image::ColorType::Rgb8, &[10, 20, 30]);
    assert_eq!(&valid[valid.len() - 12..valid.len() - 4], b"\0\0\0\0IEND");
    let decode_limits = limits();
    let resources = decode_limits.resources.clone();
    let token = CancellationToken::new();

    let mut bad_crc = valid.clone();
    *bad_crc.last_mut().unwrap() ^= 1;
    let truncated = &valid[..valid.len() - 1];
    let trailing = [valid.as_slice(), b"trailing"].concat();
    for invalid in [&bad_crc[..], truncated, &trailing[..]] {
        assert!(matches!(
            decode_image(&mut Cursor::new(invalid), &decode_limits, &token),
            Err(ascii_core::Error::Malformed(_))
        ));
        assert_eq!(resources.cpu_bytes_in_use(), 0);
        assert_eq!(resources.active_decodes(), 0);
    }

    let frame = decode_image(&mut Cursor::new(&valid), &decode_limits, &token).unwrap();
    assert_eq!(frame.pixels(), &[10, 20, 30]);
    drop(frame);
    assert_eq!(resources.cpu_bytes_in_use(), 0);
}

#[test]
fn non_srgb_png_gamma_is_rejected_without_color_management() {
    // Insert a PNG gAMA=1.0 chunk into a locally generated 1x1 gray sample (128).
    let valid = png(1, 1, image::ColorType::L8, &[128]);
    let tagged = png_with_chunk_after_ihdr(&valid, *b"gAMA", &100_000_u32.to_be_bytes());
    println!(
        "linear-gamma PNG fixture sha256={:x}",
        Sha256::digest(&tagged)
    );

    assert!(matches!(
        decode(&tagged),
        Err(ascii_core::Error::Unsupported(
            ascii_core::UnsupportedReason::ColorDescription
        ))
    ));
}

#[test]
fn png_color_metadata_policy_accepts_matching_srgb_and_rejects_other_chromaticities() {
    let valid = png(1, 1, image::ColorType::L8, &[128]);
    let srgb_chromaticities = [
        31_270_u32, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 6_000,
    ];
    let chromaticity_payload = |values: [u32; 8]| {
        values
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>()
    };

    let with_chromaticities =
        png_with_chunk_after_ihdr(&valid, *b"cHRM", &chromaticity_payload(srgb_chromaticities));
    assert!(matches!(
        decode(&with_chromaticities),
        Err(ascii_core::Error::Unsupported(
            ascii_core::UnsupportedReason::ColorDescription
        ))
    ));

    let matching = png_with_chunk_after_ihdr(
        &png_with_chunk_after_ihdr(
            &png_with_chunk_after_ihdr(
                &valid,
                *b"cHRM",
                &chromaticity_payload(srgb_chromaticities),
            ),
            *b"gAMA",
            &45_455_u32.to_be_bytes(),
        ),
        *b"sRGB",
        &[0],
    );
    assert_eq!(decode(&matching).unwrap().pixels(), &[128, 128, 128]);

    let mut nonmatching_chromaticities = srgb_chromaticities;
    nonmatching_chromaticities[7] = 6_001;
    let nonmatching = png_with_chunk_after_ihdr(
        &png_with_chunk_after_ihdr(
            &png_with_chunk_after_ihdr(
                &valid,
                *b"cHRM",
                &chromaticity_payload(nonmatching_chromaticities),
            ),
            *b"gAMA",
            &45_455_u32.to_be_bytes(),
        ),
        *b"sRGB",
        &[0],
    );
    assert!(matches!(
        decode(&nonmatching),
        Err(ascii_core::Error::Unsupported(
            ascii_core::UnsupportedReason::ColorDescription
        ))
    ));
}

#[test]
fn png_truecolor_and_gray_trns_preserve_nonpremultiplied_samples() {
    // Locally generated PNGs, with a tRNS key equal to the only source pixel.
    // Eight-bit tRNS keys are stored as big-endian 16-bit values.
    let rgb = png(1, 1, image::ColorType::Rgb8, &[100, 150, 200]);
    let keyed_rgb = png_with_chunk_after_ihdr(&rgb, *b"tRNS", &[0, 100, 0, 150, 0, 200]);
    let gray = png(1, 1, image::ColorType::L8, &[77]);
    let keyed_gray = png_with_chunk_after_ihdr(&gray, *b"tRNS", &[0, 77]);
    println!(
        "nonindexed tRNS PNG fixtures sha256={:x}/{:x}",
        Sha256::digest(&keyed_rgb),
        Sha256::digest(&keyed_gray)
    );
    assert_eq!(decode(&keyed_rgb).unwrap().pixels(), &[100, 150, 200, 0]);
    assert_eq!(decode(&keyed_gray).unwrap().pixels(), &[77, 77, 77, 0]);
}

#[test]
fn small_jpeg_with_large_dimensions_is_rejected_before_raster_allocation() {
    let bytes = jpeg_with_dimensions(&jpeg(2, 1, &[255, 0, 0, 0, 255, 0]), 4096, 4096);
    assert!(bytes.len() < 1024);

    let mut decode_limits = limits();
    decode_limits.max_width = 8192;
    decode_limits.max_height = 8192;
    let error = decode_image(
        &mut Cursor::new(bytes),
        &decode_limits,
        &CancellationToken::new(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        ascii_core::Error::BudgetExceeded(BudgetKind::RasterBytes)
    );
}

#[test]
fn cancellation_before_decode_and_view_borrows_owned_pixels() {
    let token = CancellationToken::new();
    token.cancel();
    assert!(matches!(
        decode_image(&mut Cursor::new(b"anything"), &limits(), &token),
        Err(ascii_core::Error::Cancelled)
    ));
    let frame = decode(&png(1, 1, image::ColorType::Rgb8, &[4, 5, 6])).unwrap();
    let view = frame.view();
    assert_eq!(view.pixels(), frame.pixels());
    drop(frame);
    // `view` remains lexically tied to frame and therefore cannot be used after its drop.
}

#[test]
fn limits_and_ownership_retained_frames_charge_the_shared_ledger() {
    let mut decode_limits = limits();
    let resources = DecodeResources::new(1, 16 << 20);
    decode_limits.resources = resources.clone();
    let bytes = png(1, 1, image::ColorType::Rgba8, &[1, 2, 3, 4]);

    let first = decode_image(
        &mut Cursor::new(&bytes),
        &decode_limits,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(resources.active_decodes(), 0);
    assert_eq!(resources.cpu_bytes_in_use(), 4);
    assert_eq!(first.view().pixels(), &[1, 2, 3, 4]);

    let second = decode_image(
        &mut Cursor::new(&bytes),
        &decode_limits,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(resources.cpu_bytes_in_use(), 8);
    drop(second);
    assert_eq!(resources.cpu_bytes_in_use(), 4);
    drop(first);
    assert_eq!(resources.cpu_bytes_in_use(), 0);

    let next = decode_image(
        &mut Cursor::new(&bytes),
        &decode_limits,
        &CancellationToken::new(),
    )
    .unwrap();
    drop(next);
    assert_eq!(resources.cpu_bytes_in_use(), 0);
}

struct CancelAfterScratchReservation<'a> {
    input: Cursor<&'a [u8]>,
    cancel: &'a CancellationToken,
    resources: &'a DecodeResources,
    saw_reservation: bool,
}

impl Read for CancelAfterScratchReservation<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(buffer)?;
        if count > 0 && self.resources.cpu_bytes_in_use() > 0 {
            self.saw_reservation = true;
            self.cancel.cancel();
        }
        Ok(count)
    }
}

impl Seek for CancelAfterScratchReservation<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.input.seek(position)
    }
}

#[test]
fn limits_and_ownership_cancel_and_errors_release_all_ledger_permits() {
    let decode_limits = limits();
    let resources = decode_limits.resources.clone();
    let bytes = png(1, 1, image::ColorType::Rgb8, &[4, 5, 6]);
    let token = CancellationToken::new();
    let mut input = CancelAfterScratchReservation {
        input: Cursor::new(&bytes),
        cancel: &token,
        resources: &resources,
        saw_reservation: false,
    };
    assert!(matches!(
        decode_image(&mut input, &decode_limits, &token),
        Err(ascii_core::Error::Cancelled)
    ));
    assert!(input.saw_reservation);
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);

    assert!(matches!(
        decode_image(
            &mut Cursor::new(b"\x89PNG\r\n\x1a\n"),
            &decode_limits,
            &CancellationToken::new(),
        ),
        Err(ascii_core::Error::Malformed(_))
    ));
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);

    let jpeg = jpeg(1, 1, &[20, 40, 60]);
    let frame = decode_image(
        &mut Cursor::new(&jpeg),
        &decode_limits,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(frame.view().dimensions(), (1, 1));
    drop(frame);
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}

struct BlockingInput<'a> {
    input: Cursor<&'a [u8]>,
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    blocked: bool,
}

impl Read for BlockingInput<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if !self.blocked {
            self.blocked = true;
            self.entered.wait();
            self.release.wait();
        }
        self.input.read(buffer)
    }
}

impl Seek for BlockingInput<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.input.seek(position)
    }
}

#[test]
fn limits_and_ownership_decode_concurrency_is_shared() {
    let mut decode_limits = limits();
    let resources = DecodeResources::new(1, 16 << 20);
    decode_limits.resources = resources.clone();
    let bytes = png(1, 1, image::ColorType::Rgb8, &[1, 2, 3]);
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker_limits = decode_limits.clone();
    let worker_bytes = bytes.clone();
    let worker_entered = Arc::clone(&entered);
    let worker_release = Arc::clone(&release);
    let worker = thread::spawn(move || {
        let mut input = BlockingInput {
            input: Cursor::new(&worker_bytes),
            entered: worker_entered,
            release: worker_release,
            blocked: false,
        };
        decode_image(&mut input, &worker_limits, &CancellationToken::new())
    });

    entered.wait();
    assert!(matches!(
        decode_image(
            &mut Cursor::new(&bytes),
            &decode_limits,
            &CancellationToken::new()
        ),
        Err(ascii_core::Error::BudgetExceeded(
            BudgetKind::DecodeConcurrency
        ))
    ));
    release.wait();
    let frame = worker.join().unwrap().unwrap();
    drop(frame);
    assert_eq!(resources.active_decodes(), 0);
    assert_eq!(resources.cpu_bytes_in_use(), 0);

    let later = decode_image(
        &mut Cursor::new(&bytes),
        &decode_limits,
        &CancellationToken::new(),
    )
    .unwrap();
    drop(later);
    assert_eq!(resources.cpu_bytes_in_use(), 0);
}

struct PositionTrackingInput<'a> {
    input: Cursor<&'a [u8]>,
    furthest_read: u64,
}

impl Read for PositionTrackingInput<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(buffer)?;
        if count != 0 {
            self.furthest_read = self.furthest_read.max(self.input.position());
        }
        Ok(count)
    }
}

impl Seek for PositionTrackingInput<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.input.seek(position)
    }
}

#[test]
fn png_compressed_icc_is_rejected_before_inflation_after_snapshotting() {
    // Rights-cleared profile: 512 KiB of zeroes generated with the image PNG encoder.
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    encoder.set_icc_profile(vec![0; 512 << 10]).unwrap();
    encoder
        .write_image(&[1, 2, 3], 1, 1, image::ColorType::Rgb8.into())
        .unwrap();
    assert!(bytes.len() < 64 << 10);
    let mut input = PositionTrackingInput {
        input: Cursor::new(&bytes),
        furthest_read: 0,
    };
    let mut decode_limits = limits();
    decode_limits.max_decoder_bytes = 64 << 10;
    assert!(matches!(
        decode_image(&mut input, &decode_limits, &CancellationToken::new()),
        Err(ascii_core::Error::Unsupported(
            ascii_core::UnsupportedReason::ColorDescription
        ))
    ));
    assert_eq!(input.furthest_read, u64::try_from(bytes.len()).unwrap());
    assert_eq!(decode_limits.resources.cpu_bytes_in_use(), 0);
    assert_eq!(decode_limits.resources.active_decodes(), 0);
}

#[test]
fn compressed_raster_exceeding_budget_is_rejected() {
    // A compact rights-cleared all-black 256x256 RGB PNG has a 196608-byte raster.
    let samples = vec![0; 256 * 256 * 3];
    let bytes = png(256, 256, image::ColorType::Rgb8, &samples);
    assert!(bytes.len() < samples.len() / 8);
    let mut decode_limits = limits();
    decode_limits.budget = Budget::new(1 << 20, 4096, 1 << 20);
    assert!(matches!(
        decode_image(
            &mut Cursor::new(&bytes),
            &decode_limits,
            &CancellationToken::new()
        ),
        Err(ascii_core::Error::BudgetExceeded(BudgetKind::RasterBytes))
    ));
    assert_eq!(decode_limits.resources.cpu_bytes_in_use(), 0);
    assert_eq!(decode_limits.resources.active_decodes(), 0);
}
