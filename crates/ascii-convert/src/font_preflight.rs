//! Bounded preflight for the fontdue parser's cmap, kerning, and outline work.

extern crate alloc;

use alloc::vec::Vec;
use ascii_core::{BudgetKind, Error, MalformedReason, UnsupportedReason};
use ttf_parser::{Face, GlyphId, OutlineBuilder, RawFace, Tag};

const MAX_CMAP_RECORDS: usize = 64;
const MAX_CMAP_WORK: u64 = 1_000_000;
const MAX_FONT_PARSER_WORK: u64 = 16_000_000;
const MAX_FONT_PARSER_BYTES: u64 = 256 * 1024 * 1024;
const ESTIMATED_GLYPH_BYTES: u64 = 128;
const ESTIMATED_HASH_ENTRY_BYTES: u64 = 32;
const ESTIMATED_OUTLINE_LINE_BYTES: u64 = 128;
// The 64-segment curve stacks and estimator locals stay below this fixed allowance.
const CURVE_STACK_BYTES: u64 = 4096;
const GLYPH_SET_BITS: usize = u64::BITS as usize;
const UNICODE_SCALAR_LIMIT: u32 = 0x11_0000;

#[derive(Debug)]
pub(crate) struct FontPreflight {
    pub(crate) parser_bytes: u64,
    pub(crate) work_units: u64,
    pub(crate) raster_edge_work: Vec<u64>,
}

#[derive(Clone, Copy, Debug, Default)]
struct CmapStats {
    work_units: u64,
    mapped_codepoints: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct KernStats {
    work_units: u64,
    pairs: u64,
}

/// Validates parser iteration inputs and estimates fontdue's retained parser allocations.
pub(crate) fn preflight_font(
    bytes: &[u8],
    collection_index: u32,
    scale: f32,
    raster_size: f32,
    max_raster_bytes: u64,
    max_work_units: u64,
) -> Result<FontPreflight, Error> {
    let raw_face = RawFace::parse(bytes, collection_index)
        .map_err(|_| Error::Malformed(MalformedReason::InvalidEncodedData))?;
    let maxp = raw_face
        .table(Tag::from_bytes(b"maxp"))
        .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
    if maxp.len() < 6 {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let glyph_count = usize::from(read_u16(maxp, 4)?);
    if glyph_count == 0 {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let glyph_count_u64 = u64::try_from(glyph_count).map_err(|_| Error::SizeOverflow)?;

    let cmap_stats = raw_face
        .table(Tag::from_bytes(b"cmap"))
        .map(|cmap| inspect_cmap(cmap, max_work_units.min(MAX_FONT_PARSER_WORK / 2)))
        .transpose()?
        .unwrap_or_default();
    let kern_work_limit = max_work_units
        .min(MAX_FONT_PARSER_WORK / 2)
        .checked_sub(cmap_stats.work_units)
        .ok_or(Error::BudgetExceeded(BudgetKind::WorkUnits))?;
    let kern_stats = raw_face
        .table(Tag::from_bytes(b"kern"))
        .map(|kern| inspect_kern(kern, kern_work_limit))
        .transpose()?
        .unwrap_or_default();
    let pass_work = cmap_stats
        .work_units
        .checked_add(kern_stats.work_units)
        .ok_or(Error::SizeOverflow)?;
    let double_pass_work = pass_work.checked_mul(2).ok_or(Error::SizeOverflow)?;
    let fixed_work = double_pass_work
        .checked_add(glyph_count_u64)
        .ok_or(Error::SizeOverflow)?;
    if fixed_work > max_work_units || fixed_work > MAX_FONT_PARSER_WORK {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }

    let face = Face::parse(bytes, collection_index)
        .map_err(|_| Error::Malformed(MalformedReason::InvalidEncodedData))?;
    let units_per_em = face.units_per_em();
    if !(16..=16_384).contains(&units_per_em)
        || !scale.is_finite()
        || scale <= 0.0
        || !raster_size.is_finite()
        || raster_size <= 0.0
    {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let max_area = 6.0 * (f32::from(units_per_em) / scale);
    if !max_area.is_finite() || max_area <= 0.0 {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }

    let glyph_set_len = glyph_count
        .checked_add(GLYPH_SET_BITS - 1)
        .ok_or(Error::SizeOverflow)?
        / GLYPH_SET_BITS;
    let glyph_set_bytes = u64::try_from(glyph_set_len)
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(u64::try_from(core::mem::size_of::<u64>()).map_err(|_| Error::SizeOverflow)?)
        .ok_or(Error::SizeOverflow)?;
    let raster_edge_bytes = glyph_count_u64
        .checked_mul(u64::try_from(core::mem::size_of::<u64>()).map_err(|_| Error::SizeOverflow)?)
        .ok_or(Error::SizeOverflow)?;
    let fixed_scratch_bytes = glyph_set_bytes
        .checked_add(CURVE_STACK_BYTES)
        .and_then(|value| value.checked_add(raster_edge_bytes))
        .ok_or(Error::SizeOverflow)?;
    if fixed_scratch_bytes > max_raster_bytes || fixed_scratch_bytes > MAX_FONT_PARSER_BYTES {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
    }
    let mut glyph_set = Vec::new();
    glyph_set
        .try_reserve_exact(glyph_set_len)
        .map_err(|_| Error::AllocationFailed)?;
    glyph_set.resize(glyph_set_len, 0u64);
    let mut raster_edge_work = Vec::new();
    raster_edge_work
        .try_reserve_exact(glyph_count)
        .map_err(|_| Error::AllocationFailed)?;
    raster_edge_work.resize(glyph_count, 0u64);
    glyph_set[0] = 1;
    let mut mapped_codepoints = 0u64;
    let mut invalid_mapping = false;
    if let Some(cmap) = face.tables().cmap {
        for subtable in cmap.subtables {
            subtable.codepoints(|codepoint| {
                if char::from_u32(codepoint).is_none() {
                    invalid_mapping = true;
                    return;
                }
                let Some(glyph_id) = subtable.glyph_index(codepoint) else {
                    return;
                };
                let index = usize::from(glyph_id.0);
                if index >= glyph_count {
                    invalid_mapping = true;
                } else if glyph_id.0 != 0 {
                    mapped_codepoints = mapped_codepoints.saturating_add(1);
                    glyph_set[index / GLYPH_SET_BITS] |= 1u64 << (index % GLYPH_SET_BITS);
                }
            });
        }
    }
    if invalid_mapping || mapped_codepoints > MAX_CMAP_WORK {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }

    let remaining_work = max_work_units
        .checked_sub(fixed_work)
        .ok_or(Error::BudgetExceeded(BudgetKind::WorkUnits))?
        .min(MAX_FONT_PARSER_WORK.saturating_sub(fixed_work));
    let outline_work_limit = remaining_work / 2;
    let mut outlines = OutlineEstimate::new(
        max_area,
        outline_work_limit,
        raster_size / f32::from(units_per_em),
    );
    for index in 0..glyph_count {
        if glyph_set[index / GLYPH_SET_BITS] & (1u64 << (index % GLYPH_SET_BITS)) == 0 {
            continue;
        }
        outlines.raster_edge_work = 0;
        face.outline_glyph(
            GlyphId(u16::try_from(index).map_err(|_| Error::SizeOverflow)?),
            &mut outlines,
        );
        if outlines.exceeded {
            return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
        }
        if outlines.invalid {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        raster_edge_work[index] = outlines.raster_edge_work;
    }
    let font_len = u64::try_from(bytes.len()).map_err(|_| Error::SizeOverflow)?;
    let hash_entries = glyph_count_u64
        .max(mapped_codepoints)
        .checked_add(glyph_count_u64)
        .and_then(|value| value.checked_add(kern_stats.pairs))
        .ok_or(Error::SizeOverflow)?;
    let parser_bytes = font_len
        .checked_add(
            glyph_count_u64
                .checked_mul(ESTIMATED_GLYPH_BYTES)
                .ok_or(Error::SizeOverflow)?,
        )
        .and_then(|value| {
            hash_entries
                .checked_mul(ESTIMATED_HASH_ENTRY_BYTES)
                .and_then(|entries| value.checked_add(entries))
        })
        .and_then(|value| {
            outlines
                .line_count
                .checked_mul(ESTIMATED_OUTLINE_LINE_BYTES)
                .and_then(|lines| value.checked_add(lines))
        })
        .and_then(|value| value.checked_add(fixed_scratch_bytes))
        .ok_or(Error::SizeOverflow)?;
    if parser_bytes > max_raster_bytes || parser_bytes > MAX_FONT_PARSER_BYTES {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
    }

    let outline_double_pass = outlines
        .work_units
        .checked_mul(2)
        .ok_or(Error::SizeOverflow)?;
    let work_units = fixed_work
        .checked_add(outline_double_pass)
        .ok_or(Error::SizeOverflow)?;
    if work_units > max_work_units || work_units > MAX_FONT_PARSER_WORK {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }

    Ok(FontPreflight {
        parser_bytes,
        work_units,
        raster_edge_work,
    })
}

fn inspect_cmap(cmap: &[u8], max_work_units: u64) -> Result<CmapStats, Error> {
    if read_u16(cmap, 0)? != 0 {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let record_count = usize::from(read_u16(cmap, 2)?);
    if record_count > MAX_CMAP_RECORDS {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }
    let records_end = 4usize
        .checked_add(record_count.checked_mul(8).ok_or(Error::SizeOverflow)?)
        .ok_or(Error::SizeOverflow)?;
    if records_end > cmap.len() {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }

    let work_limit = max_work_units.min(MAX_CMAP_WORK);
    let mut stats = CmapStats::default();
    charge(
        &mut stats.work_units,
        u64::try_from(record_count).map_err(|_| Error::SizeOverflow)?,
        work_limit,
    )?;
    for index in 0..record_count {
        let record = 4 + index * 8;
        let offset =
            usize::try_from(read_u32(cmap, record + 4)?).map_err(|_| Error::SizeOverflow)?;
        let format = read_u16(cmap, offset)?;
        let (subtable_len, work, mapped) = match format {
            0 => inspect_format_zero(cmap, offset)?,
            2 => return Err(Error::Unsupported(UnsupportedReason::UnsupportedFormat)),
            4 => inspect_format_four(cmap, offset)?,
            6 => inspect_format_six(cmap, offset)?,
            10 => inspect_format_ten(cmap, offset)?,
            12 | 13 => inspect_format_twelve_or_thirteen(cmap, offset)?,
            8 | 14 => (read_format_length(cmap, offset, format)?, 0, 0),
            _ => (2, 0, 0),
        };
        let end = offset
            .checked_add(subtable_len)
            .ok_or(Error::SizeOverflow)?;
        if end > cmap.len() {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        charge(&mut stats.work_units, work, work_limit)?;
        stats.mapped_codepoints = stats
            .mapped_codepoints
            .checked_add(mapped)
            .ok_or(Error::SizeOverflow)?;
    }
    Ok(stats)
}

fn inspect_format_zero(cmap: &[u8], offset: usize) -> Result<(usize, u64, u64), Error> {
    let len = read_format_length(cmap, offset, 0)?;
    if len < 262 || offset.checked_add(len).ok_or(Error::SizeOverflow)? > cmap.len() {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let mappings = cmap[offset + 6..offset + 262]
        .iter()
        .filter(|glyph| **glyph != 0)
        .count();
    Ok((
        len,
        256,
        u64::try_from(mappings).map_err(|_| Error::SizeOverflow)?,
    ))
}

fn inspect_format_four(cmap: &[u8], offset: usize) -> Result<(usize, u64, u64), Error> {
    let len = read_format_length(cmap, offset, 4)?;
    if len < 16 || offset.checked_add(len).ok_or(Error::SizeOverflow)? > cmap.len() {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let segment_count_x2 = usize::from(read_u16(cmap, offset + 6)?);
    if segment_count_x2 == 0 || segment_count_x2 % 2 != 0 {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let segment_count = segment_count_x2 / 2;
    let arrays_end = 16usize
        .checked_add(segment_count.checked_mul(8).ok_or(Error::SizeOverflow)?)
        .ok_or(Error::SizeOverflow)?;
    if arrays_end > len {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let ends = offset + 14;
    let starts = offset + 16 + segment_count * 2;
    let mut work = 0u64;
    for segment in 0..segment_count {
        let start = u32::from(read_u16(cmap, starts + segment * 2)?);
        let end = u32::from(read_u16(cmap, ends + segment * 2)?);
        if start > end {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        if start == 0xffff && end == 0xffff {
            continue;
        }
        validate_scalar_range(start, end)?;
        work = work
            .checked_add(u64::from(end - start) + 1)
            .ok_or(Error::SizeOverflow)?;
    }
    Ok((len, work, work))
}

fn inspect_format_six(cmap: &[u8], offset: usize) -> Result<(usize, u64, u64), Error> {
    let len = read_format_length(cmap, offset, 6)?;
    if len < 10 || offset.checked_add(len).ok_or(Error::SizeOverflow)? > cmap.len() {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let first = u32::from(read_u16(cmap, offset + 6)?);
    let count = u32::from(read_u16(cmap, offset + 8)?);
    let expected = 10usize
        .checked_add(
            usize::try_from(count)
                .map_err(|_| Error::SizeOverflow)?
                .checked_mul(2)
                .ok_or(Error::SizeOverflow)?,
        )
        .ok_or(Error::SizeOverflow)?;
    if expected > len {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    if count == 0 {
        return Ok((len, 0, 0));
    }
    let end = first.checked_add(count - 1).ok_or(Error::SizeOverflow)?;
    validate_scalar_range(first, end)?;
    Ok((len, u64::from(count), u64::from(count)))
}

fn inspect_format_ten(cmap: &[u8], offset: usize) -> Result<(usize, u64, u64), Error> {
    let len = read_format_length(cmap, offset, 10)?;
    if len < 20 || offset.checked_add(len).ok_or(Error::SizeOverflow)? > cmap.len() {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let first = read_u32(cmap, offset + 12)?;
    let count = read_u32(cmap, offset + 16)?;
    let expected = 20usize
        .checked_add(
            usize::try_from(count)
                .map_err(|_| Error::SizeOverflow)?
                .checked_mul(2)
                .ok_or(Error::SizeOverflow)?,
        )
        .ok_or(Error::SizeOverflow)?;
    if expected > len {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    if count == 0 {
        return Ok((len, 0, 0));
    }
    let end = first.checked_add(count - 1).ok_or(Error::SizeOverflow)?;
    validate_scalar_range(first, end)?;
    Ok((len, u64::from(count), u64::from(count)))
}

fn inspect_format_twelve_or_thirteen(
    cmap: &[u8],
    offset: usize,
) -> Result<(usize, u64, u64), Error> {
    let format = read_u16(cmap, offset)?;
    let len = read_format_length(cmap, offset, format)?;
    if len < 16 || offset.checked_add(len).ok_or(Error::SizeOverflow)? > cmap.len() {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let group_count =
        usize::try_from(read_u32(cmap, offset + 12)?).map_err(|_| Error::SizeOverflow)?;
    let expected = 16usize
        .checked_add(group_count.checked_mul(12).ok_or(Error::SizeOverflow)?)
        .ok_or(Error::SizeOverflow)?;
    if expected > len {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let mut work = u64::try_from(group_count).map_err(|_| Error::SizeOverflow)?;
    for group in 0..group_count {
        let start = read_u32(cmap, offset + 16 + group * 12)?;
        let end = read_u32(cmap, offset + 20 + group * 12)?;
        validate_scalar_range(start, end)?;
        work = work
            .checked_add(u64::from(end - start) + 1)
            .ok_or(Error::SizeOverflow)?;
    }
    Ok((len, work, work))
}

fn inspect_kern(kern: &[u8], max_work_units: u64) -> Result<KernStats, Error> {
    let version = read_u16(kern, 0)?;
    let (mut offset, subtable_count) = match version {
        0 => (4usize, u32::from(read_u16(kern, 2)?)),
        1 => (8usize, read_u32(kern, 4)?),
        _ => return Ok(KernStats::default()),
    };
    if subtable_count > 1024 {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }
    let mut stats = KernStats::default();
    for _ in 0..subtable_count {
        let (subtable_len, format, horizontal, header_len) = if version == 0 {
            let len = usize::from(read_u16(kern, offset + 2)?);
            let format = *kern
                .get(offset + 4)
                .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
            let coverage = *kern
                .get(offset + 5)
                .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
            (len, format, coverage & 1 != 0, 6usize)
        } else {
            let len = usize::try_from(read_u32(kern, offset)?).map_err(|_| Error::SizeOverflow)?;
            let coverage = *kern
                .get(offset + 4)
                .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
            let format = *kern
                .get(offset + 5)
                .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
            (len, format, coverage & 0x80 == 0, 8usize)
        };
        if subtable_len < header_len {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        let end = offset
            .checked_add(subtable_len)
            .ok_or(Error::SizeOverflow)?;
        if end > kern.len() {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        if horizontal && format == 3 {
            return Err(Error::Unsupported(UnsupportedReason::UnsupportedFormat));
        }
        if horizontal && format == 0 {
            let body = offset + header_len;
            let pairs = u64::from(read_u16(kern, body)?);
            let expected = header_len
                .checked_add(8)
                .and_then(|value| value.checked_add(usize::try_from(pairs).ok()?.checked_mul(6)?))
                .ok_or(Error::SizeOverflow)?;
            if expected > subtable_len {
                return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
            }
            stats.pairs = stats.pairs.checked_add(pairs).ok_or(Error::SizeOverflow)?;
            stats.work_units = stats
                .work_units
                .checked_add(pairs)
                .ok_or(Error::SizeOverflow)?;
            if stats.work_units > max_work_units {
                return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
            }
        }
        offset = end;
    }
    Ok(stats)
}

fn read_format_length(cmap: &[u8], offset: usize, format: u16) -> Result<usize, Error> {
    match format {
        8 | 10 | 12 | 13 => {
            usize::try_from(read_u32(cmap, offset + 4)?).map_err(|_| Error::SizeOverflow)
        }
        14 => usize::try_from(read_u32(cmap, offset + 2)?).map_err(|_| Error::SizeOverflow),
        _ => Ok(usize::from(read_u16(cmap, offset + 2)?)),
    }
}

fn validate_scalar_range(start: u32, end: u32) -> Result<(), Error> {
    if start > end || end >= UNICODE_SCALAR_LIMIT || (start <= 0xdfff && end >= 0xd800) {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let raw = bytes
        .get(offset..offset.checked_add(2).ok_or(Error::SizeOverflow)?)
        .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
    Ok(u16::from_be_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let raw = bytes
        .get(offset..offset.checked_add(4).ok_or(Error::SizeOverflow)?)
        .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
    Ok(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn charge(total: &mut u64, amount: u64, limit: u64) -> Result<(), Error> {
    *total = total.checked_add(amount).ok_or(Error::SizeOverflow)?;
    if *total > limit {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default)]
struct Point {
    x: f32,
    y: f32,
}

impl Point {
    const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Segment {
    start: Point,
    end: Point,
    t_start: f32,
    t_end: f32,
}

#[derive(Debug)]
struct OutlineEstimate {
    previous: Point,
    start: Point,
    max_area: f32,
    work_limit: u64,
    work_units: u64,
    line_count: u64,
    raster_scale: f64,
    raster_edge_work: u64,
    invalid: bool,
    exceeded: bool,
}

impl OutlineEstimate {
    fn new(max_area: f32, work_limit: u64, raster_scale: f32) -> Self {
        Self {
            previous: Point::default(),
            start: Point::default(),
            max_area,
            work_limit,
            work_units: 0,
            line_count: 0,
            raster_scale: f64::from(raster_scale),
            raster_edge_work: 0,
            invalid: false,
            exceeded: false,
        }
    }

    fn charge(&mut self, amount: u64) {
        if self.exceeded {
            return;
        }
        match self.work_units.checked_add(amount) {
            Some(work) if work <= self.work_limit => self.work_units = work,
            _ => self.exceeded = true,
        }
    }

    fn push_line(&mut self, start: Point, end: Point) {
        if self.exceeded || self.invalid {
            return;
        }
        if !start.x.is_finite() || !start.y.is_finite() || !end.x.is_finite() || !end.y.is_finite()
        {
            self.invalid = true;
            return;
        }
        if start.y.to_bits() != end.y.to_bits() {
            match self.line_count.checked_add(1) {
                Some(count) => self.line_count = count,
                None => self.exceeded = true,
            }

            // fontdue's v_line walks Y, and m_line walks X plus Y, then each
            // performs a final add. With the caller's 16,384-pixel dimension
            // limit, repositioning, f32 scaling/offset and one-ULP nudging
            // introduce less than one pixel of error per axis. Four extra
            // steps per axis also cover truncation to pixel coordinates.
            let y = self.scaled_span(start.y, end.y);
            let edge_work = if start.x.to_bits() == end.x.to_bits() {
                y.and_then(|value| value.checked_add(5))
            } else {
                self.scaled_span(start.x, end.x)
                    .and_then(|x| y.and_then(|y| x.checked_add(y)))
                    .and_then(|value| value.checked_add(9))
            };
            // An unrequested glyph may overflow this estimate. Preserve that
            // fact per glyph; the caller returns SizeOverflow if it is used.
            self.raster_edge_work = edge_work
                .and_then(|value| self.raster_edge_work.checked_add(value))
                .unwrap_or(u64::MAX);
        }
    }

    fn scaled_span(&self, start: f32, end: f32) -> Option<u64> {
        let span = ((f64::from(start) - f64::from(end)).abs() * self.raster_scale).ceil();
        if !span.is_finite() || span >= u64::MAX as f64 {
            None
        } else {
            Some(span as u64)
        }
    }

    fn tessellate_quadratic(&mut self, control: Point, end: Point) {
        let origin = self.previous;
        let mut stack = [Segment::default(); 64];
        let mut stack_len = 1usize;
        stack[0] = Segment {
            start: origin,
            end,
            t_start: 0.0,
            t_end: 1.0,
        };
        while stack_len != 0 && !self.exceeded {
            stack_len -= 1;
            let segment = stack[stack_len];
            self.charge(1);
            if self.exceeded {
                break;
            }
            let middle_t = (segment.t_start + segment.t_end) * 0.5;
            let middle = quadratic_point(origin, control, end, middle_t);
            let area = (middle.x - segment.start.x) * (segment.end.y - segment.start.y)
                - (segment.end.x - segment.start.x) * (middle.y - segment.start.y);
            if area.abs() > self.max_area {
                if middle_t == segment.t_start
                    || middle_t == segment.t_end
                    || stack_len + 2 > stack.len()
                {
                    self.exceeded = true;
                    break;
                }
                stack[stack_len] = Segment {
                    start: middle,
                    end: segment.end,
                    t_start: middle_t,
                    t_end: segment.t_end,
                };
                stack_len += 1;
                stack[stack_len] = Segment {
                    start: segment.start,
                    end: middle,
                    t_start: segment.t_start,
                    t_end: middle_t,
                };
                stack_len += 1;
            } else {
                self.push_line(segment.start, segment.end);
            }
        }
        self.previous = end;
    }

    fn tessellate_cubic(&mut self, first: Point, second: Point, end: Point) {
        let origin = self.previous;
        let mut stack = [Segment::default(); 64];
        let mut stack_len = 1usize;
        stack[0] = Segment {
            start: origin,
            end,
            t_start: 0.0,
            t_end: 1.0,
        };
        while stack_len != 0 && !self.exceeded {
            stack_len -= 1;
            let segment = stack[stack_len];
            self.charge(1);
            if self.exceeded {
                break;
            }
            let middle_t = (segment.t_start + segment.t_end) * 0.5;
            let middle = cubic_point(origin, first, second, end, middle_t);
            let area = (middle.x - segment.start.x) * (segment.end.y - segment.start.y)
                - (segment.end.x - segment.start.x) * (middle.y - segment.start.y);
            if area.abs() > self.max_area {
                if middle_t == segment.t_start
                    || middle_t == segment.t_end
                    || stack_len + 2 > stack.len()
                {
                    self.exceeded = true;
                    break;
                }
                stack[stack_len] = Segment {
                    start: middle,
                    end: segment.end,
                    t_start: middle_t,
                    t_end: segment.t_end,
                };
                stack_len += 1;
                stack[stack_len] = Segment {
                    start: segment.start,
                    end: middle,
                    t_start: segment.t_start,
                    t_end: middle_t,
                };
                stack_len += 1;
            } else {
                self.push_line(segment.start, segment.end);
            }
        }
        self.previous = end;
    }
}

impl OutlineBuilder for OutlineEstimate {
    fn move_to(&mut self, x: f32, y: f32) {
        self.charge(1);
        self.previous = Point::new(x, y);
        self.start = self.previous;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.charge(1);
        let end = Point::new(x, y);
        self.push_line(self.previous, end);
        self.previous = end;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.tessellate_quadratic(Point::new(x1, y1), Point::new(x, y));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.tessellate_cubic(Point::new(x1, y1), Point::new(x2, y2), Point::new(x, y));
    }

    fn close(&mut self) {
        self.charge(1);
        if self.start.x != self.previous.x || self.start.y != self.previous.y {
            self.push_line(self.previous, self.start);
        }
        self.previous = self.start;
    }
}

fn quadratic_point(origin: Point, control: Point, end: Point, t: f32) -> Point {
    let tm = 1.0 - t;
    let a = tm * tm;
    let b = 2.0 * tm * t;
    let c = t * t;
    Point::new(
        a * origin.x + b * control.x + c * end.x,
        a * origin.y + b * control.y + c * end.y,
    )
}

fn cubic_point(origin: Point, first: Point, second: Point, end: Point, t: f32) -> Point {
    let tm = 1.0 - t;
    let a = tm * tm * tm;
    let b = 3.0 * (tm * tm) * t;
    let c = 3.0 * tm * (t * t);
    let d = t * t * t;
    Point::new(
        a * origin.x + b * first.x + c * second.x + d * end.x,
        a * origin.y + b * first.y + c * second.y + d * end.y,
    )
}

#[cfg(test)]
mod tests {
    use super::{OutlineBuilder, OutlineEstimate, Point};

    #[test]
    fn raster_edge_estimate_covers_both_line_kinds_and_closing_edges() {
        let mut outline = OutlineEstimate::new(150.0, 100, 1.0);
        outline.move_to(0.0, 0.0);
        outline.line_to(8.0, 12.0);
        outline.line_to(8.0, 20.0);
        outline.line_to(2.0, 20.0); // horizontal edges are not retained
        outline.close();

        assert_eq!(outline.line_count, 3);
        assert_eq!(outline.raster_edge_work, 29 + 13 + 31);
    }

    #[test]
    fn cubic_segments_are_charged_at_the_requested_scale() {
        let mut small = OutlineEstimate::new(150.0, 10_000, 0.008);
        let mut large = OutlineEstimate::new(150.0, 10_000, 0.128);
        for outline in [&mut small, &mut large] {
            outline.move_to(0.0, 0.0);
            outline.curve_to(0.0, 700.0, 500.0, 700.0, 500.0, 0.0);
            outline.close();
        }
        assert!(small.line_count > 1);
        assert_eq!(small.line_count, large.line_count);
        assert!(large.raster_edge_work > small.raster_edge_work);
        assert!(!small.exceeded && !large.exceeded);
    }

    #[test]
    fn overflowing_edge_estimate_is_preserved_for_the_requested_glyph() {
        let mut outline = OutlineEstimate::new(150.0, 10, 256.0);
        outline.push_line(Point::new(0.0, 0.0), Point::new(f32::MAX, f32::MAX));
        assert_eq!(outline.raster_edge_work, u64::MAX);
    }
}
