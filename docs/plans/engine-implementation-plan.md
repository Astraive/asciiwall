# Engine Implementation Plan

> **For agentic workers:** Use a task-by-task execution/review workflow; where available, use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. The plan also works as a human implementation checklist.

**Goal:** Deliver the engine work units without weakening the shared contracts.

**Architecture:** Rust libraries own portable algorithms and validation. Executables and native adapters compose them through bounded, versioned contracts; no hosted backend is introduced.

**Tech stack:** Rust Cargo workspace, wgpu/egui where applicable, image/fontdue, replaceable FFmpeg binding, native OS APIs, schema/ZIP tools and local IPC.

**Spec:** [System overview](../architecture/system-overview.md), [data contracts](../architecture/data-model-and-contracts.md), [package specification](../package/format-specification.md), and the linked subsystem documents.

## Global constraints

Stable package format is draft 1.0; it is not frozen until conformance passes. Core operation is offline. Control IPC is capped at 1 MiB. Decoded frame queues are bounded to two or three. Package archives are at most 1 GiB compressed and 2 GiB actually expanded. All visual inputs and work are resource-bounded. Packages cannot grant permissions or execute arbitrary code. No native platform is supported solely because it compiles.

All signatures below are target interfaces. Define missing types in the listed owning task, reusing the core models rather than duplicating them. Paths are future application-repository paths, not files implemented in this documentation bundle. The CLI crate's proposed package name is `ascii-cli`; the build helper package is `xtask`.

## Review focus

Untrusted paths and nested I/O: tests P03/H02 deny filesystem/network escape.

Stale async completion after Apply/seek: tests E09/D10 reject old generations.

Mixed coordinate spaces and missing observer support: tests D02/D08/D09 use explicit unknown states.

Resource totals across displays/transitions: tests E09/H03 enforce bounded aggregate usage.

User control after shell/worker failure: tests D03/D07/H05 restore a stoppable safe state.

## File ownership

The [repository layout](../architecture/repository-structure.md) defines crate boundaries. Each task below lists its production and test files. Core models are owned by E01; glyph models E02; media ownership E05; platform capabilities D02; settings P01; package contracts P02; IPC H01. Do not create shadow types with incompatible semantics in other crates.

## E01: Core frame and budget contracts

**Files:** Create or modify `crates/ascii-core/src/{lib.rs,frame.rs,budget.rs,error.rs}`. Integration test: `crates/ascii-core/tests/frame_contract.rs`. No dependency or I/O setup is required.

**Consumes and owns:** None; defines `AsciiCell`, immutable `AsciiFrame` and `FrameMetadata`, fixed-width `GlyphTableId`, borrowed `RasterFrameView`, `Budget`, typed `Error`, and `CancellationToken`.

**Produces:**
The following Rust signatures omit function bodies:
```rust
pub fn validate_grid_budget(
    size: GridSize,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<GridBudget, Error>

pub fn validate_frame(
    frame: &AsciiFrame,
    expected_glyph_table_id: GlyphTableId,
    glyph_count: u32,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<(), Error>

pub fn validate_raster_frame<'a>(
    view: RasterFrameView<'a>,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<ValidatedRasterFrameView<'a>, Error>
```

`AsciiFrame::try_new` takes `FrameMetadata` and an already allocated `Vec<AsciiCell>`. It validates
shape, cell bounds, capacity, cancellation, and byte/work limits without allocating. Consumers use
`validate_frame` to compare the declared glyph-table identity with their expected table. Producers
call `validate_grid_budget` first; it checks platform `usize` conversion, checked cell bytes,
Rust's `isize::MAX` allocation-layout limit, byte budget, and work budget. The exact `u32 × u32`
cell count fits in `u64`; test checked byte or platform-size overflow instead. `into_parts` returns
the original cell vector for allocation reuse.

Raster validation supports top-down 8-bit Gray/RGB/RGBA with sRGB primaries and transfer. It rejects unsupported sample depth, orientation, color/transfer/alpha metadata, undersized stride, and backing shorter than `(height - 1) * row_stride + row_bytes`; trailing bytes and unused final-row padding are allowed. It returns a borrowed validated view, performs no pixel copies, and checks raster-byte/work budgets and cancellation.

**Dependencies:** none.

- [ ] Add behavior-focused tests in `crates/ascii-core/tests/frame_contract.rs`: exact cell counts and glyph bounds/table identity; preflight byte/work limits and checked byte/platform-size overflow; over-budget vector capacity; cancellation; padded stride; one-byte-short backing; trailing bytes; zero dimensions; unsupported raster metadata; and typed malformed/unsupported/budget/cancellation errors.
- [ ] Run `cargo test -p ascii-core --test frame_contract -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interfaces with explicit error categories and bounded validation. The core must remain `no_std`, `forbid(unsafe_code)`, dependency-free, and free of I/O.
- [ ] Run `cargo test -p ascii-core --test frame_contract -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## E02: Font masks and glyph descriptors

**Files:** Create or modify `crates/ascii-convert/src/{glyphs.rs,font.rs}`. Test file: `crates/ascii-convert/tests/glyph_table.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E01; defines GlyphConfig and immutable GlyphTable with fingerprint.

**Produces:** `build_glyph_table(font_bytes: &[u8], charset: &str, config: &GlyphConfig, budget: &Budget) -> Result<GlyphTable, Error>`

**Dependencies:** E01.

- [ ] Write the failing contract test in `crates/ascii-convert/tests/glyph_table.rs`. Assertions: Reject controls and unsupported multi-cell symbols; retain stable ordering for equal coverage; fingerprint changes with font bytes.
- [ ] Run `cargo test -p ascii-convert --test glyph_table -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-convert --test glyph_table -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## E03: Density conversion and aspect-aware sampling

**Files:** Create or modify `crates/ascii-convert/src/{density.rs,sampling.rs,color.rs}`. Test file: `crates/ascii-convert/tests/density_matcher.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E01 E02; defines validated ConversionSettings and linear-light preprocessing.

**Produces:** `convert_density(input: RasterFrameView<'_>, glyphs: &GlyphTable, settings: &ConversionSettings, budget: &Budget) -> Result<AsciiFrame, Error>`

**Dependencies:** E01 E02.

- [ ] Write the failing contract test in `crates/ascii-convert/tests/density_matcher.rs`. Assertions: For 1920x1080 and 240 columns with 8x16 cells derive 68 rows; black/white endpoints stable; padded stride and alpha fixtures correct.
- [ ] Run `cargo test -p ascii-convert --test density_matcher -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-convert --test density_matcher -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## E04: Structural matcher and temporal state

**Files:** Create or modify `crates/ascii-convert/src/{structural.rs,temporal.rs}`. Test file: `crates/ascii-convert/tests/structural_matcher.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E03; extends the converter without OS/media dependency.

**Produces:** `convert_structural(input: RasterFrameView<'_>, glyphs: &GlyphTable, settings: &ConversionSettings, previous: Option<&AsciiFrame>, budget: &Budget) -> Result<AsciiFrame, Error>`

**Dependencies:** E03.

- [ ] Write the failing contract test in `crates/ascii-convert/tests/structural_matcher.rs`. Assertions: A vertical-edge fixture prefers a better mask over a same-density poor shape; zero temporal weight matches independent frames; cut resets history.
- [ ] Run `cargo test -p ascii-convert --test structural_matcher -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-convert --test structural_matcher -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## E05: Bounded image decoding

**Files:** Create or modify `crates/ascii-media/src/{image.rs,decoded_frame.rs}`. Test file: `crates/ascii-media/tests/image_decode.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E01; defines ReadSeek, DecodeLimits and owned DecodedFrame.

**Produces:** `decode_image(input: &mut dyn ReadSeek, limits: &DecodeLimits, cancel: &CancellationToken) -> Result<DecodedFrame, Error>`

**Dependencies:** E01.

- [ ] Write the failing contract test in `crates/ascii-media/tests/image_decode.rs`. Assertions: Reject oversized dimensions before full allocation; orientation applied once; transparent/disposal fixtures preserve expected pixels; cancel releases buffers.
- [ ] Run `cargo test -p ascii-media --test image_decode -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-media --test image_decode -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## E06: GPU atlas presenter and preview

**Files:** Create or modify `crates/ascii-render/src/{renderer.rs,atlas.rs,surface.rs}`. Test file: `crates/ascii-render/tests/glyph_render.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E01 E02; defines RenderTarget and PresentOutcome, device-limit checks.

**Produces:** `Renderer::present(&mut self, frame: &AsciiFrame, target: &RenderTarget) -> Result<PresentOutcome, Error>`

**Dependencies:** E01 E02.

- [ ] Write the failing contract test in `crates/ascii-render/tests/glyph_render.rs`. Assertions: Render fixed grid against a tolerant golden; invalid glyph rejected; lost surface returns recoverable outcome; zero-size target does not spin.
- [ ] Run `cargo test -p ascii-render --test glyph_render -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-render --test glyph_render -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## E07: Declarative scene execution

**Files:** Create or modify `crates/ascii-scene/src/{graph.rs,nodes.rs,executor.rs}`. Test file: `crates/ascii-scene/tests/scene_graph.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E01 E02; defines typed SceneDocument/SceneProgram and node registry.

**Produces:** `compile_scene(scene: SceneDocument, budget: &Budget) -> Result<SceneProgram, Error>`

**Dependencies:** E01 E02.

- [ ] Write the failing contract test in `crates/ascii-scene/tests/scene_graph.rs`. Assertions: Reject cycle/missing node/duplicate slot/unreachable node; generator has zero inputs; unary one and composite two; identical seed/time gives identical grid.
- [ ] Run `cargo test -p ascii-scene --test scene_graph -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-scene --test scene_graph -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## E08: FFmpeg decoder boundary

**Files:** Create or modify `crates/ascii-media/src/{video.rs,decoder.rs,io_policy.rs}`. Test file: `crates/ascii-media/tests/video_decode.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E05; defines decoder construction from approved I/O, PTS/duration and seek/drain contract.

**Produces:** `VideoDecoder::next_frame(&mut self, cancel: &CancellationToken) -> Result<Option<DecodedFrame>, Error>`

**Dependencies:** E05.

- [ ] Write the failing contract test in `crates/ascii-media/tests/video_decode.rs`. Assertions: B-frame/VFR/drain fixtures produce valid presentation timestamps; disallowed nested URL/file denied; EAGAIN handled without busy retry; cancel terminates.
- [ ] Run `cargo test -p ascii-media --test video_decode -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-media --test video_decode -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## E09: Media and simulation scheduler

**Files:** Create or modify `crates/ascii-runtime/src/{clock.rs,scheduler.rs,pause.rs}`. Test file: `crates/ascii-runtime/tests/scheduler.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E01 E08; defines MonoTime, RuntimeInputs, RuntimeActions and pause-reason set.

**Produces:** `Scheduler::tick(&mut self, now: MonoTime, inputs: &RuntimeInputs) -> RuntimeActions`

**Dependencies:** E01 E08.

- [ ] Write the failing contract test in `crates/ascii-runtime/tests/scheduler.rs`. Assertions: Queue never exceeds three; late frames dropped; manual pause survives fullscreen clearing; long suspend clamps catch-up; seek increments generation.
- [ ] Run `cargo test -p ascii-runtime --test scheduler -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-runtime --test scheduler -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## E10: Precomputed character-clip codec

**Files:** Create or modify `crates/ascii-media/src/{clip_reader.rs,clip_writer.rs}`. Test file: `crates/ascii-media/tests/clip_codec.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E01; implements exact draft little-endian header/index/cell encoding.

**Produces:** `ClipReader::read_frame(&mut self, index: u64, budget: &Budget) -> Result<AsciiFrame, Error>`

**Dependencies:** E01.

- [ ] Write the failing contract test in `crates/ascii-media/tests/clip_codec.rs`. Assertions: Reject wrong magic, reserved bits, overlapping/overflow offsets and invalid glyph IDs; round-trip a 2x2 full frame with exact timestamps and CRC.
- [ ] Run `cargo test -p ascii-media --test clip_codec -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-media --test clip_codec -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## E11: Text, PNG and video export

**Files:** Create or modify `crates/ascii-render/src/export.rs; apps/cli/src/export.rs`. Test file: `crates/ascii-render/tests/export_pipeline.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E03 E06 E08 E10; defines output representation and atomic export receipt.

**Produces:** `export(request: &ExportRequest, cancel: &CancellationToken) -> Result<ExportReceipt, Error>`

**Dependencies:** E03 E06 E08 E10.

- [ ] Write the failing contract test in `crates/ascii-render/tests/export_pipeline.rs`. Assertions: Cancel/disk-full preserves previous target; text contains no control escapes; fixed seed/time reproduces cell output; unavailable encoder reports capability error.
- [ ] Run `cargo test -p ascii-render --test export_pipeline -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-render --test export_pipeline -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## E12: Reproducible benchmark harness

**Files:** Create or modify `benches/; crates/ascii-runtime/src/metrics.rs`. Test file: `crates/ascii-runtime/tests/benchmark_report.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E04 E06 E09; defines versioned report with environment/fixture hashes and percentiles.

**Produces:** `run_benchmark(spec: &BenchmarkSpec) -> Result<BenchmarkReport, Error>`

**Dependencies:** E04 E06 E09.

- [ ] Write the failing contract test in `crates/ascii-runtime/tests/benchmark_report.rs`. Assertions: Report distinguishes CPU submit from GPU timing; static idle produces no conversion ticks; repeated fixed workload emits comparable metric keys without source pixels.
- [ ] Run `cargo test -p ascii-runtime --test benchmark_report -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-runtime --test benchmark_report -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## Completion evidence

Record targeted command output, source revision, fixture hashes, any native test environment, and accepted limitations. A local compile does not prove wallpaper layering, sandbox confinement, energy use, or accessibility. Promote only the behaviors actually verified and update the backlog/support matrix accordingly.
