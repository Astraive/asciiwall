#[allow(dead_code)]
mod support;

use ascii_convert::{
    ColorFitError, ColorFitStrategy, ConversionSettings, EncodedSrgb, GlyphConfig, GlyphRepertoire,
    GlyphTable, GradientStop, GridSizing, LinearRgb, OutputEncoding, Palette, PaletteDefinition,
    PaletteDistanceSpace, PaletteError, PaletteKind, MAX_GRADIENT_STOPS, MAX_PALETTE_COLORS,
    MAX_PALETTE_PAIR_RESOLUTION_WORK_UNITS, MAX_PALETTE_RESOLUTION_WORK_UNITS,
    build_glyph_table, convert_density_detailed, decode_srgb, encode_output, encode_srgb,
    estimate_color_fit_work, fit_color_patch,
};
use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, Error, FrameMetadata,
    GlyphTableId, GridSize, PixelFormat, RasterFrameView, RasterOrientation,
};
use support::synthetic_font::synthetic_font;
use sha2::{Digest, Sha256};


const LARGE_BUDGET: u64 = 100_000_000;
const SYNTHETIC_COLOR_FIT_CONFIGURATION: &str = concat!(
    "synthetic_font(false);repertoire=\"BA \";cell=8x12;baseline=9;",
    "source=RGB8/sRGB/opaque/top-down;",
    "reconstruction=8x12;tiles=[08,24,48],[220,184,88],[36,172,204],[180,52,128];",
    "index=(x+3*y)%4;",
    "two-cell=16x12;left=[28,94,176];right=[211,126,36];",
    "palette=original,monochrome,fixed,ansi16,ansi256,gradient;",
    "monochrome=linear:[0.9,0.15,0.1]/[0.1,0.2,0.9];",
    "fixed=[black,white,srgb8:[12,32,56],[228,170,76]];",
    "palette-scene=srgb8:[12,32,56],[228,170,76],[42,188,220],[192,64,136];",
    "palette-index=(x+3*y)%4;",
    "gradient-stops=0:black,0.5:(0.2,0.65,0.8),1:white;",
    "inflight-cancel=16x18;ansi256;three-glyphs;",
    "fit=two-color-least-squares;fixed-background=linear-black;",
);

fn color(red: f64, green: f64, blue: f64) -> LinearRgb {
    LinearRgb::new(red, green, blue).expect("synthetic color is in range")
}

fn encoded_color(red: u8, green: u8, blue: u8) -> LinearRgb {
    decode_srgb(
        EncodedSrgb::new(
            f64::from(red) / 255.0,
            f64::from(green) / 255.0,
            f64::from(blue) / 255.0,
        )
        .expect("synthetic encoded color is in range"),
    )
}

fn budget(work_units: u64) -> Budget {
    Budget::new(LARGE_BUDGET, LARGE_BUDGET, work_units)
}

fn palette(definition: PaletteDefinition<'_>, space: PaletteDistanceSpace) -> Palette {
    let cancellation = CancellationToken::new();
    Palette::new(definition, space, &budget(LARGE_BUDGET), &cancellation)
        .expect("synthetic palette is valid")
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!((actual - expected).abs() <= tolerance, "{actual} != {expected}");
}

fn encoded_distance_squared(left: LinearRgb, right: LinearRgb) -> f64 {
    let left = encode_srgb(left);
    let right = encode_srgb(right);
    (left.red() - right.red()).powi(2)
        + (left.green() - right.green()).powi(2)
        + (left.blue() - right.blue()).powi(2)
}

#[test]
fn original_and_monochrome_preserve_their_distinct_pair_semantics() {
    let original = palette(
        PaletteDefinition::Original,
        PaletteDistanceSpace::LinearRgb,
    );
    let mono_fg = color(1.0, 0.1, 0.0);
    let mono_bg = color(0.0, 0.1, 1.0);
    let source_fg = color(0.7, 0.2, 0.1);
    let source_bg = color(0.1, 0.2, 0.7);
    let cancellation = CancellationToken::new();
    let original_pair = original
        .resolve_pair(source_fg, source_bg, &budget(0), &cancellation)
        .expect("original mode needs no search work");
    assert_eq!(original_pair.foreground().color(), source_fg);
    assert_eq!(original_pair.background().color(), source_bg);

    let monochrome = palette(
        PaletteDefinition::Monochrome {
            foreground: mono_fg,
            background: mono_bg,
        },
        PaletteDistanceSpace::EncodedSrgb,
    );
    let mono_pair = monochrome
        .resolve_pair(source_fg, source_bg, &budget(0), &cancellation)
        .expect("monochrome mode returns its explicit pair");
    assert_eq!(monochrome.kind(), PaletteKind::Monochrome);
    assert_eq!(mono_pair.foreground().color(), mono_fg);
    assert_eq!(mono_pair.background().color(), mono_bg);
}

#[test]
fn fixed_palette_ties_are_stable_and_distance_space_is_part_of_identity() {
    let colors = [color(1.0, 0.0, 0.0), color(0.0, 0.0, 1.0)];
    let linear = palette(
        PaletteDefinition::Fixed(&colors),
        PaletteDistanceSpace::LinearRgb,
    );
    let encoded = palette(
        PaletteDefinition::Fixed(&colors),
        PaletteDistanceSpace::EncodedSrgb,
    );
    let cancellation = CancellationToken::new();
    let midpoint = color(0.5, 0.0, 0.5);
    for _ in 0..8 {
        let match_color = linear
            .resolve(midpoint, &budget(6), &cancellation)
            .expect("two fixed colors fit the search budget");
        assert_eq!(match_color.index(), Some(0));
        assert_eq!(match_color.color(), colors[0]);
    }
    assert_eq!(
        linear
            .resolve(midpoint, &budget(5), &cancellation)
            .expect_err("palette lookup rejects insufficient work before iteration"),
        PaletteError::Core(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
    assert_ne!(linear.cache_identity(), encoded.cache_identity());
    let duplicate_colors = [LinearRgb::BLACK, LinearRgb::BLACK];
    let duplicate_palette = palette(
        PaletteDefinition::Fixed(&duplicate_colors),
        PaletteDistanceSpace::LinearRgb,
    );
    let duplicate_match = duplicate_palette
        .resolve(LinearRgb::BLACK, &budget(6), &cancellation)
        .expect("duplicate colors are accepted");
    assert_eq!(duplicate_match.index(), Some(0));
}

#[test]
fn ansi_palettes_resolve_explicit_xterm_colors_and_report_terminal_caveat() {
    let cancellation = CancellationToken::new();
    let ansi16 = palette(
        PaletteDefinition::Ansi16,
        PaletteDistanceSpace::LinearRgb,
    );
    let black = ansi16
        .resolve(LinearRgb::BLACK, &budget(48), &cancellation)
        .expect("ANSI-16 lookup is bounded");
    assert_eq!(black.index(), Some(0));
    assert_eq!(black.color(), LinearRgb::BLACK);
    assert!(ansi16.terminal_appearance_notice().is_some());

    let ansi256 = palette(
        PaletteDefinition::Ansi256,
        PaletteDistanceSpace::LinearRgb,
    );
    let cube_color = encoded_color(215, 135, 0);
    let resolved = ansi256
        .resolve(cube_color, &budget(768), &cancellation)
        .expect("ANSI-256 cube lookup is bounded");
    assert_eq!(resolved.index(), Some(172));
    assert_eq!(resolved.color(), cube_color);
    assert_eq!(ansi256.kind(), PaletteKind::Ansi256);
}

#[test]
fn gradient_requires_ordered_distinct_positions_and_interpolates_linear_light_endpoints() {
    let stops = [
        GradientStop::new(0.0, LinearRgb::BLACK).expect("valid start"),
        GradientStop::new(1.0, LinearRgb::WHITE).expect("valid end"),
    ];
    let gradient = palette(
        PaletteDefinition::Gradient(&stops),
        PaletteDistanceSpace::LinearRgb,
    );
    let cancellation = CancellationToken::new();
    let middle = gradient
        .resolve(color(0.25, 0.25, 0.25), &budget(3), &cancellation)
        .expect("linear gradient projection is exact");
    close(middle.color().red(), 0.25, 1.0e-12);
    close(middle.color().green(), 0.25, 1.0e-12);
    close(middle.color().blue(), 0.25, 1.0e-12);
    close(middle.gradient_position().expect("gradient position"), 0.25, 1.0e-12);


    assert_eq!(
        GradientStop::new(f64::NAN, LinearRgb::BLACK),
        Err(PaletteError::InvalidDefinition)
    );
    assert_eq!(
        GradientStop::new(f64::INFINITY, LinearRgb::BLACK),
        Err(PaletteError::InvalidDefinition)
    );
    assert_eq!(
        GradientStop::new(1.1, LinearRgb::BLACK),
        Err(PaletteError::InvalidDefinition)
    );
    let duplicate_positions = [
        GradientStop::new(0.0, LinearRgb::BLACK).expect("valid start"),
        GradientStop::new(0.5, color(0.3, 0.3, 0.3)).expect("valid stop"),
        GradientStop::new(0.5, color(0.7, 0.7, 0.7)).expect("position is individually valid"),
        GradientStop::new(1.0, LinearRgb::WHITE).expect("valid end"),
    ];
    let error = Palette::new(
        PaletteDefinition::Gradient(&duplicate_positions),
        PaletteDistanceSpace::LinearRgb,
        &budget(LARGE_BUDGET),
        &cancellation,
    )
    .expect_err("duplicate ordered positions are malformed");
    assert_eq!(error, PaletteError::InvalidDefinition);

    let unsorted = [
        GradientStop::new(0.0, LinearRgb::BLACK).expect("valid start"),
        GradientStop::new(0.8, color(0.8, 0.8, 0.8)).expect("valid position"),
        GradientStop::new(0.7, color(0.7, 0.7, 0.7)).expect("valid position"),
        GradientStop::new(1.0, LinearRgb::WHITE).expect("valid end"),
    ];
    assert_eq!(
        Palette::new(
            PaletteDefinition::Gradient(&unsorted),
            PaletteDistanceSpace::LinearRgb,
            &budget(LARGE_BUDGET),
            &cancellation,
        )
        .expect_err("unsorted stops fail validation"),
        PaletteError::InvalidDefinition
    );
    let missing_endpoint = [
        GradientStop::new(0.2, color(0.2, 0.2, 0.2)).expect("valid position"),
        GradientStop::new(1.0, LinearRgb::WHITE).expect("valid end"),
    ];
    assert_eq!(
        Palette::new(
            PaletteDefinition::Gradient(&missing_endpoint),
            PaletteDistanceSpace::LinearRgb,
            &budget(LARGE_BUDGET),
            &cancellation,
        )
        .expect_err("gradient must begin at position zero"),
        PaletteError::InvalidDefinition
    );

    let duplicate_colors = [
        GradientStop::new(0.0, LinearRgb::BLACK).expect("valid start"),
        GradientStop::new(0.5, LinearRgb::BLACK).expect("valid duplicate color"),
        GradientStop::new(1.0, LinearRgb::BLACK).expect("valid end"),
    ];
    let constant_gradient = palette(
        PaletteDefinition::Gradient(&duplicate_colors),
        PaletteDistanceSpace::LinearRgb,
    );
    let match_color = constant_gradient
        .resolve(LinearRgb::BLACK, &budget(6), &cancellation)
        .expect("duplicate gradient colors are valid");
    assert_eq!(match_color.gradient_position(), Some(0.0));
}

#[test]
fn encoded_gradient_search_finds_global_minimum_without_unimodality_assumptions() {
    let stops = [
        GradientStop::new(0.0, color(0.345_250_081, 0.016_078_705, 0.188_061_400))
            .expect("valid start"),
        GradientStop::new(1.0, color(0.000_904_923, 0.095_861_072, 0.860_698_881))
            .expect("valid end"),
    ];
    let gradient = palette(
        PaletteDefinition::Gradient(&stops),
        PaletteDistanceSpace::EncodedSrgb,
    );
    let requested = color(0.000_244_244, 0.304_238_494, 0.024_489_139);
    let work = gradient.resolution_work_units();
    assert_eq!(work, 513);
    let cancellation = CancellationToken::new();
    let resolved = gradient
        .resolve(requested, &budget(work), &cancellation)
        .expect("bounded global search resolves this gradient");
    let position = resolved.gradient_position().expect("gradient position");
    assert!(position > 0.95, "expected the global basin near the end, got {position}");
    let reference_parameter = 0.976;
    let reference = color(
        stops[0].color().red()
            + (stops[1].color().red() - stops[0].color().red()) * reference_parameter,
        stops[0].color().green()
            + (stops[1].color().green() - stops[0].color().green()) * reference_parameter,
        stops[0].color().blue()
            + (stops[1].color().blue() - stops[0].color().blue()) * reference_parameter,
    );
    assert!(
        encoded_distance_squared(requested, resolved.color())
            <= encoded_distance_squared(requested, reference) + 1.0e-6
    );
    assert_eq!(
        gradient
            .resolve(requested, &budget(work - 1), &cancellation)
            .expect_err("encoded search rejects a short work budget"),
        PaletteError::Core(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
}

#[test]
fn encoded_gradient_interval_bounds_preserve_a_nonzero_endpoint_minimum() {
    let stops = [
        GradientStop::new(0.0, color(0.5, 0.0, 0.0)).expect("valid start"),
        GradientStop::new(1.0, color(0.4, 0.01, 0.0)).expect("valid end"),
    ];
    let gradient = palette(
        PaletteDefinition::Gradient(&stops),
        PaletteDistanceSpace::EncodedSrgb,
    );
    let requested = color(1.0, 0.001, 0.0);
    let resolved = gradient
        .resolve(
            requested,
            &budget(gradient.resolution_work_units()),
            &CancellationToken::new(),
        )
        .expect("interval bounds tighten within the search limit");
    assert_eq!(resolved.color(), stops[0].color());
    assert_eq!(resolved.gradient_position(), Some(0.0));
    assert!(encoded_distance_squared(requested, resolved.color()) > 0.0);
}

#[test]
fn encoded_gradient_exact_ties_keep_the_earliest_segment() {
    let stops = [
        GradientStop::new(0.0, LinearRgb::BLACK).expect("valid start"),
        GradientStop::new(0.5, color(0.0001, 0.0, 0.0)).expect("valid middle"),
        GradientStop::new(1.0, color(0.00005, 0.0, 0.0)).expect("valid end"),
    ];
    let gradient = palette(
        PaletteDefinition::Gradient(&stops),
        PaletteDistanceSpace::EncodedSrgb,
    );
    let exact_color = color(0.00005, 0.0, 0.0);
    let resolved = gradient
        .resolve(
            exact_color,
            &budget(gradient.resolution_work_units()),
            &CancellationToken::new(),
        )
        .expect("exact matching segments resolve deterministically");
    assert_eq!(resolved.color(), exact_color);
    assert_eq!(resolved.gradient_position(), Some(0.25));
}

#[test]
fn encoded_gradient_tiny_exact_match_retains_earliest_tied_segment() {
    let stops = [
        GradientStop::new(0.0, LinearRgb::BLACK).expect("valid start"),
        GradientStop::new(0.5, color(1.0e-200, 0.0, 0.0)).expect("valid middle"),
        GradientStop::new(1.0, color(5.0e-201, 0.0, 0.0)).expect("valid end"),
    ];
    let gradient = palette(
        PaletteDefinition::Gradient(&stops),
        PaletteDistanceSpace::EncodedSrgb,
    );
    let requested = color(5.0e-201, 0.0, 0.0);
    let resolved = gradient
        .resolve(
            requested,
            &budget(gradient.resolution_work_units()),
            &CancellationToken::new(),
        )
        .expect("tiny finite colors remain exactly searchable");
    assert_eq!(resolved.color(), requested);
    assert_eq!(resolved.gradient_position(), Some(0.25));
}

#[test]
fn palette_resolution_work_covers_each_mode_and_cancellation() {
    let fixed_colors = [LinearRgb::BLACK, LinearRgb::WHITE];
    let linear_stops = [
        GradientStop::new(0.0, LinearRgb::BLACK).expect("valid start"),
        GradientStop::new(1.0, LinearRgb::WHITE).expect("valid end"),
    ];
    let original = palette(
        PaletteDefinition::Original,
        PaletteDistanceSpace::LinearRgb,
    );
    let monochrome = palette(
        PaletteDefinition::Monochrome {
            foreground: LinearRgb::BLACK,
            background: LinearRgb::WHITE,
        },
        PaletteDistanceSpace::LinearRgb,
    );
    let fixed = palette(
        PaletteDefinition::Fixed(&fixed_colors),
        PaletteDistanceSpace::LinearRgb,
    );
    let ansi16 = palette(
        PaletteDefinition::Ansi16,
        PaletteDistanceSpace::LinearRgb,
    );
    let ansi256 = palette(
        PaletteDefinition::Ansi256,
        PaletteDistanceSpace::LinearRgb,
    );
    let linear_gradient = palette(
        PaletteDefinition::Gradient(&linear_stops),
        PaletteDistanceSpace::LinearRgb,
    );
    let encoded_gradient = palette(
        PaletteDefinition::Gradient(&linear_stops),
        PaletteDistanceSpace::EncodedSrgb,
    );
    let modes = [
        (&original, 0),
        (&monochrome, 6),
        (&fixed, 6),
        (&ansi16, 48),
        (&ansi256, 768),
        (&linear_gradient, 3),
        (&encoded_gradient, 513),
    ];
    let cancellation = CancellationToken::new();
    for (palette, expected_work) in modes {
        assert_eq!(palette.resolution_work_units(), expected_work);
        palette
            .resolve(LinearRgb::BLACK, &budget(expected_work), &cancellation)
            .expect("the declared maximum resolution work is sufficient");
        if expected_work > 0 {
            assert_eq!(
                palette
                    .resolve(
                        LinearRgb::BLACK,
                        &budget(expected_work - 1),
                        &cancellation,
                    )
                    .expect_err("one unit below the declared maximum is rejected"),
                PaletteError::Core(Error::BudgetExceeded(BudgetKind::WorkUnits))
            );
        }
    }

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    for (palette, _) in modes {
        assert_eq!(
            palette
                .resolve(LinearRgb::BLACK, &budget(0), &cancelled)
                .expect_err("cancellation wins over a zero work budget"),
            PaletteError::Core(Error::Cancelled)
        );
    }

    let last_index =
        f64::from(u32::try_from(MAX_GRADIENT_STOPS - 1).expect("maximum stop index fits in u32"));
    let maximum_stops: Vec<_> = (0..MAX_GRADIENT_STOPS)
        .map(|index| {
            GradientStop::new(
                f64::from(u32::try_from(index).expect("stop index fits in u32")) / last_index,
                LinearRgb::BLACK,
            )
            .expect("maximum-size gradient stop is valid")
        })
        .collect();
    let maximum_encoded_gradient = palette(
        PaletteDefinition::Gradient(&maximum_stops),
        PaletteDistanceSpace::EncodedSrgb,
    );
    let maximum_work =
        171 * 3 * u64::try_from(MAX_GRADIENT_STOPS - 1).expect("segment count fits in u64");
    assert_eq!(
        maximum_encoded_gradient.resolution_work_units(),
        maximum_work
    );
    assert_eq!(maximum_work, 65_151);
    assert!(maximum_work <= MAX_PALETTE_RESOLUTION_WORK_UNITS);

    let target = [LinearRgb::BLACK];
    let coverage = [255];
    assert_eq!(
        fit_color_patch(
            &target,
            &coverage,
            &original,
            ColorFitStrategy::TwoColorLeastSquares,
            &budget(0),
            &cancelled,
        )
        .expect_err("fit cancellation is returned before work"),
        ColorFitError::Core(Error::Cancelled)
    );
}


#[test]
fn palette_pair_budget_allows_two_individually_bounded_gradient_resolutions() {
    let stop_count = 65;
    let last_index = f64::from(u32::try_from(stop_count - 1).expect("stop index fits in u32"));
    let stops: Vec<_> = (0..stop_count)
        .map(|index| {
            GradientStop::new(
                f64::from(u32::try_from(index).expect("stop index fits in u32")) / last_index,
                LinearRgb::BLACK,
            )
            .expect("valid gradient stop")
        })
        .collect();
    let gradient = palette(
        PaletteDefinition::Gradient(&stops),
        PaletteDistanceSpace::EncodedSrgb,
    );
    let pair_work = gradient.resolution_work_units() * 2;
    assert_eq!(gradient.resolution_work_units(), 32_832);
    assert_eq!(pair_work, 65_664);
    assert!(pair_work > MAX_PALETTE_RESOLUTION_WORK_UNITS);
    assert!(pair_work <= MAX_PALETTE_PAIR_RESOLUTION_WORK_UNITS);

    let cancellation = CancellationToken::new();
    let pair = gradient
        .resolve_pair(
            LinearRgb::BLACK,
            LinearRgb::WHITE,
            &budget(pair_work),
            &cancellation,
        )
        .expect("the pair hard limit permits both individually bounded resolutions");
    assert_eq!(pair.foreground().color(), LinearRgb::BLACK);
    assert_eq!(pair.background().color(), LinearRgb::BLACK);
    assert_eq!(
        gradient
            .resolve_pair(
                LinearRgb::BLACK,
                LinearRgb::WHITE,
                &budget(pair_work - 1),
                &cancellation,
            )
            .expect_err("pair work is preflighted against the caller budget"),
        PaletteError::Core(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
}
#[test]
fn fixed_background_fit_preflights_encoded_palette_resolution_work() {
    let stops = [
        GradientStop::new(0.0, LinearRgb::BLACK).expect("valid start"),
        GradientStop::new(1.0, LinearRgb::WHITE).expect("valid end"),
    ];
    let gradient = palette(
        PaletteDefinition::Gradient(&stops),
        PaletteDistanceSpace::EncodedSrgb,
    );
    let strategy = ColorFitStrategy::FixedBackground(LinearRgb::BLACK);
    let estimate = estimate_color_fit_work(1, &gradient, strategy).expect("work estimate");
    assert_eq!(estimate, 3 * 3 + gradient.resolution_work_units());
    let cancellation = CancellationToken::new();
    let target = [LinearRgb::BLACK];
    let coverage = [255];
    assert_eq!(
        fit_color_patch(
            &target,
            &coverage,
            &gradient,
            strategy,
            &budget(estimate - 1),
            &cancellation,
        )
        .expect_err("fixed-background fitting charges its palette resolution"),
        ColorFitError::Core(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
    let fit = fit_color_patch(
        &target,
        &coverage,
        &gradient,
        strategy,
        &budget(estimate),
        &cancellation,
    )
    .expect("the declared maximum fit work is sufficient");
    assert_eq!(fit.work_units(), estimate);
}

#[test]
fn empty_palette_and_palette_storage_budget_fail_before_resolution() {
    let cancellation = CancellationToken::new();
    let empty: [LinearRgb; 0] = [];
    assert_eq!(
        Palette::new(
            PaletteDefinition::Fixed(&empty),
            PaletteDistanceSpace::LinearRgb,
            &budget(LARGE_BUDGET),
            &cancellation,
        )
        .expect_err("empty fixed palette is invalid"),
        PaletteError::InvalidDefinition
    );
    let too_many = vec![LinearRgb::BLACK; MAX_PALETTE_COLORS + 1];
    assert_eq!(
        Palette::new(
            PaletteDefinition::Fixed(&too_many),
            PaletteDistanceSpace::LinearRgb,
            &budget(LARGE_BUDGET),
            &cancellation,
        )
        .expect_err("fixed palette hard count is enforced"),
        PaletteError::InvalidDefinition
    );

    let colors = [LinearRgb::BLACK, LinearRgb::WHITE];
    let too_small = Budget::new(LARGE_BUDGET, 1, LARGE_BUDGET);
    assert_eq!(
        Palette::new(
            PaletteDefinition::Fixed(&colors),
            PaletteDistanceSpace::LinearRgb,
            &too_small,
            &cancellation,
        )
        .expect_err("palette copies are budgeted"),
        PaletteError::Core(Error::BudgetExceeded(BudgetKind::RasterBytes))
    );
}

#[test]
fn two_color_fit_reconstructs_both_polarities_with_candidate_specific_colors() {
    let background = color(0.1, 0.2, 0.3);
    let foreground = color(0.9, 0.8, 0.7);
    let coverage = [0, 85, 170, 255];
    let target = coverage.map(|mask| {
        let mask = f64::from(mask) / 255.0;
        color(
            background.red() + (foreground.red() - background.red()) * mask,
            background.green() + (foreground.green() - background.green()) * mask,
            background.blue() + (foreground.blue() - background.blue()) * mask,
        )
    });
    let original = palette(
        PaletteDefinition::Original,
        PaletteDistanceSpace::LinearRgb,
    );
    let cancellation = CancellationToken::new();
    let fit = fit_color_patch(
        &target,
        &coverage,
        &original,
        ColorFitStrategy::TwoColorLeastSquares,
        &budget(10_000),
        &cancellation,
    )
    .expect("full rank mask has a finite joint fit");
    close(fit.foreground().color().red(), foreground.red(), 1.0e-10);
    close(fit.foreground().color().green(), foreground.green(), 1.0e-10);
    close(fit.background().color().red(), background.red(), 1.0e-10);
    close(fit.background().color().green(), background.green(), 1.0e-10);
    assert!(fit.mean_squared_error() < 1.0e-20);

    let light_on_dark = [LinearRgb::BLACK, LinearRgb::WHITE];
    let dark_on_light = [LinearRgb::WHITE, LinearRgb::BLACK];
    let binary_mask = [0, 255];
    let light_fit = fit_color_patch(
        &light_on_dark,
        &binary_mask,
        &original,
        ColorFitStrategy::TwoColorLeastSquares,
        &budget(1_000),
        &cancellation,
    )
    .expect("light foreground polarity fits");
    let dark_fit = fit_color_patch(
        &dark_on_light,
        &binary_mask,
        &original,
        ColorFitStrategy::TwoColorLeastSquares,
        &budget(1_000),
        &cancellation,
    )
    .expect("dark foreground polarity fits");
    assert_eq!(light_fit.foreground().color(), LinearRgb::WHITE);
    assert_eq!(light_fit.background().color(), LinearRgb::BLACK);
    assert_eq!(dark_fit.foreground().color(), LinearRgb::BLACK);
    assert_eq!(dark_fit.background().color(), LinearRgb::WHITE);
    assert_eq!(light_fit.mean_squared_error(), 0.0);
    assert_eq!(dark_fit.mean_squared_error(), 0.0);
}

#[test]
fn fixed_background_fit_clamps_and_preserves_the_requested_background() {
    let background = color(0.2, 0.2, 0.2);
    let target = [color(0.0, 0.0, 0.0), color(0.9, 0.6, 0.3)];
    let coverage = [0, 255];
    let original = palette(
        PaletteDefinition::Original,
        PaletteDistanceSpace::LinearRgb,
    );
    let cancellation = CancellationToken::new();
    let fit = fit_color_patch(
        &target,
        &coverage,
        &original,
        ColorFitStrategy::FixedBackground(background),
        &budget(1_000),
        &cancellation,
    )
    .expect("fixed-background fit is bounded");
    assert_eq!(fit.background().color(), background);
    assert_eq!(fit.foreground().color(), color(0.9, 0.6, 0.3));
    assert!(fit.mean_squared_error() >= 0.0);
    let barely_inked = fit_color_patch(
        &[LinearRgb::WHITE],
        &[1],
        &original,
        ColorFitStrategy::FixedBackground(LinearRgb::BLACK),
        &budget(1_000),
        &cancellation,
    )
    .expect("fixed-background foreground is clamped to the color range");
    assert_eq!(barely_inked.foreground().color(), LinearRgb::WHITE);
    close(
        barely_inked.mean_squared_error(),
        (254.0_f64 / 255.0).powi(2),
        1.0e-12,
    );

}

#[test]
fn zero_full_constant_and_near_degenerate_masks_are_finite_and_repeatable() {
    let target = [color(0.1, 0.4, 0.8), color(0.9, 0.6, 0.2)];
    let original = palette(
        PaletteDefinition::Original,
        PaletteDistanceSpace::LinearRgb,
    );
    let cancellation = CancellationToken::new();

    for mask in [[0, 0], [255, 255], [128, 128]] {
        let first = fit_color_patch(
            &target,
            &mask,
            &original,
            ColorFitStrategy::TwoColorLeastSquares,
            &budget(1_000),
            &cancellation,
        )
        .expect("singular masks use the equal-color mean solution");
        let second = fit_color_patch(
            &target,
            &mask,
            &original,
            ColorFitStrategy::TwoColorLeastSquares,
            &budget(1_000),
            &cancellation,
        )
        .expect("repeat fit is deterministic");
        assert_eq!(first, second);
        assert_eq!(first.foreground().color(), first.background().color());
        assert!(first.mean_squared_error().is_finite());
        assert!((0.0..=1.0).contains(&first.foreground().color().red()));
    }

    let near_degenerate = [127, 128];
    let near_first = fit_color_patch(
        &target,
        &near_degenerate,
        &original,
        ColorFitStrategy::TwoColorLeastSquares,
        &budget(1_000),
        &cancellation,
    )
    .expect("adjacent coverage values remain numerically stable");
    let near_second = fit_color_patch(
        &target,
        &near_degenerate,
        &original,
        ColorFitStrategy::TwoColorLeastSquares,
        &budget(1_000),
        &cancellation,
    )
    .expect("near-degenerate repeat is deterministic");
    assert_eq!(near_first, near_second);
    assert!(near_first.mean_squared_error().is_finite());
    for channel in [
        near_first.foreground().color().red(),
        near_first.foreground().color().green(),
        near_first.foreground().color().blue(),
        near_first.background().color().red(),
        near_first.background().color().green(),
        near_first.background().color().blue(),
    ] {
        assert!((0.0..=1.0).contains(&channel));
    }
}

#[test]
fn finite_palette_pair_search_scores_the_actual_allowed_pair() {
    let colors = [LinearRgb::BLACK, LinearRgb::WHITE];
    let fixed = palette(
        PaletteDefinition::Fixed(&colors),
        PaletteDistanceSpace::LinearRgb,
    );
    let target = [color(0.1, 0.1, 0.1), color(0.9, 0.9, 0.9)];
    let mask = [0, 255];
    let cancellation = CancellationToken::new();
    let fit = fit_color_patch(
        &target,
        &mask,
        &fixed,
        ColorFitStrategy::TwoColorLeastSquares,
        &budget(100_000),
        &cancellation,
    )
    .expect("finite palette pair search is bounded");
    assert_eq!(fit.foreground().color(), LinearRgb::WHITE);
    assert_eq!(fit.foreground().index(), Some(1));
    assert_eq!(fit.background().color(), LinearRgb::BLACK);
    assert_eq!(fit.background().index(), Some(0));
    close(fit.mean_squared_error(), 0.01, 1.0e-12);
}

#[test]
fn gradient_fit_returns_colors_on_the_gradient_and_budget_preflight_is_explicit() {
    let stops = [
        GradientStop::new(0.0, LinearRgb::BLACK).expect("valid start"),
        GradientStop::new(1.0, LinearRgb::WHITE).expect("valid end"),
    ];
    let gradient = palette(
        PaletteDefinition::Gradient(&stops),
        PaletteDistanceSpace::LinearRgb,
    );
    let target = [color(0.1, 0.2, 0.3), color(0.8, 0.7, 0.6)];
    let mask = [0, 255];
    let cancellation = CancellationToken::new();
    let fit = fit_color_patch(
        &target,
        &mask,
        &gradient,
        ColorFitStrategy::TwoColorLeastSquares,
        &budget(100_000),
        &cancellation,
    )
    .expect("gradient pair is fit against the same glyph mask");
    assert!(fit.foreground().gradient_position().is_some());
    assert!(fit.background().gradient_position().is_some());
    assert!(fit.mean_squared_error().is_finite());

    let ansi256 = palette(
        PaletteDefinition::Ansi256,
        PaletteDistanceSpace::LinearRgb,
    );
    let estimate = estimate_color_fit_work(
        2,
        &ansi256,
        ColorFitStrategy::TwoColorLeastSquares,
    )
    .expect("checked work estimate");
    assert_eq!(estimate, 65_537 * 6);
    let error = fit_color_patch(
        &target,
        &mask,
        &ansi256,
        ColorFitStrategy::TwoColorLeastSquares,
        &budget(100_000),
        &cancellation,
    )
    .expect_err("fit rejects work above the explicit budget before searching pairs");
    assert_eq!(
        error,
        ColorFitError::Core(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
}

#[test]
fn invalid_color_fit_patch_is_rejected_before_work() {
    let original = palette(
        PaletteDefinition::Original,
        PaletteDistanceSpace::LinearRgb,
    );
    let cancellation = CancellationToken::new();
    assert_eq!(
        fit_color_patch(
            &[],
            &[],
            &original,
            ColorFitStrategy::TwoColorLeastSquares,
            &budget(0),
            &cancellation,
        )
        .expect_err("empty patch is invalid"),
        ColorFitError::InvalidPatch
    );
}
fn conversion_table() -> GlyphTable {
    build_glyph_table(
        &synthetic_font(false),
        &GlyphRepertoire::new("BA ").expect("fixture repertoire is valid"),
        &GlyphConfig::new(8.0, 8, 12, 9).expect("fixture cell geometry is valid"),
        &Budget::new(1_000_000, 2_000_000, 2_000_000),
        &CancellationToken::new(),
    )
    .expect("synthetic glyph table builds")
}

fn conversion_metadata(table: &GlyphTable) -> FrameMetadata {
    FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 19, 7, -123, 41_000)
}

fn conversion_settings<'a>(
    table: &GlyphTable,
    cancellation: &'a CancellationToken,
    palette: &'a Palette,
    strategy: ColorFitStrategy,
    width: u32,
    height: u32,
) -> ConversionSettings<'a> {
    ConversionSettings::new(
        GridSizing::fixed_pixel_surface(width, height),
        conversion_metadata(table),
        cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .expect("conversion settings are valid")
    .with_color_fit(palette, strategy)
}

fn rgb_raster(pixels: &[u8], width: u32, height: u32) -> RasterFrameView<'_> {
    RasterFrameView::new(
        width,
        height,
        usize::try_from(width).expect("fixture width fits usize") * 3,
        PixelFormat::rgb8(),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
        pixels,
    )
}

fn rgb8(color: LinearRgb) -> [u8; 3] {
    let channels = encode_output(color, OutputEncoding::Srgb8).channels();
    [channels[0], channels[1], channels[2]]
}

fn rgba8(color: LinearRgb) -> [u8; 4] {
    let channels = rgb8(color);
    [channels[0], channels[1], channels[2], u8::MAX]
}

fn assert_palette_member(
    palette: &Palette,
    encoded: [u8; 3],
    budget: &Budget,
    cancellation: &CancellationToken,
) {
    let decoded = encoded_color(encoded[0], encoded[1], encoded[2]);
    let resolved = palette
        .resolve(decoded, budget, cancellation)
        .expect("resolved frame color remains within its palette");
    match palette.kind() {
        PaletteKind::Fixed | PaletteKind::Ansi16 | PaletteKind::Ansi256 => {
            assert_eq!(rgb8(resolved.color()), encoded);
        }
        PaletteKind::Gradient => {
            let distance = (decoded.red() - resolved.color().red()).powi(2)
                + (decoded.green() - resolved.color().green()).powi(2)
                + (decoded.blue() - resolved.color().blue()).powi(2);
            assert!(distance <= 0.0001, "frame color is off the gradient: {distance}");
        }
        PaletteKind::Original | PaletteKind::Monochrome => {
            panic!("explicit mode does not use nearest-color validation")
        }
    }
}

#[test]
fn reconstruction_candidate_scoring_uses_each_color_solution_in_conversion() {
    let table = conversion_table();
    let mut pixels = Vec::new();
    let color_pattern = [
        [8, 24, 48],
        [220, 184, 88],
        [36, 172, 204],
        [180, 52, 128],
    ];
    for y in 0..12 {
        for x in 0..8 {
            let channels = color_pattern[(x + 3 * y) % color_pattern.len()];
            pixels.extend_from_slice(&channels);
        }
    }
    let target: Vec<_> = pixels
        .chunks_exact(3)
        .map(|sample| encoded_color(sample[0], sample[1], sample[2]))
        .collect();
    let original = palette(
        PaletteDefinition::Original,
        PaletteDistanceSpace::LinearRgb,
    );
    let cancellation = CancellationToken::new();
    let strategy = ColorFitStrategy::TwoColorLeastSquares;
    let conversion_budget = budget(LARGE_BUDGET);
    let settings = conversion_settings(&table, &cancellation, &original, strategy, 8, 12);
    let converted = convert_density_detailed(
        rgb_raster(&pixels, 8, 12),
        &table,
        &settings,
        &conversion_budget,
    )
    .expect("color-aware conversion succeeds");

    let mut expected: Option<(usize, ascii_convert::ColorFit)> = None;
    for (position, glyph) in table.glyphs().iter().enumerate() {
        let fit = fit_color_patch(
            &target,
            glyph.mask(),
            &original,
            strategy,
            &conversion_budget,
            &cancellation,
        )
        .expect("every candidate receives its own bounded fit");
        if expected.is_none_or(|(_, best)| {
            fit.mean_squared_error() < best.mean_squared_error()
        }) {
            expected = Some((position, fit));
        }
    }
    let (position, fit) = expected.expect("candidate table is nonempty");
    let cell = converted.frame().cells()[0];
    assert_eq!(
        cell.glyph(),
        u32::try_from(position).expect("fixture position fits u32")
    );
    assert_eq!(cell.fg_srgba8(), rgba8(fit.foreground().color()));
    assert_eq!(cell.bg_srgba8(), rgba8(fit.background().color()));
    assert_eq!(converted.color_fit_strategy(), Some(strategy));
}

#[test]
fn original_mode_preserves_distinct_colors_for_each_converted_cell() {
    let table = conversion_table();
    let left = [28, 94, 176];
    let right = [211, 126, 36];
    let mut pixels = Vec::new();
    for _ in 0..12 {
        for x in 0..16 {
            pixels.extend_from_slice(if x < 8 { &left } else { &right });
        }
    }
    let original = palette(
        PaletteDefinition::Original,
        PaletteDistanceSpace::LinearRgb,
    );
    let cancellation = CancellationToken::new();
    let settings = conversion_settings(
        &table,
        &cancellation,
        &original,
        ColorFitStrategy::TwoColorLeastSquares,
        16,
        12,
    );
    let converted = convert_density_detailed(
        rgb_raster(&pixels, 16, 12),
        &table,
        &settings,
        &budget(LARGE_BUDGET),
    )
    .expect("two independently colored cells convert");

    assert_eq!(converted.frame().size(), GridSize::new(2, 1));
    assert_eq!(converted.palette_kind(), Some(PaletteKind::Original));
    assert_eq!(
        converted.palette_cache_identity(),
        Some(*original.cache_identity())
    );
    assert_eq!(converted.terminal_appearance_notice(), None);
    for (cell, expected) in converted.frame().cells().iter().zip([left, right]) {
        assert_eq!(cell.fg_srgba8(), [expected[0], expected[1], expected[2], u8::MAX]);
        assert_eq!(cell.bg_srgba8(), [expected[0], expected[1], expected[2], u8::MAX]);
    }
}

#[test]
fn palette_modes_convert_to_explicit_resolved_colors_and_notice_ansi() {
    let table = conversion_table();
    let mut pixels = Vec::new();
    let color_pattern = [[12, 32, 56], [228, 170, 76], [42, 188, 220], [192, 64, 136]];
    for y in 0..12 {
        for x in 0..8 {
            pixels.extend_from_slice(&color_pattern[(x + 3 * y) % color_pattern.len()]);
        }
    }
    let fixed_colors = [
        LinearRgb::BLACK,
        LinearRgb::WHITE,
        encoded_color(12, 32, 56),
        encoded_color(228, 170, 76),
    ];
    let gradient_stops = [
        GradientStop::new(0.0, LinearRgb::BLACK).expect("valid first endpoint"),
        GradientStop::new(0.5, color(0.2, 0.65, 0.8)).expect("valid middle stop"),
        GradientStop::new(1.0, LinearRgb::WHITE).expect("valid last endpoint"),
    ];
    let original = palette(
        PaletteDefinition::Original,
        PaletteDistanceSpace::LinearRgb,
    );
    let mono_foreground = color(0.9, 0.15, 0.1);
    let mono_background = color(0.1, 0.2, 0.9);
    let monochrome = palette(
        PaletteDefinition::Monochrome {
            foreground: mono_foreground,
            background: mono_background,
        },
        PaletteDistanceSpace::LinearRgb,
    );
    let fixed = palette(
        PaletteDefinition::Fixed(&fixed_colors),
        PaletteDistanceSpace::LinearRgb,
    );
    let ansi16 = palette(
        PaletteDefinition::Ansi16,
        PaletteDistanceSpace::LinearRgb,
    );
    let ansi256 = palette(
        PaletteDefinition::Ansi256,
        PaletteDistanceSpace::LinearRgb,
    );
    let gradient = palette(
        PaletteDefinition::Gradient(&gradient_stops),
        PaletteDistanceSpace::LinearRgb,
    );
    let modes = [
        &original,
        &monochrome,
        &fixed,
        &ansi16,
        &ansi256,
        &gradient,
    ];
    let cancellation = CancellationToken::new();
    let conversion_budget = budget(LARGE_BUDGET);
    let strategy = ColorFitStrategy::FixedBackground(LinearRgb::BLACK);

    for palette in modes {
        let settings =
            conversion_settings(&table, &cancellation, palette, strategy, 8, 12);
        let converted = convert_density_detailed(
            rgb_raster(&pixels, 8, 12),
            &table,
            &settings,
            &conversion_budget,
        )
        .expect("each explicit palette mode converts");
        let cell = converted.frame().cells()[0];
        assert_eq!(converted.palette_kind(), Some(palette.kind()));
        assert_eq!(
            converted.palette_cache_identity(),
            Some(*palette.cache_identity())
        );
        assert_eq!(converted.color_fit_strategy(), Some(strategy));
        assert_eq!(cell.fg_srgba8()[3], u8::MAX);
        assert_eq!(cell.bg_srgba8()[3], u8::MAX);
        match palette.kind() {
            PaletteKind::Original => {
                assert_eq!(cell.bg_srgba8(), rgba8(LinearRgb::BLACK));
            }
            PaletteKind::Monochrome => {
                assert_eq!(cell.fg_srgba8(), rgba8(mono_foreground));
                assert_eq!(cell.bg_srgba8(), rgba8(mono_background));
            }
            PaletteKind::Fixed | PaletteKind::Ansi16 | PaletteKind::Ansi256 => {
                assert_palette_member(
                    palette,
                    [cell.fg_srgba8()[0], cell.fg_srgba8()[1], cell.fg_srgba8()[2]],
                    &conversion_budget,
                    &cancellation,
                );
                assert_palette_member(
                    palette,
                    [cell.bg_srgba8()[0], cell.bg_srgba8()[1], cell.bg_srgba8()[2]],
                    &conversion_budget,
                    &cancellation,
                );
            }
            PaletteKind::Gradient => {
                assert_palette_member(
                    palette,
                    [cell.fg_srgba8()[0], cell.fg_srgba8()[1], cell.fg_srgba8()[2]],
                    &conversion_budget,
                    &cancellation,
                );
                assert_palette_member(
                    palette,
                    [cell.bg_srgba8()[0], cell.bg_srgba8()[1], cell.bg_srgba8()[2]],
                    &conversion_budget,
                    &cancellation,
                );
            }
        }
        match palette.kind() {
            PaletteKind::Ansi16 | PaletteKind::Ansi256 => {
                assert!(converted.terminal_appearance_notice().is_some());
            }
            _ => assert_eq!(converted.terminal_appearance_notice(), None),
        }
    }
}

#[test]
fn validation_preflights_candidate_work_and_cancellation_publishes_no_frame() {
    let table = conversion_table();
    let pixels = [96, 144, 220].repeat(8 * 12);
    let ansi256 = palette(
        PaletteDefinition::Ansi256,
        PaletteDistanceSpace::LinearRgb,
    );
    let cancellation = CancellationToken::new();
    let expensive_settings = conversion_settings(
        &table,
        &cancellation,
        &ansi256,
        ColorFitStrategy::TwoColorLeastSquares,
        8,
        12,
    );
    let small_work_budget = Budget::new(LARGE_BUDGET, LARGE_BUDGET, 1_000_000);
    assert_eq!(
        convert_density_detailed(
            rgb_raster(&pixels, 8, 12),
            &table,
            &expensive_settings,
            &small_work_budget,
        )
        .expect_err("all candidate fit work is preflighted before publishing a frame"),
        ascii_convert::DensityConversionError::Core(Error::BudgetExceeded(
            BudgetKind::WorkUnits
        ))
    );

    let original = palette(
        PaletteDefinition::Original,
        PaletteDistanceSpace::LinearRgb,
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let cancelled_settings = conversion_settings(
        &table,
        &cancelled,
        &original,
        ColorFitStrategy::TwoColorLeastSquares,
        8,
        12,
    );
    assert_eq!(
        convert_density_detailed(
            rgb_raster(&pixels, 8, 12),
            &table,
            &cancelled_settings,
            &budget(LARGE_BUDGET),
        )
        .expect_err("cancelled conversion returns no partial frame"),
        ascii_convert::DensityConversionError::Core(Error::Cancelled)
    );

    let recovered_cancellation = CancellationToken::new();
    let recovered_settings = conversion_settings(
        &table,
        &recovered_cancellation,
        &original,
        ColorFitStrategy::TwoColorLeastSquares,
        8,
        12,
    );
    convert_density_detailed(
        rgb_raster(&pixels, 8, 12),
        &table,
        &recovered_settings,
        &budget(LARGE_BUDGET),
    )
    .expect("a valid conversion succeeds after cancellation");
}
fn reference_mean_squared_error(
    target: &[LinearRgb],
    coverage: &[u8],
    foreground: LinearRgb,
    background: LinearRgb,
) -> f64 {
    let mut squared_error = 0.0;
    for (target, coverage) in target.iter().zip(coverage.iter().copied()) {
        let mask = f64::from(coverage) / 255.0;
        for (foreground, background, target) in [
            (foreground.red(), background.red(), target.red()),
            (foreground.green(), background.green(), target.green()),
            (foreground.blue(), background.blue(), target.blue()),
        ] {
            let reconstruction = background + (foreground - background) * mask;
            squared_error += (target - reconstruction).powi(2);
        }
    }
    squared_error / (f64::from(u32::try_from(target.len()).expect("fixture fits u32")) * 3.0)
}

fn gradient_color_at(start: LinearRgb, end: LinearRgb, position: f64) -> LinearRgb {
    color(
        start.red() + (end.red() - start.red()) * position,
        start.green() + (end.green() - start.green()) * position,
        start.blue() + (end.blue() - start.blue()) * position,
    )
}

#[test]
fn gradient_pair_fit_matches_dense_objective_reference_for_synthetic_patches() {
    let start = color(0.03, 0.72, 0.18);
    let end = color(0.91, 0.12, 0.84);
    let stops = [
        GradientStop::new(0.0, start).expect("valid gradient start"),
        GradientStop::new(1.0, end).expect("valid gradient end"),
    ];
    let gradient = palette(
        PaletteDefinition::Gradient(&stops),
        PaletteDistanceSpace::LinearRgb,
    );
    let cases = [
        (
            [
                color(0.02, 0.11, 0.92),
                color(0.21, 0.81, 0.17),
                color(0.92, 0.13, 0.29),
                color(0.48, 0.49, 0.51),
                color(0.76, 0.32, 0.62),
            ],
            [0, 64, 127, 191, 255],
        ),
        (
            [
                color(0.87, 0.06, 0.14),
                color(0.09, 0.77, 0.65),
                color(0.43, 0.45, 0.97),
                color(0.61, 0.28, 0.08),
                color(0.15, 0.89, 0.37),
            ],
            [255, 223, 159, 71, 0],
        ),
        (
            [
                color(0.13, 0.24, 0.82),
                color(0.71, 0.56, 0.19),
                color(0.04, 0.92, 0.49),
                color(0.96, 0.14, 0.73),
                color(0.39, 0.31, 0.05),
            ],
            [15, 83, 146, 207, 248],
        ),
    ];
    let cancellation = CancellationToken::new();
    let fit_budget = budget(1_000_000);

    for (target, coverage) in cases {
        let fit = fit_color_patch(
            &target,
            &coverage,
            &gradient,
            ColorFitStrategy::TwoColorLeastSquares,
            &fit_budget,
            &cancellation,
        )
        .expect("bounded gradient pair fitting succeeds");
        let mut dense_reference = f64::INFINITY;
        for foreground_step in 0..=64 {
            let foreground = gradient_color_at(start, end, f64::from(foreground_step) / 64.0);
            for background_step in 0..=64 {
                let background = gradient_color_at(start, end, f64::from(background_step) / 64.0);
                dense_reference = dense_reference.min(reference_mean_squared_error(
                    &target,
                    &coverage,
                    foreground,
                    background,
                ));
            }
        }
        assert!(
            fit.mean_squared_error() <= dense_reference + 1.0e-12,
            "analytic fit {} exceeded dense reference {dense_reference}",
            fit.mean_squared_error(),
        );
    }
}
#[test]
fn synthetic_color_fit_fixture_configuration_hash_is_stable() {
    let configuration_hash: [u8; 32] =
        Sha256::digest(SYNTHETIC_COLOR_FIT_CONFIGURATION.as_bytes()).into();
    let fixture_hash: [u8; 32] =
        Sha256::digest(include_bytes!("support/synthetic_font.rs")).into();
    assert_eq!(
        configuration_hash,
        [
            0xc5, 0x78, 0x43, 0x09, 0xd7, 0x9a, 0x7a, 0x11, 0x16, 0x31, 0x9a, 0x71, 0x8d, 0xa3,
            0x46, 0x5c, 0xf6, 0xff, 0xa0, 0x32, 0xde, 0x7a, 0xb2, 0x2f, 0x4b, 0x51, 0xc4, 0x06,
            0x55, 0xe4, 0xeb, 0xed,
        ]
    );
    assert_eq!(
        fixture_hash,
        [
            0x8a, 0x0b, 0x3b, 0x4e, 0x8e, 0xe1, 0x50, 0xd6, 0xd1, 0x6b, 0x08, 0x4e, 0x51, 0x20,
            0x05, 0x25, 0x11, 0x2e, 0xd6, 0x42, 0x48, 0x55, 0xaf, 0xa6, 0x52, 0xa5, 0x5e, 0xd3,
            0x27, 0x41, 0x76, 0xc8,
        ]
    );
}

#[test]
fn validation_cancellation_during_candidate_search_publishes_no_partial_frame() {
    let table = build_glyph_table(
        &synthetic_font(false),
        &GlyphRepertoire::new("BA ").expect("fixture repertoire is valid"),
        &GlyphConfig::new(8.0, 16, 18, 13).expect("fixture cell geometry is valid"),
        &Budget::new(1_000_000, 2_000_000, 2_000_000),
        &CancellationToken::new(),
    )
    .expect("larger synthetic glyph table builds");
    let palette = palette(
        PaletteDefinition::Ansi256,
        PaletteDistanceSpace::LinearRgb,
    );
    let cancellation = std::sync::Arc::new(CancellationToken::new());
    let settings = ConversionSettings::new(
        GridSizing::fixed_pixel_surface(16, 18),
        conversion_metadata(&table),
        cancellation.as_ref(),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .expect("conversion settings are valid")
    .with_color_fit(&palette, ColorFitStrategy::TwoColorLeastSquares);
    let pixels = [64, 110, 170].repeat(16 * 18);
    let budget = Budget::new(LARGE_BUDGET, LARGE_BUDGET, 1_000_000_000);
    let cancellation_signal = std::sync::Arc::clone(&cancellation);
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(10));
        cancellation_signal.cancel();
    });

    let result = convert_density_detailed(
        rgb_raster(&pixels, 16, 18),
        &table,
        &settings,
        &budget,
    );
    canceller.join().expect("cancellation request thread succeeds");
    assert_eq!(
        result.expect_err("in-flight cancellation publishes no partial frame"),
        ascii_convert::DensityConversionError::Core(Error::Cancelled)
    );
}
