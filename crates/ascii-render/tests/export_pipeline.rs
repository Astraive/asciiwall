//! Behavior-focused text/ANSI export goldens and terminal-safety regressions.
//!
//! Fixture parameters: locally generated TrueType outlines at 8x12 pixels, 8px em, baseline 9;
//! Fixture metadata carries bidi and ESC sentinels which must never enter output.
//! The fixture contains no external assets or I/O.

mod support;

use ascii_convert::{
    GlyphConfig, GlyphRepertoire, GlyphTable, build_glyph_table, is_supported_single_cell_scalar,
};
use ascii_core::{
    AsciiCell, AsciiFrame, Budget, BudgetKind, CancellationToken, Error, FrameMetadata,
    GlyphTableId, GridSize, MalformedReason,
};
use ascii_render::{AnsiColorMode, ExportError, ExportTarget, export};
use support::synthetic_font::synthetic_font;

const FONT_BUDGET: Budget = Budget::new(1_000_000, 1_000_000, 1_000_000);
const FG: [u8; 4] = [255, 0, 0, 255];
const BG: [u8; 4] = [0, 0, 255, 255];

fn config() -> GlyphConfig {
    GlyphConfig::new(8.0, 8, 12, 9).expect("fixture geometry is valid")
}

fn build_table(charset: &str) -> GlyphTable {
    let repertoire =
        GlyphRepertoire::new(charset).expect("fixture repertoire contains supported scalars");
    let token = CancellationToken::new();
    build_glyph_table(
        &synthetic_font(),
        &repertoire,
        &config(),
        &FONT_BUDGET,
        &token,
    )
    .expect("synthetic repertoire exists in the fixture font")
}

fn grid_frame(table: &GlyphTable, characters: &[char], columns: u32, rows: u32) -> AsciiFrame {
    grid_frame_with_colors(table, characters, columns, rows, FG, BG)
}

fn grid_frame_with_colors(
    table: &GlyphTable,
    characters: &[char],
    columns: u32,
    rows: u32,
    foreground: [u8; 4],
    background: [u8; 4],
) -> AsciiFrame {
    let token = CancellationToken::new();
    let glyph_ids: Vec<u32> = characters
        .iter()
        .map(|character| {
            u32::try_from(
                table
                    .glyphs()
                    .iter()
                    .position(|glyph| glyph.character() == *character)
                    .expect("frame glyph exists in the table"),
            )
            .expect("fixture glyph index fits")
        })
        .collect();
    let cells = glyph_ids
        .into_iter()
        .map(|glyph| AsciiCell::new(glyph, foreground, background))
        .collect();
    let table_id = GlyphTableId::new(*table.fingerprint());
    AsciiFrame::try_new(
        GridSize::new(columns, rows),
        FrameMetadata::new(table_id, 0x202e, 0x1b, 900_000, 33_333),
        cells,
        u32::try_from(table.glyphs().len()).expect("fixture glyph count fits"),
        &FONT_BUDGET,
        &token,
    )
    .expect("fixture frame satisfies shared core contracts")
}

fn valid_fixture() -> (GlyphTable, AsciiFrame) {
    let table = build_table(" █⠁");
    let frame = grid_frame(&table, &[' ', '█', '⠁', '⠁', ' ', '█'], 3, 2);
    (table, frame)
}

#[test]
fn text_ansi_golden() {
    let (table, frame) = valid_fixture();
    let budget = FONT_BUDGET;
    let cancellation = CancellationToken::new();

    let text = export(
        &frame,
        &table,
        ExportTarget::Text,
        &budget,
        &cancellation,
    )
    .expect("printable text export succeeds");
    assert_eq!(text.bytes(), " █⠁\n⠁ █\n".as_bytes());
    assert_eq!(text.size(), GridSize::new(3, 2));
    assert_eq!(
        text.loss_notice(),
        "Plain text preserves the printable character grid but loses font, colors, and motion; exact aspect depends on the viewer."
    );

    let ansi = export(
        &frame,
        &table,
        ExportTarget::Ansi(AnsiColorMode::TrueColor),
        &budget,
        &cancellation,
    )
    .expect("true-color ANSI export succeeds");
    let cell = "\x1b[38;2;255;0;0;48;2;0;0;255m";
    let expected = format!(
        "\x1b[0m{cell} {cell}█{cell}⠁\x1b[0m\n{cell}⠁{cell} {cell}█\x1b[0m\n"
    );
    assert_eq!(ansi.bytes(), expected.as_bytes());
    assert_eq!(ansi.size(), GridSize::new(3, 2));
    assert_eq!(
        ansi.loss_notice(),
        "ANSI true-color output flattens background alpha against black and foreground alpha against the flattened background; it uses direct sRGB color parameters but still depends on the terminal font and does not preserve live effects."
    );
    for (mode, palette_cell) in [
        (AnsiColorMode::Ansi16, "\x1b[91;104m"),
        (AnsiColorMode::Ansi256, "\x1b[38;5;9;48;5;12m"),
    ] {
        let ansi = export(
            &frame,
            &table,
            ExportTarget::Ansi(mode),
            &budget,
            &cancellation,
        )
        .expect("resolved ANSI palette is supported");
        let expected = format!(
            "\x1b[0m{palette_cell} {palette_cell}█{palette_cell}⠁\x1b[0m\n{palette_cell}⠁{palette_cell} {palette_cell}█\x1b[0m\n"
        );
        assert_eq!(ansi.bytes(), expected.as_bytes());
    }
}

#[test]
fn ansi_alpha_flattening_and_palette_quantization_are_reported() {
    let table = build_table("█");
    let frame = grid_frame_with_colors(
        &table,
        &['█'],
        1,
        1,
        [255, 0, 0, 128],
        [0, 0, 255, 128],
    );
    let cancellation = CancellationToken::new();

    let true_color = export(
        &frame,
        &table,
        ExportTarget::Ansi(AnsiColorMode::TrueColor),
        &FONT_BUDGET,
        &cancellation,
    )
    .expect("true-color export succeeds");
    assert_eq!(
        true_color.bytes(),
        "\x1b[0m\x1b[38;2;128;0;64;48;2;0;0;128m█\x1b[0m\n".as_bytes()
    );
    assert_eq!(
        true_color.loss_notice(),
        "ANSI true-color output flattens background alpha against black and foreground alpha against the flattened background; it uses direct sRGB color parameters but still depends on the terminal font and does not preserve live effects."
    );

    let ansi16 = export(
        &frame,
        &table,
        ExportTarget::Ansi(AnsiColorMode::Ansi16),
        &FONT_BUDGET,
        &cancellation,
    )
    .expect("ANSI16 export succeeds");
    assert_eq!(
        ansi16.bytes(),
        "\x1b[0m\x1b[31;44m█\x1b[0m\n".as_bytes()
    );
    assert_eq!(
        ansi16.loss_notice(),
        "ANSI 16-color output flattens background alpha against black and foreground alpha against the flattened background, then quantizes both colors to the conventional 16-color palette; terminal palette and font affect appearance, and live effects are not preserved."
    );

    let ansi256 = export(
        &frame,
        &table,
        ExportTarget::Ansi(AnsiColorMode::Ansi256),
        &FONT_BUDGET,
        &cancellation,
    )
    .expect("ANSI256 export succeeds");
    assert_eq!(
        ansi256.bytes(),
        "\x1b[0m\x1b[38;5;89;48;5;4m█\x1b[0m\n".as_bytes()
    );
    assert_eq!(
        ansi256.loss_notice(),
        "ANSI 256-color output flattens background alpha against black and foreground alpha against the flattened background, then quantizes both colors to the xterm 256-color palette; terminal palette and font affect appearance, and live effects are not preserved."
    );
}

#[test]
fn terminal_safety() {
    let (table, frame) = valid_fixture();
    let cancellation = CancellationToken::new();
    for mode in [
        AnsiColorMode::Ansi16,
        AnsiColorMode::Ansi256,
        AnsiColorMode::TrueColor,
    ] {
        let output = export(
            &frame,
            &table,
            ExportTarget::Ansi(mode),
            &FONT_BUDGET,
            &cancellation,
        )
        .expect("resolved ANSI palette is supported");
        assert_only_glyphs_and_approved_sgr(output.bytes(), mode);
    }

    let text = export(
        &frame,
        &table,
        ExportTarget::Text,
        &FONT_BUDGET,
        &cancellation,
    )
    .expect("plain text export succeeds");
    assert_only_glyphs_and_newlines(text.bytes());

    for character in [' ', '~', '█', '⠁'] {
        assert!(is_supported_single_cell_scalar(character));
    }
    for character in [
        '\x1b',
        '\0',
        '\n',
        '\u{7f}',
        '\u{85}',
        '\u{202e}',
        '\u{2066}',
        '\u{200b}',
        '\u{fe0f}',
        '\u{200d}',
        '\u{301}',
        '界',
        '😀',
    ] {
        assert!(
            !is_supported_single_cell_scalar(character),
            "unsafe scalar accepted: {character:?}"
        );
    }
}

#[test]
fn ansi_palette_work_budget_covers_both_color_encoding_passes() {
    let table = build_table("█");
    let frame = grid_frame(&table, &['█'], 1, 1);
    let cancellation = CancellationToken::new();
    let glyph_work = u64::try_from(table.glyphs().len()).expect("fixture glyph count fits");

    for (mode, work_per_cell) in [
        (AnsiColorMode::Ansi16, 67),
        (AnsiColorMode::Ansi256, 75),
    ] {
        let minimum_work = glyph_work + work_per_cell;
        let one_unit_short = Budget::new(1_000_000, 1_000_000, minimum_work - 1);
        assert_eq!(
            export(
                &frame,
                &table,
                ExportTarget::Ansi(mode),
                &one_unit_short,
                &cancellation,
            ),
            Err(ExportError::Core(Error::BudgetExceeded(BudgetKind::WorkUnits)))
        );

        let exact_work = Budget::new(1_000_000, 1_000_000, minimum_work);
        assert!(
            export(
                &frame,
                &table,
                ExportTarget::Ansi(mode),
                &exact_work,
                &cancellation,
            )
            .is_ok()
        );
    }
}

#[test]
fn invalid_frame_binding_and_glyph_indices_are_rejected_before_export() {
    let (table, frame) = valid_fixture();
    let token = CancellationToken::new();
    let invalid_cells = vec![AsciiCell::new(99, FG, BG)];
    assert_eq!(
        AsciiFrame::try_new(
            GridSize::new(1, 1),
            FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 1, 2, 0, 1),
            invalid_cells,
            u32::try_from(table.glyphs().len()).expect("fixture glyph count fits"),
            &FONT_BUDGET,
            &token,
        ),
        Err(Error::Malformed(MalformedReason::GlyphIndexOutOfRange))
    );

    let mismatch_budget = FONT_BUDGET;
    let mismatch = export(
        &frame,
        &build_table("█"),
        ExportTarget::Text,
        &mismatch_budget,
        &token,
    );
    assert!(matches!(
        mismatch,
        Err(ExportError::Core(Error::Malformed(
            MalformedReason::GlyphTableMismatch
        )))
    ));
}

#[test]
fn budget_and_cancellation_fail_without_publishing_partial_bytes() {
    let (table, frame) = valid_fixture();
    let token = CancellationToken::new();
    let constrained_bytes = Budget::new(100, 1_000_000, 1_000_000);
    assert_eq!(
        export(
            &frame,
            &table,
            ExportTarget::Ansi(AnsiColorMode::TrueColor),
            &constrained_bytes,
            &token,
        ),
        Err(ExportError::Core(Error::BudgetExceeded(BudgetKind::FrameBytes)))
    );

    let constrained_work = Budget::new(1_000_000, 1_000_000, 6);
    assert_eq!(
        export(
            &frame,
            &table,
            ExportTarget::Text,
            &constrained_work,
            &token,
        ),
        Err(ExportError::Core(Error::BudgetExceeded(BudgetKind::WorkUnits)))
    );

    token.cancel();
    assert_eq!(
        export(
            &frame,
            &table,
            ExportTarget::Text,
            &FONT_BUDGET,
            &token,
        ),
        Err(ExportError::Core(Error::Cancelled))
    );
}

fn assert_only_glyphs_and_newlines(bytes: &[u8]) {
    let text = core::str::from_utf8(bytes).expect("export is UTF-8");
    for character in text.chars() {
        assert!(character == '\n' || is_fixture_glyph(character));
        if character != '\n' {
            assert!(!character.is_control());
        }
    }
    assert!(text.ends_with('\n'));
}

fn assert_only_glyphs_and_approved_sgr(bytes: &[u8], mode: AnsiColorMode) {
    let mut cursor = 0usize;
    let mut reset_count = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] == 0x1b {
            assert_eq!(bytes.get(cursor + 1), Some(&b'['), "only CSI SGR is allowed");
            let start = cursor + 2;
            let end = bytes[start..]
                .iter()
                .position(|byte| *byte == b'm')
                .map(|relative| start + relative)
                .expect("SGR has a terminator");
            let parameters = parse_sgr_parameters(&bytes[start..end]);
            if parameters == [0] {
                reset_count += 1;
            } else {
                assert!(approved_color_parameters(&parameters, mode));
            }
            cursor = end + 1;
        } else {
            let suffix = core::str::from_utf8(&bytes[cursor..]).expect("glyph stream is UTF-8");
            let character = suffix.chars().next().expect("nonempty suffix has a character");
            assert!(character == '\n' || is_fixture_glyph(character));
            if character != '\n' {
                assert!(!character.is_control());
            }
            cursor += character.len_utf8();
        }
    }
    assert_eq!(reset_count, 3, "the stream starts clean and each row resets terminal attributes");
}

fn parse_sgr_parameters(parameters: &[u8]) -> Vec<u16> {
    assert!(!parameters.is_empty());
    assert!(parameters.iter().all(|byte| byte.is_ascii_digit() || *byte == b';'));
    parameters
        .split(|byte| *byte == b';')
        .map(|digits| {
            core::str::from_utf8(digits)
                .expect("SGR parameter is ASCII")
                .parse()
                .expect("SGR parameter is numeric")
        })
        .collect()
}

fn approved_color_parameters(parameters: &[u16], mode: AnsiColorMode) -> bool {
    match mode {
        AnsiColorMode::Ansi16 => {
            parameters.len() == 2
                && matches!(parameters[0], 30..=37 | 90..=97)
                && matches!(parameters[1], 40..=47 | 100..=107)
        }
        AnsiColorMode::Ansi256 => {
            parameters.len() == 6
                && parameters[0..2] == [38, 5]
                && parameters[3..5] == [48, 5]
                && parameters[2] <= 255
                && parameters[5] <= 255
        }
        AnsiColorMode::TrueColor => {
            parameters.len() == 9
                && parameters[0..2] == [38, 2]
                && parameters[5..7] == [48, 2]
                && parameters[2..5].iter().all(|value| *value <= 255)
                && parameters[7..9].iter().all(|value| *value <= 255)
        }
    }
}

fn is_fixture_glyph(character: char) -> bool {
    matches!(character, ' ' | '█' | '⠁')
}
