/// Builds a tiny rights-cleared TrueType SFNT fixture entirely from source constants.
///
/// Glyphs 0, 2, and 3 are single rectangular outlines; glyph 1 is the blank space glyph. With
/// `narrow_b`, glyph 3 has a narrower outline so the font bytes and measured table both change.
pub fn synthetic_font(narrow_b: bool) -> Vec<u8> {
    let glyphs = vec![
        rectangle(600, 800),
        Vec::new(),
        rectangle(500, 700),
        rectangle(if narrow_b { 350 } else { 500 }, 700),
    ];
    font_with_glyphs(glyphs, false)
}

/// A source-generated font with many long diagonal outline edges.
pub fn diagonal_font(alias_b: bool) -> Vec<u8> {
    font_with_glyphs(
        vec![
            rectangle(600, 800),
            Vec::new(),
            outline_heavy(false),
            rectangle(500, 700),
        ],
        alias_b,
    )
}

/// A source-generated font whose outline contains many quadratic curves.
pub fn quadratic_font() -> Vec<u8> {
    font_with_glyphs(
        vec![
            rectangle(600, 800),
            Vec::new(),
            outline_heavy(true),
            rectangle(500, 700),
        ],
        false,
    )
}

/// A composite glyph containing the same outline-heavy component twice.
pub fn composite_font() -> Vec<u8> {
    font_with_glyphs(
        vec![
            rectangle(600, 800),
            Vec::new(),
            composite(),
            outline_heavy(false),
        ],
        false,
    )
}

fn font_with_glyphs(mut glyphs: Vec<Vec<u8>>, alias_b: bool) -> Vec<u8> {
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
    let tables = vec![
        (*b"cmap", cmap(alias_b)),
        (*b"glyf", glyf),
        (*b"head", head()),
        (*b"hhea", hhea()),
        (*b"hmtx", hmtx()),
        // A version-0 kern table with no subtables is valid and inert.
        (*b"kern", vec![0, 0, 0, 0]),
        (*b"loca", loca),
        (*b"maxp", maxp()),
    ];
    sfnt(tables)
}

fn outline_heavy(quadratic: bool) -> Vec<u8> {
    let contours = if quadratic { 128u16 } else { 256u16 };
    let points = if quadratic {
        [(0i16, 0i16), (0, 700), (500, 700), (500, 0)]
    } else {
        [(0i16, 20i16), (500, 720), (500, 700), (0, 0)]
    };
    let mut glyph = Vec::new();
    push_i16(
        &mut glyph,
        i16::try_from(contours).expect("contour count fits"),
    );
    for bound in [0, 0, 500, if quadratic { 700 } else { 720 }] {
        push_i16(&mut glyph, bound);
    }
    for contour in 0..contours {
        push_u16(&mut glyph, contour * 4 + 3);
    }
    push_u16(&mut glyph, 0); // no instructions
    for _ in 0..contours {
        glyph.extend_from_slice(if quadratic { &[1, 0, 1, 0] } else { &[1; 4] });
    }
    for axis in 0..2 {
        let mut previous = 0i16;
        for _ in 0..contours {
            for &(x, y) in &points {
                let coordinate = if axis == 0 { x } else { y };
                push_i16(&mut glyph, coordinate - previous);
                previous = coordinate;
            }
        }
    }
    glyph
}

fn composite() -> Vec<u8> {
    let mut glyph = Vec::new();
    push_i16(&mut glyph, -1); // composite
    for bound in [0, 0, 500, 720] {
        push_i16(&mut glyph, bound);
    }
    for flags in [0x0023, 0x0003] {
        push_u16(&mut glyph, flags); // word-sized XY offsets; first has another component
        push_u16(&mut glyph, 3);
        push_i16(&mut glyph, 0);
        push_i16(&mut glyph, 0);
    }
    glyph
}

fn rectangle(width: u16, height: u16) -> Vec<u8> {
    let mut glyph = Vec::new();
    push_i16(&mut glyph, 1); // one contour
    push_i16(&mut glyph, 0);
    push_i16(&mut glyph, 0);
    push_i16(
        &mut glyph,
        i16::try_from(width).expect("fixture width fits"),
    );
    push_i16(
        &mut glyph,
        i16::try_from(height).expect("fixture height fits"),
    );
    push_u16(&mut glyph, 3); // four points, indexed from zero
    push_u16(&mut glyph, 0); // no instructions
    glyph.extend_from_slice(&[1, 1, 1, 1]); // on-curve points, signed 16-bit deltas
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
    push_u32(&mut table, 0x0001_0000); // version
    push_u32(&mut table, 0x0001_0000); // font revision
    push_u32(&mut table, 0); // checksum adjustment
    push_u32(&mut table, 0x5f0f_3cf5); // magic
    push_u16(&mut table, 0); // flags
    push_u16(&mut table, 1000); // units per em
    table.extend_from_slice(&[0; 16]); // created and modified
    push_i16(&mut table, 0);
    push_i16(&mut table, 0);
    push_i16(&mut table, 600);
    push_i16(&mut table, 800);
    push_u16(&mut table, 0); // mac style
    push_u16(&mut table, 8); // lowest recommended PPEM
    push_i16(&mut table, 2); // font direction hint
    push_i16(&mut table, 0); // short loca offsets
    push_i16(&mut table, 0); // glyph data format
    table
}

fn hhea() -> Vec<u8> {
    let mut table = Vec::with_capacity(36);
    push_u32(&mut table, 0x0001_0000); // version
    push_i16(&mut table, 800); // ascent
    push_i16(&mut table, -200); // descent
    push_i16(&mut table, 0); // line gap
    push_u16(&mut table, 600); // advance width max
    push_i16(&mut table, 0); // min left side bearing
    push_i16(&mut table, 0); // min right side bearing
    push_i16(&mut table, 600); // x max extent
    push_i16(&mut table, 1); // caret slope rise
    push_i16(&mut table, 0); // caret slope run
    push_i16(&mut table, 0); // caret offset
    table.extend_from_slice(&[0; 8]); // reserved fields
    push_i16(&mut table, 0); // metric data format
    push_u16(&mut table, 4); // number of long horizontal metrics
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
    push_u32(&mut table, 0x0001_0000); // TrueType outlines
    push_u16(&mut table, 4);
    for value in [1024, 256, 2048, 512, 1, 0, 0, 0, 0, 0, 0, 2, 1] {
        push_u16(&mut table, value);
    }
    table
}

fn cmap(alias_b: bool) -> Vec<u8> {
    let mut subtable = Vec::new();
    push_u16(&mut subtable, 4); // format 4
    push_u16(&mut subtable, 0); // length, filled below
    push_u16(&mut subtable, 0); // language
    push_u16(&mut subtable, 6); // segCountX2
    push_u16(&mut subtable, 4); // searchRange
    push_u16(&mut subtable, 1); // entrySelector
    push_u16(&mut subtable, 2); // rangeShift
    for end in [0x0020, 0x0042, 0xffff] {
        push_u16(&mut subtable, end);
    }
    push_u16(&mut subtable, 0); // reservedPad
    for start in [0x0020, 0x0041, 0xffff] {
        push_u16(&mut subtable, start);
    }
    for delta in [
        1u16.wrapping_sub(0x0020),
        if alias_b {
            0
        } else {
            2u16.wrapping_sub(0x0041)
        },
        1,
    ] {
        push_u16(&mut subtable, delta);
    }
    for offset in [0, if alias_b { 4 } else { 0 }, 0] {
        push_u16(&mut subtable, offset);
    }
    if alias_b {
        push_u16(&mut subtable, 2);
        push_u16(&mut subtable, 2);
    }
    let length = u16::try_from(subtable.len()).expect("fixture cmap length fits");
    subtable[2..4].copy_from_slice(&length.to_be_bytes());

    let mut table = Vec::new();
    push_u16(&mut table, 0); // version
    push_u16(&mut table, 1); // one encoding record
    push_u16(&mut table, 3); // Windows platform
    push_u16(&mut table, 1); // Unicode BMP encoding
    push_u32(&mut table, 12); // subtable offset
    table.extend_from_slice(&subtable);
    table
}

fn sfnt(mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_unstable_by_key(|(tag, _)| *tag);
    let count = u16::try_from(tables.len()).expect("fixture table count fits");
    let power = 1u16 << (u32::from(count).ilog2());
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
        offset = offset
            .checked_add(table.len())
            .expect("fixture total size fits");
    }

    let mut font = Vec::with_capacity(align4(offset));
    push_u32(&mut font, 0x0001_0000); // TrueType scaler type
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
    for ((_, table), (_, _, table_offset, _)) in tables.iter().zip(records.iter()) {
        font.resize(
            usize::try_from(*table_offset).expect("fixture offset fits"),
            0,
        );
        font.extend_from_slice(table);
    }
    font.resize(align4(font.len()), 0);
    font
}

fn checksum(bytes: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in bytes.chunks(4) {
        let mut word = [0u8; 4];
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
