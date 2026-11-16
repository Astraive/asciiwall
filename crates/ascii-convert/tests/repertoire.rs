mod support;

use ascii_convert::{
    GlyphCellClassification, GlyphConfig, GlyphPreviewStatus, GlyphRepertoire,
    GlyphRepertoirePreset, GlyphTable, build_glyph_preview, build_glyph_table,
    is_supported_single_cell_scalar,
};
use ascii_core::{
    Budget, BudgetKind, CancellationToken, Error, GlyphFailureReason, MalformedReason,
};
use support::repertoire_font::{complete_repertoire_font, sparse_ascii_font};

fn budget() -> Budget {
    Budget::new(1024, 128 * 1024 * 1024, 200_000_000)
}

fn config() -> GlyphConfig {
    GlyphConfig::new(8.0, 8, 12, 9).expect("supported raster geometry")
}

fn record(table: &GlyphTable, character: char) -> &ascii_convert::GlyphRecord {
    table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == character)
        .expect("fixture includes requested scalar")
}

fn region_has_ink(
    mask: &[u8],
    width: usize,
    height: usize,
    x_start: usize,
    x_end: usize,
    y_start: usize,
    y_end: usize,
) -> bool {
    (y_start..y_end.min(height))
        .any(|y| (x_start..x_end.min(width)).any(|x| mask[y * width + x] != 0))
}

fn ink_in_rows(mask: &[u8], width: usize, first_row: usize, end_row: usize) -> usize {
    mask.chunks_exact(width)
        .skip(first_row)
        .take(end_row - first_row)
        .flatten()
        .filter(|coverage| **coverage != 0)
        .count()
}

fn assert_braille_pattern_masks(config: GlyphConfig) {
    let font = complete_repertoire_font(None);
    let repertoire = GlyphRepertoire::from_preset(GlyphRepertoirePreset::BraillePatterns)
        .expect("curated Braille preset");
    let table = build_glyph_table(
        &font,
        &repertoire,
        &config,
        &budget(),
        &CancellationToken::new(),
    )
    .expect("all synthetic Braille glyphs rasterize");
    assert_eq!(table.glyphs().len(), 256);
    let width = usize::try_from(config.cell_width()).expect("fixture width fits");
    let height = usize::try_from(config.cell_height()).expect("fixture height fits");
    let x_split = width / 2;
    let row_centers = [710.0, 510.0, 310.0, 110.0]
        .map(|units| config.baseline() as f32 - config.raster_size() * units / 1000.0);
    for pattern in 0..=255u32 {
        let character = char::from_u32(0x2800 + pattern).expect("Braille scalar");
        let mask = record(&table, character).mask();
        for (dot, (right_column, row)) in [
            (false, 0),
            (false, 1),
            (false, 2),
            (true, 0),
            (true, 1),
            (true, 2),
            (false, 3),
            (true, 3),
        ]
        .into_iter()
        .enumerate()
        {
            let row_center = row_centers[row];
            let x_start = if right_column { x_split } else { 0 };
            let x_end = if right_column { width } else { x_split };
            let half_band = config.raster_size() * 0.06;
            let y_start = (row_center - half_band).floor().max(0.0) as usize;
            let y_end = (row_center + half_band).ceil().max(0.0) as usize;
            let found = region_has_ink(mask, width, height, x_start, x_end, y_start, y_end);
            assert_eq!(
                found,
                pattern & (1 << dot) != 0,
                "U+{:04X} dot {} has wrong orientation or coverage at {}x{}",
                character as u32,
                dot + 1,
                width,
                height,
            );
        }
    }
}

#[test]
fn curated_repertoires_cover_printable_ascii_blocks_and_all_braille() {
    let ascii =
        GlyphRepertoire::from_preset(GlyphRepertoirePreset::PrintableAscii).expect("ASCII preset");
    assert_eq!(ascii.characters().len(), 95);
    assert_eq!(ascii.characters().first(), Some(&' '));
    assert_eq!(ascii.characters().last(), Some(&'~'));

    let blocks =
        GlyphRepertoire::from_preset(GlyphRepertoirePreset::BlockElements).expect("block preset");
    assert_eq!(blocks.characters().len(), 32);
    assert_eq!(blocks.characters().first(), Some(&'\u{2580}'));
    assert_eq!(blocks.characters().last(), Some(&'\u{259f}'));

    let braille = GlyphRepertoire::from_preset(GlyphRepertoirePreset::BraillePatterns)
        .expect("Braille preset");
    assert_eq!(braille.characters().len(), 256);
    assert_eq!(braille.characters().first(), Some(&'\u{2800}'));
    assert_eq!(braille.characters().last(), Some(&'\u{28ff}'));

    let all =
        GlyphRepertoire::from_preset(GlyphRepertoirePreset::AllSupported).expect("combined preset");
    assert_eq!(all.characters().len(), 383);
    assert!(all.characters().contains(&' '));
    assert!(
        all.characters()
            .iter()
            .copied()
            .all(is_supported_single_cell_scalar)
    );
    for scalar in (0x20..=0x7e).chain(0x2580..=0x259f).chain(0x2800..=0x28ff) {
        assert!(is_supported_single_cell_scalar(
            char::from_u32(scalar).expect("range contains Unicode scalars")
        ));
    }
}

#[test]
fn complete_repertoire_has_font_glyphs_and_single_cell_metrics() {
    let repertoire = GlyphRepertoire::from_preset(GlyphRepertoirePreset::AllSupported)
        .expect("combined strict-cell preset");
    let table = build_glyph_table(
        &complete_repertoire_font(None),
        &repertoire,
        &config(),
        &budget(),
        &CancellationToken::new(),
    )
    .expect("synthetic font contains every curated scalar");
    assert_eq!(table.glyphs().len(), 383);
    assert!(repertoire.characters().iter().all(|character| {
        table
            .glyphs()
            .iter()
            .any(|record| record.character() == *character)
    }));
    assert!(table.glyphs().iter().all(|record| {
        record.metrics().cell_classification(table.cell_width())
            == GlyphCellClassification::SingleCell
    }));
    let space = record(&table, ' ');
    assert!(space.mask().iter().all(|coverage| *coverage == 0));
}

#[test]
fn rejects_controls_bidi_combining_variation_cjk_and_emoji_with_the_offender() {
    for rejected in [
        '\u{0000}',
        '\u{000a}',
        '\u{001b}',
        '\u{007f}',
        '\u{0085}',
        '\u{009f}',
        '\u{061c}',
        '\u{200f}',
        '\u{202e}',
        '\u{0301}',
        '\u{fe0f}',
        '\u{4e00}',
        '\u{1f600}',
        '\u{200d}',
    ] {
        assert_eq!(
            GlyphRepertoire::new(&rejected.to_string()),
            Err(Error::Glyph {
                character: rejected,
                reason: GlyphFailureReason::UnsupportedScalar,
            }),
            "rejected scalar U+{:04X}",
            rejected as u32,
        );
    }
}

#[test]
fn duplicates_and_empty_repertoires_have_deterministic_boundary_errors() {
    let duplicate = Error::Glyph {
        character: 'A',
        reason: GlyphFailureReason::DuplicateScalar,
    };
    assert_eq!(GlyphRepertoire::new("BAA"), Err(duplicate));
    assert_eq!(GlyphRepertoire::new("AAB"), Err(duplicate));
    let unsupported = Error::Glyph {
        character: '界',
        reason: GlyphFailureReason::UnsupportedScalar,
    };
    assert_eq!(GlyphRepertoire::new("😀界"), Err(unsupported));
    assert_eq!(GlyphRepertoire::new("界😀"), Err(unsupported));
    assert_eq!(
        GlyphRepertoire::new(""),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    );
    let too_many = "A".repeat(4097);
    assert_eq!(
        GlyphRepertoire::new(&too_many),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
}

#[test]
fn rendered_blocks_and_fixed_cell_metrics_come_from_masks() {
    let font = complete_repertoire_font(None);
    let repertoire =
        GlyphRepertoire::from_preset(GlyphRepertoirePreset::BlockElements).expect("block preset");
    let table = build_glyph_table(
        &font,
        &repertoire,
        &config(),
        &budget(),
        &CancellationToken::new(),
    )
    .expect("all block glyphs are present in the synthetic font");
    assert_eq!(table.glyphs().len(), 32);
    assert!(table.glyphs().iter().all(|glyph| {
        glyph.metrics().cell_classification(table.cell_width())
            == GlyphCellClassification::SingleCell
    }));

    let upper_half = record(&table, '\u{2580}');
    let lower_eighth = record(&table, '\u{2581}');
    let lower_half = record(&table, '\u{2584}');
    let full_block = record(&table, '\u{2588}');
    assert!(upper_half.mean_coverage() > lower_eighth.mean_coverage());
    assert!(full_block.mean_coverage() > upper_half.mean_coverage());
    let upper_ink = ink_in_rows(upper_half.mask(), table.cell_width() as usize, 0, 6);
    let lower_ink = ink_in_rows(upper_half.mask(), table.cell_width() as usize, 6, 12);
    assert!(
        upper_ink > lower_ink,
        "upper-half outline is rasterized in the upper cell"
    );
    let lower_upper_ink = ink_in_rows(lower_half.mask(), table.cell_width() as usize, 0, 6);
    let lower_lower_ink = ink_in_rows(lower_half.mask(), table.cell_width() as usize, 6, 12);
    assert!(
        lower_lower_ink > lower_upper_ink,
        "lower-half outline is rasterized in the lower cell"
    );

    let narrow_cell = GlyphConfig::new(8.0, 4, 12, 9).expect("valid narrow cell");
    assert_eq!(
        build_glyph_table(
            &complete_repertoire_font(Some('A')),
            &GlyphRepertoire::new("A").expect("ASCII scalar"),
            &narrow_cell,
            &budget(),
            &CancellationToken::new(),
        ),
        Err(Error::Glyph {
            character: 'A',
            reason: GlyphFailureReason::MultiCellMetrics,
        })
    );
}

#[test]
fn fractional_block_masks_match_cell_relative_eighths() {
    let font = complete_repertoire_font(None);
    let repertoire =
        GlyphRepertoire::from_preset(GlyphRepertoirePreset::BlockElements).expect("block preset");
    let table = build_glyph_table(
        &font,
        &repertoire,
        &config(),
        &budget(),
        &CancellationToken::new(),
    )
    .expect("all block glyphs are present in the synthetic font");
    let width = table.cell_width() as usize;
    let height = table.cell_height() as usize;
    assert_eq!((width, height), (8, 12));

    let full_block = record(&table, '\u{2588}').mask();
    let full_coverage = full_block
        .iter()
        .map(|&coverage| u64::from(coverage))
        .sum::<u64>();
    let first_full_column = (0..width)
        .find(|&column| full_block.chunks_exact(width).any(|row| row[column] != 0))
        .expect("full block has ink");
    let last_full_column = (0..width)
        .rfind(|&column| full_block.chunks_exact(width).any(|row| row[column] != 0))
        .expect("full block has ink");

    for (character, eighths, anchored_left) in [
        ('\u{2589}', 7, true),
        ('\u{258a}', 6, true),
        ('\u{258b}', 5, true),
        ('\u{258c}', 4, true),
        ('\u{258d}', 3, true),
        ('\u{258e}', 2, true),
        ('\u{258f}', 1, true),
        ('\u{2595}', 1, false),
    ] {
        let mask = record(&table, character).mask();
        let coverage = mask
            .iter()
            .map(|&coverage| u64::from(coverage))
            .sum::<u64>();
        let expected_scaled_coverage = full_coverage * eighths;
        let measured_scaled_coverage = coverage * 8;
        let tolerance = expected_scaled_coverage / 20;
        assert!(
            measured_scaled_coverage.abs_diff(expected_scaled_coverage) <= tolerance,
            "U+{:04X} mask should cover {eighths}/8 of the full-cell mask: \
             scaled coverage {measured_scaled_coverage}, expected {expected_scaled_coverage} \
             (tolerance {tolerance})",
            character as u32,
        );

        for (row_index, (full_row, block_row)) in full_block
            .chunks_exact(width)
            .zip(mask.chunks_exact(width))
            .enumerate()
        {
            assert_eq!(
                block_row.iter().any(|&coverage| coverage != 0),
                full_row.iter().any(|&coverage| coverage != 0),
                "U+{:04X} must retain the full block's vertical extent on row {row_index}",
                character as u32,
            );
        }

        let (expected_column, measured_column) = if anchored_left {
            let measured_column = (0..width)
                .find(|&column| mask.chunks_exact(width).any(|row| row[column] != 0))
                .expect("fractional block has ink");
            (first_full_column, measured_column)
        } else {
            let measured_column = (0..width)
                .rfind(|&column| mask.chunks_exact(width).any(|row| row[column] != 0))
                .expect("right eighth block has ink");
            (last_full_column, measured_column)
        };
        assert_eq!(
            measured_column,
            expected_column,
            "U+{:04X} must remain aligned to the {} edge of the full block",
            character as u32,
            if anchored_left { "left" } else { "right" },
        );
    }
}

#[test]
fn every_braille_dot_pattern_rasterizes_at_two_sizes_with_correct_orientation() {
    assert_braille_pattern_masks(config());
    assert_braille_pattern_masks(
        GlyphConfig::new(10.0, 10, 15, 12).expect("second supported raster geometry"),
    );
}

#[test]
fn preview_reuses_matcher_masks_and_distinguishes_space_missing_and_invisible() {
    let font = complete_repertoire_font(None);
    let repertoire = GlyphRepertoire::new("A ").expect("ASCII preview repertoire");
    let table = build_glyph_table(
        &font,
        &repertoire,
        &config(),
        &budget(),
        &CancellationToken::new(),
    )
    .expect("matcher table");
    let preview = build_glyph_preview(&font, 'A', &config(), &budget(), &CancellationToken::new())
        .expect("available glyph preview");
    let matched = record(&table, 'A');
    assert_eq!(preview.status(), GlyphPreviewStatus::Available);
    assert_eq!(preview.mask(), Some(matched.mask()));
    assert_eq!(preview.mean_coverage(), Some(matched.mean_coverage()));
    assert_eq!((preview.cell_width(), preview.cell_height()), (8, 12));

    let space = build_glyph_preview(&font, ' ', &config(), &budget(), &CancellationToken::new())
        .expect("space preview");
    assert_eq!(space.status(), GlyphPreviewStatus::Available);
    assert!(
        space
            .mask()
            .expect("space mask")
            .iter()
            .all(|coverage| *coverage == 0)
    );

    let invisible = build_glyph_preview(
        &font,
        '\u{2800}',
        &config(),
        &budget(),
        &CancellationToken::new(),
    )
    .expect("blank Braille pattern preview");
    assert_eq!(invisible.status(), GlyphPreviewStatus::Invisible);
    assert_eq!(invisible.mean_coverage(), Some(0.0));

    let missing = build_glyph_preview(
        &sparse_ascii_font(),
        'C',
        &config(),
        &budget(),
        &CancellationToken::new(),
    )
    .expect("missing glyph is a visible editor state");
    assert_eq!(missing.status(), GlyphPreviewStatus::Unavailable);
    assert!(missing.mask().is_none());
    assert!(missing.mean_coverage().is_none());

    let multicell_font = complete_repertoire_font(Some('A'));
    let multicell = build_glyph_preview(
        &multicell_font,
        'A',
        &config(),
        &budget(),
        &CancellationToken::new(),
    )
    .expect("multi-cell classification is a preview state");
    assert_eq!(multicell.status(), GlyphPreviewStatus::MultiCell);
    assert!(multicell.mask().is_none());
}

#[test]
fn preview_budget_and_cancellation_fail_without_partial_publication() {
    let font = complete_repertoire_font(None);
    assert_eq!(
        build_glyph_preview(
            &font,
            'A',
            &config(),
            &Budget::new(1024, 1_000_000, 1),
            &CancellationToken::new(),
        ),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        build_glyph_preview(&font, 'A', &config(), &budget(), &cancelled),
        Err(Error::Cancelled)
    );
}
