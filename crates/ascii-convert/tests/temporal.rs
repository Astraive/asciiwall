use ascii_convert::{
    TemporalCandidate, TemporalFrameContext, TemporalOptions, TemporalReset, TemporalScratch,
};
use ascii_core::{Budget, BudgetKind, CancellationToken, Error};

const GOOD_BUDGET: Budget = Budget::new(1_000_000, 1_000_000, 10_000);
const SOURCE: [u8; 32] = [1; 32];
const GLYPHS: [u8; 32] = [2; 32];
const SETTINGS: [u8; 32] = [3; 32];

fn context(time_ns: u64) -> TemporalFrameContext {
    TemporalFrameContext::new(7, time_ns, 2, 1, SOURCE, GLYPHS, SETTINGS)
}

fn options(weight: f64) -> TemporalOptions {
    TemporalOptions::new(weight, 0.2, 2_000_000_000).expect("valid temporal settings")
}

#[test]
fn history_lifetime_has_a_hard_upper_bound() {
    assert_eq!(
        TemporalOptions::new(1.0, 0.2, 30_000_000_001),
        Err(Error::Malformed(
            ascii_core::MalformedReason::InvalidConfiguration
        ))
    );
}

fn candidates(
    best: u32,
    best_score: f64,
    previous_score: f64,
    motion: f64,
) -> [TemporalCandidate; 2] {
    [
        TemporalCandidate {
            glyph_id: best,
            best_score,
            previous_score,
            motion,
        },
        TemporalCandidate {
            glyph_id: best,
            best_score,
            previous_score,
            motion,
        },
    ]
}

#[test]
fn static_noise_is_suppressed_but_motion_and_cuts_are_responsive() {
    let mut state = TemporalScratch::default();
    assert_eq!(
        state
            .stabilize(
                context(0),
                &candidates(10, 0.1, 0.1, 0.0),
                options(1.0),
                &GOOD_BUDGET,
                &CancellationToken::new(),
            )
            .expect("initial frame starts clean"),
        [10, 10]
    );
    assert_eq!(
        state
            .stabilize(
                context(1_000_000_000),
                &candidates(11, 0.1, 0.2, 0.0),
                options(1.0),
                &GOOD_BUDGET,
                &CancellationToken::new(),
            )
            .expect("static noise can retain history"),
        [10, 10]
    );
    assert_eq!(
        state
            .stabilize(
                context(1_100_000_000),
                &candidates(12, 0.05, 0.3, 0.9),
                options(1.0),
                &GOOD_BUDGET,
                &CancellationToken::new(),
            )
            .expect("motion remains responsive"),
        [12, 12]
    );
    assert_eq!(
        state
            .stabilize(
                context(1_200_000_000).with_reset(TemporalReset::HardCut),
                &candidates(13, 0.1, 0.1, 0.0),
                options(1.0),
                &GOOD_BUDGET,
                &CancellationToken::new(),
            )
            .expect("cut resets history"),
        [13, 13]
    );
}

#[test]
fn every_explicit_reset_reason_discards_history() {
    let resets = [
        TemporalReset::HardCut,
        TemporalReset::Seek,
        TemporalReset::SourceChange,
        TemporalReset::FontOrGridChange,
        TemporalReset::DecodeDiscontinuity,
    ];
    for (offset, reset) in resets.into_iter().enumerate() {
        let mut state = TemporalScratch::default();
        state
            .stabilize(
                context(1),
                &candidates(20, 0.1, 0.1, 0.0),
                options(1.0),
                &GOOD_BUDGET,
                &CancellationToken::new(),
            )
            .expect("initial frame succeeds");
        let current = u32::try_from(offset + 30).expect("small fixture id");
        assert_eq!(
            state
                .stabilize(
                    context(2).with_reset(reset),
                    &candidates(current, 0.1, 0.1, 0.0),
                    options(1.0),
                    &GOOD_BUDGET,
                    &CancellationToken::new(),
                )
                .expect("explicit reset succeeds"),
            [current, current],
            "reset {reset:?} must not reuse old glyphs"
        );
    }
}

#[test]
fn fingerprints_geometry_time_and_generation_prevent_incompatible_reuse() {
    let mut state = TemporalScratch::default();
    state
        .stabilize(
            context(10),
            &candidates(1, 0.1, 0.1, 0.0),
            options(1.0),
            &GOOD_BUDGET,
            &CancellationToken::new(),
        )
        .expect("initial frame succeeds");
    let incompatible = [
        TemporalFrameContext::new(7, 20, 2, 1, [9; 32], GLYPHS, SETTINGS),
        TemporalFrameContext::new(7, 30, 2, 1, SOURCE, [9; 32], SETTINGS),
        TemporalFrameContext::new(7, 40, 2, 1, SOURCE, GLYPHS, [9; 32]),
        TemporalFrameContext::new(7, 50, 1, 2, SOURCE, GLYPHS, SETTINGS),
    ];
    for (index, frame) in incompatible.into_iter().enumerate() {
        let candidate_id = u32::try_from(index + 2).expect("small fixture id");
        assert_eq!(
            state
                .stabilize(
                    frame,
                    &candidates(candidate_id, 0.1, 0.1, 0.0),
                    options(1.0),
                    &GOOD_BUDGET,
                    &CancellationToken::new(),
                )
                .expect("incompatible identities start clean"),
            [candidate_id, candidate_id]
        );
    }
    assert_eq!(
        state
            .stabilize(
                context(49),
                &candidates(6, 0.1, 0.1, 0.0),
                options(1.0),
                &GOOD_BUDGET,
                &CancellationToken::new(),
            )
            .expect("backwards time cannot reuse history"),
        [6, 6]
    );
    state.invalidate(8);
    assert_eq!(
        state.stabilize(
            context(60),
            &candidates(7, 0.1, 0.1, 0.0),
            options(1.0),
            &GOOD_BUDGET,
            &CancellationToken::new(),
        ),
        Err(Error::Cancelled),
        "old generation cannot publish after invalidation"
    );
}

#[test]
fn zero_weight_is_exactly_independent_frame_and_stale_history_expires() {
    let mut state = TemporalScratch::default();
    state
        .stabilize(
            context(100),
            &candidates(1, 0.1, 0.1, 0.0),
            options(1.0),
            &GOOD_BUDGET,
            &CancellationToken::new(),
        )
        .expect("initial frame succeeds");
    assert_eq!(
        state
            .stabilize(
                context(200),
                &candidates(2, 0.1, 0.1, 0.0),
                options(0.0),
                &GOOD_BUDGET,
                &CancellationToken::new(),
            )
            .expect("zero weight preserves independent result"),
        [2, 2]
    );
    assert_eq!(
        state
            .stabilize(
                context(3_000_000_000),
                &candidates(3, 0.1, 0.1, 0.0),
                options(1.0),
                &GOOD_BUDGET,
                &CancellationToken::new(),
            )
            .expect("expired state starts clean"),
        [3, 3]
    );
}

#[test]
fn cancellation_and_budget_failures_do_not_publish_and_budget_recovers() {
    let mut state = TemporalScratch::default();
    state
        .stabilize(
            context(1),
            &candidates(5, 0.1, 0.1, 0.0),
            options(1.0),
            &GOOD_BUDGET,
            &CancellationToken::new(),
        )
        .expect("initial frame succeeds");
    let failed_budget = Budget::new(1_000_000, 4, 10_000);
    assert_eq!(
        state.stabilize(
            context(2),
            &candidates(6, 0.1, 0.1, 0.0),
            options(1.0),
            &failed_budget,
            &CancellationToken::new(),
        ),
        Err(Error::BudgetExceeded(BudgetKind::RasterBytes))
    );
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert_eq!(
        state.stabilize(
            context(2),
            &candidates(6, 0.1, 0.1, 0.0),
            options(1.0),
            &GOOD_BUDGET,
            &cancellation,
        ),
        Err(Error::Cancelled)
    );
    assert_eq!(
        state
            .stabilize(
                context(2),
                &candidates(6, 0.1, 0.1, 0.0),
                options(1.0),
                &GOOD_BUDGET,
                &CancellationToken::new(),
            )
            .expect("valid budget recovers after failed calls"),
        [5, 5]
    );
}

#[test]
fn synthetic_metrics_measure_selected_glyph_fidelity_and_responsiveness() {
    let glyph_value = |glyph: u32| match glyph {
        10 => 0.45,
        11 => 0.55,
        12 => 0.46,
        13 => 0.54,
        30 => 0.10,
        31 => 0.90,
        40 => 0.20,
        41 => 0.80,
        _ => panic!("unexpected synthetic glyph {glyph}"),
    };
    let reconstruction_error = |selected: &[u32], reference: &[f64]| {
        selected
            .iter()
            .zip(reference)
            .map(|(glyph, expected)| (glyph_value(*glyph) - expected).abs())
            .sum::<f64>()
            / reference.len() as f64
    };
    let changed_fraction = |before: &[u32], after: &[u32]| {
        before
            .iter()
            .zip(after)
            .filter(|(left, right)| left != right)
            .count() as f64
            / before.len() as f64
    };
    let frame = |glyphs: [u32; 2], best_score, previous_score, motion| {
        glyphs.map(|glyph_id| TemporalCandidate {
            glyph_id,
            best_score,
            previous_score,
            motion,
        })
    };

    let mut state = TemporalScratch::default();
    let static_reference = [0.5, 0.5];
    let initial = state
        .stabilize(
            context(0),
            &frame([10, 11], 0.05, 0.05, 0.0),
            options(1.0),
            &GOOD_BUDGET,
            &CancellationToken::new(),
        )
        .expect("initial frame starts clean")
        .to_vec();
    let noisy_independent = [12, 13];
    let noisy = state
        .stabilize(
            context(1_000_000_000),
            &frame(noisy_independent, 0.04, 0.05, 0.0),
            options(1.0),
            &GOOD_BUDGET,
            &CancellationToken::new(),
        )
        .expect("static noise can be retained")
        .to_vec();
    let noisy_error = reconstruction_error(&noisy, &static_reference);
    let noisy_independent_error = reconstruction_error(&noisy_independent, &static_reference);
    assert_eq!(changed_fraction(&initial, &noisy), 0.0);
    assert_eq!(changed_fraction(&initial, &noisy_independent), 1.0);
    assert!(
        noisy_error <= noisy_independent_error + 0.02,
        "retention fidelity regression exceeded tolerance: selected={noisy_error}, independent={noisy_independent_error}"
    );

    let motion_reference = [0.1, 0.9];
    let moving_independent = [30, 31];
    let moving = state
        .stabilize(
            context(1_100_000_000),
            &frame(moving_independent, 0.0, 0.4, 0.9),
            options(1.0),
            &GOOD_BUDGET,
            &CancellationToken::new(),
        )
        .expect("motion remains responsive")
        .to_vec();
    assert_eq!(changed_fraction(&noisy, &moving), 1.0);
    assert_eq!(
        reconstruction_error(&moving, &motion_reference),
        reconstruction_error(&moving_independent, &motion_reference)
    );

    let cut_reference = [0.2, 0.8];
    let cut = state
        .stabilize(
            context(1_200_000_000).with_reset(TemporalReset::HardCut),
            &frame([40, 41], 0.0, 0.0, 0.0),
            options(1.0),
            &GOOD_BUDGET,
            &CancellationToken::new(),
        )
        .expect("cut responds in the first frame")
        .to_vec();
    assert_eq!(changed_fraction(&moving, &cut), 1.0);
    assert_eq!(
        reconstruction_error(&cut, &cut_reference),
        reconstruction_error(&[40, 41], &cut_reference)
    );
}
