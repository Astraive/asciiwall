use ascii_media::{ClipGlyphTableError, MAX_CLIP_GLYPH_COUNT, parse_clip_glyph_table};

fn append_entry(table: &mut Vec<u8>, glyph: &str) {
    let length = u16::try_from(glyph.len()).expect("test glyph length fits u16");
    table.extend_from_slice(&length.to_le_bytes());
    table.extend_from_slice(glyph.as_bytes());
}

#[test]
fn parses_supported_single_cell_glyphs_in_serialized_order() {
    let mut table = Vec::new();
    append_entry(&mut table, "A");
    append_entry(&mut table, " ");
    append_entry(&mut table, "░");
    append_entry(&mut table, "⠁");

    let parsed = parse_clip_glyph_table(&table, 4).expect("supported table parses");

    assert_eq!(parsed.glyphs(), &['A', ' ', '░', '⠁']);
}

#[test]
fn rejects_truncated_and_out_of_range_lengths() {
    assert_eq!(
        parse_clip_glyph_table(&[1], 0),
        Err(ClipGlyphTableError::TruncatedLengthPrefix {
            index: 0,
            available: 1,
        })
    );
    assert_eq!(
        parse_clip_glyph_table(&[0, 0], 1),
        Err(ClipGlyphTableError::InvalidGlyphLength {
            index: 0,
            length: 0,
        })
    );
    assert_eq!(
        parse_clip_glyph_table(&[33, 0], 1),
        Err(ClipGlyphTableError::InvalidGlyphLength {
            index: 0,
            length: 33,
        })
    );
    assert_eq!(
        parse_clip_glyph_table(&[4, 0, b'A', b'B'], 1),
        Err(ClipGlyphTableError::TruncatedGlyphBytes {
            index: 0,
            length: 4,
            available: 2,
        })
    );
}

#[test]
fn rejects_invalid_utf8() {
    assert_eq!(
        parse_clip_glyph_table(&[2, 0, 0xc3, 0x28], 1),
        Err(ClipGlyphTableError::InvalidUtf8 {
            index: 0,
            valid_up_to: 0,
        })
    );
}

#[test]
fn rejects_unsupported_and_control_scalars() {
    for (glyph, character) in [("\u{1b}", '\u{1b}'), ("é", 'é'), ("🧡", '🧡')] {
        let mut table = Vec::new();
        append_entry(&mut table, glyph);

        assert_eq!(
            parse_clip_glyph_table(&table, 1),
            Err(ClipGlyphTableError::UnsupportedScalar {
                index: 0,
                character,
            })
        );
    }
}

#[test]
fn rejects_multi_scalar_entries() {
    let mut table = Vec::new();
    append_entry(&mut table, "AB");

    assert_eq!(
        parse_clip_glyph_table(&table, 1),
        Err(ClipGlyphTableError::MultipleScalars {
            index: 0,
            second: 'B',
        })
    );
}

#[test]
fn rejects_declared_and_actual_count_mismatches() {
    let mut one_entry = Vec::new();
    append_entry(&mut one_entry, "A");
    assert_eq!(
        parse_clip_glyph_table(&one_entry, 2),
        Err(ClipGlyphTableError::CountMismatch {
            declared: 2,
            actual: 1,
        })
    );

    let mut two_entries = one_entry;
    append_entry(&mut two_entries, "B");
    assert_eq!(
        parse_clip_glyph_table(&two_entries, 1),
        Err(ClipGlyphTableError::CountMismatch {
            declared: 1,
            actual: 2,
        })
    );
}

#[test]
fn enforces_the_documented_glyph_count_limit() {
    assert_eq!(
        parse_clip_glyph_table(&[], MAX_CLIP_GLYPH_COUNT + 1),
        Err(ClipGlyphTableError::GlyphCountLimitExceeded {
            count: MAX_CLIP_GLYPH_COUNT + 1,
            maximum: MAX_CLIP_GLYPH_COUNT,
        })
    );
}
