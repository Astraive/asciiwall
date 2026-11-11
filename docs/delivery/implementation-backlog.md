# Implementation backlog and ownership

## Status

The following 40 work units are an implementation handoff, not completed tickets. Each unit has an independently testable outcome and a detailed test-first plan linked below. Use dependency order rather than treating this table as a calendar. Native platform promotion is per host; the aggregate final review can exclude adapters that remain experimental.

| ID | Work unit | Dependencies | Detailed plan |
|---|---|---|---|
| E01 | Core frame and budget contracts | none | [engine plan](../plans/engine-implementation-plan.md#e01-core-frame-and-budget-contracts) |
| E02 | Font masks and glyph descriptors | E01 | [engine plan](../plans/engine-implementation-plan.md#e02-font-masks-and-glyph-descriptors) |
| E03 | Density conversion and aspect-aware sampling | E01 E02 | [engine plan](../plans/engine-implementation-plan.md#e03-density-conversion-and-aspect-aware-sampling) |
| E04 | Structural matcher and temporal state | E03 | [engine plan](../plans/engine-implementation-plan.md#e04-structural-matcher-and-temporal-state) |
| E05 | Bounded image decoding | E01 | [engine plan](../plans/engine-implementation-plan.md#e05-bounded-image-decoding) |
| E06 | GPU atlas presenter and preview | E01 E02 | [engine plan](../plans/engine-implementation-plan.md#e06-gpu-atlas-presenter-and-preview) |
| E07 | Declarative scene execution | E01 E02 | [engine plan](../plans/engine-implementation-plan.md#e07-declarative-scene-execution) |
| E08 | FFmpeg decoder boundary | E05 | [engine plan](../plans/engine-implementation-plan.md#e08-ffmpeg-decoder-boundary) |
| E09 | Media and simulation scheduler | E01 E08 | [engine plan](../plans/engine-implementation-plan.md#e09-media-and-simulation-scheduler) |
| E10 | Precomputed character-clip codec | E01 | [engine plan](../plans/engine-implementation-plan.md#e10-precomputed-character-clip-codec) |
| E11 | Text, PNG and video export | E03 E06 E08 E10 | [engine plan](../plans/engine-implementation-plan.md#e11-text-png-and-video-export) |
| E12 | Reproducible benchmark harness | E04 E06 E09 | [engine plan](../plans/engine-implementation-plan.md#e12-reproducible-benchmark-harness) |
| D01 | Native hosting feasibility spikes | E01 | [desktop plan](../plans/desktop-implementation-plan.md#d01-native-hosting-feasibility-spikes) |
| D02 | Platform capability and surface contracts | E01 D01 | [desktop plan](../plans/desktop-implementation-plan.md#d02-platform-capability-and-surface-contracts) |
| D03 | Windows production host | D02 E06 | [desktop plan](../plans/desktop-implementation-plan.md#d03-windows-production-host) |
| D04 | macOS host | D02 E06 | [desktop plan](../plans/desktop-implementation-plan.md#d04-macos-host) |
| D05 | Named X11 host combinations | D02 E06 | [desktop plan](../plans/desktop-implementation-plan.md#d05-named-x11-host-combinations) |
| D06 | Layer-shell Wayland host | D02 E06 | [desktop plan](../plans/desktop-implementation-plan.md#d06-layer-shell-wayland-host) |
| D07 | Plasma/GNOME bridge experiments | D02 E06 H01 | [desktop plan](../plans/desktop-implementation-plan.md#d07-plasmagnome-bridge-experiments) |
| D08 | Passive pointer and click effects | D02 E07 P01 | [desktop plan](../plans/desktop-implementation-plan.md#d08-passive-pointer-and-click-effects) |
| D09 | Window metadata and occlusion policy | D02 E09 P01 | [desktop plan](../plans/desktop-implementation-plan.md#d09-window-metadata-and-occlusion-policy) |
| D10 | Transactional Apply and transitions | E06 E07 E09 P01 | [desktop plan](../plans/desktop-implementation-plan.md#d10-transactional-apply-and-transitions) |
| D11 | Displays, power and session lifecycle | D02 E09 D10 | [desktop plan](../plans/desktop-implementation-plan.md#d11-displays-power-and-session-lifecycle) |
| D12 | Native editor vertical slice | D10 P01 P05 | [desktop plan](../plans/desktop-implementation-plan.md#d12-native-editor-vertical-slice) |
| P01 | Settings registry and migrations | E01 | [packages plan](../plans/packages-implementation-plan.md#p01-settings-registry-and-migrations) |
| P02 | Manifest and scene semantic validator | E07 P01 | [packages plan](../plans/packages-implementation-plan.md#p02-manifest-and-scene-semantic-validator) |
| P03 | Safe staged archive importer | P02 H02 | [packages plan](../plans/packages-implementation-plan.md#p03-safe-staged-archive-importer) |
| P04 | Deterministic portable packer/exporter | P02 P03 | [packages plan](../plans/packages-implementation-plan.md#p04-deterministic-portable-packerexporter) |
| P05 | Local immutable library index | P03 | [packages plan](../plans/packages-implementation-plan.md#p05-local-immutable-library-index) |
| P06 | Optional signed catalog client | P04 P05 | [packages plan](../plans/packages-implementation-plan.md#p06-optional-signed-catalog-client) |
| P07 | Community repository build pipeline | P04 P06 E11 | [packages plan](../plans/packages-implementation-plan.md#p07-community-repository-build-pipeline) |
| P08 | CLI command and error contract | E11 P04 P05 H01 | [packages plan](../plans/packages-implementation-plan.md#p08-cli-command-and-error-contract) |
| H01 | Authenticated local IPC and supervision | E01 D02 | [hardening plan](../plans/hardening-implementation-plan.md#h01-authenticated-local-ipc-and-supervision) |
| H02 | Restricted media-worker runner | E05 E08 H01 | [hardening plan](../plans/hardening-implementation-plan.md#h02-restricted-media-worker-runner) |
| H03 | Adaptive quality and aggregate budgets | E12 D11 | [hardening plan](../plans/hardening-implementation-plan.md#h03-adaptive-quality-and-aggregate-budgets) |
| H04 | Accessibility and localization gate | D12 | [hardening plan](../plans/hardening-implementation-plan.md#h04-accessibility-and-localization-gate) |
| H05 | Diagnostics and recovery tools | D11 H01 | [hardening plan](../plans/hardening-implementation-plan.md#h05-diagnostics-and-recovery-tools) |
| H06 | Packaging, signing and release CI | P08 H02 H04 H05 | [hardening plan](../plans/hardening-implementation-plan.md#h06-packaging-signing-and-release-ci) |
| H07 | Fuzz and regression corpus | E10 P03 H01 | [hardening plan](../plans/hardening-implementation-plan.md#h07-fuzz-and-regression-corpus) |
| H08 | Native support promotion | D03 D04 D05 D06 D07 H03 H06 H07 | [hardening plan](../plans/hardening-implementation-plan.md#h08-native-support-promotion) |

## Ownership and definition of done

An owner implements a work unit with its specification open, writes a failing test, confirms the failure, implements the smallest complete behavior, runs the targeted and affected regression tests, and records evidence before committing. Review unsafe/native/security changes independently where possible.

Do not merge an empty adapter skeleton and mark its platform supported. A schema file alone does not finish secure import. A benchmark command without actual controlled runs does not finish performance claims. A library recipe brief without licensed assets/preview/review is not a published wallpaper.

## Cross-cutting review focus

Path traversal/races, stale asynchronous generations, coordinate-space confusion, capability denial, and aggregate resource exhaustion appear in several subsystems. Their tests are assigned to the owning task rather than left as a final vague hardening item. [Edge-case IDs](../quality/edge-case-matrix.md) are the release evidence checklist.

## Interface discipline

Signatures in the plans are proposed contracts, not compiled APIs. The first owning task defines each type and its invariants. If a spike changes a contract, update its consumers, schema examples, tests, and decision record in the same reviewed change. Keep public crates narrow until real reuse justifies stabilization.
