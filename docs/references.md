# References and external research provenance

> **Epistemic boundary:** Cited specifications, vendor pages, project issues, and reference repositories document **external APIs, design precedents, and observed repository snapshots (checked 2026-11-11)**. API documentation is not proof that AsciiWall's proposed integration has been built or tested.

## Specification pack and infrastructure guides

The `asciiwall/docs/` tree contains the frozen **100 original specification documents** indexed in [documentation index](documentation-index.md) alongside central infrastructure and planning guides:

- **Root & backlog guides:** [readme](readme.md), [start here](start-here.md), [documentation index](documentation-index.md), [backlog validation](backlog-validation.md), [backlog publication](backlog-publication.md), [GitHub setup](github-setup.md), and the 11 central planning entrypoints (`plan.md`, `roadmap.md`, `architecture.md`, `development.md`, `issue-conventions.md`, `dependency-graph.md`, `quality-gates.md`, `requirements-traceability.md`, `platform-support.md`, `release-process.md`, and this file).
- **100 original design specifications (17 folders):** `architecture/` (10), `cli/` (3), `decisions/` (4), `delivery/` (6), `engine/` (11), `interaction/` (5), `library/` (6), `package/` (7), `plans/` (4), `platform/` (10), `product/` (4), `quality/` (6), `reference/` (1), `research/` (5), `security/` (5), `settings/` (3), `tech-stack/` (3), `ui/` (4), plus root `readme.md`, `start-here.md`, `documentation-index.md` (`100` total verified by `ORIGINAL_DOCS` in [validate-backlog.py](../scripts/validate-backlog.py)).

## Research source register (62 primary sources)

All **62** primary vendor, protocol, crate, licensing, and security references are catalogued in [source register](research/source-register.md) (`S01`–`S62`, snapshot checked `2026-11-11`), with synthesis and limits in [research summary](research/research-summary.md), [prior art and differentiation](research/prior-art-and-differentiation.md), [feasibility matrix](research/feasibility-matrix.md), and [assumptions and open questions](research/assumptions-and-open-questions.md):

| Source IDs | Domain | Representative references |
|---|---|---|
| `S01`–`S08`, `S50`, `S58`, `S59` | Windows desktop, input, window bounds, power | Microsoft `IDesktopWallpaper`, `SetParent`, `SetWinEventHook`, `GetWindowRect`, `DwmGetWindowAttribute`, Raw Input, `WM_DISPLAYCHANGE`, `WM_POWERBROADCAST`, Lively WorkerW reports (`#2074`, `#3193`) |
| `S09`–`S14`, `S45`, `S48`, `S52` | macOS AppKit, Core Graphics, distribution | `NSWindow` collection behaviors, `CGWindowLevelKey`, Event Monitoring, `CGWindowListCopyWindowInfo`, `setDesktopImageURL`, reduced motion, notarization, `objc2::MainThreadMarker` |
| `S15`–`S23`, `S46`, `S47`, `S56`, `S57` | Linux X11, Wayland, KDE Plasma, GNOME | Wayland Protocol, `wlr-layer-shell`, `ext/wlr-foreign-toplevel`, EWMH, KDE Plasma plugins/scripting & Qt Quick scene graph, GNOME extensions & `St.ImageContent.set_data`, `smithay-client-toolkit`, Flatpak, freedesktop autostart |
| `S24`–`S34`, `S51` | Rust ecosystem, rendering, codecs, workspaces | `wgpu`, `egui-wgpu 0.37.0` (`wgpu ^30.0` snapshot), `fontdue`, `image` & `Limits`, `ffmpeg-the-third`, `ffmpeg-next`, FFmpeg send/receive & protocol options, Cargo workspaces, `tray-icon` |
| `S35`–`S40` | Archives, schemas, color, prior art | `zip::ZipFile`, JSON Schema Draft 2020-12, W3C CSS Color 4, **Chafa**, **Structure-based ASCII Art** (Xu, Zhang, Wong, SIGGRAPH 2010), **Lively** |
| `S41`–`S44`, `S49`, `S60`–`S62` | Distribution, security, trust, accessibility | GitHub large files / releases / `pull_request_target` / build attestations, TUF security, `egui` AccessKit, `ed25519-dalek` strict verification, RustSec `cargo-audit` |
| `S33`, `S53`–`S55` | Licensing & compliance | FFmpeg legal guidance, SIL OFL guidelines, Creative Commons `CC BY 4.0` and `CC BY-SA 4.0` |

## Voltshift reference audit (five inspected issues)

Observed read-only on `2026-11-11` via `gh api` against `Astraive/voltshift` (`../.issues/reference-audit.md`) solely to align Astraive organizational issue-tracking conventions:

- **Five inspected issues:**
  1. `Astraive/voltshift#2` — Master delivery roadmap (preserving crosswalks when scope reorganizes rather than silently deleting items).
  2. `Astraive/voltshift#3` — Foundation epic (structured goal, why-it-matters, ownership boundaries, exit criteria, and native sub-issues `/sub_issues`).
  3. `Astraive/voltshift#24` — QA epic (cross-cutting verification framing).
  4. `Astraive/voltshift#30` — Detailed implementation ticket (in/out scope, numbered requirements, technical notes, test procedures, `/parent`, `/dependencies/blocking`).
  5. `Astraive/voltshift#675` — Milestone acceptance gate (evidence row per ticket, Pass/Waived accounting, `/dependencies/blocked_by`, owner sign-off).
- **Supporting reference reads:** `Astraive/voltshift` labels/milestones REST endpoints and `.github/pull_request_template.md`, plus live Astraive organization introspection (`GET /orgs/Astraive/issue-types` showing `Task`, `Bug`, `Feature`; Project 4 `PVT_kwDOEF_JvM4BrI0s` showing `0` items, `13` fields, `1` view `View 1`, and `ProjectV2ViewConfigurationInput` exposing `visibleFieldIds` only).
- **Strict isolation from game content:** **Zero** Voltshift game content, assets, characters, gameplay, Unity/Convex/mobile/web domains, or commercial references were copied into AsciiWall requirements or backlog definitions.

## External backlog artifacts (local-only, not pushed)

All detailed planning records and regenerated reports remain outside Git under `../.issues/` (never committed or pushed):

- **Canonical records & registries:** `../.issues/schema.json`, `../.issues/labels.json`, `../.issues/milestones.json`, `../.issues/project.json`, `../.issues/reference-audit.md`, `../.issues/coverage/*.json` (8 slices), `../.issues/asciiwall/<ID>/{issue.json,CONTENT.md}` (`278`), `../.issues/asciiwall-library/<ID>/{issue.json,CONTENT.md}` (`78`).
- **Derived reports (regenerated via `python scripts/validate-backlog.py --write-reports`):**
  - `../.issues/reports/backlog-audit.json`
  - `../.issues/reports/backlog-creation-ledger.json`
  - `../.issues/reports/backlog-inventory.md`
  - `../.issues/reports/backlog-dependency-graph.md`
  - `../.issues/reports/backlog-requirement-traceability.md`

## Observed GitHub repository baseline (2026-11-11)

Read-only GitHub API snapshot recorded in `../.issues/reference-audit.md`:
- **`Astraive/asciiwall`:** Public, MIT (`LICENSE` SHA `6efc4912be82573179e8f1e2182ed32a1701577e` on initial tree `d936da4be08294ed946ce692c539bc644d4cb9b1`), default branch `main`, unprotected at observation (`404 Branch not protected`), `0` issues. Documentation PR #1 (`docs: add product and engineering specifications`, merge SHA `3801d374bd4b879105e7d6e2d08179eeed0695e4`) merged the 100 original specification docs prior to navigation link repairs.
- **`Astraive/asciiwall-library`:** Public, MIT (`tree 926502304b73d4fcd516a86dcc34a70685f41011` containing MIT `LICENSE` only at baseline), `0` issues.
- **Organization Project 4:** `https://github.com/orgs/Astraive/projects/4` (`PVT_kwDOEF_JvM4BrI0s`), private, `0` items (`0` remote issues/drafts created).
