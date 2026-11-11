# Platform support policy and tuple status

> **Current platform status:** **All native platforms are currently unimplemented and untested (`Experimental / Unimplemented`).** Zero native wallpaper integration tests, permission probes, shell-bridge benchmarks, or clean-machine installer drills have been run ([documentation verification](quality/documentation-verification.md)).

## Support is a measured tuple, not an OS logo

Under [platform support policy](platform/support-policy.md), AsciiWall never publishes blanket operating-system support promises. A **Supported** claim applies solely to an explicitly tested tuple:

`(OS + build, session type, desktop/compositor version, GPU backend, packaging format, enabled capability set)`

Key distinctions enforced across architecture and UI:
- A working normal preview window does **not** prove desktop wallpaper placement (`preview-only` is a valid distinct tier).
- A working wallpaper surface does **not** imply global pointer tracking, passive desktop clicks, or window rectangle geometry.
- Unsupported capabilities stay visible and explain their fallback in the UI (`requested` vs `effective` settings), rather than silently showing no-op toggles or escalating privileges.

| Tier | Policy meaning |
|---|---|
| `Supported` | Named tuple passes install, below-icons placement, input preservation, suspend/lock/hotplug recovery, and clean removal |
| `Experimental` | Partial/prototype implementation with known gaps; prominently labelled with a safe Stop/fallback path |
| `Preview/export only` | Core engine/CLI/preview works without a desktop wallpaper host |
| `Unsupported` | No safe host integration or incompatible ABI; stops cleanly with exact explanation rather than retry loops |

## Independent native host status

Development order begins with **Windows first (`M3`/`M5`/`M8`)**, followed by independently gated **macOS**, **X11**, **Wayland `layer-shell`**, **KDE Plasma**, and **GNOME Shell** adapters (`M7`/`M8`):

| Target | Planned milestones & tickets | Current status | Core integration mechanism and hard boundaries |
|---|---|---|---|
| **Windows** (first product target) | `M0` (`WND-001`, `SEC-002`), `M3` (`WND-002`), `M5` (`WND-003`), `M8` (`REL-003`) | Unimplemented / Experimental spike pending | Explorer `Progman`/`WorkerW`/`DefView` placement is shell-sensitive, **not** a stable live-wallpaper API ([S01](research/source-register.md#s01)–[S03](research/source-register.md#s03), [S50](research/source-register.md#s50)). `SetParent` alone is insufficient; no Explorer code injection, memory patching, foreign subclassing, admin rights, click capture/replay, or topmost fullscreen fallback ([Windows spec](platform/windows.md)). |
| **macOS** | `M0` (`SEC-003`), `M7` (`MAC-001`, `MAC-002`), `M8` (`REL-004`) | Unimplemented / Experimental | AppKit borderless desktop-level windows owned on the main thread (`objc2::MainThreadMarker`, [S09](research/source-register.md#s09)–[S14](research/source-register.md#s14), [S52](research/source-register.md#s52)). Spaces, Mission Control, and Stage Manager behavior unverified; passive mouse/window metadata are API/version/permission dependent; wallpaper rendering never requires Accessibility or Screen Recording; signed + hardened runtime + notarized ([macOS spec](platform/macos.md)). |
| **Linux X11** | `M0` (`SEC-004`), `M7` (`LNX-001`, `LNX-002`), `M8` (`REL-005`) | Unimplemented / Experimental | Non-activating EWMH `_NET_WM_WINDOW_TYPE_DESKTOP` host via `x11rb` ([S19](research/source-register.md#s19)). Desktop icon managers vary in stacking; requires named `(distro, DE/WM, icon-manager)` tuples; no pointer grabs, no `/dev/input`, no root access ([X11 spec](platform/linux-x11.md)). |
| **Linux Wayland (`layer-shell`)** | `M0` (`SEC-004`), `M7` (`LNX-003`), `M8` (`REL-005`) | Unimplemented / Experimental | Uses `wlr-layer-shell` only when advertised by the compositor ([S15](research/source-register.md#s15)–[S18](research/source-register.md#s18), [S23](research/source-register.md#s23)). Empty input regions allow click-through without yielding global pointer/click streams; foreign-toplevel protocols are not universal window geometry feeds; **no** silent screen-share/portal/root workarounds. Plain limited mode (no cursor/window reactions) is a valid fallback ([Wayland spec](platform/linux-wayland.md)). |
| **KDE Plasma** | `M7` (`LNX-004`–`006`), `M8` (`REL-005`) | Unimplemented / Experimental | QML wallpaper plugin where Qt Quick owns the scene graph ([S20](research/source-register.md#s20), [S21](research/source-register.md#s21), [S56](research/source-register.md#s56)). Does **not** assume an embeddable `wgpu` surface; `LNX-005` must compare offscreen RGBA transfer vs compact glyph-grid presentation and KWin bridge responsiveness; activity-aware, zero decoders inside Plasma ([Plasma spec](platform/linux-kde-plasma.md)). |
| **GNOME Shell** | `M7` (`LNX-004`, `LNX-007`, `LNX-008`), `M8` (`REL-005`) | Unimplemented / Experimental | GJS background actor extension ([S22](research/source-register.md#s22), [S57](research/source-register.md#s57)). Base64 4K RGBA over D-Bus/JSON is unviable (~995 MB/s raw transfer at 30 FPS); requires bounded shared-memory/actor spike (`LNX-007`), idempotent `enable`/`disable`, version allowlist, and zero media/package parsing inside Shell ([GNOME spec](platform/linux-gnome.md)). |

## Independent capability model

`WPH-001` separates host reporting into independent capabilities (`available`, `permission-required`, `unavailable`, `experimental`, `temporarily-unavailable`):
- `wallpaper-surface`, `pointer-position`, `pointer-buttons`, `window-rectangles`, `window-state`, `lock-state`, `battery-state`, `per-output-scale`, `static-wallpaper-setting`, `tray-availability`.
- Any missing observer disables only the affected reaction with a clear diagnostic reason while keeping static, procedural, image, and video playback usable.

## Preserved privacy, offline, and declarative guarantees

Across every platform and future host tuple ([permissions and privacy](platform/permissions-and-privacy.md), [power/lock policy](platform/power-lock-and-session.md), [multi-monitor scaling](platform/multi-monitor-and-scaling.md)):
- **Offline core:** Local playback, conversion, presets, and package import/export require zero network access; `library.online-catalog` defaults to `"disabled"`.
- **Consent and data minimization:** `privacy.pointer-observation`, `privacy.button-observation`, and `privacy.window-observation` default to `false`; interaction defaults to `"desktop-only"` hit-policy verification; zero screenshots, keystroke logging, or window titles are collected.
- **Session & lock safety:** `power.pause-on-lock` is enforced `true` in v1; rendering and observers pause on lock, UAC/secure-desktop, and sleep without preventing system sleep ([S59](research/source-register.md#s59)).
- **Declarative packages:** `.asciiwall` v1 packages contain data-only scenes/assets and portable visual defaults (`P`); they can **never** grant permissions, install shell bridges, or execute code.

## Promotion gates

No host moves from `Experimental` to `Supported` until:
1. Its named-tuple acceptance matrix passes (`WND-002` / `QA-M3` for Windows; `WPH-005` / `QA-M7` for macOS & Linux adapters; `QA-003` / `QA-M8` for beta certification).
2. Worker confinement (`SEC-002`–`SEC-004`) and clean distribution/uninstall (`REL-003`–`REL-006`) are verified on clean target environments rather than inferred from compilation or virtual-display previews.
