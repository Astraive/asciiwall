use ascii_media::{ClipFrameError, ClipReadError, parse_ascii_clip};

const HEADER_LEN: usize = 64;
const INDEX_LEN: usize = 40;
const PAYLOAD_LEN: usize = 24;
const GLYPH_OFFSET: usize = HEADER_LEN;
const GLYPH_TABLE_LEN: usize = 6;
const INDEX_OFFSET: usize = GLYPH_OFFSET + GLYPH_TABLE_LEN;
const PAYLOAD_OFFSET: usize = INDEX_OFFSET + INDEX_LEN;

fn valid_clip() -> Vec<u8> {
    let mut bytes = vec![0; PAYLOAD_OFFSET + PAYLOAD_LEN];
    bytes[0..8].copy_from_slice(b"ASCIICL1");
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[12..16].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
    bytes[16..20].copy_from_slice(&2u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&2u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
    bytes[32..36].copy_from_slice(&1_000_000u32.to_le_bytes());
    bytes[36..44].copy_from_slice(&1u64.to_le_bytes());
    bytes[44..52].copy_from_slice(&(GLYPH_OFFSET as u64).to_le_bytes());
    bytes[52..60].copy_from_slice(&(INDEX_OFFSET as u64).to_le_bytes());

    bytes[GLYPH_OFFSET..GLYPH_OFFSET + GLYPH_TABLE_LEN].copy_from_slice(&[1, 0, b'A', 1, 0, b' ']);
    bytes[PAYLOAD_OFFSET..PAYLOAD_OFFSET + 4].copy_from_slice(&0u32.to_le_bytes());
    bytes[PAYLOAD_OFFSET + 4..PAYLOAD_OFFSET + 8].copy_from_slice(&[255, 0, 0, 255]);
    bytes[PAYLOAD_OFFSET + 8..PAYLOAD_OFFSET + 12].copy_from_slice(&[0, 0, 0, 0]);
    bytes[PAYLOAD_OFFSET + 12..PAYLOAD_OFFSET + 16].copy_from_slice(&1u32.to_le_bytes());
    bytes[PAYLOAD_OFFSET + 16..PAYLOAD_OFFSET + 20].copy_from_slice(&[0, 255, 0, 255]);
    bytes[PAYLOAD_OFFSET + 20..PAYLOAD_OFFSET + 24].copy_from_slice(&[255, 255, 255, 255]);

    bytes[INDEX_OFFSET..INDEX_OFFSET + 8].copy_from_slice(&(-10i64).to_le_bytes());
    bytes[INDEX_OFFSET + 8..INDEX_OFFSET + 16].copy_from_slice(&10_000u64.to_le_bytes());
    bytes[INDEX_OFFSET + 16..INDEX_OFFSET + 24]
        .copy_from_slice(&(PAYLOAD_OFFSET as u64).to_le_bytes());
    bytes[INDEX_OFFSET + 24..INDEX_OFFSET + 28]
        .copy_from_slice(&(PAYLOAD_LEN as u32).to_le_bytes());
    bytes[INDEX_OFFSET + 28..INDEX_OFFSET + 32]
        .copy_from_slice(&(PAYLOAD_LEN as u32).to_le_bytes());
    let checksum = crc32(&bytes[PAYLOAD_OFFSET..PAYLOAD_OFFSET + PAYLOAD_LEN]);
    bytes[INDEX_OFFSET + 36..INDEX_OFFSET + 40].copy_from_slice(&checksum.to_le_bytes());
    bytes
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[test]
fn imports_complete_frame_and_preserves_valid_glyph_table_order() {
    let clip = parse_ascii_clip(&valid_clip()).expect("valid v1 clip imports");

    assert_eq!(clip.columns(), 2);
    assert_eq!(clip.rows(), 1);
    assert_eq!(clip.glyphs(), &['A', ' ']);
    assert_eq!(clip.frames().len(), 1);
    let frame = &clip.frames()[0];
    assert_eq!(frame.pts_us(), -10);
    assert_eq!(frame.duration_us(), 10_000);
    assert_eq!(frame.cells().len(), 2);
    assert_eq!(frame.cells()[0].glyph_index(), 0);
    assert_eq!(frame.cells()[0].foreground(), [255, 0, 0, 255]);
    assert_eq!(frame.cells()[0].background(), [0, 0, 0, 0]);
    assert_eq!(frame.cells()[1].glyph_index(), 1);
    assert_eq!(frame.cells()[1].foreground(), [0, 255, 0, 255]);
    assert_eq!(frame.cells()[1].background(), [255, 255, 255, 255]);
}

#[test]
fn rejects_invalid_container_header_and_offsets() {
    assert_eq!(
        parse_ascii_clip(&[0; 63]),
        Err(ClipReadError::TruncatedHeader { available: 63 })
    );

    let mut invalid = valid_clip();
    invalid[0] = b'X';
    assert_eq!(parse_ascii_clip(&invalid), Err(ClipReadError::InvalidMagic));

    let mut invalid = valid_clip();
    invalid[8..10].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::UnsupportedVersion { version: 2 })
    );

    let mut invalid = valid_clip();
    invalid[10..12].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::UnsupportedFlags { flags: 1 })
    );

    let mut invalid = valid_clip();
    invalid[52..60].copy_from_slice(&64u64.to_le_bytes());
    assert_eq!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::OverlappingSections {
            first: "glyph table",
            second: "frame index"
        })
    );
}

#[test]
fn rejects_unsupported_glyph_table_entries_through_shared_predicate() {
    let mut invalid = valid_clip();
    invalid[GLYPH_OFFSET + 2] = 0x1b;
    assert!(matches!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::InvalidGlyphTable(
            ascii_media::ClipGlyphTableError::UnsupportedScalar {
                index: 0,
                character: '\u{1b}'
            }
        ))
    ));
}

#[test]
fn rejects_corrupt_payloads_and_invalid_glyph_references() {
    let mut invalid = valid_clip();
    invalid[PAYLOAD_OFFSET + 4] ^= 1;
    assert!(matches!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::InvalidFrame {
            index: 0,
            reason: ClipFrameError::ChecksumMismatch { .. }
        })
    ));

    let mut invalid = valid_clip();
    invalid[PAYLOAD_OFFSET..PAYLOAD_OFFSET + 4].copy_from_slice(&2u32.to_le_bytes());
    let checksum = crc32(&invalid[PAYLOAD_OFFSET..PAYLOAD_OFFSET + PAYLOAD_LEN]);
    invalid[INDEX_OFFSET + 36..INDEX_OFFSET + 40].copy_from_slice(&checksum.to_le_bytes());
    assert!(matches!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::InvalidFrame {
            index: 0,
            reason: ClipFrameError::GlyphIndexOutOfBounds {
                cell: 0,
                glyph_index: 2
            }
        })
    ));
}

#[test]
fn rejects_invalid_frame_metadata_and_payload_overlaps() {
    let mut invalid = valid_clip();
    invalid[INDEX_OFFSET + 8..INDEX_OFFSET + 16].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::InvalidFrame {
            index: 0,
            reason: ClipFrameError::ZeroDuration
        })
    );

    let mut invalid = valid_clip();
    invalid[INDEX_OFFSET + 32] = 1;
    assert_eq!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::InvalidFrame {
            index: 0,
            reason: ClipFrameError::UnsupportedCodec { codec: 1 }
        })
    );

    let mut invalid = valid_clip();
    invalid[INDEX_OFFSET + 16..INDEX_OFFSET + 24]
        .copy_from_slice(&(INDEX_OFFSET as u64).to_le_bytes());
    assert_eq!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::InvalidFrame {
            index: 0,
            reason: ClipFrameError::PayloadOverlap
        })
    );
}

#[test]
fn rejects_oversized_dimensions_and_frame_counts_before_allocation() {
    let mut invalid = valid_clip();
    invalid[16..20].copy_from_slice(&1_048_577u32.to_le_bytes());
    assert!(matches!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::DecodedLimitExceeded { .. })
    ));

    let mut invalid = valid_clip();
    invalid[36..44].copy_from_slice(&65_537u64.to_le_bytes());
    assert!(matches!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::FrameCountLimitExceeded { .. })
    ));

    let mut invalid = valid_clip();
    invalid[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        parse_ascii_clip(&invalid),
        Err(ClipReadError::InvalidGlyphTable(_))
    ));
}
