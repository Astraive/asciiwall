# Assumptions, uncertainty, and remaining research

## Evidence categories

**Observed source fact:** a primary page documents an API/constraint or published dependency metadata. **Design decision:** this project chooses a behavior/default. **Implementation hypothesis:** a proposed integration could work but needs a native spike. **Open question:** a release-critical detail is unresolved. Do not present these categories interchangeably.

## Assumptions used

The project is a new open-source local desktop app with no existing repository inspected in this task. The user wants Windows/macOS/Linux, but accepts no explicit universal support commitment here. Rust owns conversion/scene behavior; external libraries handle codecs/native interfaces. The second repository is a proposed community content source, not an already populated gallery.

The recommended primary target is Windows first, with other hosts gated independently. Native egui is selected for a cohesive Rust stack; no UI prototype or accessibility audit has been completed. The package format and clip binary layout are drafts, not stable deployed standards.

## Open questions and resolution owners

| Question | Resolution evidence | Owner/work unit |
|---|---|---|
| Exact supported OS/compositor versions | Native support matrix on named environments | D01-D07, H08 |
| FFmpeg wrapper and native build family | Cross-platform compile/decode/seek/security review | E08, H02 |
| Rust compiler/MSRV and graphics family | Clean builds and lockfile across targets | E06, H06 |
| Plasma/GNOME presenter path | Copy/latency/shell-response benchmark | D07 |
| Native worker sandbox per distribution | Denied unrelated-file/network tests | H02 |
| Structural quality/cost frontier | Same-quality comparative benchmarks | E04, E12 |
| Hardware video decode benefit | End-to-end transfer/energy measurement | E08, E12 |
| Catalog signing/rotation operation | Threat review and recovery rehearsal | P06, P07 |
| Font bundle choice and redistribution | Verified license/coverage/appearance | E02, H06 |
| Accessibility readiness | Real keyboard/screen-reader tests | H04 |
| Sustainable gallery review ownership | Maintainer/review process and rights records | P07 |

## Limits of this research

API documentation can describe a capability without proving the chosen combination works inside a wallpaper host. Project issues show concrete failures but not statistical prevalence. Latest-version documentation can change after this research snapshot. Native packaging and permission behavior need testing on actual release targets.

No CPU/GPU/power measurements, app builds, live desktop integration, remote repository creation, signed releases, or populated wallpaper library were performed. The delivery contains researched design documents, draft schemas, fixture examples, implementation plans, and document-level validation.

## How to update the pack

When a spike resolves a question, add the observed environment and result, update the relevant decision/support document, and adjust affected schema/setting/test contracts. Preserve the distinction between proposed budgets and measured values. Source URLs are starting points, not a permanent guarantee of current API behavior.
