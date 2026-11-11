# Research source register

**Research snapshot:** 2026-11-11. These are primary vendor/project/specification sources used for the design. API availability is evidence of an interface, not proof that the proposed AsciiWall integration has been built or tested. Project issues document individual observations, not prevalence statistics. Published dependency metadata is a snapshot, not a verified working lockfile.

Inline source labels throughout the pack link to the entries below. Recommendations, proposed defaults, architecture choices, arithmetic, schemas, and test plans are the design's own synthesis. Short factual source summaries are not copied documentation. Consult the original references for current API details before implementation.

## S01

**Microsoft: IDesktopWallpaper**

Reference: <https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-idesktopwallpaper>

Source type: Official API reference.

Used for: Documents image/slideshow wallpaper management, not a general-purpose live GPU wallpaper surface.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S02

**Microsoft: SetParent**

Reference: <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setparent>

Source type: Official API reference.

Used for: Window parenting has style and DPI-awareness constraints. It does not itself establish a supported wallpaper contract.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S03

**Lively: raised desktop compatibility discussion**

Reference: <https://github.com/lively-community/lively/issues/2074>

Source type: Primary project issue; implementation evidence, not an OS guarantee.

Used for: Records changed Progman, WorkerW, and DefView behavior. A maintainer-reported Microsoft explanation is not a public stable API.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S04

**Microsoft: SetWinEventHook**

Reference: <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook>

Source type: Official API reference.

Used for: Out-of-context window event observation requires a message loop and careful callback handling.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S05

**Microsoft: GetWindowRect**

Reference: <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowrect>

Source type: Official API reference.

Used for: GetWindowRect is DPI-virtualized and can include invisible resize borders; visible DWM bounds differ.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S06

**Microsoft: DwmGetWindowAttribute**

Reference: <https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmgetwindowattribute>

Source type: Official API reference.

Used for: Queries compositor attributes; use validated attributes and treat failed queries as unknown.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S07

**Microsoft: Raw Input Overview**

Reference: <https://learn.microsoft.com/en-us/windows/win32/inputdev/about-raw-input>

Source type: Official API reference.

Used for: RIDEV_INPUTSINK can provide background raw input. Raw device motion is not a desktop-coordinate cursor stream.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S08

**Microsoft: RAWINPUTDEVICE**

Reference: <https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputdevice>

Source type: Official API reference.

Used for: Defines mouse registration flags; avoid capture and legacy-suppression flags for passive wallpaper observation.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S09

**Apple: NSWindow collection behaviors**

Reference: <https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct>

Source type: Official API reference.

Used for: Spaces, Mission Control, fullscreen, and Stage Manager have distinct behaviors; some flags are mutually exclusive.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S10

**Apple: CGWindowLevelKey**

Reference: <https://developer.apple.com/documentation/coregraphics/cgwindowlevelkey>

Source type: Official API reference.

Used for: Exposes desktop-related window levels; exact wallpaper layering still requires a native integration experiment.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S11

**Apple: Monitoring Events**

Reference: <https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/MonitoringEvents/MonitoringEvents.html>

Source type: Official archived guide.

Used for: Local and global event monitors have different scopes. Global monitoring does not replace or suppress observed events.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S12

**Apple: Advances in macOS Security, WWDC19**

Reference: <https://developer.apple.com/videos/play/wwdc2019/701/>

Source type: Official historical presentation.

Used for: Distinguishes protected window contents/names from some available window metadata. Current OS behavior still needs permission-matrix testing.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S13

**Apple: CGWindowListCopyWindowInfo**

Reference: <https://developer.apple.com/documentation/coregraphics/cgwindowlistcopywindowinfo(_:_:)>

Source type: Official API reference.

Used for: Window metadata enumeration is distinct from pixel capture; do not assume all metadata is always available.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S14

**Apple: Set desktop image**

Reference: <https://developer.apple.com/documentation/appkit/nsworkspace/setdesktopimageurl(_:for:options:)>

Source type: Official API reference.

Used for: Provides a per-screen image-wallpaper fallback; must be called on the main thread.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S15

**Wayland: protocol and model of operation**

Reference: <https://wayland.freedesktop.org/docs/book/Protocol.html>

Source type: Official protocol documentation.

Used for: Ordinary clients do not know global surface positions or access other clients surfaces. Pointer coordinates are surface-local.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S16

**wlr layer-shell protocol**

Reference: <https://wayland.app/protocols/wlr-layer-shell-unstable-v1>

Source type: Rendered upstream protocol specification.

Used for: Background layers and input regions are distinct. Empty input regions mean no normal pointer delivery to the wallpaper surface.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S17

**ext foreign toplevel list protocol**

Reference: <https://wayland.app/protocols/ext-foreign-toplevel-list-v1>

Source type: Rendered upstream protocol specification.

Used for: Toplevel listing does not supply a universal stream of global window rectangles.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S18

**wlr foreign toplevel management protocol**

Reference: <https://wayland.app/protocols/wlr-foreign-toplevel-management-unstable-v1>

Source type: Rendered upstream protocol specification.

Used for: Supports toplevel metadata/state/output association; do not confuse this with unrestricted global geometry.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S19

**freedesktop: Extended Window Manager Hints**

Reference: <https://specifications.freedesktop.org/wm/latest-single/>

Source type: Primary specification.

Used for: Defines desktop window hints and stacking lists. Window managers can differ in how they implement hints.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S20

**KDE: Plasma themes and plugins**

Reference: <https://develop.kde.org/docs/plasma/>

Source type: Official developer documentation.

Used for: Plasma has QML wallpaper plugins; this is a native integration surface rather than a universal Wayland mechanism.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S21

**KDE: Plasma scripting API**

Reference: <https://develop.kde.org/docs/plasma/scripting/api/>

Source type: Official developer documentation.

Used for: Desktops, screens, activities, and wallpaper plugin configuration have their own host-managed model.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S22

**GNOME: extension architecture**

Reference: <https://gjs.guide/extensions/overview/architecture.html>

Source type: Official community developer documentation.

Used for: GNOME Shell extensions use GJS and can access Shell/Mutter concepts; their code runs in a sensitive desktop process.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S23

**Smithay: layer-shell module**

Reference: <https://smithay.github.io/client-toolkit/smithay_client_toolkit/shell/wlr_layer/index.html>

Source type: Maintainer API documentation.

Used for: Provides Rust client-side layer-shell building blocks, not compositor support where the protocol is absent.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S24

**wgpu: backend documentation**

Reference: <https://wgpu.rs/doc/wgpu/struct.Backends.html>

Source type: Maintainer API documentation.

Used for: Cross-platform backend abstractions exist; actual adapter features and limits must be queried.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S25

**egui-wgpu published dependency metadata**

Reference: <https://docs.rs/crate/egui-wgpu/0.37.0>

Source type: Published crate metadata.

Used for: Research snapshot: egui-wgpu 0.37.0 declares wgpu ^30.0 and optional winit ^0.30.13. This is not a tested AsciiWall dependency lock.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S26

**fontdue: Font**

Reference: <https://docs.rs/fontdue/latest/fontdue/struct.Font.html>

Source type: Maintainer API documentation.

Used for: Provides glyph rasterization; full text shaping is a separate concern.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S27

**image: crate documentation**

Reference: <https://docs.rs/image/latest/image/>

Source type: Maintainer API documentation.

Used for: Provides local image codecs and animation-related abstractions; enabled feature flags define actual support.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S28

**image: decoding limits**

Reference: <https://docs.rs/image/latest/image/struct.Limits.html>

Source type: Maintainer API documentation.

Used for: Decoder limits have strict and non-strict support considerations; process budgets remain necessary.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S29

**ffmpeg-the-third: published metadata**

Reference: <https://docs.rs/crate/ffmpeg-the-third/latest>

Source type: Published crate metadata.

Used for: Snapshot showed 6.0.0+ffmpeg-9.0. Its maintenance comparison with other wrappers is the authors characterization, not independent evidence.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S30

**ffmpeg-next: current API documentation**

Reference: <https://docs.rs/ffmpeg-next/latest/ffmpeg_next/>

Source type: Published crate documentation.

Used for: Snapshot showed ffmpeg-next 9.0.0. Do not repeat an unqualified claim that this alternative has no current releases.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S31

**FFmpeg: send/receive API**

Reference: <https://ffmpeg.org/doxygen/trunk/group__lavc__encdec.html>

Source type: Official API documentation.

Used for: Packet/frame relationships are not necessarily one-to-one; correct EAGAIN, draining, and flushing behavior is required.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S32

**FFmpeg: protocol options**

Reference: <https://ffmpeg.org/ffmpeg-protocols.html>

Source type: Official documentation.

Used for: Network and nested protocols exist. Offline operation needs an explicit I/O restriction design.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S33

**FFmpeg: legal and licensing considerations**

Reference: <https://ffmpeg.org/legal.html>

Source type: Official project licensing guidance, not individual legal advice.

Used for: FFmpeg configuration changes licensing obligations. Review the exact distributed build and all bundled dependencies.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S34

**Cargo: workspaces**

Reference: <https://doc.rust-lang.org/cargo/reference/workspaces.html>

Source type: Official Rust documentation.

Used for: Workspace dependency policy, lockfiles, and explicit resolver choices support a shared codebase.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S35

**zip: ZipFile**

Reference: <https://docs.rs/zip/latest/zip/read/struct.ZipFile.html>

Source type: Maintainer API documentation.

Used for: Safe path helpers exist, but package-specific limits, collisions, and special-file policies still need enforcement.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S36

**JSON Schema draft 2020-12**

Reference: <https://json-schema.org/draft/2020-12>

Source type: Primary specification.

Used for: Provides a schema vocabulary; archive safety and semantic compatibility cannot be solved by JSON Schema alone.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S37

**W3C: CSS Color 4**

Reference: <https://www.w3.org/TR/2026/CRD-css-color-4-20260728/>

Source type: Primary specification snapshot.

Used for: Documents color-space conversion including sRGB and linear-light sRGB; use explicit color semantics in the engine.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S38

**Chafa project**

Reference: <https://hpjansson.org/chafa/>

Source type: Primary project documentation.

Used for: Shows existing symbol-based image rendering, multiple palettes and alphabets. Use as a benchmark reference, not the engine implementation.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S39

**Structure-based ASCII Art, author project page**

Reference: <https://ttwong12.github.io/papers/asciiart/asciiart.html>

Source type: Primary research author page, SIGGRAPH 2010.

Used for: Establishes structure-aware character matching as prior work. Our proposed cell matcher is not claimed to reproduce the full paper.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S40

**Lively project**

Reference: <https://github.com/lively-community/lively>

Source type: Primary project repository.

Used for: Live wallpaper precedent. The formerly linked rocksdanister repository redirected here during research.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S41

**GitHub: large files**

Reference: <https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github>

Source type: Official hosting documentation.

Used for: Ordinary Git is unsuitable for a growing collection of large video binaries; use separate binary distribution artifacts.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S42

**GitHub: releases**

Reference: <https://docs.github.com/en/enterprise-cloud@latest/repositories/releasing-projects-on-github/about-releases>

Source type: Official hosting documentation.

Used for: Release assets can distribute packages independently of normal source history. Hosting limits are not a permanent SLA.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S43

**GitHub: safely using pull_request_target**

Reference: <https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target>

Source type: Official security guidance.

Used for: Untrusted pull-request code must not execute with privileged tokens or release secrets.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S44

**The Update Framework: security**

Reference: <https://theupdateframework.io/docs/security/>

Source type: Primary security framework documentation.

Used for: Hashes alone do not address compromised repositories, rollback, or freeze attacks. A signed static catalog is not automatically a full TUF implementation.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S45

**Apple: notarization**

Reference: <https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution>

Source type: Official distribution documentation.

Used for: Developer ID distribution has signing, hardened runtime, and notarization requirements; sign bundled helpers as well.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S46

**Flatpak: sandbox permissions**

Reference: <https://flatpak-docs.readthedocs.io/en/latest/sandbox-permissions.html>

Source type: Official project documentation.

Used for: Sandboxed applications have restricted host access. Packaging does not automatically grant wallpaper-shell integration.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S47

**freedesktop: autostart specification**

Reference: <https://specifications.freedesktop.org/autostart/latest/>

Source type: Primary specification.

Used for: Defines per-user desktop autostart; system services are not required for a user wallpaper application.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S48

**Apple: reduced motion**

Reference: <https://developer.apple.com/documentation/appkit/nsworkspace/accessibilitydisplayshouldreducemotion>

Source type: Official API reference.

Used for: A platform setting exists for reduced motion, with change notifications.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S49

**egui: accessibility**

Reference: <https://github.com/emilk/egui/blob/main/docs/accessibility.md>

Source type: Primary project documentation.

Used for: AccessKit integration is available; accessibility still requires real assistive-technology testing.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S50

**Lively: wallpaper-over-icons report**

Reference: <https://github.com/lively-community/lively/issues/3193>

Source type: Primary project issue; observed problem, not prevalence evidence.

Used for: A 2026 report illustrates that desktop-layer regressions remain a practical integration risk.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S51

**tray-icon: platform requirements**

Reference: <https://docs.rs/tray-icon/latest/tray_icon/>

Source type: Maintainer API documentation.

Used for: Tray implementations have event-loop and Linux backend requirements. The presence of a crate does not guarantee a visible tray on every desktop.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S52

**objc2: MainThreadMarker**

Reference: <https://docs.rs/objc2/latest/objc2/struct.MainThreadMarker.html>

Source type: Maintainer API documentation.

Used for: Models main-thread-only native API usage; keep AppKit ownership explicit.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S53

**SIL: using OFL fonts**

Reference: <https://openfontlicense.org/how-to-use-ofl-fonts>

Source type: License steward guidance.

Used for: Font redistribution, license retention, and reserved-name rules must be checked separately from artwork licensing.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S54

**Creative Commons: Attribution 4.0 legal code**

Reference: <https://creativecommons.org/licenses/by/4.0/legalcode.en>

Source type: Primary license text.

Used for: Attribution and indication of changes are relevant to shared artwork. Do not assume converting an image removes source obligations.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S55

**Creative Commons: Attribution-ShareAlike 4.0**

Reference: <https://creativecommons.org/licenses/by-sa/4.0/>

Source type: License steward summary.

Used for: Adaptations can have share-alike conditions; keep asset licenses independent from engine source-code licensing.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S56

**Qt: Quick scene graph**

Reference: <https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph.html>

Source type: Official Qt documentation.

Used for: Qt owns its scene graph and render-thread lifecycle. Integrating an external engine needs an explicit presentation bridge, not an assumed wgpu surface.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S57

**GNOME: St.ImageContent.set_data**

Reference: <https://gnome.pages.gitlab.gnome.org/gnome-shell/st/method.ImageContent.set_data.html>

Source type: Generated primary API documentation.

Used for: Accepts image pixels and copies them to texture memory. API presence is not proof that a high-rate wallpaper pipeline will be efficient.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S58

**Microsoft: WM_DISPLAYCHANGE**

Reference: <https://learn.microsoft.com/en-us/windows/win32/gdi/wm-displaychange>

Source type: Official API documentation.

Used for: Notifies resolution changes. Re-enumerate the actual output topology rather than treating message dimensions as all-monitor geometry.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S59

**Microsoft: WM_POWERBROADCAST**

Reference: <https://learn.microsoft.com/en-us/windows/win32/power/wm-powerbroadcast>

Source type: Official API documentation.

Used for: Reports suspend/resume and power changes; wallpaper should respect power/session transitions rather than block sleep.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S60

**ed25519-dalek: strict verification**

Reference: <https://docs.rs/ed25519-dalek/latest/ed25519_dalek/struct.VerifyingKey.html>

Source type: Maintainer API documentation.

Used for: Strict verification and weak-key checks are available. Signature verification still requires a separately trusted key and a defined signed byte sequence.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S61

**GitHub: build artifact attestations**

Reference: <https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations>

Source type: Official hosting documentation.

Used for: Build provenance attestations describe how an artifact was built; they are not proof that content or dependencies are harmless.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.

## S62

**RustSec: cargo-audit**

Reference: <https://github.com/rustsec/rustsec/blob/main/cargo-audit/README.md>

Source type: Primary tool documentation.

Used for: Checks dependency versions against reported advisories. A passing scan does not establish absence of vulnerabilities.

Snapshot checked: 2026-11-11. A `latest` URL may resolve to different content after this date.
