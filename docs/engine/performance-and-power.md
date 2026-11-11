# Performance budgets and power policy

All numbers here are **initial design targets**, not measured product claims. Benchmark results must name hardware, OS, graphics backend, display resolution/refresh, source codec, grid, scene, and measurement method.

## Work categories

Account separately for decoding, preprocessing, glyph matching, simulation, GPU upload, drawing, composition, IPC, and background monitoring. A low glyph count does not imply low energy use if a 4K decoder or a high-rate global event poll continues running.

## Proposed profiles

| Profile | Animation cap | Typical starting grid width | Behavior |
| --- | --- | --- | --- |
| Quiet | 20 FPS | 160 columns | minimal effects, aggressive offscreen/idle suppression |
| Balanced | 30 FPS | 240 columns | default quality, bounded structural matching |
| Performance | 60 FPS | 320 columns | higher cost with explicit user choice |
| Battery | 10 FPS or static | 120 columns | reduce optional effects and monitoring |
| Static | event-driven | chosen image grid | no continuous animation timer |

Rows derive from actual content/cell aspect, not a square-grid assumption. Limit total cells, texture sizes, particles, and decoder instances in addition to column count.

## First benchmark budgets

On a chosen baseline integrated-GPU machine, target an end-to-end 30 FPS frame deadline of 33.3 ms with headroom. A provisional split might reserve 8 ms p95 for conversion at the documented baseline grid and 4 ms p95 for rendering, leaving decode/scheduling overhead. These are engineering goals to validate, not a promise for any image, codec, or computer.

Record CPU time per frame, p50/p95/p99 latency, dropped/late frames, wakeups, resident memory, GPU memory, and incremental power where instrumentation permits. A single Task Manager percentage is insufficient, especially across different core counts.

## Adaptation

Reduce only marked adaptive controls: presentation FPS, analysis resolution, quality candidate count, optional postprocessing, and monitoring frequency. Apply hysteresis to avoid oscillation. Show effective settings separately from requested settings. Never silently alter the source palette, semantic scene parameters, or saved preset.

Pause/throttle policies operate per display when reliable visibility exists. Fully covered opaque windows can justify reduced work; transparent/unknown windows cannot prove total occlusion. Retain conservative behavior rather than pausing incorrectly.

## Resource ceilings

Use bounded frame queues, one shared decoder for genuinely synchronized identical streams, atlas reuse, and cancellation on invalidation. Limit simultaneous exports and imports. Warn before high-resolution dual-scene transitions because they temporarily increase memory.

Do not advertise '0% CPU/GPU' as an absolute. Even a paused process can have event handling and bookkeeping overhead. The practical requirement is no unnecessary animation/decoding work and measured low idle wakeups.
