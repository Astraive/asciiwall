//! Bounded conversion from immutable frame/glyph contracts to terminal-safe byte streams.

use std::error::Error as StdError;
use std::fmt;
use std::vec::Vec;

use ascii_convert::GlyphTable;
use ascii_core::{
    AsciiCell, AsciiFrame, Budget, BudgetKind, CancellationToken, Error, GlyphTableId,
    GridSize, MalformedReason, validate_frame,
};

const CANCELLATION_POLL_INTERVAL: usize = 1024;

const ANSI16_PALETTE: [[u8; 3]; 16] = [
    [0, 0, 0],
    [128, 0, 0],
    [0, 128, 0],
    [128, 128, 0],
    [0, 0, 128],
    [128, 0, 128],
    [0, 128, 128],
    [192, 192, 192],
    [128, 128, 128],
    [255, 0, 0],
    [0, 255, 0],
    [255, 255, 0],
    [0, 0, 255],
    [255, 0, 255],
    [0, 255, 255],
    [255, 255, 255],
];

const XTERM_CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// A supported terminal palette policy for ANSI export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnsiColorMode {
    /// Quantize to the conventional 16-color ANSI palette.
    Ansi16,
    /// Quantize to the xterm 256-color palette.
    Ansi256,
    /// Preserve sRGB channel values with ANSI true-color SGR parameters.
    TrueColor,
}

/// The requested serialization format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportTarget {
    /// Printable glyph rows separated and terminated by LF.
    Text,
    /// Printable glyph rows with engine-generated SGR colors.
    Ansi(AnsiColorMode),
}

/// A terminal-safe export failure that never embeds untrusted glyph or frame metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportError {
    /// The shared core rejected frame shape, identity, resource use, or cancellation.
    Core(Error),
    /// A glyph-table entry is outside the strict printable single-cell repertoire.
    UnsupportedGlyph { glyph_index: u32 },
}

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => write!(formatter, "export failed: {error}"),
            Self::UnsupportedGlyph { glyph_index } => write!(
                formatter,
                "glyph table entry {glyph_index} is not a supported printable single-cell glyph"
            ),
        }
    }
}

impl StdError for ExportError {}

/// Complete, owned export bytes and the source frame's logical grid dimensions.
#[derive(Debug, Eq, PartialEq)]
pub struct ExportOutput {
    bytes: Vec<u8>,
    size: GridSize,
    target: ExportTarget,
}

impl ExportOutput {
    /// Returns the complete serialized bytes. No partial output is returned on failure.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consumes the result and returns its complete serialized bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Returns the logical frame dimensions in columns and rows.
    #[must_use]
    pub const fn size(&self) -> GridSize {
        self.size
    }

    /// Returns the selected target and, for ANSI, its resolved color mode.
    #[must_use]
    pub const fn target(&self) -> ExportTarget {
        self.target
    }

    /// Returns the target-specific losses callers should disclose to users.
    #[must_use]
    pub const fn loss_notice(&self) -> &'static str {
        match self.target {
            ExportTarget::Text => {
                "Plain text preserves the printable character grid but loses font, colors, and motion; exact aspect depends on the viewer."
            }
            ExportTarget::Ansi(AnsiColorMode::TrueColor) => {
                "ANSI true-color output flattens background alpha against black and foreground alpha against the flattened background; it uses direct sRGB color parameters but still depends on the terminal font and does not preserve live effects."
            }
            ExportTarget::Ansi(AnsiColorMode::Ansi16) => {
                "ANSI 16-color output flattens background alpha against black and foreground alpha against the flattened background, then quantizes both colors to the conventional 16-color palette; terminal palette and font affect appearance, and live effects are not preserved."
            }
            ExportTarget::Ansi(AnsiColorMode::Ansi256) => {
                "ANSI 256-color output flattens background alpha against black and foreground alpha against the flattened background, then quantizes both colors to the xterm 256-color palette; terminal palette and font affect appearance, and live effects are not preserved."
            }
        }
    }
}

/// Serializes a frame after revalidating its binding, cell indices, and every glyph-table entry.
///
/// The result is all-or-error: its buffer is not exposed unless every row has been serialized.
/// Both the returned buffer size and the conservative operation-work estimate are bounded by
/// `budget`; cancellation is polled during validation, sizing, and serialization. The caller owns
/// destination selection and transactional publication.
pub fn export(
    frame: &AsciiFrame,
    glyph_table: &GlyphTable,
    target: ExportTarget,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<ExportOutput, ExportError> {
    if cancellation.is_cancelled() {
        return Err(ExportError::Core(Error::Cancelled));
    }

    let glyphs = glyph_table.glyphs();
    if glyphs.is_empty() {
        return Err(ExportError::Core(Error::Malformed(
            MalformedReason::InvalidConfiguration,
        )));
    }
    let glyph_count = u32::try_from(glyphs.len())
        .map_err(|_| ExportError::Core(Error::SizeOverflow))?;
    let glyph_table_id = GlyphTableId::new(*glyph_table.fingerprint());
    validate_frame(
        frame,
        glyph_table_id,
        glyph_count,
        budget,
        cancellation,
    )
    .map_err(ExportError::Core)?;

    let size = frame.size();
    let cells = frame.cells();
    let required_work = estimated_work(size, glyph_count, target)?;
    if required_work > budget.max_work_units() {
        return Err(ExportError::Core(Error::BudgetExceeded(
            BudgetKind::WorkUnits,
        )));
    }
    for (index, glyph) in glyphs.iter().enumerate() {
        if index % CANCELLATION_POLL_INTERVAL == 0 && cancellation.is_cancelled() {
            return Err(ExportError::Core(Error::Cancelled));
        }
        if !ascii_convert::is_supported_single_cell_scalar(glyph.character()) {
            return Err(ExportError::UnsupportedGlyph {
                glyph_index: u32::try_from(index)
                    .map_err(|_| ExportError::Core(Error::SizeOverflow))?,
            });
        }
    }
    if cancellation.is_cancelled() {
        return Err(ExportError::Core(Error::Cancelled));
    }

    let columns =
        usize::try_from(size.columns()).map_err(|_| ExportError::Core(Error::SizeOverflow))?;
    let output_len = serialized_size(cells, columns, glyphs, target, cancellation)?;
    if output_len > budget.max_frame_bytes() {
        return Err(ExportError::Core(Error::BudgetExceeded(
            BudgetKind::FrameBytes,
        )));
    }
    let output_capacity = usize::try_from(output_len)
        .map_err(|_| ExportError::Core(Error::SizeOverflow))?;
    let _allocation_layout = isize::try_from(output_capacity)
        .map_err(|_| ExportError::Core(Error::SizeOverflow))?;

    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(output_capacity)
        .map_err(|_| ExportError::Core(Error::AllocationFailed))?;
    let actual_capacity = u64::try_from(bytes.capacity())
        .map_err(|_| ExportError::Core(Error::SizeOverflow))?;
    if actual_capacity > budget.max_frame_bytes() {
        return Err(ExportError::Core(Error::BudgetExceeded(
            BudgetKind::FrameBytes,
        )));
    }

    serialize_into(&mut bytes, cells, columns, glyphs, target, cancellation)?;
    if u64::try_from(bytes.len()).map_err(|_| ExportError::Core(Error::SizeOverflow))? != output_len {
        return Err(ExportError::Core(Error::SizeOverflow));
    }

    Ok(ExportOutput {
        bytes,
        size,
        target,
    })
}

fn estimated_work(
    size: GridSize,
    glyph_count: u32,
    target: ExportTarget,
) -> Result<u64, ExportError> {
    // Account for frame validation, sizing, and serialization, plus every palette
    // candidate distance evaluated during both color-encoding passes.
    let work_per_cell = match target {
        ExportTarget::Text | ExportTarget::Ansi(AnsiColorMode::TrueColor) => 3,
        // Two colors per cell, sixteen candidates per color, in two encoding passes.
        ExportTarget::Ansi(AnsiColorMode::Ansi16) => 3 + 2 * 16 * 2,
        // ANSI16 candidates plus one cube and one grayscale candidate per color/pass.
        ExportTarget::Ansi(AnsiColorMode::Ansi256) => 3 + 2 * 18 * 2,
    };
    size.cell_count()
        .checked_mul(work_per_cell)
        .and_then(|work| work.checked_add(u64::from(glyph_count)))
        .ok_or(ExportError::Core(Error::SizeOverflow))
}

fn serialized_size(
    cells: &[AsciiCell],
    columns: usize,
    glyphs: &[ascii_convert::GlyphRecord],
    target: ExportTarget,
    cancellation: &CancellationToken,
) -> Result<u64, ExportError> {
    let mut size = match target {
        ExportTarget::Text => 0,
        ExportTarget::Ansi(_) => 4,
    };
    for (index, cell) in cells.iter().enumerate() {
        if index % CANCELLATION_POLL_INTERVAL == 0 && cancellation.is_cancelled() {
            return Err(ExportError::Core(Error::Cancelled));
        }
        let glyph_index = usize::try_from(cell.glyph())
            .map_err(|_| ExportError::Core(Error::SizeOverflow))?;
        let glyph = glyphs[glyph_index].character();
        size = size
            .checked_add(u64::try_from(glyph.len_utf8()).map_err(|_| ExportError::Core(Error::SizeOverflow))?)
            .ok_or(ExportError::Core(Error::SizeOverflow))?;

        if let ExportTarget::Ansi(mode) = target {
            let colors = encode_cell_colors(*cell, mode);
            size = size
                .checked_add(ansi_sequence_size(colors))
                .ok_or(ExportError::Core(Error::SizeOverflow))?;
        }

        if (index + 1) % columns == 0 {
            let row_bytes = match target {
                ExportTarget::Text => 1,
                ExportTarget::Ansi(_) => 5,
            };
            size = size
                .checked_add(row_bytes)
                .ok_or(ExportError::Core(Error::SizeOverflow))?;
        }
    }
    if cancellation.is_cancelled() {
        return Err(ExportError::Core(Error::Cancelled));
    }
    Ok(size)
}

fn serialize_into(
    output: &mut Vec<u8>,
    cells: &[AsciiCell],
    columns: usize,
    glyphs: &[ascii_convert::GlyphRecord],
    target: ExportTarget,
    cancellation: &CancellationToken,
) -> Result<(), ExportError> {
    if matches!(target, ExportTarget::Ansi(_)) {
        output.extend_from_slice(b"\x1b[0m");
    }
    for (index, cell) in cells.iter().enumerate() {
        if index % CANCELLATION_POLL_INTERVAL == 0 && cancellation.is_cancelled() {
            return Err(ExportError::Core(Error::Cancelled));
        }
        let glyph_index = usize::try_from(cell.glyph())
            .map_err(|_| ExportError::Core(Error::SizeOverflow))?;
        if let ExportTarget::Ansi(mode) = target {
            append_ansi_sequence(output, encode_cell_colors(*cell, mode));
        }
        let character = glyphs[glyph_index].character();
        let mut encoded = [0; 4];
        output.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());

        if (index + 1) % columns == 0 {
            if matches!(target, ExportTarget::Ansi(_)) {
                output.extend_from_slice(b"\x1b[0m");
            }
            output.push(b'\n');
        }
    }
    if cancellation.is_cancelled() {
        return Err(ExportError::Core(Error::Cancelled));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum EncodedColors {
    Basic { foreground: u8, background: u8 },
    Indexed { foreground: u8, background: u8 },
    Rgb { foreground: [u8; 3], background: [u8; 3] },
}

fn encode_cell_colors(cell: AsciiCell, mode: AnsiColorMode) -> EncodedColors {
    let background = composite_srgba(cell.bg_srgba8(), [0, 0, 0]);
    let foreground = composite_srgba(cell.fg_srgba8(), background);
    match mode {
        AnsiColorMode::Ansi16 => EncodedColors::Basic {
            foreground: nearest_ansi16(foreground),
            background: nearest_ansi16(background),
        },
        AnsiColorMode::Ansi256 => EncodedColors::Indexed {
            foreground: nearest_xterm_256(foreground),
            background: nearest_xterm_256(background),
        },
        AnsiColorMode::TrueColor => EncodedColors::Rgb {
            foreground,
            background,
        },
    }
}

fn composite_srgba(source: [u8; 4], background: [u8; 3]) -> [u8; 3] {
    let alpha = u32::from(source[3]);
    let inverse_alpha = 255 - alpha;
    [0, 1, 2].map(|channel| {
        let source_channel = u32::from(source[channel]);
        let background_channel = u32::from(background[channel]);
        u8::try_from((source_channel * alpha + background_channel * inverse_alpha + 127) / 255)
            .expect("alpha compositing remains in the byte range")
    })
}

fn nearest_ansi16(color: [u8; 3]) -> u8 {
    nearest_palette_color(color, &ANSI16_PALETTE).0
}

fn nearest_palette_color(color: [u8; 3], palette: &[[u8; 3]]) -> (u8, u32) {
    let mut best_index = 0u8;
    let mut best_distance = u32::MAX;
    for (index, candidate) in palette.iter().enumerate() {
        let distance = color_distance(color, *candidate);
        if distance < best_distance {
            best_index = u8::try_from(index).expect("palette index fits in a byte");
            best_distance = distance;
        }
    }
    (best_index, best_distance)
}

fn nearest_xterm_256(color: [u8; 3]) -> u8 {
    let (mut best_index, mut best_distance) = nearest_palette_color(color, &ANSI16_PALETTE);

    let red = nearest_cube_level(color[0]);
    let green = nearest_cube_level(color[1]);
    let blue = nearest_cube_level(color[2]);
    let cube_index = 16 + 36 * red + 6 * green + blue;
    let cube_color = [
        XTERM_CUBE_LEVELS[usize::from(red)],
        XTERM_CUBE_LEVELS[usize::from(green)],
        XTERM_CUBE_LEVELS[usize::from(blue)],
    ];
    let cube_distance = color_distance(color, cube_color);
    if cube_distance < best_distance {
        best_index = u8::try_from(cube_index).expect("xterm color index fits in a byte");
        best_distance = cube_distance;
    }

    let mean = (u16::from(color[0]) + u16::from(color[1]) + u16::from(color[2]) + 1) / 3;
    let gray_step = if mean <= 8 {
        0
    } else {
        ((mean - 8 + 4) / 10).min(23)
    };
    let gray_index = 232 + u8::try_from(gray_step).expect("grayscale index fits in a byte");
    let gray_value = 8 + 10 * u8::try_from(gray_step).expect("grayscale step fits in a byte");
    let gray_distance = color_distance(color, [gray_value; 3]);
    if gray_distance < best_distance {
        best_index = gray_index;
    }
    best_index
}

fn nearest_cube_level(channel: u8) -> u8 {
    match channel {
        0..=47 => 0,
        48..=115 => 1,
        116..=155 => 2,
        156..=195 => 3,
        196..=235 => 4,
        _ => 5,
    }
}

fn color_distance(left: [u8; 3], right: [u8; 3]) -> u32 {
    (0..3)
        .map(|channel| {
            let difference = i32::from(left[channel]) - i32::from(right[channel]);
            u32::try_from(difference * difference).expect("squared channel delta is nonnegative")
        })
        .sum()
}

fn ansi_sequence_size(colors: EncodedColors) -> u64 {
    match colors {
        EncodedColors::Basic {
            foreground,
            background,
        } => {
            let foreground_code = if foreground < 8 {
                30 + foreground
            } else {
                90 + foreground - 8
            };
            let background_code = if background < 8 {
                40 + background
            } else {
                100 + background - 8
            };
            decimal_size(foreground_code) + decimal_size(background_code) + 4
        }
        EncodedColors::Indexed {
            foreground,
            background,
        } => 14 + decimal_size(foreground) + decimal_size(background),
        EncodedColors::Rgb {
            foreground,
            background,
        } => {
            18 + foreground
                .into_iter()
                .chain(background)
                .map(decimal_size)
                .sum::<u64>()
        }
    }
}

fn append_ansi_sequence(output: &mut Vec<u8>, colors: EncodedColors) {
    output.extend_from_slice(b"\x1b[");
    match colors {
        EncodedColors::Basic {
            foreground,
            background,
        } => {
            let foreground_code = if foreground < 8 {
                30 + foreground
            } else {
                90 + foreground - 8
            };
            let background_code = if background < 8 {
                40 + background
            } else {
                100 + background - 8
            };
            append_decimal(output, foreground_code);
            output.push(b';');
            append_decimal(output, background_code);
        }
        EncodedColors::Indexed {
            foreground,
            background,
        } => {
            output.extend_from_slice(b"38;5;");
            append_decimal(output, foreground);
            output.extend_from_slice(b";48;5;");
            append_decimal(output, background);
        }
        EncodedColors::Rgb {
            foreground,
            background,
        } => {
            output.extend_from_slice(b"38;2;");
            append_rgb(output, foreground);
            output.extend_from_slice(b";48;2;");
            append_rgb(output, background);
        }
    }
    output.push(b'm');
}

fn append_rgb(output: &mut Vec<u8>, color: [u8; 3]) {
    append_decimal(output, color[0]);
    output.push(b';');
    append_decimal(output, color[1]);
    output.push(b';');
    append_decimal(output, color[2]);
}

fn append_decimal(output: &mut Vec<u8>, value: u8) {
    if value >= 100 {
        output.push(b'0' + value / 100);
        output.push(b'0' + (value / 10) % 10);
        output.push(b'0' + value % 10);
    } else if value >= 10 {
        output.push(b'0' + value / 10);
        output.push(b'0' + value % 10);
    } else {
        output.push(b'0' + value);
    }
}

const fn decimal_size(value: u8) -> u64 {
    if value >= 100 {
        3
    } else if value >= 10 {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::{nearest_cube_level, nearest_xterm_256};

    #[test]
    fn xterm_palette_ties_choose_the_lower_index() {
        assert_eq!(nearest_cube_level(115), 1);
        assert_eq!(nearest_cube_level(155), 2);
        assert_eq!(nearest_cube_level(195), 3);
        assert_eq!(nearest_cube_level(235), 4);
        assert_eq!(nearest_xterm_256([13, 13, 13]), 232);
    }
}
