# Requirements traceability summary

> **Planning traceability, not completion:** The **2,915** requirement mappings summarized here connect the 100 original specification documents ([requirements matrix](product/requirements-traceability.md)), 40 work units, 172 settings, 120 edge cases, and 36 starter recipes to **356** local `Backlog` tickets (`0` published remotely). Line-by-line requirement tables live externally in `../.issues/reports/backlog-requirement-traceability.md` and `../.issues/coverage/*.json`.

## Coverage slice summary (2,915 mappings)

Every one of the **100** original specification documents has verified read attestations and requirement mappings in `../.issues/coverage/*.json` (`0` validation errors in `../.issues/reports/backlog-audit.json`):

| Coverage slice file | Source docs read | Requirement IDs | Mapped count | Primary focus |
|---|---:|---|---:|---|
| `foundation.json` | `29` | `FOUNDATION-REQ-001`–`411` | `411` | Workspace, core contracts, ADRs, tech stack, CLI |
| `conversion.json` | `11` | `CONVERSION-REQ-001`–`225` | `225` | Image decode, color/alpha, font masks, density/structural matching, video/clips/export |
| `rendering.json` | `11` | `RENDERING-REQ-001`–`276` | `276` | `wgpu` atlas presenter, v1 procedural scenes, effects graph, transitions |
| `platform.json` | `15` | `PLATFORM-REQ-001`–`089` | `89` | Windows, macOS, X11, Wayland, Plasma, GNOME, multi-monitor, power/privacy |
| `settings-ui.json` | `10` | `SETTINGS-UI-REQ-001`–`326` | `326` | 172 settings, precedence/presets, migrations, `egui` editor & accessibility |
| `package.json` | `15` | `PACKAGE-REQ-001`–`182` | `182` | Draft v1 manifest/scene schemas, safe staged import, export, catalog client |
| `library.json` | `11` | `LIBRARY-REQ-001`–`473` | `473` | `asciiwall-library` layout, catalog/signatures, licensing, 36 recipe briefs |
| `security-qa-delivery.json` | `23` | `SQD-REQ-001`–`933` | `933` | Threat model, confinement, supply chain, 120 edge cases, 16 milestone gates |
| **Total (deduplicated docs: 100)** | **100 unique** | **8 prefixes** | **2,915** | **100% original specification pack covered** |

## High-level requirements R01–R36 crosswalk

Crosswalk from [product requirements traceability](product/requirements-traceability.md) to local epics, owning ticket groups, [implementation work units](delivery/implementation-backlog.md), and [edge-case matrix](quality/edge-case-matrix.md) coverage:

| Req ID | Product obligation | Work units | Owning epic(s) & key tasks | Failure coverage |
|---|---|---|---|---|
| `R01` | Original Rust ASCII conversion | `E02`, `E03`, `E04` | `FNT-EPIC`, `CNV-EPIC` (`CNV-001`–`012`) | `EC-001`–`010` |
| `R02` | Procedural scenes without source media | `E07` | `SCN-EPIC` (`SCN-001`–`011`) | `EC-071`–`076` |
| `R03` | Images, alpha, color, crop, orientation | `E03`, `E05` | `IMG-EPIC` (`IMG-001`–`004`), `CNV-002`–`005` | `EC-002`–`010` |
| `R04` | Live video with VFR, seek, and loops | `E08`, `E09` | `VID-EPIC` (`VID-001`–`006`) | `EC-021`–`030` |
| `R05` | Precomputed editable character animation | `E10` | `VID-EPIC` (`VID-007`–`009`) | `EC-077`–`080` |
| `R06` | Baked video mode with honest editing limits | `E08`, `E11` | `VID-EPIC` (`VID-012`–`014`), `UIX-007` | `EC-021`–`030` |
| `R07` | GPU glyph rendering and bounded fallback | `E06` | `RND-EPIC` (`RND-001`–`005`) | `EC-011`–`020` |
| `R08` | Rich settings and effective-value explanations | `P01`, `D12` | `CFG-EPIC` (`CFG-001`–`016`), `UIX-006` | `EC-081`–`090` |
| `R09` | Cursor and click reactions without stealing input | `D08` | `INT-EPIC` (`INT-001`–`002`), `FX-003`–`004` | `EC-031`–`036` |
| `R10` | Windows-above-wallpaper awareness | `D09` | `WIN-EPIC` (`WIN-001`–`002`), `FX-005` | `EC-037`–`040` |
| `R11` | Refresh and source reload semantics | `D10`, `D11` | `WPH-EPIC` (`WPH-003`–`004`) | `EC-041`–`050`, `084` |
| `R12` | Entrance, exit, and change animations | `D10` | `FX-EPIC` (`FX-013`), `WPH-002` | `EC-020`, `085`–`086`, `113` |
| `R13` | Local rules and optional reactive signals | `E07`, `E09`, `P01` | `INT-EPIC` (`INT-003`–`004`, `006`), `CFG-016` | `EC-027`, `059` |
| `R14` | Windows native desktop integration | `D01`, `D03` | `WND-EPIC` (`WND-001`–`003`) | `EC-041`–`047` |
| `R15` | macOS native desktop integration | `D04` | `MAC-EPIC` (`MAC-001`–`002`) | `EC-042`, `044`–`046`, `052` |
| `R16` | Linux X11 and layer-shell distinctions | `D05`, `D06` | `LNX-EPIC` (`LNX-001`–`003`) | `EC-031`, `040`, `049` |
| `R17` | Plasma and GNOME bridge boundaries | `D07` | `LNX-EPIC` (`LNX-004`–`008`) | `EC-048`–`049`, `109` |
| `R18` | Multi-output, DPI, rotation, hotplug | `D11` | `MON-EPIC` (`MON-001`–`003`) | `EC-051`–`057` |
| `R19` | Power, fullscreen, lock, session policy | `E09`, `D09`, `D11` | `PWR-EPIC` (`PWR-001`–`004`), `WIN-002` | `EC-045`–`047`, `058`–`060` |
| `R20` | Permissions and private input/window data | `D02`, `D08`, `D09` | `INT-005`, `CFG-009`, `WPH-001` | `EC-044`–`046`, `083`, `108` |
| `R21` | Offline operation, no server/account | `H02`, `P06` | `SEC-006`, `LIBAPP-005` | `EC-030`, `091` |
| `R22` | Safe `.asciiwall` import and export | `P02`, `P03`, `P04` | `PKG-EPIC` (`PKG-005`–`010`) | `EC-061`–`080` |
| `R23` | Manifest/scene schemas and compatibility | `P02` | `PKG-EPIC` (`PKG-001`–`004`, `011`–`012`), `FX-001` | `EC-065`, `068`, `071`–`076` |
| `R24` | Portable variants and immutable identity | `P04`, `P05` | `LIBAPP-EPIC` (`LIBAPP-001`–`002`), `PKG-008`–`011`, `UIX-008` | `EC-069`, `089`–`090` |
| `R25` | Second artwork repository (`asciiwall-library`) | `P07` | `LIB-EPIC` (`LIB-001`–`005`), `LCOM-EPIC`, `LLEG-EPIC` | `EC-098`–`100` |
| `R26` | Static trusted catalog and explicit downloads | `P06`, `P07` | `CAT-EPIC` (`CAT-001`–`005`), `LIBAPP-004`–`006`, `LREL-EPIC` | `EC-091`–`098` |
| `R27` | Gallery curation, licenses, starter content | `P07` | `AST-EPIC` (`AST-001`–`038`), `LCOM-002`, `LLEG-002`–`004` | `EC-100` |
| `R28` | Native editor, preview, and undo | `D12` | `UIX-EPIC` (`UIX-001`–`008`) | `EC-084`–`087`, `118` |
| `R29` | Accessibility and localization | `H04` | `ACC-EPIC` (`ACC-001`–`004`) | `EC-111`–`117` |
| `R30` | CLI batch conversion and local automation | `P08` | `CLI-EPIC` (`CLI-001`–`006`) | `EC-101`–`107` |
| `R31` | Authenticated bounded IPC | `H01` | `SEC-001`, `COR-010`, `ARC-003` | `EC-103`–`110` |
| `R32` | Untrusted media/archive isolation | `H02`, `P03`, `H07` | `SEC-002`–`005`, `PKG-005`–`007`, `QA-002` | `EC-010`, `030`, `061`–`080` |
| `R33` | No executable community packages | `P02`, `H02` | `SEC-007`, `PKG-003`, `FX-001` | `EC-076`, `083` |
| `R34` | Performance, power, and measured quality | `E12`, `H03` | `PERF-EPIC` (`PERF-001`–`006`), `QA-006`, `QA-009` | `EC-019`, `028`, `060`, `116` |
| `R35` | Clean release, signing, and removal | `H06`, `H08` | `REL-EPIC` (`REL-001`–`011`), `LEG-001` | `EC-098`, `120` |
| `R36` | Diagnostics, recovery, and test coverage | `H05`, `H07`, `H08` | `QA-EPIC` (`QA-001`–`010`, `101`–`124`, `M0`–`M10`) | `EC-001`–`120` |

## Planning map for 172 proposed settings

All **172** proposed controls in [settings reference](settings/settings-reference.md) (`80` portable visual `P`, `83` application policy `A`, `6` output `O`, `3` session `S`; `164` `v1` + `8` `later`) are mapped to `CFG-001`–`CFG-016` as **planning targets, not implemented features**:

| Backlog ticket | Milestone | Sections owned | Key count | Scope / phase notes |
|---|---|---|---:|---|
| `CFG-001`–`CFG-003` | `M0` / `M5` | Registry, migrations, precedence, presets | *(infra)* | Foundation validation, requested/effective provenance, bounded preset inheritance |
| `CFG-004` | `M5` | `Grid` (`7`), `Font` (`5`), `Charset` (`3`) | `15` | Portable visual (`P`) / `v1` |
| `CFG-005` | `M5` | `Conversion` (`15`), `Color` (`6`) | `21` | Portable visual (`P`) / `v1` |
| `CFG-006` | `M5` | `Playback` (`7`), `Scene` (`5`) | `12` | Portable visual (`P`) / `v1` |
| `CFG-007` | `M5` | `Effects` (`11`), `Transition` (`6`) | `17` | Portable visual (`P`) / `v1` |
| `CFG-008` | `M5` | `Interaction` (`15`) | `15` | Portable visual (`P`) / `v1`; consent remains independent |
| `CFG-009` | `M5` | `Privacy` (`10`) | `10` | App policy (`A`) / `v1`; `privacy.crash-upload` locked `false` in v1 |
| `CFG-010` | `M5` | `Display` (`6`) | `6` | Output (`O`) / `v1` |
| `CFG-011` | `M5` | `Performance` (`10`), `Power` (`7`), `Visibility` (`8`) | `25` | App policy (`A`) / `v1`; `power.pause-on-lock` locked `true` in v1 |
| `CFG-012` | `M5` | `Accessibility` (`5`), `Ui` (`8`) | `13` | App policy (`A`) / `v1` |
| `CFG-013` | `M5` | `Storage` (`8`), `Library` (`5`) | `13` | App policy (`A`) / `v1` |
| `CFG-014` | `M5` | `Startup` (`4`), `Recovery` (`3`), `Diagnostics` (`3`) | `10` | `7` `A` + `3` `S` / `v1` |
| `CFG-015` | `M5` | `Export` (`7`) | `7` | App policy (`A`) / `v1` |
| `CFG-016` | `M10` | `Audio` (`4`), `Signals` (`2`), `Playlist` (`2`) | `8` | App policy (`A`) / `later` (post-1.0 only) |

## Planning map for 36 starter recipes and 120 edge cases

- **36 starter wallpaper recipe briefs ([starter catalog](library/starter-wallpaper-catalog.md)):** Tracked as planning items only—zero generated artwork packages exist yet:
  - **20 `v1` declarative scenes (`AST-001`–`AST-020`, `L2`):** `matrix-rain`, `quiet-waves`, `deep-starfield`, `wireframe-cube`, `mist-field`, `amber-terminal`, `paper-waves`, `blue-hour`, `constellation-drift`, `forest-rain`, `magnetic-stars`, `desktop-ripples`, `window-wake`, `monochrome-orbit`, `solar-haze`, `ocean-lines`, `rain-duet`, `still-night`, `paper-cube`, `slow-tide`.
  - **8 rights-cleared media candidates (`AST-021`–`AST-028`, `L3`):** `mountain-study`, `botanical-study`, `city-outline`, `portrait-study` (`image`), `cloud-loop`, `ocean-loop` (`video`), `ember-loop` (`ascii-clip`), `ink-loop` (`rendered-video`).
  - **8 later-engine candidates (`AST-029`–`AST-036`, `L4`):** `lorenz-attractor`, `boid-garden`, `reaction-diffusion`, `cellular-life`, `ascii-fire`, `wireframe-torus`, `audio-spectrum`, `day-night-field`, gated by `AST-038` readiness review and `AST-037` authoring contracts (`L1`).
- **120 edge-case scenarios (`EC-001`–`EC-120`):** Mapped cleanly in groups of 5 to `QA-101`–`QA-124` (see [quality gates](quality-gates.md#edge-case-matrix-ownership)).

To regenerate `../.issues/reports/backlog-requirement-traceability.md` locally:

```bash
python scripts/validate-backlog.py --write-reports
```
