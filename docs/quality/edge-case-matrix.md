# Edge-case and failure-mode matrix

## Status

This matrix defines **120 proposed scenarios** and expected outcomes. It is not a record of tests already executed. P0 blocks a release claiming support for the affected capability/host; P1 requires a pass or an explicit accepted limitation. Unsupported capabilities are tested for safe fallback rather than fabricated parity.

## Input and conversion

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-001 | P0 | Zero dimensions or overflowed grid | Reject before allocation; checked products |
| EC-002 | P1 | Image smaller than requested grid | Resample consistently; do not read past source |
| EC-003 | P1 | Extreme portrait/wide image | Fit/crop without accidental aspect distortion |
| EC-004 | P1 | EXIF rotated image | Apply orientation once before grid derivation |
| EC-005 | P1 | Transparent colored edges | Composite correctly; no dark alpha halo |
| EC-006 | P1 | Unsupported HDR/profile | Explain conversion limitation; no silent wrong pixel format |
| EC-007 | P1 | All-black/all-white/flat image | Stable density mapping; no divide-by-zero normalization |
| EC-008 | P0 | Odd row stride and padded data | Honor stride and buffer extent |
| EC-009 | P0 | Malformed/truncated image | Worker failure with safe UI error |
| EC-010 | P0 | Small compressed image with huge dimensions | Reject against pixel/byte budget before full allocation |

### QA-102 portable regression coverage

The EC-006–EC-008 PASS rows below are portable CPU/API results; historical and exact-head run revisions are recorded separately in the execution-evidence subsections. These results are limited to the named boundaries and are not a QA-004 acceptance receipt.

| ID | Exercised real interface | Remaining boundary |
|---|---|---|
| EC-006 | **PASS (portable detailed conversion).** PQ HDR-transfer and Rec.2020 wide-gamut metadata return their corresponding typed normalization causes and exact actionable `Display` limitations: `HDR transfer Pq is unsupported; provide SDR input or use an explicitly supported tone-mapping path`; `wide-gamut primaries Rec2020 are unsupported; convert to sRGB with tested color-managed tooling`. No frame is returned. | This does not exercise decoder-extracted metadata or CLI/UI messaging. The convenience `convert_density` API intentionally maps unsupported color to a generic core error. |
| EC-007 | **PASS (portable density conversion).** All-black, all-white, and flat mid-gray Gray8 inputs convert repeatably with exact repeated cells; black selects blank space, white selects ink glyph `A`, descriptors/samples remain finite, gradients are zero, and endpoint luminance is exactly 0/1. | This covers the CPU conversion path only, not native preview/rendering. |
| EC-008 | **PASS (padded-raster converter and decoder-to-converter smoke).** The exact-minimum 3×2 Gray8 view with stride 5 ignores poison padding and converts black/white rows deterministically; a separate odd-width 3×2 synthetic PNG was decoded with `ascii-media::decode_image`, borrowed via `DecodedFrame::view()`, and passed directly to `convert_density_detailed` (decoded RGB stride 9; output 3×2). | Portable CPU/API boundaries only: the decoder returns tightly packed output, so padding behavior is covered by the direct raster test, not decoder-added padding. Device-specific stride remains untested. |
| EC-009 | **NOT RUN.** The separately named raw-raster backing validation regression verifies that the converter preserves `RasterBackingTooShort`; it is not EC-009 because it does not use malformed encoded input, a restricted worker, or a UI. | The current three-member workspace has no SEC-005 restricted media worker or UIX-001 application UI/safe-error surface. The required malformed/truncated encoded-input → worker failure → safe UI error path is unavailable; no worker/UI pass is claimed. |
| EC-010 | **PASS (exact PR #337 head `27c8bc179436a1851a865f489d92c009e7ba2247` at the time of the rerun; tested source/test revision `fb8ddb206df60a7083083e46754c32fe14f3ddd8`).** `image::codec_cancellation_tests::oversized_jpeg_is_rejected_before_output_allocation` in `crates/ascii-media/src/image.rs` feeds a sub-1-KiB JPEG advertising 4096×4096 through the decoder with an injected output allocator. The test asserts `BudgetExceeded(RasterBytes)` and that the allocator was never invoked; the exact-head command passed, 1 passed, 11 filtered. | Decoder budget-guard coverage only; this is not a QA-004 evidence receipt. The generated JPEG is not separately persisted or hashed; no QA-004 archive/receipt is claimed. |

#### Historical QA-102 portable execution evidence

The following checks were run on replacement source/test revision `fb8ddb206df60a7083083e46754c32fe14f3ddd8`. They remain historical evidence for that revision and do not establish results for later source/test snapshots:

- `cargo test --locked -p ascii-convert --test qa_102_input_conversion` — **PASS**, 6 passed.
- `ascii-convert` package test suites — **PASS**, 80 passed across 6 suites.
- `ascii-media --lib` — **PASS**, 12 passed; `image_decode` — **PASS**, 20 passed.
- `cargo fmt --all -- --check` — **PASS**.
- Strict workspace Clippy (`cargo clippy --locked --workspace --all-targets -- -D warnings`) — **PASS**.

At that time, PR commits `4f2c01fdf9e12bbce2b831b3f5fdc2978f5c782c` and `27c8bc179436a1851a865f489d92c009e7ba2247` were docs-only and the QA-102 conversion suite was not rerun at either head. This historical limitation is superseded by the exact-head run below, not by changing the results above.

#### QA-102 exact-head portable execution evidence

EC-006–EC-008 were exercised at exact PR #337 head and tested source/test revision `37f12f1afeefdab509f3315a47c90fc1a7e84d25`:

- `cargo test --locked -p ascii-convert` — **PASS**, 85 passed across 8 suites.
- **Host/environment:** Windows x64; isolated `CARGO_HOME` and `CARGO_TARGET_DIR`; portable CPU test run.

This tested snapshot includes the updated normalization/conversion/sampling code and QA-102 tests using `NormalizedRasterResources`. The explicit owner bounds concurrently retained owned normalized-output bytes; each operation's `Budget` remains separate for input extent and per-operation limits. This evidence is limited to the portable normalization/conversion API and resource-owner boundary. It does not claim QA-004 receipt integration, restricted-worker/UI behavior, native/manual evidence, or full QA-102 acceptance; existing NOT RUN limitations remain.

#### EC-010 source revision and exact-head evidence

The original run below was on historical predecessor head `dd699e480f31c77e50f51f5831c81f5e240febaa`; `44688a862052f2e597ba7a61df80f3928d33158c` and `0e447854ec0e986e45678fdbfa31c889da2e5d41` were docs-only, with `0e447854ec0e986e45678fdbfa31c889da2e5d41` serving as prior-head evidence. EC-010 was verified on tested source/test revision `fb8ddb206df60a7083083e46754c32fe14f3ddd8` and rerun on exact PR #337 head `27c8bc179436a1851a865f489d92c009e7ba2247`. PR commits `4f2c01fdf9e12bbce2b831b3f5fdc2978f5c782c` and `27c8bc179436a1851a865f489d92c009e7ba2247` were docs-only; later documentation-only edits do not alter the tested source/test code.

- `cargo test --locked -p ascii-media --lib oversized_jpeg_is_rejected_before_output_allocation -- --nocapture` — **PASS**, 1 passed, 11 filtered. The runner reports `image::codec_cancellation_tests::oversized_jpeg_is_rejected_before_output_allocation`.

The same exact command was run on prior PR head `0e447854ec0e986e45678fdbfa31c889da2e5d41`:

- `cargo test --locked -p ascii-media --lib oversized_jpeg_is_rejected_before_output_allocation -- --nocapture` — **PASS**, 1 passed.

The exact command below was run on tested source/test revision `fb8ddb206df60a7083083e46754c32fe14f3ddd8` and on PR #337 head `4f2c01fdf9e12bbce2b831b3f5fdc2978f5c782c` at that time:

- `cargo test --locked -p ascii-media --lib oversized_jpeg_is_rejected_before_output_allocation -- --nocapture` — **PASS**, 1 passed, 11 filtered, on `fb8ddb206df60a7083083e46754c32fe14f3ddd8`.
- `cargo test --locked -p ascii-media --lib oversized_jpeg_is_rejected_before_output_allocation -- --nocapture` — **PASS**, 1 passed, 11 filtered, on PR #337 head `4f2c01fdf9e12bbce2b831b3f5fdc2978f5c782c`.

The same command was rerun on exact PR #337 head `27c8bc179436a1851a865f489d92c009e7ba2247`:

- `cargo test --locked -p ascii-media --lib oversized_jpeg_is_rejected_before_output_allocation -- --nocapture` — **PASS**, 1 passed, 11 filtered.

This confirms decoder budget-guard coverage only; it is not a QA-004 receipt and does not establish EC-009, native/manual, worker/UI, or full QA-102 acceptance. QA-004 #35 remains a hard dependency; no archived QA-004 receipt or full QA-102 acceptance is claimed.

#### Historical local execution evidence (portable CPU snapshot; superseded base)

The following recorded run used exact CNV-001 #314 base head `b78920f82ff496e2ee9bd9b5ea454d906a003fa0` (`feat/cnv-001-density-matching-linear`). All commands, counts, hashes, and outcomes in this subsection are historical evidence for that superseded snapshot only; they are not replacement-head verification for PR #337. The run records portable CPU/API behavior only; no application binary, UI, restricted worker, native/GPU, or manual-host scenario was run.

- **Date:** 2026-11-20. **Host:** Microsoft Windows `[Version 10.0.26300.9974]`, x64. **Session/compositor/GPU/permissions:** N/A (headless CPU-only test process; no desktop or worker boundary used). **Reviewer:** this run did not produce a reviewer-linked QA-102 scenario receipt.
- **Environment:** commands used `CARGO_HOME=D:/target/qa102-cargo-home` and `CARGO_TARGET_DIR=D:/target/qa102-target`.
- **Focused QA-102 run:** `cargo test --locked -p ascii-convert --test qa_102_input_conversion -- --nocapture` — **PASS**, 6 passed, 0 failed. This covers EC-006, EC-007, direct padded-raster EC-008, and decoder-to-converter EC-008. The smoke reports `decoded=3x2 stride=9 converted=3x2`; PNG fixture SHA-256: `1d2a30b93e806b147de7b09d809331d041f520fcf39b2842e59ce4ae05b88012`.
- **Workspace:** `cargo test --locked --workspace` — **PASS**, 155 passed, 0 failed.
- **Formatting and lint:** `cargo fmt --all` and `cargo fmt --all -- --check` — **PASS**; `cargo clippy --locked --workspace --all-targets -- -D warnings` — **PASS**.
- **Diff hygiene:** `git diff --check` — **PASS**.
- **SHA-256, QA/conversion sources and binaries:** `crates/ascii-convert/tests/qa_102_input_conversion.rs` `0f1294da433520a1b7e6d459cbd8e59ca38fbc823fbb5f0535ba1a381add61b7`; focused test executable `qa_102_input_conversion-26e74f95670d2e50.exe` `7bccfb40855a914e0d9ff11b14569fdd2ec1a1a068a30be4f352b8039fed9c7e`.
- `crates/ascii-media/tests/image_decode.rs` `ce6a699b7b3765e18d95c061d9e22c7462cf6c5030125b02aa7d438e400c0108`; decoder `crates/ascii-media/src/image.rs` `cf90f3bb1c6f503c823062589e33e07fc1a05ea6d596a9c1fdd24d8c529487c4`; existing decoder-test executable `image_decode-7773c6b353196bb5.exe` `a3379248a4fd57080ed78f1d41fcb6870daf6761c4b034dd630caa22607850f6`.
- `ascii-convert/src/color.rs` `12b503c767b009d4c469fba7224f8fe7256fe6fe53d1f6a7bcad888d31c37afa`; `density.rs` `5ebd619fa34632bfdcf867b3122d6f976eaee36b0045684b714f01bd7c78278c`; `sampling.rs` `0b42905098eaf712ebe9bb71456a3b3d5834add4b64862009df265dc146970ea`; raster contract `ascii-core/src/frame.rs` `3d5d1a5832c2430de20ad81ff57134e05d0d42b9d2059e277f667fd03566bde2`; synthetic-font generator `ascii-convert/tests/support/synthetic_font.rs` `8a0b3b4e8ee150d6d16b084e51200525112ed6424855afa652a55ed3274176c8`; `Cargo.lock` `a1af433632c3d70a159a748480cec7eaae4018dbc66dfa13deb14b48108e6939`.
- **Rights-cleared input hashes:** EC-006 RGB bytes `[40, 96, 180]` `9912b1bfd9248345293ccdccc67e378fde14311cf29e84897a556bec27aac5ea`; EC-007 black/gray-128/white buffers `2ea9ab9198d1638007400cd2c3bef1cc745b864b76011a0e1bc52180ac6452d4` / `1b34669db94decae583e183ee2ffeb07cf504b9f52fae0056c5cf343325157e4` / `ce65609cc564ce3d8cb72d46bc44cd5495afbad7bc61b5edfa0799baa5063191`.
- EC-008 direct padded-raster buffers at stride 5 hash to `c19ddecee2e30d70144a5a3c78cae2896812fe7e4e8b9f49d48aca03bb9f20c0` and `177c9e1875457c6e32e1385bdebd2d919cae8f4c294e431aada3d1bd5945d324`; the separate encoded PNG hash is listed above. No external media was used.

QA-004 issue [#35](https://github.com/Astraive/asciiwall/issues/35) remains an open hard dependency. These portable tests are not a QA-004 evidence-recorder receipt; no native/manual or restricted-worker/UI scenario, archived QA-004 receipt, or full QA-102 acceptance is claimed.

## Glyphs and rendering

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-011 | P1 | Missing glyph in selected font | Explicit fallback or reject, never arbitrary atlas memory |
| EC-012 | P0 | Custom charset contains control/bidi characters | Reject unsupported entries |
| EC-013 | P1 | Combining/emoji/multi-cell sequence | Reject or explain unsupported repertoire in v1 |
| EC-014 | P1 | Duplicate glyphs or identical coverage | Normalize repertoire and keep stable tie-break |
| EC-015 | P1 | Font file changed at same path | Invalidate font fingerprint and glyph-dependent caches |
| EC-016 | P0 | Corrupt or oversized font | Restricted parser rejects under budget |
| EC-017 | P0 | Atlas allocation exceeds device limit | Reduce/fallback or error before GPU allocation |
| EC-018 | P0 | GPU lost or surface outdated | Bounded recreate, generation invalidation, safe fallback |
| EC-019 | P1 | Zero-size/minimized preview | Suspend redraw rather than busy-loop |
| EC-020 | P0 | Different font during transition | Keep both atlases alive until last contribution |

## Video and clocks

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-021 | P1 | Variable frame rate | Schedule by timestamps, not frame index divided by nominal FPS |
| EC-022 | P0 | B-frame reordering and decoder EAGAIN | Correct packet/frame state machine and drain |
| EC-023 | P1 | Missing or nonmonotonic timestamps | Apply documented repair or reject, never negative sleep |
| EC-024 | P0 | Seek during conversion | Discard old-generation frames |
| EC-025 | P1 | Loop at non-keyframe boundary | Seek/decode dependencies then trim to requested interval |
| EC-026 | P1 | Invalid loop start/end | Reject before playback |
| EC-027 | P0 | Long suspend or clock jump | Rebase monotonic playback; bounded catch-up |
| EC-028 | P0 | Decoder slower than presentation | Drop late work; queues remain bounded |
| EC-029 | P1 | Animated GIF disposal/partial frames | Composite frames correctly with individual durations |
| EC-030 | P0 | Playlist/nested media references URL or outside file | Deny through approved-I/O and OS restrictions |

## Pointer and windows

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-031 | P0 | Wallpaper is click-through but no observer feed | Disable reactions; do not claim native delivery |
| EC-032 | P1 | 1000 Hz pointer device | Coalesce movement; no 1000 Hz physics loop |
| EC-033 | P0 | Click over icon/context menu | Preserve desktop action; apply desktop-only scope policy |
| EC-034 | P0 | Double click or drag-select | No consumed/replayed events and no altered timing |
| EC-035 | P1 | Lost button release after focus/session change | Clear held state on feed invalidation |
| EC-036 | P1 | Pointer crosses negative-origin monitor | Correct scene coordinate transform |
| EC-037 | P1 | Overlapping application rectangles | Use union coverage, not sum |
| EC-038 | P0 | Window handle destroyed then reused | Generation prevents stale object mutation |
| EC-039 | P1 | Translucent window and unknown shape | Treat coverage as estimate, no exact-occlusion claim |
| EC-040 | P0 | Wayland toplevel list without rectangles | State-only features may work; geometry effects disabled |

## Desktop lifecycle

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-041 | P0 | Explorer or shell restarts | Rediscover adapter references; no stale parent reuse |
| EC-042 | P0 | Win+D, overview, Mission Control or Task View | No topmost wallpaper/focus steal |
| EC-043 | P0 | Desktop icons hidden/shown | Layering and interactions remain correct or fallback |
| EC-044 | P0 | Permission revoked while running | Unsubscribe and clear sensitive transient history |
| EC-045 | P0 | Lock/UAC/secure desktop | No observation/rendering attempt into protected session |
| EC-046 | P0 | Fast user switch or remote disconnect | Per-session isolation and bounded pause |
| EC-047 | P0 | App killed during exit transition | No promise of animation; resources disappear safely |
| EC-048 | P0 | GNOME/Plasma extension disabled repeatedly | No leaked actor/timer/signal/IPC connection |
| EC-049 | P1 | Unsupported shell/compositor version | Explain unsupported adapter; preview/export still usable |
| EC-050 | P0 | Tray unavailable | Relaunch/CLI can open controls and stop wallpapers |

## Displays and resources

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-051 | P0 | Monitor unplugged during Apply | Cancel candidate for removed surface and park assignment |
| EC-052 | P1 | Mixed 100/125/200 percent scaling | No double scaling, clipped input, or wrong bounds |
| EC-053 | P1 | Portrait rotation and fractional scale | Recompute transforms and grid |
| EC-054 | P1 | Duplicate EDID/display identity | Ask local remap instead of silently selecting wrong screen |
| EC-055 | P0 | Different GPU per output | Separate compatible devices; no invalid shared resources |
| EC-056 | P1 | Mirrored outputs | Avoid duplicate audio/decoder work where semantics allow |
| EC-057 | P1 | 4K span with missing monitor gap | Explicit virtual-scene fitting, no unbounded texture |
| EC-058 | P1 | Low battery threshold oscillation | Hysteresis preserves stable power profile |
| EC-059 | P0 | Manual pause plus fullscreen then fullscreen ends | Manual pause remains active |
| EC-060 | P0 | Global memory budget across many sources | Enforce aggregate cap, not only per-worker cap |

## Archives and packages

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-061 | P0 | Traversal/absolute/drive/UNC entry | Reject before extraction |
| EC-062 | P0 | Symlink/junction/hardlink/special entry | Reject or prevent safe-root escape |
| EC-063 | P0 | Case-equivalent duplicate or reserved Windows name | Reject portable path ambiguity |
| EC-064 | P0 | Encrypted/multipart/unsupported ZIP method | Reject with clear format error |
| EC-065 | P0 | Duplicate root manifest or duplicate JSON keys | Reject ambiguity |
| EC-066 | P0 | Undeclared asset or hash mismatch | Reject and delete private staging |
| EC-067 | P0 | ZIP bomb or lying size headers | Count actual output and stop at cap |
| EC-068 | P1 | Missing license/font/source asset | Reject invalid portable package |
| EC-069 | P0 | Same ID/version different hash | Conflict, never silent replace |
| EC-070 | P0 | Cancel/crash/disk full during import | Existing packages/assignments unchanged |

## Scenes and clips

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-071 | P0 | Graph cycle or missing node | Reject semantic graph |
| EC-072 | P0 | Duplicate destination slot/wrong arity | Reject ambiguous execution |
| EC-073 | P0 | Unreachable hidden expensive nodes | Reject graph before execution |
| EC-074 | P0 | Many small nodes exceed aggregate budget | Reject or require cheaper preset before allocation |
| EC-075 | P1 | Observer node missing capability declaration | Reject package semantics |
| EC-076 | P0 | Unknown node/shader/script payload | Reject closed stable-v1 format |
| EC-077 | P0 | Clip magic/version/reserved flags invalid | Reject before reading payload |
| EC-078 | P0 | Clip offset/length arithmetic overflow | Checked validation rejects |
| EC-079 | P0 | Clip glyph ID out of range | Reject frame; never index atlas unchecked |
| EC-080 | P0 | Compressed clip expands past declared cap | Stop decompression, reject frame |

## Settings and transactions

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-081 | P0 | NaN/Infinity or inverted thresholds | Reject finite/cross-field invalid state |
| EC-082 | P1 | Unknown setting typo | Reject or suggest correction without silent reinterpretation |
| EC-083 | P0 | Package attempts to grant permission/network | Reject nonportable key |
| EC-084 | P1 | Rapid slider changes and delayed jobs | Newest generation wins |
| EC-085 | P0 | Apply new source fails to decode | Old wallpaper remains valid |
| EC-086 | P0 | Many Apply requests mid-transition | At most bounded states retained, cancel obsolete jobs |
| EC-087 | P1 | CLI/UI stale revision | Conflict rather than lost update |
| EC-088 | P1 | Preset inheritance cycle/depth overflow | Reject before settings resolution |
| EC-089 | P0 | Config migration interrupted or disk full | Atomic old config and backup survive |
| EC-090 | P0 | Factory reset/cache clear | Never delete referenced user originals |

## Catalog and distribution

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-091 | P0 | Network entirely denied | All core local features continue working |
| EC-092 | P0 | Bad signature or untrusted signing key | Do not install remote package |
| EC-093 | P0 | Older sequence or expired catalog | Warn/block fresh remote install per policy; local art stays |
| EC-094 | P0 | Redirect to unapproved/private origin | Reject or require explicit approved origin policy |
| EC-095 | P0 | Captive portal returns HTML for archive | Reject content/hash; never execute |
| EC-096 | P0 | Partial download/resume with changed bytes | Restart or fail; final whole-file hash required |
| EC-097 | P0 | Catalog claims incompatible feature set | Actual package revalidation decides activation |
| EC-098 | P0 | Untrusted PR alters workflow/build script | No production secrets or release privileges |
| EC-099 | P1 | Large asset added to normal Git history | Contribution policy redirects to pinned artifact storage |
| EC-100 | P1 | License attribution unclear or missing | Block curated publication pending review |

## CLI and diagnostics

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-101 | P1 | Binary stdout combined with JSON | Usage error rather than corrupt output |
| EC-102 | P0 | ANSI escapes inside metadata/name | Sanitize terminal output |
| EC-103 | P0 | Wrong user/session IPC peer | Reject before command processing |
| EC-104 | P0 | Message length/nesting flood | Bound and disconnect safely |
| EC-105 | P0 | Stale endpoint or multiple instances | Validate ownership; one controller |
| EC-106 | P1 | Ctrl+C during export | Cancel cleanly; keep previous target intact |
| EC-107 | P1 | Batch contains good and bad files | Per-item outcomes and nonzero aggregate status |
| EC-108 | P0 | Diagnostics includes sensitive local path | Redact by default; preview explicit export |
| EC-109 | P0 | Controller dies while host continues | Host notices owner loss and exits/falls back |
| EC-110 | P0 | Media worker sends invalid shared buffer | Consumer validates dimensions/extent/generation |

## UX and accessibility

| ID | Priority | Condition | Required outcome |
|---|---|---|---|
| EC-111 | P1 | Keyboard-only first run and stop | All essential flows work without pointer |
| EC-112 | P1 | Screen reader on advanced control | Name/role/value/bounds announced |
| EC-113 | P1 | Reduced motion enabled mid-animation | Switch to safe reduced behavior immediately |
| EC-114 | P1 | Long translated labels/RTL UI | No clipped mandatory controls or wrong order |
| EC-115 | P1 | UI scale large on small monitor | Scrollable usable layout, visible focus |
| EC-116 | P1 | Many animated gallery entries | Static thumbnails by default, bounded preview work |
| EC-117 | P1 | Unsupported optional reaction | Clear requested/effective distinction |
| EC-118 | P1 | Unsaved draft then switch output | Prompt save/discard, never apply to wrong output |
| EC-119 | P0 | Delete managed entry referencing original media | Original remains untouched |
| EC-120 | P0 | Uninstall with active wallpaper/autostart | Clean owned surfaces/startup, explicit data retention choice |

## Execution record fields

For each run record case ID, engine commit/binary hash, fixture hash, OS/session/compositor/GPU, permission state, observed outcome, pass/fail, evidence location, issue, reviewer, and date. Automatable tests belong in CI; native-shell and accessibility tests retain a manual script and reproducible setup.

Use the [acceptance criteria](acceptance-criteria.md) to decide release readiness. A documented limitation cannot waive archive escape, unauthorized observation, destructive file loss, or a desktop that cannot be recovered.
