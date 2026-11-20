//! Rights-cleared synthetic TrueType fixture built entirely from local constants.
//!
//! Glyph 1 is blank (space); glyphs 2 and 3 are simple outlines mapped to the block and Braille
//! cells.

pub fn synthetic_font() -> Vec<u8> {
    let mut glyphs = vec![
        rectangle(600, 800),
        Vec::new(),
        rectangle(500, 700),
        rectangle(350, 700),
    ];
    let mut glyf = Vec::new();
    let mut offsets = Vec::with_capacity(glyphs.len() + 1);
    for glyph in &mut glyphs {
        offsets.push(u16::try_from(glyf.len() / 2).expect("fixture loca offset fits"));
        glyf.append(glyph);
    }
    offsets.push(u16::try_from(glyf.len() / 2).expect("fixture loca offset fits"));

    let mut loca = Vec::new();
    for offset in offsets {
        push_u16(&mut loca, offset);
    }
    let mappings = vec![(0x0020, 1), (0x2588, 2), (0x2801, 3)];
    let tables = vec![
        (*b"cmap", cmap(&mappings)),
        (*b"glyf", glyf),
        (*b"head", head()),
        (*b"hhea", hhea()),
        (*b"hmtx", hmtx()),
        (*b"kern", vec![0, 0, 0, 0]),
        (*b"loca", loca),
        (*b"maxp", maxp()),
    ];
    sfnt(tables)
}

fn rectangle(width: u16, height: u16) -> Vec<u8> {
    let mut glyph = Vec::new();
    push_i16(&mut glyph, 1);
    push_i16(&mut glyph, 0);
    push_i16(&mut glyph, 0);
    push_i16(&mut glyph, i16::try_from(width).expect("fixture width fits"));
    push_i16(&mut glyph, i16::try_from(height).expect("fixture height fits"));
    push_u16(&mut glyph, 3);
    push_u16(&mut glyph, 0);
    glyph.extend_from_slice(&[1, 1, 1, 1]);
    for delta in [
        0,
        i16::try_from(width).expect("fixture width fits"),
        0,
        -i16::try_from(width).expect("fixture width fits"),
    ] {
        push_i16(&mut glyph, delta);
    }
    for delta in [0, 0, i16::try_from(height).expect("fixture height fits"), 0] {
        push_i16(&mut glyph, delta);
    }
    glyph
}

fn head() -> Vec<u8> {
    let mut table = Vec::with_capacity(54);
    push_u32(&mut table, 0x0001_0000);
    push_u32(&mut table, 0x0001_0000);
    push_u32(&mut table, 0);
    push_u32(&mut table, 0x5f0f_3cf5);
    push_u16(&mut table, 0);
    push_u16(&mut table, 1000);
    table.extend_from_slice(&[0; 16]);
    for value in [0, 0, 600, 800] {
        push_i16(&mut table, value);
    }
    push_u16(&mut table, 0);
    push_u16(&mut table, 8);
    push_i16(&mut table, 2);
    push_i16(&mut table, 0);
    push_i16(&mut table, 0);
    table
}

fn hhea() -> Vec<u8> {
    let mut table = Vec::with_capacity(36);
    push_u32(&mut table, 0x0001_0000);
    push_i16(&mut table, 800);
    push_i16(&mut table, -200);
    push_i16(&mut table, 0);
    push_u16(&mut table, 600);
    push_i16(&mut table, 0);
    push_i16(&mut table, 0);
    push_i16(&mut table, 600);
    push_i16(&mut table, 1);
    push_i16(&mut table, 0);
    push_i16(&mut table, 0);
    table.extend_from_slice(&[0; 8]);
    push_i16(&mut table, 0);
    push_u16(&mut table, 4);
    table
}

fn hmtx() -> Vec<u8> {
    let mut table = Vec::with_capacity(16);
    for advance in [600, 300, 600, 600] {
        push_u16(&mut table, advance);
        push_i16(&mut table, 0);
    }
    table
}

fn maxp() -> Vec<u8> {
    let mut table = Vec::with_capacity(32);
    push_u32(&mut table, 0x0001_0000);
    push_u16(&mut table, 4);
    table.resize(32, 0);
    table
}

fn cmap(mappings: &[(u16, u16)]) -> Vec<u8> {
    let segment_count = mappings.len() + 1;
    let segment_count_u16 = u16::try_from(segment_count).expect("fixture cmap segment count fits");
    let search_power = 1u16 << segment_count_u16.ilog2();
    let mut subtable = Vec::new();
    push_u16(&mut subtable, 4);
    push_u16(&mut subtable, 0);
    push_u16(&mut subtable, 0);
    push_u16(&mut subtable, segment_count_u16 * 2);
    push_u16(&mut subtable, search_power * 2);
    push_u16(
        &mut subtable,
        u16::try_from(search_power.ilog2()).expect("fixture selector fits"),
    );
    push_u16(
        &mut subtable,
        segment_count_u16 * 2 - search_power * 2,
    );
    for (character, _) in mappings {
        push_u16(&mut subtable, *character);
    }
    push_u16(&mut subtable, u16::MAX);
    push_u16(&mut subtable, 0);
    for (character, _) in mappings {
        push_u16(&mut subtable, *character);
    }
    push_u16(&mut subtable, u16::MAX);
    for (character, glyph) in mappings {
        push_u16(&mut subtable, glyph.wrapping_sub(*character));
    }
    push_u16(&mut subtable, 1);
    subtable.resize(subtable.len() + 2 * segment_count, 0);
    let length = u16::try_from(subtable.len()).expect("fixture cmap length fits");
    subtable[2..4].copy_from_slice(&length.to_be_bytes());

    let mut table = Vec::new();
    push_u16(&mut table, 0);
    push_u16(&mut table, 1);
    push_u16(&mut table, 3);
    push_u16(&mut table, 1);
    push_u32(&mut table, 12);
    table.extend_from_slice(&subtable);
    table
}

fn sfnt(mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_unstable_by_key(|(tag, _)| *tag);
    let count = u16::try_from(tables.len()).expect("fixture table count fits");
    let power = 1u16 << u32::from(count).ilog2();
    let search_range = power * 16;
    let entry_selector = u16::try_from(power.ilog2()).expect("fixture selector fits");
    let range_shift = count * 16 - search_range;
    let mut offset = 12usize + 16usize * usize::from(count);
    let mut records = Vec::with_capacity(tables.len());
    for (tag, table) in &tables {
        offset = align4(offset);
        let table_offset = u32::try_from(offset).expect("fixture table offset fits");
        let table_length = u32::try_from(table.len()).expect("fixture table length fits");
        records.push((*tag, checksum(table), table_offset, table_length));
        offset = offset.checked_add(table.len()).expect("fixture total size fits");
    }

    let mut font = Vec::with_capacity(align4(offset));
    push_u32(&mut font, 0x0001_0000);
    push_u16(&mut font, count);
    push_u16(&mut font, search_range);
    push_u16(&mut font, entry_selector);
    push_u16(&mut font, range_shift);
    for (tag, sum, table_offset, table_length) in &records {
        font.extend_from_slice(tag);
        push_u32(&mut font, *sum);
        push_u32(&mut font, *table_offset);
        push_u32(&mut font, *table_length);
    }
    for ((_, table), (_, _, table_offset, _)) in tables.iter().zip(&records) {
        font.resize(usize::try_from(*table_offset).expect("fixture offset fits"), 0);
        font.extend_from_slice(table);
    }
    font.resize(align4(font.len()), 0);
    font
}

fn checksum(bytes: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in bytes.chunks(4) {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_i16(bytes: &mut Vec<u8>, value: i16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}
