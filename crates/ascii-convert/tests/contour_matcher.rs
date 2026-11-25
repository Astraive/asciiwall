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
fn source_shape(kind: char, faint_texture: bool) -> Vec<u8> {
    (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).map(move |x| {
                let inside = match kind {
                    'h' => (4..8).contains(&y),
                    'v' => (2..6).contains(&x),
                    _ => (x as i32 - y as i32 / 2).abs() <= 1,
                };
                if inside {
                    255
                } else if faint_texture && (x + 2 * y).is_multiple_of(5) {
                    8
                } else {
                    0
                }
            })
        })
        .collect()
}

fn binary_silhouette_error(source: &[u8], mask: &[u8]) -> usize {
    source
        .iter()
        .zip(mask)
        .filter(|(source, mask)| (**source >= 128) != (**mask >= 128))
        .count()
}

fn source_tone_error(source: &[u8], mask: &[u8]) -> u64 {
    source
        .iter()
        .zip(mask)
        .map(|(source, mask)| u64::from(source.abs_diff(*mask)))
        .sum()
}

#[derive(Debug, Eq, PartialEq)]
enum Orientation {
    Horizontal,
    Vertical,
    RisingDiagonal,
    FallingDiagonal,
}

fn orientation(values: &[u8]) -> Orientation {
    let mut count = 0.0;
    let mut x_sum = 0.0;
    let mut y_sum = 0.0;
    for (index, value) in values.iter().enumerate() {
        if *value >= 128 {
            count += 1.0;
            x_sum += (index % WIDTH as usize) as f64;
            y_sum += (index / WIDTH as usize) as f64;
        }
    }
    let x_mean = x_sum / count;
    let y_mean = y_sum / count;
    let (mut xx, mut yy, mut xy) = (0.0, 0.0, 0.0);
    for (index, value) in values.iter().enumerate() {
        if *value >= 128 {
            let dx = (index % WIDTH as usize) as f64 - x_mean;
            let dy = (index / WIDTH as usize) as f64 - y_mean;
            xx += dx * dx;
            yy += dy * dy;
            xy += dx * dy;
        }
    }
    if xy.abs() > (xx - yy).abs() {
        if xy.is_sign_positive() {
            Orientation::RisingDiagonal
        } else {
            Orientation::FallingDiagonal
        }
    } else if xx > yy {
        Orientation::Horizontal
    } else {
        Orientation::Vertical
    }
}

fn blank_glyph(table: &ascii_convert::GlyphTable) -> u32 {
    table
        .glyphs()
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.mean_coverage().total_cmp(&b.mean_coverage()))
        .expect("fixture has glyphs")
        .0 as u32
}

fn selected_errors(
    table: &ascii_convert::GlyphTable,
    source: &[u8],
    conversion: &ascii_convert::ContourConversion,
) -> (usize, u64) {
    let selected = &table.glyphs()[conversion.frame().cells()[0].glyph() as usize];
    (
        binary_silhouette_error(source, selected.mask()),
        source_tone_error(source, selected.mask()),
    )
}

#[test]
// Isolates source-to-selected-glyph measurements from conversion scoring.
fn matched_source_comparison_measures_silhouette_tone_loss_and_work() {
    let table = build_glyph_table(
        &equal_area_orientation_font(),
        &GlyphRepertoire::new(" BA").expect("fixture repertoire is valid"),
        &GlyphConfig::new(8.0, WIDTH, HEIGHT, 9).expect("fixture geometry is valid"),
        &BUDGET,
        &CancellationToken::new(),
    )
    .expect("synthetic fixture font builds");
    let reference = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'B')
        .expect("synthetic font contains its vertical reference glyph")
        .mask();
    let source = reference.to_vec();
    let texture: Vec<u8> = source
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if *value == 0 && index.is_multiple_of(5) {
                8
            } else {
                *value
            }
        })
        .collect();
    let matched = run(&table, &source, options(0.0, 0.0));
    let texture_suppressed = run(&table, &texture, options(0.10, 0.02));
    let (matched_silhouette, matched_tone) = selected_errors(&table, &source, &matched);
    let (_, suppressed_tone) = selected_errors(&table, &texture, &texture_suppressed);
    assert_eq!(
        matched_silhouette, 0,
        "exact reference retains its silhouette"
    );
    assert_eq!(matched_tone, 0, "exact reference has no tone loss");
    assert!(
        suppressed_tone > 0,
        "thresholded texture causes measurable tone loss"
    );
    assert_eq!(matched.work_units_bound(), 2_513);
    assert_eq!(
        matched.work_units_bound(),
        texture_suppressed.work_units_bound()
    );
}

#[test]
fn oriented_edge_matches_independent_horizontal_vertical_and_diagonal_shapes() {
    for (font, kind) in [
        (equal_area_orientation_font(), 'h'),
        (equal_area_orientation_font(), 'v'),
        (support::synthetic_font::diagonal_orientation_font(), 'd'),
    ] {
        let table = build_glyph_table(
            &font,
            &GlyphRepertoire::new(" BA").expect("fixture repertoire is valid"),
            &GlyphConfig::new(8.0, WIDTH, HEIGHT, 9).expect("fixture geometry is valid"),
            &BUDGET,
            &CancellationToken::new(),
        )
        .expect("synthetic fixture font builds");
        let source = source_shape(kind, false);
        let first = run(&table, &source, options(0.0, 0.0));
        assert_eq!(first, run(&table, &source, options(0.0, 0.0)));
        let selected = &table.glyphs()[first.frame().cells()[0].glyph() as usize];
        assert_eq!(orientation(&source), orientation(selected.mask()));
        let (silhouette_error, tone_error) = selected_errors(&table, &source, &first);
        assert!(silhouette_error < source.len());
        assert!(tone_error < source.len() as u64 * u64::from(u8::MAX));
        let rerank_work = u64::from(WIDTH * HEIGHT) * 2 * 4;
        assert!(first.work_units_bound() >= rerank_work);
    }
}

#[test]
fn threshold_endpoints_distinguish_suppressed_and_retained_weak_contours() {
    let table = build_glyph_table(
        &equal_area_orientation_font(),
        &GlyphRepertoire::new(" BA").expect("fixture repertoire is valid"),
        &GlyphConfig::new(8.0, WIDTH, HEIGHT, 9).expect("fixture geometry is valid"),
        &BUDGET,
        &CancellationToken::new(),
    )
    .expect("synthetic fixture font builds");
    let weak_contour: Vec<u8> = (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).map(move |x| {
                if x >= 3 + y / 4 && x < 5 + y / 4 {
                    64
                } else {
                    0
                }
            })
        })
        .collect();
    for threshold in [0.0, 1.0] {
        let boundary = options(threshold, threshold);
        let first = run(&table, &weak_contour, boundary);
        assert_eq!(first, run(&table, &weak_contour, boundary));
        assert_eq!(first.frame().cells().len(), 1);
        assert!(first.work_units_bound() > 0);
        assert_eq!(boundary.contrast_threshold(), threshold);
        assert_eq!(boundary.edge_threshold(), threshold);
    }
    let retained = run(&table, &weak_contour, options(0.0, 0.0));
    let suppressed = run(&table, &weak_contour, options(1.0, 1.0));
    assert_ne!(retained.frame().cells()[0].glyph(), blank_glyph(&table));
    assert_eq!(suppressed.frame().cells()[0].glyph(), blank_glyph(&table));
    assert_eq!(retained.work_units_bound(), suppressed.work_units_bound());
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
    let tone_loss = source_tone_error(&faint, background.mask());
    assert!(tone_loss > 0, "suppressed source tone remains measurable");
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
    let pixels = source_shape('v', false);
    assert_ne!(
        run(&table, &pixels, options(0.02, 0.02)).identity(),
        run(&table, &pixels, options(0.03, 0.02)).identity()
    );
    let baseline = run(&table, &pixels, options(0.02, 0.02)).identity();
    assert_ne!(
        baseline,
        run(&table, &pixels, options(0.02, 0.03)).identity()
    );
    assert_ne!(
        baseline,
        run(
            &table,
            &pixels,
            ContourOptions::new(2, 0.02, 0.02, 0.15, 0.75, 0.10)
                .expect("alternate weights are valid")
        )
        .identity()
    );
    assert_ne!(
        baseline,
        run(
            &table,
            &pixels,
            ContourOptions::new(1, 0.02, 0.02, 0.10, 0.80, 0.10)
                .expect("one-candidate limit is valid")
        )
        .identity()
    );
}
