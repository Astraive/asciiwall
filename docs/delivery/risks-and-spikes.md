# Risk register and proof-of-concept gates

## Risk-based work, not optimistic assumptions

| Risk | Impact | Required spike | Pass evidence / fallback |
|---|---|---|---|
| Windows shell hierarchy changes | Wallpaper above icons or invisible | Minimal attach/detach on named current builds | Survives Explorer/Win+D; otherwise preview/static only |
| macOS Spaces/Stage Manager behavior | Wrong desktop/fullscreen layering | AppKit-only host across spaces/screens | Nonactivating below icons; restrict supported configurations if not |
| Wayland lacks required protocol | No host or reactions | Capability discovery on selected compositors | Explicit limited mode; no privileged workaround |
| Qt/GNOME presentation copies too expensive | Shell stalls, power drain | Offscreen versus glyph-buffer bridge comparison | Measured responsive shell; keep experimental if budgets fail |
| FFmpeg wrapper/native ABI mismatch | Build/runtime crash | Minimal decode/seek/drain on each target | Locked working family; replace wrapper behind abstraction |
| Codec/parser attack | File/privacy compromise | Restricted worker denial tests and fuzzing | No unrelated file/network access; otherwise trusted-input-only build |
| Structural matcher too slow | Low FPS or battery drain | Same-quality CPU profile with shortlist sizes | Adjust grid/candidates; GPU compute only after evidence |
| Temporal stabilization smears motion | Poor perceived quality | Fast motion/hard-cut comparative fixtures | Tunable penalty and reliable history reset |
| Multi-monitor/GPU lifetime errors | Crash/leaks | Hotplug/scale/GPU-loss stress | No stale leases or leaked resources |
| Exact window geometry unavailable | Misleading interaction feature | Per-adapter metadata matrix | Disable geometry effects; retain other modes |
| Export portability fails | Shared package missing assets/font | Clean-machine network-denied round trip | All assets declared; fallback or block export |
| Library signing/CI compromise | Malicious trusted distribution | PR privilege review and key-rotation rehearsal | Separate signing, immutable artifacts, verified catalogs |
| License uncertainty | Distribution/legal exposure | Asset/native dependency ledger review | Exclude uncertain assets/build components |
| UI complexity overwhelms users | Unusable settings surface | First-run/editor usability tests | Basic presets plus searchable advanced controls |
| Tray unavailable | User cannot stop wallpaper | Tray-less desktop test | Relaunch and CLI controls always work |
| Process isolation overhead | Latency/memory copies | Shared-buffer and multi-output benchmark | Coherent batching/sharing without weakening boundaries |

## Spike discipline

A spike has one question, a minimal disposable experiment, the exact environment, observed result, and a go/no-go decision. It is not production implementation and should not be merged wholesale without tests/security review. Preserve findings and small reproducible fixtures instead of a pile of unexplained hacks.

Prioritize Windows attachment, dependency/codec compilation, worker restrictions, and shell-host bridge feasibility before building a large gallery/editor. These can invalidate assumptions that ordinary preview demos do not exercise.

## Unresolved details intentionally retained

Exact minimum OS versions, chosen FFmpeg binding/build, final Rust MSRV, Qt/GNOME presenter path, hardware decode benefit, sandbox primitives per distribution, and trusted catalog key-management process require implementation evidence. The documentation recommends a direction but does not invent successful results.

## Scope control

Custom executable plugins, arbitrary shaders, lock-screen wallpaper, microphone/system-audio capture, per-pixel app-window masks, universal Wayland parity, and automatic binary updates are not stable-v1 prerequisites. Each is a separate risk decision with its own user value and security cost.

A high-quality small release is preferable to enabling fragile hidden features to satisfy a checklist. The full blueprint remains useful as a roadmap while support claims stay narrow and truthful.
