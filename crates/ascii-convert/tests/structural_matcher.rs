#[allow(dead_code)]
mod support;

use ascii_convert::{
    ColorFitStrategy, ConversionSettings, FitMode, GlyphConfig, GlyphRepertoire, GlyphTable,
    GridSizing, LinearRgb, MAX_STRUCTURAL_CANDIDATES, NormalizedRasterResources, Palette,
    PaletteDefinition, PaletteDistanceSpace, PatchDimensions, ReconstructionWeights,
    SamplingFilter, SamplingIdentity, SamplingOptions, ShortlistOptions, StructuralDescriptor,
    StructuralScratch, StructuralWeights, build_glyph_table, convert_density_detailed,
    convert_structural,
};
use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, ColorPrimaries, Error,
    FrameMetadata, GlyphTableId, PixelFormat, RasterFrameView, RasterOrientation, TransferFunction,
};
use support::synthetic_font::{
    diagonal_font, diagonal_orientation_font, equal_area_orientation_font, synthetic_font,
};

const CELL_WIDTH: u32 = 8;
const CELL_HEIGHT: u32 = 12;
const BUDGET: Budget = Budget::new(1_000_000, 2_000_000, 2_000_000);

fn build_table(font_bytes: &[u8], repertoire: &str) -> GlyphTable {
    build_glyph_table(
        font_bytes,
        &GlyphRepertoire::new(repertoire).expect("fixture repertoire is supported"),
        &GlyphConfig::new(8.0, CELL_WIDTH, CELL_HEIGHT, 9)
            .expect("fixture glyph geometry is valid"),
        &BUDGET,
        &CancellationToken::new(),
    )
    .expect("source-generated fixture font builds")
}

fn sampling_identity(table: &GlyphTable, dimensions: PatchDimensions) -> SamplingIdentity {
    let cancellation = CancellationToken::new();
    let settings = ConversionSettings::new(
        GridSizing::fixed_pixel_surface(CELL_WIDTH, CELL_HEIGHT),
        FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 1, 0, 0, 1),
        &cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .expect("valid fixture conversion settings")
    .with_sampling(SamplingOptions::new(
        FitMode::Contain,
        SamplingFilter::Box,
        Some(dimensions),
    ));
    let pixels = [0_u8; (CELL_WIDTH * CELL_HEIGHT) as usize];
    let raster = RasterFrameView::new(
        CELL_WIDTH,
        CELL_HEIGHT,
        CELL_WIDTH as usize,
        PixelFormat::gray8(),
        ColorDescription::new(
            ColorPrimaries::Srgb,
            TransferFunction::Linear,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        &pixels,
    );
    convert_density_detailed(
        raster,
        table,
        &settings,
        &BUDGET,
        &NormalizedRasterResources::new(2_000_000),
    )
    .expect("bounded sampling conversion succeeds")
    .sampling_identity()
}

fn luminance_for_coverage(coverage: &[f32], foreground: f64, background: f64) -> Vec<f32> {
    coverage
        .iter()
        .map(|&sample| (background + f64::from(sample) * (foreground - background)) as f32)
        .collect()
}

fn descriptor<'a>(
    luminance: &'a [f32],
    dimensions: PatchDimensions,
    foreground: f64,
    background: f64,
) -> StructuralDescriptor<'a> {
    StructuralDescriptor::from_luminance(
        luminance,
        dimensions,
        foreground,
        background,
        &BUDGET,
        &CancellationToken::new(),
    )
    .expect("valid normalized patch is described")
}

fn close(left: f32, right: f32) {
    assert!((left - right).abs() <= 1.0e-6, "{left} != {right}");
}

fn shortlist_identity(
    table: &GlyphTable,
    patch: &StructuralDescriptor<'_>,
    sampling_identity: SamplingIdentity,
    options: ShortlistOptions,
) -> ascii_convert::StructuralIdentity {
    StructuralScratch::default()
        .shortlist(
            patch,
            table,
            sampling_identity,
            options,
            &BUDGET,
            &CancellationToken::new(),
        )
        .expect("valid shortlist completes")
        .identity()
}

#[test]
fn equal_density_orientation_masks_survive_a_one_candidate_shortlist() {
    let table = build_table(&equal_area_orientation_font(), "BA");
    let horizontal = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'A')
        .expect("fixture includes horizontal mask");
    let vertical = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'B')
        .expect("fixture includes vertical mask");
    close(horizontal.mean_coverage(), vertical.mean_coverage());
    assert_ne!(horizontal.edge_orientation(), vertical.edge_orientation());

    let dimensions = PatchDimensions::new(CELL_WIDTH, CELL_HEIGHT).expect("valid patch dimensions");
    let identity = sampling_identity(&table, dimensions);
    let weights = StructuralWeights::new(0.0, 0.0, 1.0).expect("edge-only weights are normalized");
    let options = ShortlistOptions::new(1, weights).expect("one candidate is within the limit");
    let mut scratch = StructuralScratch::default();

    for expected in [horizontal, vertical] {
        let coverage = expected
            .mask()
            .iter()
            .map(|&value| f32::from(value) / 255.0)
            .collect::<Vec<_>>();
        let luminance = luminance_for_coverage(&coverage, 1.0, 0.0);
        let target = descriptor(&luminance, dimensions, 1.0, 0.0);
        let shortlist = scratch
            .shortlist(
                &target,
                &table,
                identity,
                options,
                &BUDGET,
                &CancellationToken::new(),
            )
            .expect("bounded orientation shortlist completes");
        assert_eq!(shortlist.candidates().len(), 1);
        assert_eq!(shortlist.candidates()[0].character(), expected.character());
        assert!(shortlist.candidates()[0].score() <= 1.0e-6);
    }
}

#[test]
fn diagonal_orientation_alias_survives_one_candidate_shortlist_with_stable_ties() {
    let table = build_table(&diagonal_font(true), "BA");
    let diagonal_a = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'A')
        .expect("fixture includes the diagonal A mask");
    let diagonal_b = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'B')
        .expect("fixture includes the aliased diagonal B mask");
    assert_eq!(diagonal_a.glyph_index(), diagonal_b.glyph_index());
    assert_eq!(diagonal_a.mask(), diagonal_b.mask());
    let orientation = diagonal_a.edge_orientation();
    assert!(
        orientation.diagonal_positive() + orientation.diagonal_negative() > 0.0,
        "fixture mask has diagonal edge energy: {orientation:?}"
    );

    let dimensions = PatchDimensions::new(CELL_WIDTH, CELL_HEIGHT).expect("valid patch dimensions");
    let identity = sampling_identity(&table, dimensions);
    let weights = StructuralWeights::new(0.0, 0.0, 1.0).expect("edge-only weights are normalized");
    let options = ShortlistOptions::new(1, weights).expect("one candidate is within the limit");
    let coverage = diagonal_a
        .mask()
        .iter()
        .map(|&value| f32::from(value) / 255.0)
        .collect::<Vec<_>>();
    let luminance = luminance_for_coverage(&coverage, 1.0, 0.0);
    let target = descriptor(&luminance, dimensions, 1.0, 0.0);
    let mut scratch = StructuralScratch::default();
    let mut previous = None;

    for _ in 0..2 {
        let shortlist = scratch
            .shortlist(
                &target,
                &table,
                identity,
                options,
                &BUDGET,
                &CancellationToken::new(),
            )
            .expect("bounded diagonal shortlist completes");
        assert_eq!(shortlist.candidates().len(), 1);
        let candidate = shortlist.candidates()[0];
        assert_eq!(candidate.character(), 'A');
        assert!(candidate.score() <= 1.0e-6);
        if let Some(previous) = previous {
            assert_eq!(candidate, previous);
        }
        previous = Some(candidate);
    }
}

#[test]
fn diagonal_orientation_bins_distinguish_a_competitor_at_k_one() {
    let table = build_table(&diagonal_orientation_font(), "BA");
    let diagonal = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'B')
        .expect("fixture includes the diagonal target");
    let competitor = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'A')
        .expect("fixture includes the opposite diagonal competitor");
    assert_ne!(diagonal.mask(), competitor.mask());
    assert!(
        diagonal.glyph_index() > competitor.glyph_index(),
        "the lower-index A competitor must win a diagonal-blind score tie"
    );

    let diagonal_edges = diagonal.edge_orientation();
    let competitor_edges = competitor.edge_orientation();
    assert!(
        diagonal_edges.diagonal_positive() + diagonal_edges.diagonal_negative() > 0.0,
        "target mask has diagonal edge energy: {diagonal_edges:?}"
    );
    assert!(
        competitor_edges.diagonal_positive() + competitor_edges.diagonal_negative() > 0.0,
        "competitor mask has diagonal edge energy: {competitor_edges:?}"
    );

    assert!(
        (
            diagonal_edges.diagonal_positive(),
            diagonal_edges.diagonal_negative()
        ) != (
            competitor_edges.diagonal_positive(),
            competitor_edges.diagonal_negative()
        ),
        "mirrored masks have different diagonal orientation bins"
    );
    close(diagonal_edges.horizontal(), competitor_edges.horizontal());
    close(diagonal_edges.vertical(), competitor_edges.vertical());

    let dimensions = PatchDimensions::new(CELL_WIDTH, CELL_HEIGHT).expect("valid patch dimensions");
    let identity = sampling_identity(&table, dimensions);
    let weights = StructuralWeights::new(0.0, 0.0, 1.0).expect("edge-only weights are normalized");
    let options = ShortlistOptions::new(1, weights).expect("one candidate is within the limit");
    let mut scratch = StructuralScratch::default();

    for expected in [diagonal, competitor] {
        let coverage = expected
            .mask()
            .iter()
            .map(|&value| f32::from(value) / 255.0)
            .collect::<Vec<_>>();
        let luminance = luminance_for_coverage(&coverage, 1.0, 0.0);
        let target = descriptor(&luminance, dimensions, 1.0, 0.0);
        let mut previous = None;

        for _ in 0..2 {
            let shortlist = scratch
                .shortlist(
                    &target,
                    &table,
                    identity,
                    options,
                    &BUDGET,
                    &CancellationToken::new(),
                )
                .expect("bounded orientation shortlist completes");
            assert_eq!(shortlist.candidates().len(), 1);
            let candidate = shortlist.candidates()[0];
            assert_eq!(candidate.character(), expected.character());
            assert!(candidate.score() <= 1.0e-6);
            if let Some(previous) = previous {
                assert_eq!(candidate, previous);
            }
            previous = Some(candidate);
        }
    }
}

#[test]
fn normalized_features_are_contrast_and_resolution_bounded() {
    let dimensions_2 = PatchDimensions::new(2, 2).expect("valid 2x2 patch");
    let dimensions_4 = PatchDimensions::new(4, 4).expect("valid 4x4 patch");
    let coverage_2 = [0.0, 1.0, 0.0, 1.0];
    let coverage_4 = [
        0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0,
    ];
    let luminance_2 = luminance_for_coverage(&coverage_2, 0.95, 0.05);
    let contrast_luminance = luminance_for_coverage(&coverage_2, 0.65, 0.25);
    let inverted_luminance = luminance_for_coverage(&coverage_2, 0.05, 0.95);
    let luminance_4 = luminance_for_coverage(&coverage_4, 0.95, 0.05);
    let base = descriptor(&luminance_2, dimensions_2, 0.95, 0.05);
    let changed_contrast = descriptor(&contrast_luminance, dimensions_2, 0.65, 0.25);
    let inverted_polarity = descriptor(&inverted_luminance, dimensions_2, 0.05, 0.95);
    let changed_resolution = descriptor(&luminance_4, dimensions_4, 0.95, 0.05);
    close(base.mean_coverage(), 0.5);
    close(base.gradients().x_variation(), 1.0);
    close(base.gradients().y_variation(), 0.0);
    for compared in [&changed_contrast, &inverted_polarity] {
        close(base.mean_coverage(), compared.mean_coverage());
        close(
            base.gradients().x_variation(),
            compared.gradients().x_variation(),
        );
        close(
            base.gradients().y_variation(),
            compared.gradients().y_variation(),
        );
        close(
            base.edge_orientation().horizontal(),
            compared.edge_orientation().horizontal(),
        );
        close(
            base.edge_orientation().vertical(),
            compared.edge_orientation().vertical(),
        );
        close(
            base.edge_orientation().diagonal_positive(),
            compared.edge_orientation().diagonal_positive(),
        );
        close(
            base.edge_orientation().diagonal_negative(),
            compared.edge_orientation().diagonal_negative(),
        );
    }
    close(base.mean_coverage(), changed_resolution.mean_coverage());
    close(
        base.gradients().x_variation(),
        changed_resolution.gradients().x_variation(),
    );
    close(
        base.gradients().y_variation(),
        changed_resolution.gradients().y_variation(),
    );
    for feature in [
        base.mean_coverage(),
        base.gradients().x_variation(),
        base.gradients().y_variation(),
        base.edge_orientation().horizontal(),
        base.edge_orientation().vertical(),
        base.edge_orientation().diagonal_positive(),
        base.edge_orientation().diagonal_negative(),
        changed_resolution.mean_coverage(),
        changed_resolution.gradients().x_variation(),
        changed_resolution.gradients().y_variation(),
        changed_resolution.edge_orientation().horizontal(),
        changed_resolution.edge_orientation().vertical(),
        changed_resolution.edge_orientation().diagonal_positive(),
        changed_resolution.edge_orientation().diagonal_negative(),
    ] {
        assert!(feature.is_finite() && (0.0..=1.0).contains(&feature));
    }
}

#[test]
fn equivalent_patch_geometry_has_resolution_stable_candidate_ranking() {
    let table = build_table(&equal_area_orientation_font(), "BA");
    let dimensions_2 = PatchDimensions::new(2, 2).expect("valid 2x2 patch");
    let dimensions_4 = PatchDimensions::new(4, 4).expect("valid 4x4 patch");
    let luminance_2 = [0.0, 1.0, 0.0, 1.0];
    let luminance_4 = [
        0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0,
    ];
    let patch_2 = descriptor(&luminance_2, dimensions_2, 1.0, 0.0);
    let patch_4 = descriptor(&luminance_4, dimensions_4, 1.0, 0.0);
    let weights = StructuralWeights::new(0.5, 0.25, 0.25).expect("normalized weights");
    let options = ShortlistOptions::new(2, weights).expect("valid candidate limit");
    let mut scratch = StructuralScratch::default();
    let ranked_2 = scratch
        .shortlist(
            &patch_2,
            &table,
            sampling_identity(&table, dimensions_2),
            options,
            &BUDGET,
            &CancellationToken::new(),
        )
        .expect("2x2 patch ranks")
        .candidates()
        .to_vec();
    let ranked_4 = scratch
        .shortlist(
            &patch_4,
            &table,
            sampling_identity(&table, dimensions_4),
            options,
            &BUDGET,
            &CancellationToken::new(),
        )
        .expect("equivalent 4x4 patch ranks")
        .candidates();
    assert_eq!(ranked_4, ranked_2.as_slice());
}

#[test]
fn constant_patches_are_finite_and_candidate_ties_are_stable() {
    let table = build_table(&synthetic_font(false), "BA");
    let dimensions = PatchDimensions::new(CELL_WIDTH, CELL_HEIGHT).expect("valid patch dimensions");
    let constant_luminance = [0.5; 8 * 12];
    let constant = descriptor(&constant_luminance, dimensions, 1.0, 0.0);
    assert_eq!(constant.gradients().x_variation(), 0.0);
    assert_eq!(constant.gradients().y_variation(), 0.0);
    assert!(constant.edge_orientation().horizontal().is_finite());
    assert!(constant.edge_orientation().vertical().is_finite());

    let blank_luminance = [0.0; 8 * 12];
    let blank = descriptor(&blank_luminance, dimensions, 1.0, 0.0);
    let weights = StructuralWeights::new(1.0, 0.0, 0.0).expect("tone-only weights are normalized");
    let options = ShortlistOptions::new(table.glyphs().len(), weights)
        .expect("full two-glyph repertoire fits the bound");
    let identity = sampling_identity(&table, dimensions);
    let mut scratch = StructuralScratch::default();
    let first = scratch
        .shortlist(
            &blank,
            &table,
            identity,
            options,
            &BUDGET,
            &CancellationToken::new(),
        )
        .expect("full repertoire shortlist completes");
    let first_candidates = first.candidates().to_vec();
    let second = scratch
        .shortlist(
            &blank,
            &table,
            identity,
            options,
            &BUDGET,
            &CancellationToken::new(),
        )
        .expect("reused scratch returns the same shortlist");
    assert_eq!(second.candidates(), first_candidates.as_slice());
    assert_eq!(second.candidates()[0].glyph_index(), 2);
    assert_eq!(second.candidates()[1].glyph_index(), 3);
    assert_eq!(second.candidates().len(), table.glyphs().len());
    assert!(second.candidates().iter().all(|candidate| {
        candidate.score().is_finite() && (0.0..=1.0).contains(&candidate.score())
    }));
}

#[test]
fn semantic_identity_covers_masks_dimensions_features_and_candidate_controls() {
    let table = build_table(&synthetic_font(false), "BA");
    let dimensions_2 = PatchDimensions::new(2, 2).expect("valid 2x2 patch");
    let dimensions_4 = PatchDimensions::new(4, 4).expect("valid 4x4 patch");
    let coverage_2 = [0.0, 1.0, 0.0, 1.0];
    let coverage_2_horizontal = [0.0, 0.0, 1.0, 1.0];
    let coverage_4 = [
        0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0,
    ];
    let luminance_2 = luminance_for_coverage(&coverage_2, 1.0, 0.0);
    let luminance_2_horizontal = luminance_for_coverage(&coverage_2_horizontal, 1.0, 0.0);
    let luminance_4 = luminance_for_coverage(&coverage_4, 1.0, 0.0);
    let contrast_luminance = luminance_for_coverage(&coverage_2, 0.75, 0.25);
    let patch_2 = descriptor(&luminance_2, dimensions_2, 1.0, 0.0);
    let patch_2_same = descriptor(&luminance_2, dimensions_2, 1.0, 0.0);
    let patch_2_horizontal = descriptor(&luminance_2_horizontal, dimensions_2, 1.0, 0.0);
    let patch_4 = descriptor(&luminance_4, dimensions_4, 1.0, 0.0);
    let contrast_patch = descriptor(&contrast_luminance, dimensions_2, 0.75, 0.25);
    let weights = StructuralWeights::new(0.5, 0.25, 0.25).expect("normalized weights");
    let options = ShortlistOptions::new(2, weights).expect("valid candidate count");
    let sampling_2 = sampling_identity(&table, dimensions_2);
    let same = shortlist_identity(&table, &patch_2, sampling_2, options);
    assert_eq!(
        same,
        shortlist_identity(&table, &patch_2_same, sampling_2, options)
    );
    assert_ne!(
        same,
        shortlist_identity(&table, &patch_2_horizontal, sampling_2, options)
    );
    assert_ne!(
        same,
        shortlist_identity(&table, &contrast_patch, sampling_2, options)
    );

    let smaller_shortlist = ShortlistOptions::new(1, weights).expect("valid one-candidate limit");
    assert_ne!(
        same,
        shortlist_identity(&table, &patch_2, sampling_2, smaller_shortlist)
    );
    let different_weights = StructuralWeights::new(0.25, 0.5, 0.25).expect("normalized weights");
    let weighted_options = ShortlistOptions::new(2, different_weights).expect("valid options");
    assert_ne!(
        same,
        shortlist_identity(&table, &patch_2, sampling_2, weighted_options)
    );
    let sampling_4 = sampling_identity(&table, dimensions_4);
    assert_ne!(
        same,
        shortlist_identity(&table, &patch_4, sampling_4, options)
    );

    let other_table = build_table(&synthetic_font(true), "BA");
    let other_sampling = sampling_identity(&other_table, dimensions_2);
    assert_ne!(
        same,
        shortlist_identity(&other_table, &patch_2, other_sampling, options)
    );
}

#[test]
fn shortlist_work_preflight_accepts_exact_combined_bound_and_rejects_one_less() {
    let table = build_table(&synthetic_font(false), "BA");
    let dimensions = PatchDimensions::new(2, 2).expect("valid 2x2 patch");
    let target_luminance = [0.0, 1.0, 0.0, 1.0];
    let target = descriptor(&target_luminance, dimensions, 1.0, 0.0);
    let identity = sampling_identity(&table, dimensions);
    let weights = StructuralWeights::new(1.0, 0.0, 0.0).expect("tone-only weights are normalized");
    let options = ShortlistOptions::new(1, weights).expect("one candidate is within the limit");

    // K=1 uses one heap level: four ranking units per glyph plus two sort units.
    // Feature extraction is bounded at the 8x12 glyph-cell geometry, not the 2x2 source patch.
    let glyph_count = u64::try_from(table.glyphs().len()).expect("fixture glyph count fits");
    let feature_work_bound = u64::from(CELL_WIDTH * CELL_HEIGHT) * 9;
    let ranking_work_bound = glyph_count * 4 + 2;
    let combined_work_bound = feature_work_bound + ranking_work_bound;
    let exact_budget = Budget::new(1_000_000, 2_000_000, combined_work_bound);
    let mut exact_scratch = StructuralScratch::default();
    assert_eq!(
        exact_scratch
            .shortlist(
                &target,
                &table,
                identity,
                options,
                &exact_budget,
                &CancellationToken::new(),
            )
            .expect("the exact combined preflight budget succeeds")
            .candidates()
            .len(),
        1
    );

    // The public descriptor is already validated and scratch capacity is private, so the typed
    // error is the deterministic observable for this one-unit-short preflight boundary.
    let below_budget = Budget::new(1_000_000, 2_000_000, combined_work_bound - 1);
    let mut below_scratch = StructuralScratch::default();
    assert_eq!(
        below_scratch
            .shortlist(
                &target,
                &table,
                identity,
                options,
                &below_budget,
                &CancellationToken::new(),
            )
            .err(),
        Some(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
}

#[test]
fn invalid_candidate_and_work_limits_return_typed_errors() {
    let weights = StructuralWeights::new(0.5, 0.25, 0.25).expect("normalized weights");
    assert_eq!(
        StructuralWeights::new(0.2, 0.2, 0.2).err(),
        Some(Error::Malformed(
            ascii_core::MalformedReason::InvalidConfiguration
        ))
    );
    assert_eq!(
        ShortlistOptions::new(0, weights).err(),
        Some(Error::Malformed(
            ascii_core::MalformedReason::InvalidConfiguration
        ))
    );
    assert_eq!(
        ShortlistOptions::new(MAX_STRUCTURAL_CANDIDATES + 1, weights).err(),
        Some(Error::Malformed(
            ascii_core::MalformedReason::InvalidConfiguration
        ))
    );

    let table = build_table(&synthetic_font(false), "BA");
    let dimensions = PatchDimensions::new(2, 2).expect("valid patch dimensions");
    let target_luminance = [0.0, 1.0, 0.0, 1.0];
    let target = descriptor(&target_luminance, dimensions, 1.0, 0.0);
    let identity = sampling_identity(&table, dimensions);
    let over_repertoire = ShortlistOptions::new(3, weights).expect("within global maximum");
    let mut scratch = StructuralScratch::default();
    assert_eq!(
        scratch
            .shortlist(
                &target,
                &table,
                identity,
                over_repertoire,
                &BUDGET,
                &CancellationToken::new(),
            )
            .err(),
        Some(Error::Malformed(
            ascii_core::MalformedReason::InvalidConfiguration
        ))
    );

    let one_candidate = ShortlistOptions::new(1, weights).expect("valid candidate limit");
    assert_eq!(
        scratch
            .shortlist(
                &target,
                &table,
                identity,
                one_candidate,
                &Budget::new(1_000_000, 0, 2_000_000),
                &CancellationToken::new(),
            )
            .err(),
        Some(Error::BudgetExceeded(BudgetKind::RasterBytes))
    );
    assert_eq!(
        StructuralDescriptor::from_luminance(
            &[0.0, 1.0, 0.0, 1.0],
            dimensions,
            1.0,
            0.0,
            &Budget::new(1_000_000, 2_000_000, 1),
            &CancellationToken::new(),
        )
        .err(),
        Some(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
}

#[test]
fn structural_conversion_reconstructs_the_shape_of_a_vertical_edge() {
    let table = build_table(&equal_area_orientation_font(), "BA");
    let vertical = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'B')
        .expect("fixture includes vertical mask");
    let horizontal = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'A')
        .expect("fixture includes horizontal mask");
    close(vertical.mean_coverage(), horizontal.mean_coverage());

    let cancellation = CancellationToken::new();
    let settings = ConversionSettings::new(
        GridSizing::fixed_pixel_surface(CELL_WIDTH, CELL_HEIGHT),
        FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 3, 1, 0, 1),
        &cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .expect("valid fixed-background settings")
    .with_reconstruction_weights(
        ReconstructionWeights::new(0.80, 0.15, 0.05, 0.0)
            .expect("finite normalized objective weights"),
    );
    let pixels = vertical.mask();
    let raster = RasterFrameView::new(
        CELL_WIDTH,
        CELL_HEIGHT,
        CELL_WIDTH as usize,
        PixelFormat::gray8(),
        ColorDescription::new(
            ColorPrimaries::Srgb,
            TransferFunction::Linear,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        pixels,
    );
    let first = convert_structural(raster, &table, &settings, None, &BUDGET)
        .expect("structural reconstruction succeeds");
    let vertical_position = table
        .glyphs()
        .iter()
        .position(|glyph| glyph.character() == 'B')
        .expect("vertical mask position fits table");
    assert_eq!(first.cells()[0].glyph(), vertical_position as u32);
    let repeated = convert_structural(raster, &table, &settings, None, &BUDGET)
        .expect("repeated structural reconstruction succeeds");
    assert_eq!(first, repeated);

    let prior = convert_structural(raster, &table, &settings, None, &BUDGET)
        .expect("prior frame conversion succeeds");
    let with_zero_temporal_weight =
        convert_structural(raster, &table, &settings, Some(&prior), &BUDGET)
            .expect("zero-weight temporal conversion succeeds");
    assert_eq!(first, with_zero_temporal_weight);

    let palette = Palette::new(
        PaletteDefinition::Original,
        PaletteDistanceSpace::LinearRgb,
        &BUDGET,
        &cancellation,
    )
    .expect("original-color palette is valid");
    let fitted_settings = settings.with_color_fit(&palette, ColorFitStrategy::TwoColorLeastSquares);
    let fitted = convert_structural(raster, &table, &fitted_settings, None, &BUDGET)
        .expect("candidate-specific color fitting succeeds");
    assert_eq!(fitted.cells()[0].glyph(), vertical_position as u32);
}

#[test]
fn structural_conversion_preflights_total_work_across_cells() {
    let table = build_table(&equal_area_orientation_font(), "BA");
    let cancellation = CancellationToken::new();
    let metadata = FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 5, 1, 0, 1);
    let settings = ConversionSettings::new(
        GridSizing::fixed_pixel_surface(CELL_WIDTH * 2, CELL_HEIGHT),
        metadata,
        &cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .expect("valid two-cell settings")
    .with_reconstruction_weights(
        ReconstructionWeights::new(0.80, 0.15, 0.05, 0.0)
            .expect("finite normalized objective weights"),
    );
    let vertical_mask = table
        .glyphs()
        .iter()
        .find(|glyph| glyph.character() == 'B')
        .expect("fixture includes vertical mask")
        .mask();
    let mut pixels = Vec::with_capacity((CELL_WIDTH * CELL_HEIGHT * 2) as usize);
    for row in 0..CELL_HEIGHT as usize {
        let start = row * CELL_WIDTH as usize;
        let row = &vertical_mask[start..start + CELL_WIDTH as usize];
        pixels.extend_from_slice(row);
        pixels.extend_from_slice(row);
    }
    let raster = RasterFrameView::new(
        CELL_WIDTH * 2,
        CELL_HEIGHT,
        (CELL_WIDTH * 2) as usize,
        PixelFormat::gray8(),
        ColorDescription::new(
            ColorPrimaries::Srgb,
            TransferFunction::Linear,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        &pixels,
    );
    let single_settings = ConversionSettings::new(
        GridSizing::fixed_pixel_surface(CELL_WIDTH, CELL_HEIGHT),
        FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 5, 1, 0, 1),
        &cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .expect("valid one-cell settings")
    .with_reconstruction_weights(
        ReconstructionWeights::new(0.80, 0.15, 0.05, 0.0)
            .expect("finite normalized objective weights"),
    );
    let single_raster = RasterFrameView::new(
        CELL_WIDTH,
        CELL_HEIGHT,
        CELL_WIDTH as usize,
        PixelFormat::gray8(),
        ColorDescription::new(
            ColorPrimaries::Srgb,
            TransferFunction::Linear,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        vertical_mask,
    );
    let tight_budget = Budget::new(1_000_000, 2_000_000, 7_500);
    assert!(
        convert_structural(single_raster, &table, &single_settings, None, &tight_budget).is_ok()
    );
    assert_eq!(
        convert_structural(raster, &table, &settings, None, &tight_budget).err(),
        Some(Error::BudgetExceeded(BudgetKind::WorkUnits))
    );
    assert!(convert_structural(raster, &table, &settings, None, &BUDGET).is_ok());
}

#[test]
fn reconstruction_weights_reject_nonfinite_and_unnormalized_values() {
    assert!(ReconstructionWeights::new(f64::NAN, 0.0, 0.0, 0.0).is_err());
    assert!(ReconstructionWeights::new(0.5, 0.2, 0.2, 0.0).is_err());
}
