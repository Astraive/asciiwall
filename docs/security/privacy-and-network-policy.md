# Offline operation and network policy

## Offline invariant

Import, convert, preview, apply, play, edit, bake, export, inspect, and local search must work with all network access denied. No account, cloud storage, server database, API key, telemetry, analytics identifier, crash upload, or online license activation is required.

Optional online features are manual catalog refresh and approved package/preview download. They live in a separate controller module/feature, not inside media decoders or scene execution. A portable package cannot enable them.

## Explicit network boundaries

Before enabling online library access, explain the destination origin and what requests reveal to that host, such as ordinary connection metadata. Do not claim external hosting learns nothing. The app sends no installed-library inventory, active wallpaper details, cursor events, or local file paths.

Use HTTPS with certificate validation, strict redirects/origin policy, bounded payloads and timeouts, safe caching, and a clear Cancel action. No automatic retries forever. Metadata URLs are displayed as links only; opening one is a user action, not background fetching.

## Dependency audit

Offline tests include FFmpeg protocol handlers, image/font libraries, update checks, tray helpers, and error-reporting code. [S32](../research/source-register.md#s32) A network-disabled core build should compile without the optional downloader stack where feasible. A firewall-denied integration test confirms no hidden network dependency.

Build-time dependency downloads are separate from runtime behavior. Contributors may need network access to fetch Cargo/native dependencies; release binaries should not download a runtime/model/codec on first playback without explicit user consent.

## Local data

Store settings, immutable packages, user variants, derived caches, and redacted logs in appropriate per-user locations. Imported source files remain in place by default; optional managed copies are explicit. No recursive home-directory indexing.

Do not persist cursor trajectories or window titles. Metrics are coarse local performance counters and optional. Diagnostic export has a preview/redaction step and is saved locally; attaching it to an issue is the user's decision.

## Deletion and portability

Uninstall removes app-owned executables/startup entries and offers separate choices for settings, managed library, and cache. It never deletes referenced originals. A portable settings export omits machine identifiers and permissions by default.

The app can promise no built-in telemetry in the specified build; it cannot promise the operating system, GPU driver, or external download host performs no independent telemetry. Keep that distinction in privacy documentation.
