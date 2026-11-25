#[allow(dead_code)]
mod support;

use ascii_convert::{
    ContourOptions, ConversionSettings, FitMode, GlyphConfig, GlyphRepertoire, GridSizing,
    LinearRgb, SamplingFilter, SamplingOptions, build_glyph_table, convert_contour,
};
use ascii_core::{
    AlphaMode, Budget, CancellationToken, ColorDescription, ColorPrimaries, Error, FrameMetadata,
    GlyphTableId, PixelFormat, RasterFrameView, RasterOrientation, TransferFunction,
};
use support::synthetic_font::equal_area_orientation_font;

const WIDTH: u32 = 8;
const HEIGHT: u32 = 12;
const BUDGET: Budget = Budget::new(1_000_000, 2_000_000, 2_000_000);

fn run(
    table: &ascii_convert::GlyphTable,
    pixels: &[u8],
    options: ContourOptions,
) -> ascii_convert::ContourConversion {
    let cancellation = CancellationToken::new();
    let settings = ConversionSettings::new(
        GridSizing::fixed_pixel_surface(WIDTH, HEIGHT),
        FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 7, 1, 0, 1),
        &cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .expect("fixture settings are valid")
    .with_sampling(SamplingOptions::new(
        FitMode::Contain,
        SamplingFilter::Box,
        None,
    ));
    let raster = RasterFrameView::new(
        WIDTH,
        HEIGHT,
        WIDTH as usize,
        PixelFormat::gray8(),
        ColorDescription::new(
            ColorPrimaries::Srgb,
            TransferFunction::Linear,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        pixels,
    );
    convert_contour(raster, table, &settings, options, &BUDGET)
        .expect("bounded contour conversion succeeds")
}

fn options(contrast: f64, edge: f64) -> ContourOptions {
    ContourOptions::new(2, contrast, edge, 0.10, 0.80, 0.10)
        .expect("fixture contour thresholds and weights are valid")
}

#[test]
fn oriented_edge_selects_matching_equal_area_glyph_deterministically() {
    let font = equal_area_orientation_font();
    let table = build_glyph_table(
        &font,
        &GlyphRepertoire::new(" BA").expect("fixture repertoire is valid"),
        &GlyphConfig::new(8.0, WIDTH, HEIGHT, 9).expect("fixture geometry is valid"),
        &BUDGET,
        &CancellationToken::new(),
    )
    .expect("synthetic fixture font builds");
    let vertical = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'B')
        .expect("fixture has vertical mask");
    let expected = table
        .glyphs()
        .iter()
        .position(|glyph| glyph.character() == 'B')
        .expect("vertical glyph is present") as u32;
    let first = run(&table, vertical.mask(), options(0.02, 0.02));
    let repeated = run(&table, vertical.mask(), options(0.02, 0.02));
    assert_eq!(first.frame().cells()[0].glyph(), expected);
    assert_eq!(first, repeated);
    assert!(first.work_units_bound() > 0);
    assert_eq!(first.work_units_bound(), repeated.work_units_bound());
}

#[test]
fn low_contrast_texture_and_uniform_fields_use_background_policy() {
    let table = build_glyph_table(
        &equal_area_orientation_font(),
        &GlyphRepertoire::new(" BA").expect("fixture repertoire is valid"),
        &GlyphConfig::new(8.0, WIDTH, HEIGHT, 9).expect("fixture geometry is valid"),
        &BUDGET,
        &CancellationToken::new(),
    )
    .expect("synthetic fixture font builds");
    let faint = [8_u8; (WIDTH * HEIGHT) as usize];
    let empty = [0_u8; (WIDTH * HEIGHT) as usize];
    let bright_uniform = [255_u8; (WIDTH * HEIGHT) as usize];
    let suppressed = run(&table, &faint, options(0.10, 0.02));
    let empty = run(&table, &empty, options(0.10, 0.02));
    let bright = run(&table, &bright_uniform, options(0.10, 0.02));
    assert_eq!(
        suppressed.frame().cells()[0].glyph(),
        empty.frame().cells()[0].glyph()
    );
    assert_eq!(
        bright.frame().cells()[0].glyph(),
        empty.frame().cells()[0].glyph()
    );
    let background = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == ' ')
        .expect("fixture repertoire includes blank glyph");
    assert_eq!(
        suppressed.frame().cells()[0].glyph(),
        table
            .glyphs()
            .iter()
            .position(|glyph| glyph.character() == ' ')
            .expect("fixture blank glyph is present") as u32
    );
    let faint_coverage = f64::from(faint[0]) / f64::from(u8::MAX);
    let tone_loss = (faint_coverage - f64::from(background.mean_coverage())).abs();
    assert!(
        tone_loss > 0.0,
        "suppressed texture must remain visible as tone loss"
    );
}

#[test]
fn horizontal_vertical_and_diagonal_silhouettes_keep_their_orientation() {
    for (font, expected_character) in [
        (equal_area_orientation_font(), 'A'),
        (equal_area_orientation_font(), 'B'),
        (support::synthetic_font::diagonal_orientation_font(), 'A'),
        (support::synthetic_font::diagonal_orientation_font(), 'B'),
    ] {
        let table = build_glyph_table(
            &font,
            &GlyphRepertoire::new(" BA").expect("fixture repertoire is valid"),
            &GlyphConfig::new(8.0, WIDTH, HEIGHT, 9).expect("fixture geometry is valid"),
            &BUDGET,
            &CancellationToken::new(),
        )
        .expect("synthetic fixture font builds");
        let glyph = table
            .glyphs()
            .iter()
            .find(|glyph| glyph.character() == expected_character)
            .expect("fixture contains the expected oriented mask");
        let expected = table
            .glyphs()
            .iter()
            .position(|candidate| candidate.character() == expected_character)
            .expect("expected glyph is present") as u32;
        let first = run(&table, glyph.mask(), options(0.0, 0.0));
        let repeated = run(&table, glyph.mask(), options(0.0, 0.0));
        assert_eq!(first.frame().cells()[0].glyph(), expected);
        assert_eq!(first, repeated);
        assert!(first.work_units_bound() > 0);
    }
}

#[test]
fn threshold_endpoints_are_valid_deterministic_and_bounded() {
    let table = build_glyph_table(
        &equal_area_orientation_font(),
        &GlyphRepertoire::new(" BA").expect("fixture repertoire is valid"),
        &GlyphConfig::new(8.0, WIDTH, HEIGHT, 9).expect("fixture geometry is valid"),
        &BUDGET,
        &CancellationToken::new(),
    )
    .expect("synthetic fixture font builds");
    let input = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'B')
        .expect("fixture has vertical mask")
        .mask();
    for threshold in [0.0, 1.0] {
        let boundary = options(threshold, threshold);
        let first = run(&table, input, boundary);
        let repeated = run(&table, input, boundary);
        assert_eq!(first, repeated, "threshold endpoint {threshold} is stable");
        assert_eq!(first.frame().cells().len(), 1);
        assert!(first.work_units_bound() > 0);
        assert_eq!(boundary.contrast_threshold(), threshold);
        assert_eq!(boundary.edge_threshold(), threshold);
    }
}

#[test]
fn matched_threshold_comparison_reports_silhouette_tone_loss_and_work() {
    let table = build_glyph_table(
        &equal_area_orientation_font(),
        &GlyphRepertoire::new(" BA").expect("fixture repertoire is valid"),
        &GlyphConfig::new(8.0, WIDTH, HEIGHT, 9).expect("fixture geometry is valid"),
        &BUDGET,
        &CancellationToken::new(),
    )
    .expect("synthetic fixture font builds");
    let silhouette = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'B')
        .expect("fixture has vertical mask")
        .mask();
    let textured: Vec<u8> = silhouette
        .iter()
        .map(|coverage| if *coverage == 0 { 8 } else { 255 })
        .collect();
    let unsuppressed = run(&table, &textured, options(0.0, 0.0));
    let suppressed = run(&table, &textured, options(0.10, 0.02));
    assert_eq!(
        unsuppressed.frame().cells()[0].glyph(),
        suppressed.frame().cells()[0].glyph(),
        "matched silhouette remains stable when only faint texture is thresholded"
    );
    let error = |glyph_index: u32| {
        let selected = &table.glyphs()[glyph_index as usize];
        textured
            .iter()
            .zip(selected.mask())
            .map(|(target, output)| (i16::from(*target) - i16::from(*output)).unsigned_abs())
            .map(u64::from)
            .sum::<u64>()
    };
    let baseline_tone_error = error(unsuppressed.frame().cells()[0].glyph());
    let suppressed_tone_error = error(suppressed.frame().cells()[0].glyph());
    assert!(suppressed_tone_error >= baseline_tone_error);
    assert!(
        suppressed_tone_error > 0,
        "paired evidence exposes tone loss"
    );
    assert_eq!(
        unsuppressed.work_units_bound(),
        suppressed.work_units_bound(),
        "same fixture and candidate cap have identical bounded work"
    );
}

#[test]
fn thresholds_and_weights_are_validated_and_part_of_identity() {
    assert!(matches!(
        ContourOptions::new(0, 0.1, 0.1, 0.1, 0.8, 0.1),
        Err(Error::Malformed(_))
    ));
    assert!(matches!(
        ContourOptions::new(2, f64::NAN, 0.1, 0.1, 0.8, 0.1),
        Err(Error::Malformed(_))
    ));
    assert!(matches!(
        ContourOptions::new(2, 0.1, 0.1, 0.2, 0.8, 0.1),
        Err(Error::Malformed(_))
    ));
    let table = build_glyph_table(
        &equal_area_orientation_font(),
        &GlyphRepertoire::new(" BA").expect("fixture repertoire is valid"),
        &GlyphConfig::new(8.0, WIDTH, HEIGHT, 9).expect("fixture geometry is valid"),
        &BUDGET,
        &CancellationToken::new(),
    )
    .expect("synthetic fixture font builds");
    let pixels = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'B')
        .expect("fixture has vertical mask")
        .mask();
    assert_ne!(
        run(&table, pixels, options(0.02, 0.02)).identity(),
        run(&table, pixels, options(0.03, 0.02)).identity()
    );
    let baseline = run(&table, pixels, options(0.02, 0.02)).identity();
    assert_ne!(
        baseline,
        run(&table, pixels, options(0.02, 0.03)).identity()
    );
    assert_ne!(
        baseline,
        run(
            &table,
            pixels,
            ContourOptions::new(2, 0.02, 0.02, 0.15, 0.75, 0.10)
                .expect("alternate weights are valid")
        )
        .identity()
    );
    assert_ne!(
        baseline,
        run(
            &table,
            pixels,
            ContourOptions::new(1, 0.02, 0.02, 0.10, 0.80, 0.10)
                .expect("one-candidate limit is valid")
        )
        .identity()
    );
}
