//! Rights-cleared source-generated fixed-cell TrueType fixtures for repertoire tests.
use super::synthetic_font::{head, push_i16, push_u16, push_u32, sfnt};

/// A complete strict-cell cmap whose block and Braille outlines are generated from scalar data.
pub fn complete_repertoire_font(multicell: Option<char>) -> Vec<u8> {
    let mut characters = Vec::new();
    characters.extend((0x20..=0x7e).map(|scalar| char::from_u32(scalar).expect("ASCII scalar")));
    characters
        .extend((0x2580..=0x259f).map(|scalar| char::from_u32(scalar).expect("block scalar")));
    characters
        .extend((0x2800..=0x28ff).map(|scalar| char::from_u32(scalar).expect("Braille scalar")));
    font_for_characters(characters, multicell)
}

/// An ASCII-only fixture that intentionally lacks `C` for unavailable-glyph previews.
pub fn sparse_ascii_font() -> Vec<u8> {
    font_for_characters(vec![' ', 'A', 'B'], None)
}

fn font_for_characters(characters: Vec<char>, multicell: Option<char>) -> Vec<u8> {
    let mut glyphs = vec![rectangles(&[(40, 40, 560, 760)])];
    glyphs.reserve(characters.len());
    let mut advances = vec![600; characters.len() + 1];
    for (index, character) in characters.iter().copied().enumerate() {
        glyphs.push(match character {
            ' ' => Vec::new(),
            '\u{2580}'..='\u{259f}' => block_glyph(character),
            '\u{2800}'..='\u{28ff}' => braille_glyph(character),
            _ => rectangles(&[(60, 60, 540, 740)]),
        });
        if multicell == Some(character) {
            advances[index + 1] = 1200;
        }
    }
    build_font(glyphs, advances, repertoire_cmap(&characters))
}

fn braille_glyph(character: char) -> Vec<u8> {
    let pattern = character as u32 - 0x2800;
    let dots = [
        (0, 650),
        (0, 450),
        (0, 250),
        (1, 650),
        (1, 450),
        (1, 250),
        (0, 50),
        (1, 50),
    ];
    let mut dot_rectangles = Vec::new();
    for (dot, (column, y)) in dots.into_iter().enumerate() {
        if pattern & (1 << dot) != 0 {
            let x = if column == 0 { 80 } else { 380 };
            dot_rectangles.push((x, y, x + 120, y + 120));
        }
    }
    rectangles(&dot_rectangles)
}

fn block_glyph(character: char) -> Vec<u8> {
    let scalar = character as u32;
    let rects = match scalar {
        0x2580 => vec![(0, 400, 600, 800)],
        0x2581..=0x2587 => {
            let height = (scalar - 0x2580) * 100;
            vec![(0, 0, 600, height as i16)]
        }
        0x2588 => vec![(0, 0, 600, 800)],
        0x2589..=0x258f => {
            let width = (0x2590 - scalar) * 600 / 8;
            vec![(0, 0, width as i16, 800)]
        }
        0x2590 => vec![(300, 0, 600, 800)],
        0x2591..=0x2593 => shade_rectangles(scalar - 0x2590),
        0x2594 => vec![(0, 700, 600, 800)],
        0x2595 => vec![(525, 0, 600, 800)],
        0x2596 => vec![(0, 0, 300, 400)],
        0x2597 => vec![(300, 0, 600, 400)],
        0x2598 => vec![(0, 400, 300, 800)],
        0x2599 => vec![(0, 0, 300, 800), (300, 0, 600, 400)],
        0x259a => vec![(0, 400, 300, 800), (300, 0, 600, 400)],
        0x259b => vec![(0, 0, 300, 800), (300, 400, 600, 800)],
        0x259c => vec![(0, 400, 600, 800), (300, 0, 600, 400)],
        0x259d => vec![(300, 400, 600, 800)],
        0x259e => vec![(300, 400, 600, 800), (0, 0, 300, 400)],
        0x259f => vec![(0, 400, 600, 800), (0, 0, 300, 400)],
        _ => Vec::new(),
    };
    rectangles(&rects)
}

fn shade_rectangles(level: u32) -> Vec<(i16, i16, i16, i16)> {
    let mut rectangles = Vec::new();
    let step = match level {
        1 => 4,
        2 => 2,
        _ => 1,
    };
    for y in (40..800).step_by(step * 100) {
        for x in (40..600).step_by(step * 100) {
            if (x / 100 + y / 100) % step == 0 {
                rectangles.push((x as i16, y as i16, x as i16 + 55, y as i16 + 55));
            }
        }
    }
    rectangles
}

fn rectangles(rects: &[(i16, i16, i16, i16)]) -> Vec<u8> {
    if rects.is_empty() {
        return Vec::new();
    }
    let mut points = Vec::with_capacity(rects.len() * 4);
    for &(left, bottom, right, top) in rects {
        points.extend([(left, bottom), (right, bottom), (right, top), (left, top)]);
    }
    let min_x = points
        .iter()
        .map(|point| point.0)
        .min()
        .expect("outline point");
    let min_y = points
        .iter()
        .map(|point| point.1)
        .min()
        .expect("outline point");
    let max_x = points
        .iter()
        .map(|point| point.0)
        .max()
        .expect("outline point");
    let max_y = points
        .iter()
        .map(|point| point.1)
        .max()
        .expect("outline point");
    let mut glyph = Vec::new();
    push_i16(
        &mut glyph,
        i16::try_from(rects.len()).expect("contour count fits"),
    );
    for bound in [min_x, min_y, max_x, max_y] {
        push_i16(&mut glyph, bound);
    }
    for contour in 0..rects.len() {
        push_u16(
            &mut glyph,
            u16::try_from(contour * 4 + 3).expect("point index fits"),
        );
    }
    push_u16(&mut glyph, 0); // instruction length
    glyph.extend(std::iter::repeat_n(1, points.len())); // every point is on-curve
    let mut previous = 0i16;
    for &(x, _) in &points {
        push_i16(&mut glyph, x - previous);
        previous = x;
    }
    previous = 0;
    for &(_, y) in &points {
        push_i16(&mut glyph, y - previous);
        previous = y;
    }
    glyph
}

fn repertoire_cmap(characters: &[char]) -> Vec<u8> {
    let segment_count = characters.len() + 1;
    let segment_count_u16 = u16::try_from(segment_count).expect("cmap segment count fits");
    let search_power = 1usize << segment_count.ilog2();
    let search_range = u16::try_from(search_power * 2).expect("cmap search range fits");
    let entry_selector = u16::try_from(search_power.ilog2()).expect("cmap selector fits");
    let range_shift = segment_count_u16 * 2 - search_range;
    let mut subtable = Vec::new();
    push_u16(&mut subtable, 4); // format 4
    push_u16(&mut subtable, 0); // length, filled below
    push_u16(&mut subtable, 0); // language
    push_u16(&mut subtable, segment_count_u16 * 2);
    push_u16(&mut subtable, search_range);
    push_u16(&mut subtable, entry_selector);
    push_u16(&mut subtable, range_shift);
    for character in characters {
        push_u16(&mut subtable, *character as u16);
    }
    push_u16(&mut subtable, 0xffff);
    push_u16(&mut subtable, 0); // reservedPad
    for character in characters {
        push_u16(&mut subtable, *character as u16);
    }
    push_u16(&mut subtable, 0xffff);
    for (index, character) in characters.iter().enumerate() {
        let glyph_index = u16::try_from(index + 1).expect("glyph index fits");
        push_u16(&mut subtable, glyph_index.wrapping_sub(*character as u16));
    }
    push_u16(&mut subtable, 1); // sentinel maps U+FFFF to glyph zero
    for _ in 0..segment_count {
        push_u16(&mut subtable, 0); // idRangeOffset
    }
    let length = u16::try_from(subtable.len()).expect("format-4 cmap length fits");
    subtable[2..4].copy_from_slice(&length.to_be_bytes());
    let mut cmap = Vec::new();
    push_u16(&mut cmap, 0); // version
    push_u16(&mut cmap, 1); // one encoding record
    push_u16(&mut cmap, 3); // Windows platform
    push_u16(&mut cmap, 1); // Unicode BMP encoding
    push_u32(&mut cmap, 12); // subtable offset
    cmap.extend_from_slice(&subtable);
    cmap
}

fn build_font(glyphs: Vec<Vec<u8>>, advances: Vec<u16>, cmap: Vec<u8>) -> Vec<u8> {
    let mut glyf = Vec::new();
    let mut loca = Vec::with_capacity(glyphs.len() + 1);
    for glyph in glyphs {
        loca.push(u16::try_from(glyf.len() / 2).expect("short loca offset fits"));
        glyf.extend_from_slice(&glyph);
        if glyf.len() % 2 != 0 {
            glyf.push(0);
        }
    }
    loca.push(u16::try_from(glyf.len() / 2).expect("short loca offset fits"));
    let mut loca_table = Vec::with_capacity(loca.len() * 2);
    for offset in loca {
        push_u16(&mut loca_table, offset);
    }
    let glyph_count = u16::try_from(advances.len()).expect("glyph count fits");
    let tables = vec![
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head()),
        (*b"hhea", hhea(glyph_count)),
        (*b"hmtx", hmtx(&advances)),
        (*b"kern", vec![0, 0, 0, 0]),
        (*b"loca", loca_table),
        (*b"maxp", maxp(glyph_count)),
    ];
    sfnt(tables)
}

fn hhea(metric_count: u16) -> Vec<u8> {
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
    push_u16(&mut table, metric_count);
    table
}

fn hmtx(advances: &[u16]) -> Vec<u8> {
    let mut table = Vec::with_capacity(advances.len() * 4);
    for advance in advances {
        push_u16(&mut table, *advance);
        push_i16(&mut table, 0);
    }
    table
}

fn maxp(glyph_count: u16) -> Vec<u8> {
    let mut table = Vec::with_capacity(32);
    push_u32(&mut table, 0x0001_0000);
    push_u16(&mut table, glyph_count);
    for value in [1024, 256, 2048, 512, 1, 0, 0, 0, 0, 0, 0, 2, 1] {
        push_u16(&mut table, value);
    }
    table
}
