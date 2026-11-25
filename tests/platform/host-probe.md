# WND-001 Windows host-probe evidence

## Scope and strategy

This issue-scoped probe performs read-only queries only: `GetShellWindow`, bounded `EnumWindows` / `EnumChildWindows` traversal, class-name and owner-process inspection, and `GetParent` relationships confirmed with `IsChild`, plus OS-version, native-architecture, and current-session queries. The strategy identifier is `windows-shell-hierarchy-enumeration-v1`. It limits hierarchy traversal to 512 window records and a 250 ms callback deadline, and reports budget exhaustion, timeout, missing shell, and API failure explicitly. It retains class categories/counts only; no HWND or foreign process ID is returned.

It does not create a window or graphics device, attach a surface, send shell messages, restart Explorer, or change desktop state. GPU adapter/backend identity is **NOT QUERIED** by this hierarchy-only probe; the report says so explicitly rather than inferring a rendering backend.

## Recorded non-interactive host observation

The previously supplied read-only host observation (not output from this implementation or a CI run) reported the following exact environment tuple:

- OS: Windows 11 Home Single Language, build `26300`, x64.
- Session: Console.
- Graphics devices observed in the host inventory: Intel UHD, driver `32.0.101.7026`; NVIDIA RTX 3050 laptop, driver `32.0.16.2124`.
- Probe strategy: `windows-shell-hierarchy-enumeration-v1`; the probe itself does not query GPU identities.

The query result was: `GetShellWindow()` returned an Explorer-owned `Progman` window with a `SHELLDLL_DefView` child. In that same process, `FindWindowW("Progman", nullptr)` did not find a window. Top-level `WorkerW` enumeration returned 18 windows and found no `SHELLDLL_DefView` child. The probe deliberately does not rely on `FindWindowW`, a fixed parent, a numeric handle, or one universal WorkerW layout. No HWNDs or foreign process IDs are recorded here.

This is hierarchy observation only. It is not evidence that a surface can be placed below icons or that the hierarchy is stable across other Windows builds, sessions, or Explorer restarts.

## Windows test evidence

**Local Windows evidence:** `cargo fmt --package ascii-platform -- --check` passed; `cargo clippy --locked --package ascii-platform --all-targets -- -D warnings` passed; `cargo test --locked --package ascii-platform` passed (6 tests across 3 suites). The test command is non-disruptive and does not create a window:

```text
cargo test --locked --package ascii-platform
```

The package includes deterministic injected hierarchy tests for an absent shell, changed WorkerW/unknown-parent layouts, timeout, and core work-budget exhaustion, plus one native read-only report test. The source hashes below were captured immediately before the Clippy/test run and match the source revision that passed those checks:

- `crates/ascii-platform/src/lib.rs`: `5cbb5ea5a0d2fd07f283050934bce27101f8dd229c963a33ba7af7e1f3589e6b`
- `crates/ascii-platform/src/probe.rs`: `0c7a4a2a89dfc269a4fab39ec049d57a8302af3dde6127a4c13e1d61af9b8741`
- `crates/ascii-platform/src/probe/windows.rs`: `3379845b06984a7b5d029ca9d2e16d66f2915d5b4c73c27c749f4271749812df`
- `crates/ascii-platform/tests/host_probe.rs`: `ddef24f1fb9e7a509f7a2c0d9e3c4eac4038137d78fa8cd7e5e21615b899c00c`

No binary or fixture is bundled, so there are no corresponding binary/fixture hashes.

## User-interactive acceptance gates — NOT RUN

- Below-icon visible placement: **NOT RUN**; no surface was created.
- Focus/non-activation interaction: **NOT RUN**; no focus test was performed.
- Win+D / Show Desktop: **NOT RUN**.
- Explorer restart/recovery: **NOT RUN**.
- Desktop icon click/selection preservation: **NOT RUN**.

These are unfinished acceptance gates, not passes and not failed placement results. They require a user-session interaction and must be recorded separately before any production strategy or support tier is selected.
