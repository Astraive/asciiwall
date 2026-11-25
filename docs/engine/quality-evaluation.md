# ASCII quality evaluation and algorithm tradeoffs

## Quality is not one score

A useful ASCII rendering preserves recognizable structure at a chosen character budget while remaining visually coherent in motion. Pixel error alone can prefer dense glyphs that hide edges; edge emphasis alone can destroy tonal regions. Existing structure-based ASCII research is prior art, not proof that a simple cell matcher reproduces that work. [S39](../research/source-register.md#s39)

Evaluate tone fidelity, edge/orientation fidelity, silhouette recognizability, palette fidelity, glyph readability, temporal stability, and cost separately. Keep the original image, grid size, font metrics, crop, and palette fixed when comparing matchers.

## Calibration corpus

Use synthetic ramps, checkerboards, thin diagonals, vertical/horizontal edges, circles, text-like detail, solid fields, and alpha boundaries; then rights-cleared photographs with faces, foliage, buildings, night scenes, and low contrast. Include portrait/wide crops. For video include slow pan, fast motion, hard cut, fade, camera noise, and compression artifacts.

Do not select only examples that make one algorithm look good. Keep fixtures and settings hashes with every comparison. Human preference tests should present matched conditions and randomize ordering where practical.

## Proposed metrics

Reconstruct glyph masks into a comparable luminance raster and measure normalized tone error. Compare edge maps/orientation histograms at the cell scale. Measure changed-glyph fraction between adjacent frames after accounting for source motion where feasible. Track temporal lag after a scene cut. Report these alongside throughput and memory.

These metrics are engineering diagnostics, not a universal perceptual truth. A highly stable output can simply be frozen, so temporal scores must be paired with responsiveness and fidelity. A smaller grid doing less work is not an equal-quality performance win unless the reduction is explicit.

## Matcher tuning

Density mode is the fast baseline. Structural mode uses calibrated glyph masks, a cheap descriptor shortlist, and bounded detailed reranking. Tune tone/shape/edge weights with a reproducible corpus rather than hard-coding one ramp that only looks good in one font.

Temporal hysteresis is a caller-driven, portable post-matcher stage. It stores only prior glyph IDs and accepts media timestamps, semantic source/glyph/settings fingerprints, and a generation from its caller; it never reads a wall clock or native/UI state. Caller reset signals for cuts, seeks, source changes, font/grid changes, and decode discontinuities clear all retained state. A changed fingerprint, generation, geometry, backward/duplicate timestamp, or a gap beyond the configured lifetime likewise prevents reuse. Reusable history gaps are bounded to 30 seconds. `TemporalScratch::invalidate` establishes a generation barrier so work from an obsolete settings/seek generation is rejected.

For each cell the matcher supplies the independent winner score, the old glyph's score on the current frame, and local motion in `[0, 1]`. A prior glyph is retained only when its score improvement does not exceed `maximum_threshold × weight × min(elapsed_media_time / 1 s, 1) × (1 − motion)`. This is a bounded hysteresis margin, not a global penalty; motion suppresses it, and zero weight gives exactly the independent-frame glyphs. The portable module bounds storage to one million cells plus scratch budget, bounds work per frame, checks cancellation, and does not partially publish history on failure.

Temporal comparisons should report changed-glyph fraction with motion context, reconstruction error, and scene-cut lag together. Low churn alone is not a quality win: a frozen image must fail responsiveness and reconstruction checks. Record fixture and configuration hashes, platform/toolchain, and the exact commands with each measurement; do not infer a universal quality or throughput guarantee from synthetic fixtures.

### CNV-010 deterministic synthetic evidence

The checked-in two-cell synthetic sequence reports changed-glyph fraction `0/2 = 0.0` for static score noise (the retained candidates' supplied reconstruction-score proxy averages `0.20`, versus the independent winner's `0.10`), `2/2 = 1.0` for a high-motion pan (selected score proxy `0.05`), and `2/2 = 1.0` on the hard-cut frame (cut lag `0` frames). These are deterministic API-contract fixtures, not image-quality measurements or broad performance claims.

Evidence inputs: fixture SHA-256 `3ff1cbd12edf47ee7fd86717ff2657b0e1ea8a745f1ec84013aa435205a40a61` (`crates/ascii-convert/tests/temporal.rs`); canonical sequence/config SHA-256 `aefe75d5d9848245b0b1427bc45d96b98a2041f68f81af669fa1475c74897ff7` over `weight=1.0;maximum_threshold=0.2;maximum_history_gap_ns=2000000000;grid=2x1;timestamps_ns=0,1000000000,1100000000,1200000000;static=(best=0.1,prior=0.2,motion=0.0);pan=(best=0.05,prior=0.3,motion=0.9);cut=hard-cut`. Environment: Windows 10.0.26300.9974 x64, Rust `1.100.0 (0ef1bc386 2026-11-09)`, Cargo `1.100.0 (3acc79c0c 2026-11-05)`, Python `3.12.10`.

Commands and observed results (each Rust command ran with `CARGO_HOME=D:/worktrees/cnv-008-cargo-home-isolated CARGO_TARGET_DIR=D:/worktrees/cnv-010-temporal-target`): `cargo test --offline --locked -p ascii-convert --test temporal` (7 passed); `cargo test --offline --locked -p ascii-convert --test structural_matcher --test density_matcher` (41 passed); `cargo clippy --offline --locked --workspace --all-targets -- -D warnings` (passed); `cargo fmt --all -- --check` (passed); `python scripts/check-repository.py` (passed).

## Color and font effects

Dark-on-light versus light-on-dark needs deliberate calibration; inversion is not always just reversing a hand-written string. Glyph coverage depends on actual font, size, antialiasing, and cell geometry. [S26](../research/source-register.md#s26) Foreground/background colors and alpha influence perceived density, so document whether matching uses luminance-only or a color-aware reconstruction objective.

## Acceptance

The density baseline must be stable and explainable. Structural mode should show a demonstrable quality improvement on a balanced set at its stated cost. Offer both rather than forcing the most expensive algorithm. Publish representative comparisons with licenses and measured conditions, not fabricated screenshots or cherry-picked FPS claims.
