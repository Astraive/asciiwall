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

Temporal penalties should discourage gratuitous changes without resisting real motion. Reset at cuts, seeks, source changes, font/grid changes, and large discontinuities. Ordered dithering and error diffusion have different temporal behavior; test moving gradients rather than assuming still-image quality transfers to video.

## Color and font effects

Dark-on-light versus light-on-dark needs deliberate calibration; inversion is not always just reversing a hand-written string. Glyph coverage depends on actual font, size, antialiasing, and cell geometry. [S26](../research/source-register.md#s26) Foreground/background colors and alpha influence perceived density, so document whether matching uses luminance-only or a color-aware reconstruction objective.

## Acceptance

The density baseline must be stable and explainable. Structural mode should show a demonstrable quality improvement on a balanced set at its stated cost. Offer both rather than forcing the most expensive algorithm. Publish representative comparisons with licenses and measured conditions, not fabricated screenshots or cherry-picked FPS claims.
