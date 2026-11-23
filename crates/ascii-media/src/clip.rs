//! Parsing of the glyph-table section in the draft ASCII clip format.

use std::fmt;

/// Maximum number of glyphs accepted by the draft ASCII clip v1 reader contract.
pub const MAX_CLIP_GLYPH_COUNT: u32 = 4096;
const MAX_GLYPH_BYTES: u16 = 32;

/// A fully validated clip glyph table in its serialized index order.
#[derive(Debug, Eq, PartialEq)]
pub struct ClipGlyphTable {
    glyphs: Vec<char>,
}

impl ClipGlyphTable {
    /// Returns the supported glyphs in serialized table order.
    #[must_use]
    pub fn glyphs(&self) -> &[char] {
        &self.glyphs
    }
}

/// A malformed or unsupported glyph-table section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipGlyphTableError {
    /// The declared or observed table size exceeds the v1 limit.
    GlyphCountLimitExceeded { count: u32, maximum: u32 },
    /// An entry ends before its two-byte little-endian length is complete.
    TruncatedLengthPrefix { index: u32, available: usize },
    /// An entry length is outside the v1 range of 1 through 32 bytes.
    InvalidGlyphLength { index: u32, length: u16 },
    /// The entry does not contain all bytes declared by its length prefix.
    TruncatedGlyphBytes {
        index: u32,
        length: u16,
        available: usize,
    },
    /// An entry's bytes are not valid UTF-8.
    InvalidUtf8 { index: u32, valid_up_to: usize },
    /// An entry contains a scalar outside the shared strict single-cell repertoire.
    UnsupportedScalar { index: u32, character: char },
    /// An entry contains more than one supported Unicode scalar.
    MultipleScalars { index: u32, second: char },
    /// The number of complete entries differs from the declared count.
    CountMismatch { declared: u32, actual: u32 },
    /// The bounded output table could not reserve its required storage.
    AllocationFailed,
}

impl fmt::Display for ClipGlyphTableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GlyphCountLimitExceeded { count, maximum } => {
                write!(
                    formatter,
                    "clip glyph count {count} exceeds the limit {maximum}"
                )
            }
            Self::TruncatedLengthPrefix { index, available } => write!(
                formatter,
                "clip glyph entry {index} has only {available} bytes of its length prefix"
            ),
            Self::InvalidGlyphLength { index, length } => write!(
                formatter,
                "clip glyph entry {index} has invalid byte length {length}"
            ),
            Self::TruncatedGlyphBytes {
                index,
                length,
                available,
            } => write!(
                formatter,
                "clip glyph entry {index} declares {length} bytes but only {available} remain"
            ),
            Self::InvalidUtf8 { index, valid_up_to } => write!(
                formatter,
                "clip glyph entry {index} is invalid UTF-8 at byte {valid_up_to}"
            ),
            Self::UnsupportedScalar { index, character } => write!(
                formatter,
                "clip glyph entry {index} contains unsupported scalar U+{:04X}",
                *character as u32
            ),
            Self::MultipleScalars { index, second } => write!(
                formatter,
                "clip glyph entry {index} contains multiple scalars; extra scalar is U+{:04X}",
                *second as u32
            ),
            Self::CountMismatch { declared, actual } => write!(
                formatter,
                "clip glyph table declares {declared} entries but contains {actual}"
            ),
            Self::AllocationFailed => formatter.write_str("could not allocate clip glyph table"),
        }
    }
}

impl std::error::Error for ClipGlyphTableError {}

/// Parses exactly the declared glyph-table section from an ASCII clip v1 payload.
///
/// Each entry consists of a little-endian `u16` byte length followed by that many UTF-8 bytes.
/// Every entry must be 1–32 bytes and encode exactly one scalar accepted by
/// [`ascii_core::is_supported_single_cell_scalar`]. Serialized order is preserved because frame
/// glyph indices refer to that order. No bytes may be omitted or left malformed, and the total
/// entry count is capped at [`MAX_CLIP_GLYPH_COUNT`].
pub fn parse_clip_glyph_table(
    bytes: &[u8],
    declared_count: u32,
) -> Result<ClipGlyphTable, ClipGlyphTableError> {
    if declared_count > MAX_CLIP_GLYPH_COUNT {
        return Err(ClipGlyphTableError::GlyphCountLimitExceeded {
            count: declared_count,
            maximum: MAX_CLIP_GLYPH_COUNT,
        });
    }

    let capacity = declared_count as usize;
    let mut glyphs = Vec::new();
    glyphs
        .try_reserve_exact(capacity)
        .map_err(|_| ClipGlyphTableError::AllocationFailed)?;

    let mut offset = 0;
    let mut parsed_count = 0;
    while offset < bytes.len() {
        if parsed_count == MAX_CLIP_GLYPH_COUNT {
            return Err(ClipGlyphTableError::GlyphCountLimitExceeded {
                count: MAX_CLIP_GLYPH_COUNT + 1,
                maximum: MAX_CLIP_GLYPH_COUNT,
            });
        }
        let index = parsed_count;
        let prefix_available = bytes.len() - offset;
        if prefix_available < 2 {
            return Err(ClipGlyphTableError::TruncatedLengthPrefix {
                index,
                available: prefix_available,
            });
        }

        let length = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        offset += 2;
        if !(1..=MAX_GLYPH_BYTES).contains(&length) {
            return Err(ClipGlyphTableError::InvalidGlyphLength { index, length });
        }

        let glyph_length = usize::from(length);
        let glyph_available = bytes.len() - offset;
        if glyph_available < glyph_length {
            return Err(ClipGlyphTableError::TruncatedGlyphBytes {
                index,
                length,
                available: glyph_available,
            });
        }
        let glyph_bytes = &bytes[offset..offset + glyph_length];
        offset += glyph_length;

        let glyph_text = core::str::from_utf8(glyph_bytes).map_err(|error| {
            ClipGlyphTableError::InvalidUtf8 {
                index,
                valid_up_to: error.valid_up_to(),
            }
        })?;
        let mut entry_glyph = None;
        for character in glyph_text.chars() {
            if !ascii_core::is_supported_single_cell_scalar(character) {
                return Err(ClipGlyphTableError::UnsupportedScalar { index, character });
            }
            if entry_glyph.is_some() {
                return Err(ClipGlyphTableError::MultipleScalars {
                    index,
                    second: character,
                });
            }
            entry_glyph = Some(character);
        }

        if let Some(character) = entry_glyph {
            if parsed_count < declared_count {
                glyphs.push(character);
            }
        } else {
            return Err(ClipGlyphTableError::InvalidGlyphLength { index, length });
        }
        parsed_count += 1;
    }

    if parsed_count != declared_count {
        return Err(ClipGlyphTableError::CountMismatch {
            declared: declared_count,
            actual: parsed_count,
        });
    }

    Ok(ClipGlyphTable { glyphs })
}
