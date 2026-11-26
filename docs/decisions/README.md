# Architecture decision register

This register tracks the four proposed architecture decisions below. An ADR records an intended design, not evidence that code has been built, compiled, tested, or supported on a native host. The ADRs remain **proposed** until a reviewed change links decision-scoped implementation and verification evidence that justifies promotion. A draft package format is likewise not a frozen or implemented format. This policy does not assert that no implementation code exists; it states that these ADRs have no linked promotion evidence recorded here.

## Decisions

- [ADR 0001 — Own Rust engine and shared frame model](0001-native-rust-engine.md): native shared Rust ownership and reusable engine contracts.
- [ADR 0002 — Capability-aware native wallpaper hosts](0002-capability-aware-native-hosts.md): native host adapters and explicit, independently reported capabilities.
- [ADR 0003 — Declarative .asciiwall packages](0003-declarative-portable-packages.md): bounded, versioned data packages and reviewed built-in scene graphs.
- [ADR 0004 — Offline core and static community library](0004-offline-core-and-static-library.md): offline local operation and separately published static catalog/artifacts.

## Lifecycle and change record

Each ADR carries its own status, alternatives, decision rationale, consequences, and revisit criteria. Promotion requires a reviewed decision change that links relevant implementation and actual verification evidence; planning text, a proposed interface, or a stated acceptance criterion is not evidence. Record the evidence and resulting status in the same change that updates the ADR. Do not claim native support, compilation, test results, or measured dependency behavior unless that evidence is available and linked.

When measured dependency failure, a newly supported native protocol, reviewed declarative nodes, or an optional service motivates revision, update the decision rationale and **every affected specification, consumer, and example together in the same reviewed change**. State the evidence, alternatives, compatibility/consequence impact, and revisit criteria. If an affected consumer cannot be updated in that change, keep the decision proposed and defer promotion.

The existing boundaries remain in force unless an explicit reviewed product/security decision changes them: no implicit web runtime, executable-package exception, hidden online prerequisite, or unreviewed extension ABI. Any exception must state its rationale and consequences in the affected ADR and all affected contracts; silence or convenience is not approval.
