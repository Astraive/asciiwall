# Requirements traceability matrix

## Coverage map

Each requirement is a proposed product obligation, not a completed feature. The linked specification contains behavior and constraints; task IDs map to the implementation backlog; edge-case IDs map to the planned failure matrix. Ranges indicate related scenarios, not claims of executed tests.

| ID | Requirement | Specification | Work units | Failure coverage |
|---|---|---|---|---|
| R01 | Original Rust ASCII conversion | [Spec](../engine/conversion-pipeline.md) | E02 E03 E04 | EC-001-010 |
| R02 | Procedural scenes without source media | [Spec](../engine/procedural-scenes.md) | E07 | EC-071-076 |
| R03 | Images, alpha, color, crop and orientation | [Spec](../engine/color-and-image-processing.md) | E03 E05 | EC-002-010 |
| R04 | Live video with VFR, seek and loops | [Spec](../engine/video-timing-and-decoding.md) | E08 E09 | EC-021-030 |
| R05 | Precomputed editable character animation | [Spec](../engine/ascii-clip-format.md) | E10 | EC-077-080 |
| R06 | Baked video mode with honest editing limits | [Spec](../product/mode-comparison.md) | E08 E11 | EC-021-030 |
| R07 | GPU glyph rendering and bounded fallback | [Spec](../engine/gpu-rendering.md) | E06 | EC-011-020 |
| R08 | Rich settings and effective-value explanations | [Spec](../settings/settings-reference.md) | P01 D12 | EC-081-090 |
| R09 | Cursor and click reactions without stealing input | [Spec](../interaction/pointer-and-click-reactions.md) | D08 | EC-031-036 |
| R10 | Windows-above-wallpaper awareness | [Spec](../interaction/window-aware-behavior.md) | D09 | EC-037-040 |
| R11 | Refresh and source reload semantics | [Spec](../interaction/refresh-and-live-editing.md) | D10 D11 | EC-041-050 EC-084 |
| R12 | Entrance, exit and change animations | [Spec](../interaction/transitions-and-motion.md) | D10 | EC-020 EC-085-086 EC-113 |
| R13 | Local rules and optional reactive signals | [Spec](../interaction/automation-and-local-signals.md) | E07 E09 P01 | EC-027 EC-059 |
| R14 | Windows native desktop integration | [Spec](../platform/windows.md) | D01 D03 | EC-041-047 |
| R15 | macOS native desktop integration | [Spec](../platform/macos.md) | D04 | EC-042 EC-044-046 EC-052 |
| R16 | Linux X11 and layer-shell distinctions | [Spec](../platform/linux-wayland.md) | D05 D06 | EC-031 EC-040 EC-049 |
| R17 | Plasma and GNOME bridge boundaries | [Spec](../platform/linux-kde-plasma.md) | D07 | EC-048 EC-049 EC-109 |
| R18 | Multi-output, DPI, rotation and hotplug | [Spec](../platform/multi-monitor-and-scaling.md) | D11 | EC-051-057 |
| R19 | Power, fullscreen, lock and session policy | [Spec](../platform/power-lock-and-session.md) | E09 D09 D11 | EC-045-047 EC-058-060 |
| R20 | Permissions and private input/window data | [Spec](../platform/permissions-and-privacy.md) | D02 D08 D09 | EC-044-046 EC-083 EC-108 |
| R21 | Offline operation, no server/account | [Spec](../security/privacy-and-network-policy.md) | H02 P06 | EC-030 EC-091 |
| R22 | Safe .asciiwall import and export | [Spec](../package/format-specification.md) | P02 P03 P04 | EC-061-080 |
| R23 | Manifest/scene schemas and compatibility | [Spec](../package/manifest-schema.md) | P02 | EC-065 EC-068 EC-071-076 |
| R24 | Portable variants and immutable identity | [Spec](../package/export-and-reproducibility.md) | P04 P05 | EC-069 EC-089-090 |
| R25 | Second artwork repository | [Spec](../library/repository-structure.md) | P07 | EC-098-100 |
| R26 | Static trusted catalog and explicit downloads | [Spec](../library/catalog-and-index-schema.md) | P06 P07 | EC-091-098 |
| R27 | Gallery curation, licenses and starter content | [Spec](../library/starter-wallpaper-catalog.md) | P07 | EC-100 |
| R28 | Native editor, preview and undo | [Spec](../ui/editor-and-preview.md) | D12 | EC-084-087 EC-118 |
| R29 | Accessibility and localization | [Spec](../ui/accessibility-and-localization.md) | H04 | EC-111-117 |
| R30 | CLI batch conversion and local automation | [Spec](../cli/command-reference.md) | P08 | EC-101-107 |
| R31 | Authenticated bounded IPC | [Spec](../architecture/process-model-and-ipc.md) | H01 | EC-103-110 |
| R32 | Untrusted media/archive isolation | [Spec](../security/untrusted-media-and-packages.md) | H02 P03 H07 | EC-010 EC-030 EC-061-080 |
| R33 | No executable community packages | [Spec](../security/plugin-and-shader-policy.md) | P02 H02 | EC-076 EC-083 |
| R34 | Performance, power and measured quality | [Spec](../quality/benchmark-plan.md) | E12 H03 | EC-019 EC-028 EC-060 EC-116 |
| R35 | Clean release, signing and removal | [Spec](../delivery/packaging-and-signing.md) | H06 H08 | EC-098 EC-120 |
| R36 | Diagnostics, recovery and test coverage | [Spec](../quality/edge-case-matrix.md) | H05 H07 H08 | EC-001-120 |

## User-requested features accounted for

Cursor/click reactions are R09; windows above the wallpaper R10; refresh R11; in/out animation R12; extensive customization R08; image/video conversion and both playback approaches R01-R07; no backend R21; `.asciiwall` import/export R22-R24; separate community repo R25-R27; platform/architecture/stack/CLI and minute failure conditions are covered across the linked implementation and quality documents.

## Coverage is not universal completeness

No finite document can enumerate every future OS bug, codec vulnerability, hardware quirk, or creator idea. New findings become explicit requirements/tests rather than being hidden behind a claim that every imaginable condition was solved. The current pack gives a concrete broad baseline and an update process.
