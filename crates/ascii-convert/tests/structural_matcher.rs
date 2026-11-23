#[allow(dead_code)]
mod support;

use ascii_convert::{
    ConversionSettings, FitMode, GlyphConfig, GlyphRepertoire, GlyphTable, GridSizing, LinearRgb,
    MAX_STRUCTURAL_CANDIDATES, NormalizedRasterResources, PatchDimensions, SamplingFilter,
    SamplingIdentity, SamplingOptions, ShortlistOptions, StructuralDescriptor, StructuralScratch,
    StructuralWeights, build_glyph_table, convert_density_detailed,
};
use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, ColorPrimaries, Error,
    FrameMetadata, GlyphTableId, PixelFormat, RasterFrameView, RasterOrientation, TransferFunction,
};
use support::synthetic_font::{equal_area_orientation_font, synthetic_font};

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
