mod support;

use ascii_convert::{GlyphConfig, build_glyph_table};
use ascii_core::{
    Budget, BudgetKind, CancellationToken, Error, MalformedReason, UnsupportedReason,
};
use support::synthetic_font::{composite_font, diagonal_font, quadratic_font, synthetic_font};

fn config() -> GlyphConfig {
    GlyphConfig::new(8.0, 8, 12, 9).expect("fixture geometry is valid")
}

fn budget(max_raster_bytes: u64, max_work_units: u64) -> Budget {
    Budget::new(1024, max_raster_bytes, max_work_units)
}

#[test]
fn measured_masks_retain_space_and_order_equal_coverage_by_font_index() {
    let font = synthetic_font(false);
    let token = CancellationToken::new();
    let table = build_glyph_table(
        &font,
        "BA ",
        &config(),
        &budget(1_000_000, 1_000_000),
        &token,
    )
    .expect("valid fixture font builds a complete table");
    let reversed = build_glyph_table(
        &font,
        " AB",
        &config(),
        &budget(1_000_000, 1_000_000),
        &token,
    )
    .expect("canonical repertoire ordering is independent of input order");

    assert_eq!(table.fingerprint(), reversed.fingerprint());
    assert_eq!(table.cell_width(), 8);
    assert_eq!(table.cell_height(), 12);
    assert_eq!(table.glyphs().len(), 3);
    assert_eq!(table.glyphs()[0].character(), ' ');
    assert!(
        table.glyphs()[0]
            .mask()
            .iter()
            .all(|coverage| *coverage == 0)
    );
    assert!(table.glyphs()[0].metrics().advance() > 0.0);
    assert!(table.glyphs()[0].centroid().is_none());

    let a = &table.glyphs()[1];
    let b = &table.glyphs()[2];
    assert_eq!((a.character(), b.character()), ('A', 'B'));
    assert_eq!((a.glyph_index(), b.glyph_index()), (2, 3));
    assert_eq!(a.mean_coverage(), b.mean_coverage());
    assert!(a.mean_coverage() > 0.0);
    assert_eq!(a.mask().len(), 8 * 12);
    assert!(a.bounds().width() > 0 && a.bounds().height() > 0);
    assert!(
        a.centroid()
            .is_some_and(|center| center.x() < 8.0 && center.y() < 12.0)
    );
    assert_eq!(a.metrics().baseline(), 9);
    assert!(a.metrics().line_height() > 0.0);
    let edges = a.edge_orientation();
    let edge_sum = edges.horizontal()
        + edges.vertical()
        + edges.diagonal_positive()
        + edges.diagonal_negative();
    assert!((edge_sum - 1.0).abs() < 0.0001);
    assert!(
        table
            .glyphs()
            .windows(2)
            .all(|pair| { pair[0].mean_coverage() <= pair[1].mean_coverage() })
    );
}

#[test]
fn fingerprint_covers_font_raster_geometry_and_normalized_repertoire() {
    let font = synthetic_font(false);
    let other_font = synthetic_font(true);
    let token = CancellationToken::new();
    let generous = budget(1_000_000, 1_000_000);
    let original =
        build_glyph_table(&font, "AB", &config(), &generous, &token).expect("fixture builds");
    let different_font = build_glyph_table(&other_font, "AB", &config(), &generous, &token)
        .expect("second fixture builds");
    let different_config = GlyphConfig::new(9.0, 8, 12, 9).expect("valid alternate raster size");
    let different_raster = build_glyph_table(&font, "AB", &different_config, &generous, &token)
        .expect("alternate raster size builds");

    assert_ne!(original.fingerprint(), different_font.fingerprint());
    assert_ne!(original.fingerprint(), different_raster.fingerprint());
}

#[test]
fn invalid_and_missing_repertoires_return_typed_errors() {
    let font = synthetic_font(false);
    let token = CancellationToken::new();
    let generous = budget(1_000_000, 1_000_000);

    assert_eq!(
        build_glyph_table(&font, "", &config(), &generous, &token),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    );
    assert_eq!(
        build_glyph_table(&font, "A\n", &config(), &generous, &token),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    );
    assert_eq!(
        build_glyph_table(&font, "AA", &config(), &generous, &token),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    );
    assert_eq!(
        build_glyph_table(&font, "C", &config(), &generous, &token),
        Err(Error::Unsupported(UnsupportedReason::GlyphUnavailable))
    );
    assert_eq!(
        build_glyph_table(&[0, 1, 2], "A", &config(), &generous, &token),
        Err(Error::Malformed(MalformedReason::InvalidEncodedData))
    );
    assert_eq!(
        GlyphConfig::new(0.0, 8, 12, 9),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    );
    assert_eq!(
        GlyphConfig::new(8.0, 8, 12, 13),
        Err(Error::Malformed(MalformedReason::InvalidConfiguration))
    );
    let oversized_charset = "A".repeat(4097);
    assert_eq!(
        build_glyph_table(&font, &oversized_charset, &config(), &generous, &token),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
}

#[test]
fn resource_limits_and_cancellation_fail_before_publishing_a_table() {
    let font = synthetic_font(false);
    let token = CancellationToken::new();
    let oversized_font = vec![0; 16 * 1024 * 1024 + 1];
    assert_eq!(
        build_glyph_table(
            &oversized_font,
            "A",
            &config(),
            &budget(1_000_000, 1_000_000),
            &token
        ),
        Err(Error::BudgetExceeded(BudgetKind::EncodedBytes))
    );
    let large_cell = GlyphConfig::new(8.0, 20_000, 20_000, 10_000)
        .expect("valid but intentionally over-budget geometry");
    assert_eq!(
        build_glyph_table(
            &font,
            "A",
            &large_cell,
            &budget(1_000_000, 1_000_000),
            &token
        ),
        Err(Error::BudgetExceeded(BudgetKind::RasterBytes))
    );
    assert_eq!(
        build_glyph_table(&font, "A", &config(), &budget(1_000_000, 1), &token),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );

    token.cancel();
    assert_eq!(
        build_glyph_table(&font, "A", &config(), &budget(1_000_000, 1_000_000), &token),
        Err(Error::Cancelled)
    );
}

#[test]
fn outline_raster_work_is_bounded_at_the_requested_size() {
    let token = CancellationToken::new();
    let small = GlyphConfig::new(8.0, 8, 12, 9).expect("valid small raster");
    let large = GlyphConfig::new(128.0, 8, 12, 9).expect("valid large raster");
    let raster_bytes = 2_000_000;

    let diagonal = diagonal_font(false);
    let parsed = fontdue::Font::from_bytes(
        diagonal.as_slice(),
        fontdue::FontSettings {
            load_substitutions: false,
            ..fontdue::FontSettings::default()
        },
    )
    .expect("the outline-heavy fixture is a valid font");
    let metrics = parsed.metrics_indexed(2, large.raster_size());
    // The former estimate counted two font scans, the cell, cmap lookups,
    // two parser passes (one cmap record, three mappings, 256 six-callback
    // contours and two six-callback rectangles), and bitmap area + dimensions.
    let previous_work = 2 * diagonal.len()
        + 8 * 12
        + parsed.chars().len().max(1)
        + 2 * (4 + 6 * (256 + 2))
        + metrics.width * metrics.height
        + metrics.width
        + metrics.height;
    assert!(previous_work < 40_000);

    // 512 long diagonal edges cross roughly 64 by 90 pixels each, so
    // their traversal alone exceeds the same limit.
    let fixtures = [
        (diagonal, 40_000),
        (quadratic_font(), 90_000),
        (composite_font(), 55_000),
    ];
    for (font, limit) in fixtures {
        build_glyph_table(&font, "A", &small, &budget(raster_bytes, limit), &token)
            .expect("the same outline fits when rasterized small");
        assert_eq!(
            build_glyph_table(&font, "A", &large, &budget(raster_bytes, limit), &token),
            Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
        );
        build_glyph_table(&font, "A", &large, &budget(raster_bytes, 1_000_000), &token)
            .expect("the outline is valid with sufficient raster work");
    }

    build_glyph_table(
        &synthetic_font(false),
        "A",
        &large,
        &budget(raster_bytes, 40_000),
        &token,
    )
    .expect("a normal simple outline still fits the tight work limit");
}

#[test]
fn aliases_are_charged_for_each_rasterization() {
    let font = diagonal_font(true);
    let config = GlyphConfig::new(128.0, 8, 12, 9).expect("valid raster geometry");
    let token = CancellationToken::new();
    let limit = budget(2_000_000, 150_000);

    build_glyph_table(&font, "A", &config, &limit, &token).expect("one copy of the outline fits");
    assert_eq!(
        build_glyph_table(&font, "AB", &config, &limit, &token),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
    let table = build_glyph_table(&font, "AB", &config, &budget(2_000_000, 1_000_000), &token)
        .expect("both aliases work with sufficient budget");
    assert_eq!(
        table.glyphs()[0].glyph_index(),
        table.glyphs()[1].glyph_index()
    );
}

fn replace_table(font: &mut Vec<u8>, tag: &[u8; 4], table: &[u8]) {
    let count = usize::from(u16::from_be_bytes([font[4], font[5]]));
    let record = (0..count)
        .map(|index| 12 + index * 16)
        .find(|&record| &font[record..record + 4] == tag)
        .expect("fixture table exists");
    let offset = u32::try_from(font.len()).expect("fixture offset fits");
    font.extend_from_slice(table);
    font[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
    font[record + 12..record + 16].copy_from_slice(
        &u32::try_from(table.len())
            .expect("fixture table length fits")
            .to_be_bytes(),
    );
}

fn cmap_format_12(start: u32, end: u32) -> Vec<u8> {
    let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12];
    cmap.extend_from_slice(&12u16.to_be_bytes());
    cmap.extend_from_slice(&0u16.to_be_bytes());
    cmap.extend_from_slice(&28u32.to_be_bytes());
    cmap.extend_from_slice(&0u32.to_be_bytes());
    cmap.extend_from_slice(&1u32.to_be_bytes());
    cmap.extend_from_slice(&start.to_be_bytes());
    cmap.extend_from_slice(&end.to_be_bytes());
    cmap.extend_from_slice(&1u32.to_be_bytes());
    cmap
}

#[test]
fn malformed_format_twelve_scalar_ranges_are_rejected_before_fontdue() {
    for (start, end) in [(0xd7ff, 0xe000), (0x10ffff, 0x110000)] {
        let mut font = synthetic_font(false);
        replace_table(&mut font, b"cmap", &cmap_format_12(start, end));
        assert_eq!(
            build_glyph_table(
                &font,
                "A",
                &config(),
                &budget(1_000_000, 1_000_000),
                &CancellationToken::new(),
            ),
            Err(Error::Malformed(MalformedReason::InvalidEncodedData))
        );
    }
}

#[test]
fn large_valid_format_twelve_range_fails_work_budget_before_fontdue() {
    let mut font = synthetic_font(false);
    replace_table(&mut font, b"cmap", &cmap_format_12(0x10000, 0x10ffff));
    assert_eq!(
        build_glyph_table(
            &font,
            "A",
            &config(),
            &budget(1_000_000, 1_000_000),
            &CancellationToken::new(),
        ),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
}

#[test]
fn horizontal_kern_format_three_is_rejected_before_fontdue() {
    let mut font = synthetic_font(false);
    // Version 0, one horizontal format-3 subtable with an empty body.
    replace_table(&mut font, b"kern", &[0, 0, 0, 1, 0, 0, 0, 6, 3, 1]);
    assert_eq!(
        build_glyph_table(
            &font,
            "A",
            &config(),
            &budget(1_000_000, 1_000_000),
            &CancellationToken::new(),
        ),
        Err(Error::Unsupported(UnsupportedReason::UnsupportedFormat))
    );
}
