//! Bounded reader for the draft ASCII clip v1 container.
use crate::clip::{
    ClipGlyphTable, ClipGlyphTableError, MAX_CLIP_GLYPH_COUNT, parse_clip_glyph_table,
};
use std::fmt;

const HEADER_LEN: usize = 64;
const FRAME_INDEX_ENTRY_LEN: usize = 40;
const CELL_LEN: usize = 12;
const MAX_CELLS_PER_FRAME: u64 = 1_048_576;
const MAX_FRAME_COUNT: u64 = 65_536;
const MAX_DECODED_BYTES: u64 = 64 * 1024 * 1024;

/// A decoded ASCII clip v1 with validated glyph identities and complete frames.
#[derive(Debug, Eq, PartialEq)]
pub struct AsciiClip {
    columns: u32,
    rows: u32,
    glyphs: ClipGlyphTable,
    frames: Vec<ClipFrame>,
}

impl AsciiClip {
    /// Returns the number of columns in each frame.
    #[must_use]
    pub const fn columns(&self) -> u32 {
        self.columns
    }

    /// Returns the number of rows in each frame.
    #[must_use]
    pub const fn rows(&self) -> u32 {
        self.rows
    }

    /// Returns the supported glyphs in serialized table order.
    #[must_use]
    pub fn glyphs(&self) -> &[char] {
        self.glyphs.glyphs()
    }

    /// Returns decoded complete frames in presentation order.
    #[must_use]
    pub fn frames(&self) -> &[ClipFrame] {
        &self.frames
    }
}

/// A decoded, validated complete clip frame.
#[derive(Debug, Eq, PartialEq)]
pub struct ClipFrame {
    pts_us: i64,
    duration_us: u64,
    cells: Vec<ClipCell>,
}

impl ClipFrame {
    /// Returns this frame's presentation timestamp in microseconds.
    #[must_use]
    pub const fn pts_us(&self) -> i64 {
        self.pts_us
    }

    /// Returns this frame's duration in microseconds.
    #[must_use]
    pub const fn duration_us(&self) -> u64 {
        self.duration_us
    }

    /// Returns cells in row-major order.
    #[must_use]
    pub fn cells(&self) -> &[ClipCell] {
        &self.cells
    }
}

/// A clip cell referencing the validated glyph table and carrying RGBA colors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipCell {
    glyph_index: u32,
    foreground: [u8; 4],
    background: [u8; 4],
}

impl ClipCell {
    /// Returns this cell's index into [`AsciiClip::glyphs`].
    #[must_use]
    pub const fn glyph_index(self) -> u32 {
        self.glyph_index
    }

    /// Returns this cell's foreground RGBA value.
    #[must_use]
    pub const fn foreground(self) -> [u8; 4] {
        self.foreground
    }

    /// Returns this cell's background RGBA value.
    #[must_use]
    pub const fn background(self) -> [u8; 4] {
        self.background
    }
}

/// A malformed, unsupported, or over-budget ASCII clip v1 payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipReadError {
    TruncatedHeader {
        available: usize,
    },
    InvalidMagic,
    UnsupportedVersion {
        version: u16,
    },
    UnsupportedFlags {
        flags: u16,
    },
    InvalidHeaderLength {
        length: u32,
    },
    InvalidDimensions {
        columns: u32,
        rows: u32,
    },
    InvalidTimeBase {
        numerator: u32,
        denominator: u32,
    },
    InvalidReservedHeader {
        value: u32,
    },
    FrameCountLimitExceeded {
        count: u64,
        maximum: u64,
    },
    InvalidOffset {
        section: &'static str,
        offset: u64,
    },
    TruncatedSection {
        section: &'static str,
        offset: u64,
        length: u64,
    },
    OverlappingSections {
        first: &'static str,
        second: &'static str,
    },
    InvalidGlyphTable(ClipGlyphTableError),
    FrameIndexLengthOverflow,
    InvalidFrame {
        index: u64,
        reason: ClipFrameError,
    },
    DecodedLimitExceeded {
        bytes: u64,
        maximum: u64,
    },
    AllocationFailed,
}

/// The reason a frame index entry or its payload is invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipFrameError {
    ZeroDuration,
    NonIncreasingTimestamp,
    InvalidReserved,
    UnsupportedCodec { codec: u8 },
    UnsupportedFrameKind { kind: u8 },
    RawLengthMismatch { declared: u32, expected: u64 },
    StoredLengthMismatch { declared: u32, expected: u32 },
    PayloadOutOfBounds,
    PayloadOverlap,
    ChecksumMismatch { expected: u32, actual: u32 },
    GlyphIndexOutOfBounds { cell: u64, glyph_index: u32 },
}

impl fmt::Display for ClipReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedHeader { available } => write!(
                formatter,
                "ASCII clip header has {available} bytes; {HEADER_LEN} required"
            ),
            Self::InvalidMagic => formatter.write_str("invalid ASCII clip magic"),
            Self::UnsupportedVersion { version } => {
                write!(formatter, "unsupported ASCII clip version {version}")
            }
            Self::UnsupportedFlags { flags } => {
                write!(formatter, "unsupported ASCII clip flags 0x{flags:04X}")
            }
            Self::InvalidHeaderLength { length } => {
                write!(formatter, "invalid ASCII clip header length {length}")
            }
            Self::InvalidDimensions { columns, rows } => {
                write!(formatter, "invalid ASCII clip dimensions {columns}x{rows}")
            }
            Self::InvalidTimeBase {
                numerator,
                denominator,
            } => write!(
                formatter,
                "unsupported ASCII clip time base {numerator}/{denominator}"
            ),
            Self::InvalidReservedHeader { value } => write!(
                formatter,
                "ASCII clip reserved header field is {value}, expected zero"
            ),
            Self::FrameCountLimitExceeded { count, maximum } => write!(
                formatter,
                "ASCII clip frame count {count} exceeds limit {maximum}"
            ),
            Self::InvalidOffset { section, offset } => {
                write!(formatter, "invalid {section} offset {offset}")
            }
            Self::TruncatedSection {
                section,
                offset,
                length,
            } => write!(
                formatter,
                "{section} at {offset} with length {length} extends past the clip"
            ),
            Self::OverlappingSections { first, second } => {
                write!(formatter, "ASCII clip {first} overlaps {second}")
            }
            Self::InvalidGlyphTable(error) => {
                write!(formatter, "invalid ASCII clip glyph table: {error}")
            }
            Self::FrameIndexLengthOverflow => {
                formatter.write_str("ASCII clip frame-index length overflows")
            }
            Self::InvalidFrame { index, reason } => {
                write!(formatter, "invalid ASCII clip frame {index}: {reason}")
            }
            Self::DecodedLimitExceeded { bytes, maximum } => write!(
                formatter,
                "ASCII clip decoded data size {bytes} exceeds limit {maximum}"
            ),
            Self::AllocationFailed => {
                formatter.write_str("could not allocate bounded ASCII clip data")
            }
        }
    }
}

impl fmt::Display for ClipFrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDuration => formatter.write_str("duration is zero"),
            Self::NonIncreasingTimestamp => {
                formatter.write_str("presentation timestamps are not increasing")
            }
            Self::InvalidReserved => formatter.write_str("reserved index field is nonzero"),
            Self::UnsupportedCodec { codec } => {
                write!(formatter, "codec {codec} is not supported by the v1 reader")
            }
            Self::UnsupportedFrameKind { kind } => {
                write!(formatter, "frame kind {kind} is not supported by v1")
            }
            Self::RawLengthMismatch { declared, expected } => write!(
                formatter,
                "raw length {declared} does not match expected {expected}"
            ),
            Self::StoredLengthMismatch { declared, expected } => write!(
                formatter,
                "stored length {declared} does not match expected {expected}"
            ),
            Self::PayloadOutOfBounds => formatter.write_str("payload is outside the clip"),
            Self::PayloadOverlap => {
                formatter.write_str("payload overlaps a protected section or another payload")
            }
            Self::ChecksumMismatch { expected, actual } => {
                write!(formatter, "CRC-32 is {actual:08X}, expected {expected:08X}")
            }
            Self::GlyphIndexOutOfBounds { cell, glyph_index } => write!(
                formatter,
                "cell {cell} references glyph {glyph_index} outside the glyph table"
            ),
        }
    }
}

impl std::error::Error for ClipReadError {}

/// Parses and fully validates an uncompressed ASCII clip v1 payload.
///
/// The reader honors the format's offsets, checks every protected range and CRC-32, and decodes
/// only complete frames. Zstandard (`codec=1`) is rejected because v1 has no enabled compression
/// feature in its zero-flags header. The decoded-memory ceiling is independent of input size.
pub fn parse_ascii_clip(bytes: &[u8]) -> Result<AsciiClip, ClipReadError> {
    if bytes.len() < HEADER_LEN {
        return Err(ClipReadError::TruncatedHeader {
            available: bytes.len(),
        });
    }
    if &bytes[..8] != b"ASCIICL1" {
        return Err(ClipReadError::InvalidMagic);
    }
    let version = u16_at(bytes, 8);
    if version != 1 {
        return Err(ClipReadError::UnsupportedVersion { version });
    }
    let flags = u16_at(bytes, 10);
    if flags != 0 {
        return Err(ClipReadError::UnsupportedFlags { flags });
    }
    let header_length = u32_at(bytes, 12);
    if header_length != HEADER_LEN as u32 {
        return Err(ClipReadError::InvalidHeaderLength {
            length: header_length,
        });
    }
    let columns = u32_at(bytes, 16);
    let rows = u32_at(bytes, 20);
    if columns == 0 || rows == 0 {
        return Err(ClipReadError::InvalidDimensions { columns, rows });
    }
    let glyph_count = u32_at(bytes, 24);
    let time_numerator = u32_at(bytes, 28);
    let time_denominator = u32_at(bytes, 32);
    if time_numerator != 1 || time_denominator != 1_000_000 {
        return Err(ClipReadError::InvalidTimeBase {
            numerator: time_numerator,
            denominator: time_denominator,
        });
    }
    let frame_count = u64_at(bytes, 36);
    if frame_count > MAX_FRAME_COUNT {
        return Err(ClipReadError::FrameCountLimitExceeded {
            count: frame_count,
            maximum: MAX_FRAME_COUNT,
        });
    }
    let glyph_offset = usize_offset(u64_at(bytes, 44), "glyph table")?;
    let index_offset = usize_offset(u64_at(bytes, 52), "frame index")?;
    let reserved = u32_at(bytes, 60);
    if reserved != 0 {
        return Err(ClipReadError::InvalidReservedHeader { value: reserved });
    }

    if glyph_count > MAX_CLIP_GLYPH_COUNT {
        return Err(ClipReadError::InvalidGlyphTable(
            ClipGlyphTableError::GlyphCountLimitExceeded {
                count: glyph_count,
                maximum: MAX_CLIP_GLYPH_COUNT,
            },
        ));
    }

    let cell_count = u64::from(columns)
        .checked_mul(u64::from(rows))
        .ok_or(ClipReadError::InvalidDimensions { columns, rows })?;
    if cell_count == 0 || cell_count > MAX_CELLS_PER_FRAME {
        return Err(ClipReadError::DecodedLimitExceeded {
            bytes: cell_count.saturating_mul(CELL_LEN as u64),
            maximum: MAX_CELLS_PER_FRAME * CELL_LEN as u64,
        });
    }
    let raw_length = cell_count
        .checked_mul(CELL_LEN as u64)
        .ok_or(ClipReadError::FrameIndexLengthOverflow)?;
    let index_length = frame_count
        .checked_mul(FRAME_INDEX_ENTRY_LEN as u64)
        .ok_or(ClipReadError::FrameIndexLengthOverflow)?;
    let index_end = index_offset
        .checked_add(
            usize::try_from(index_length).map_err(|_| ClipReadError::FrameIndexLengthOverflow)?,
        )
        .ok_or(ClipReadError::FrameIndexLengthOverflow)?;
    if index_offset < HEADER_LEN {
        return Err(ClipReadError::InvalidOffset {
            section: "frame index",
            offset: index_offset as u64,
        });
    }
    if index_end > bytes.len() {
        return Err(ClipReadError::TruncatedSection {
            section: "frame index",
            offset: index_offset as u64,
            length: index_length,
        });
    }

    let table_end = glyph_table_end(bytes, glyph_offset, glyph_count)?;
    let glyph_table = parse_clip_glyph_table(&bytes[glyph_offset..table_end], glyph_count)
        .map_err(ClipReadError::InvalidGlyphTable)?;
    let header_range = (0usize, HEADER_LEN);
    let table_range = (glyph_offset, table_end);
    let index_range = (index_offset, index_end);
    if overlaps(header_range, table_range) {
        return Err(ClipReadError::OverlappingSections {
            first: "header",
            second: "glyph table",
        });
    }
    if overlaps(header_range, index_range) {
        return Err(ClipReadError::OverlappingSections {
            first: "header",
            second: "frame index",
        });
    }
    if overlaps(table_range, index_range) {
        return Err(ClipReadError::OverlappingSections {
            first: "glyph table",
            second: "frame index",
        });
    }

    let decoded_bytes = raw_length
        .checked_mul(frame_count)
        .ok_or(ClipReadError::FrameIndexLengthOverflow)?;
    if decoded_bytes > MAX_DECODED_BYTES {
        return Err(ClipReadError::DecodedLimitExceeded {
            bytes: decoded_bytes,
            maximum: MAX_DECODED_BYTES,
        });
    }

    let frame_capacity =
        usize::try_from(frame_count).map_err(|_| ClipReadError::AllocationFailed)?;
    let mut entries = Vec::new();
    let mut payload_ranges = Vec::new();
    entries
        .try_reserve_exact(frame_capacity)
        .map_err(|_| ClipReadError::AllocationFailed)?;
    payload_ranges
        .try_reserve_exact(frame_capacity)
        .map_err(|_| ClipReadError::AllocationFailed)?;
    let mut previous_pts = None;
    for index in 0..frame_count {
        let start = index_offset + index as usize * FRAME_INDEX_ENTRY_LEN;
        let entry = &bytes[start..start + FRAME_INDEX_ENTRY_LEN];
        let pts = i64::from_le_bytes(entry[0..8].try_into().expect("fixed-size field"));
        let duration = u64_at(entry, 8);
        let payload_offset_u64 = u64_at(entry, 16);
        let stored_length = u32_at(entry, 24);
        let declared_raw_length = u32_at(entry, 28);
        let codec = entry[32];
        let frame_kind = entry[33];
        let entry_reserved = u16_at(entry, 34);
        let checksum = u32_at(entry, 36);
        let invalid = |reason| ClipReadError::InvalidFrame { index, reason };
        if duration == 0 {
            return Err(invalid(ClipFrameError::ZeroDuration));
        }
        if previous_pts.is_some_and(|previous| pts <= previous) {
            return Err(invalid(ClipFrameError::NonIncreasingTimestamp));
        }
        previous_pts = Some(pts);
        if entry_reserved != 0 {
            return Err(invalid(ClipFrameError::InvalidReserved));
        }
        if codec != 0 {
            return Err(invalid(ClipFrameError::UnsupportedCodec { codec }));
        }
        if frame_kind != 0 {
            return Err(invalid(ClipFrameError::UnsupportedFrameKind {
                kind: frame_kind,
            }));
        }
        if u64::from(declared_raw_length) != raw_length {
            return Err(invalid(ClipFrameError::RawLengthMismatch {
                declared: declared_raw_length,
                expected: raw_length,
            }));
        }
        let expected_stored =
            u32::try_from(raw_length).map_err(|_| ClipReadError::FrameIndexLengthOverflow)?;
        if stored_length != expected_stored {
            return Err(invalid(ClipFrameError::StoredLengthMismatch {
                declared: stored_length,
                expected: expected_stored,
            }));
        }
        let payload_offset = usize::try_from(payload_offset_u64)
            .map_err(|_| invalid(ClipFrameError::PayloadOutOfBounds))?;
        let payload_end = payload_offset
            .checked_add(stored_length as usize)
            .ok_or_else(|| invalid(ClipFrameError::PayloadOutOfBounds))?;
        if payload_end > bytes.len() {
            return Err(invalid(ClipFrameError::PayloadOutOfBounds));
        }
        let payload_range = (payload_offset, payload_end);
        if overlaps(payload_range, header_range)
            || overlaps(payload_range, table_range)
            || overlaps(payload_range, index_range)
        {
            return Err(invalid(ClipFrameError::PayloadOverlap));
        }
        let actual_checksum = crc32(&bytes[payload_offset..payload_end]);
        if actual_checksum != checksum {
            return Err(invalid(ClipFrameError::ChecksumMismatch {
                expected: checksum,
                actual: actual_checksum,
            }));
        }
        payload_ranges.push(payload_range);
        entries.push((pts, duration, payload_offset));
    }
    payload_ranges.sort_unstable_by_key(|range| range.0);
    if payload_ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(ClipReadError::OverlappingSections {
            first: "frame payload",
            second: "another frame payload",
        });
    }

    let mut frames = Vec::new();
    frames
        .try_reserve_exact(frame_capacity)
        .map_err(|_| ClipReadError::AllocationFailed)?;
    for (index, (pts_us, duration_us, payload_offset)) in entries.into_iter().enumerate() {
        let payload = &bytes[payload_offset..payload_offset + raw_length as usize];
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(cell_count as usize)
            .map_err(|_| ClipReadError::AllocationFailed)?;
        for (cell_index, chunk) in payload.as_chunks::<CELL_LEN>().0.iter().enumerate() {
            let glyph_index = u32::from_le_bytes(chunk[0..4].try_into().expect("fixed-size field"));
            if glyph_index as usize >= glyph_table.glyphs().len() {
                return Err(ClipReadError::InvalidFrame {
                    index: index as u64,
                    reason: ClipFrameError::GlyphIndexOutOfBounds {
                        cell: cell_index as u64,
                        glyph_index,
                    },
                });
            }
            cells.push(ClipCell {
                glyph_index,
                foreground: [chunk[4], chunk[5], chunk[6], chunk[7]],
                background: [chunk[8], chunk[9], chunk[10], chunk[11]],
            });
        }
        frames.push(ClipFrame {
            pts_us,
            duration_us,
            cells,
        });
    }
    Ok(AsciiClip {
        columns,
        rows,
        glyphs: glyph_table,
        frames,
    })
}

fn glyph_table_end(bytes: &[u8], offset: usize, count: u32) -> Result<usize, ClipReadError> {
    if offset < HEADER_LEN || offset > bytes.len() {
        return Err(ClipReadError::InvalidOffset {
            section: "glyph table",
            offset: offset as u64,
        });
    }
    let mut cursor = offset;
    for index in 0..count {
        let available = bytes.len().saturating_sub(cursor);
        if available < 2 {
            return Err(ClipReadError::InvalidGlyphTable(
                ClipGlyphTableError::TruncatedLengthPrefix { index, available },
            ));
        }
        let length = u16_at(bytes, cursor);
        cursor += 2;
        if !(1..=32).contains(&length) {
            return Err(ClipReadError::InvalidGlyphTable(
                ClipGlyphTableError::InvalidGlyphLength { index, length },
            ));
        }
        let available = bytes.len().saturating_sub(cursor);
        if available < usize::from(length) {
            return Err(ClipReadError::InvalidGlyphTable(
                ClipGlyphTableError::TruncatedGlyphBytes {
                    index,
                    length,
                    available,
                },
            ));
        }
        cursor += usize::from(length);
    }
    Ok(cursor)
}

fn usize_offset(offset: u64, section: &'static str) -> Result<usize, ClipReadError> {
    usize::try_from(offset).map_err(|_| ClipReadError::InvalidOffset { section, offset })
}

fn overlaps(first: (usize, usize), second: (usize, usize)) -> bool {
    first.0 < second.1 && second.0 < first.1
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("fixed-size field"),
    )
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("fixed-size field"),
    )
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
