# Benchmark plan and performance methodology

## No measured claims yet

Every target in this pack is a design budget, not a benchmark result. Measure named hardware, OS/build, compositor, GPU/driver, power mode, display configuration, engine commit, codec build, and source/settings hashes. Do not report 'lightweight' based only on Rust or GPU usage.

## Workload matrix

| Workload | Grid / source | What it isolates |
|---|---|---|
| Static image cold load | 1080p and 4K image; 120/240/320 columns | Decode, preprocess, glyph matching, atlas creation |
| Static image idle | Same frame for 60 seconds | Unnecessary timers, redraw, background CPU |
| Procedural rain/waves | 160/240/320 columns; 20/30/60 FPS | Simulation and glyph presentation |
| Live video density | 1080p30 and 4K30 local source | Decode/resize/CPU match and scheduling |
| Live video structural | Same source/seed/fit, varied candidate count | Quality/cost tradeoff of matcher |
| ASCII clip playback | Same grid and timing | Parsing/decompression/upload without conversion |
| Baked video | Comparable visual output | Decoder/presentation baseline, not editable parity |
| Two/four outputs | Independent and shared sources | Aggregate memory, devices, pacing |
| Shell bridge | Same output through Qt/GNOME candidate | IPC/copy cost and shell responsiveness |
| Interaction storm | Coalesced high-rate pointer and window moves | Event scheduling and bounded effects |

Use rights-cleared fixtures and record their hashes. Include both smooth motion and hard cuts; temporal stabilization can look cheap on a nearly static source while failing on fast motion.

## Metrics

Track cold-start/import latency, first-valid-frame latency, conversion and simulation p50/p95/p99, CPU frame submission, actual GPU time where timestamp queries are supported, frame lateness/drop rate, peak/resident memory, allocations, decoder queue depth, IPC throughput, cache hit ratio, and process count.

Do not label CPU submission time as GPU execution time. When GPU timestamp queries are unavailable, report the limitation and use end-to-end timing. Report percentile distributions and warmup policy, not one best sample.

## Proposed initial budgets

Balanced 30 FPS has a 33.3 ms presentation interval. On a declared reference machine, start with a density-conversion p95 budget of 8 ms and GPU rendering p95 budget of 4 ms, leaving room for decode/IPC/pacing. Structural quality mode may need a different density/FPS target. These targets must be revised from evidence, not treated as universal guarantees.

At 240 x 68 cells, 12 raw bytes per cell is 195,840 bytes/frame; at 30 FPS about 5.88 MB/s before metadata/copies. A structural matcher testing 12 candidates with 128 mask samples does about 25.1 million candidate-pixel comparisons per frame. This arithmetic motivates descriptors/shortlists and profiling; it does not prove a CPU implementation meets 30 FPS.

## Energy and idle

Measure static versus animated versus paused under controlled brightness, power profile, and background workload. Include integrated and discrete GPUs. GPU residency may matter even when CPU is low. Do not claim exact battery-hour savings from process CPU alone.

An unchanged static wallpaper should become event-driven; test that decode/conversion counters stop. A paused video should not continue decoding invisibly. Hidden/fullscreen behavior needs realistic window/capability conditions.

## Benchmark procedure

Warm caches separately from cold runs; perform at least five repeated trials; use fixed seeds and a documented warmup/frame count; record thermal state and outliers; keep screenshots/cell outputs for quality comparisons. A performance improvement that noticeably destroys visual fidelity is not equivalent work.

Create regression thresholds only after stable baselines exist. CI virtual runners are useful for algorithm regressions, not authoritative GPU/energy numbers.
