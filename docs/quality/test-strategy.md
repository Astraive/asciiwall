# Testing strategy and evidence levels

## Evidence levels

Document-schema validation proves that examples match the proposed schema; it does not prove the app exists. Unit tests prove bounded functions for selected cases; they do not prove wallpaper layering. A normal GPU preview does not prove desktop integration. Manual native testing is required for shell behavior, permissions, power, accessibility, and installer cleanup.

Tag reports as document QA, unit/property, integration, fuzz, benchmark, native manual, or release acceptance. Record exact source/binary hash and environment. Never merge these into one vague 'tested' badge.

## Engine correctness

Unit-test color transforms, alpha composition, cell aspect ratio, glyph calibration, deterministic seeds, matcher score normalization, row strides, frame dimensions, timestamp arithmetic, and cache fingerprints. Property tests enforce checked multiplication, no out-of-range glyph IDs, finite values, valid buffers, and deterministic output under identical inputs.

Golden tests compare character grids exactly for deterministic CPU algorithms. Raster screenshots use documented tolerances because font/GPU/platform differences can affect pixels. Keep goldens small and licensed. Changing a golden requires an explanation, not automatic acceptance of every generated difference.

## Media and scheduling

Use fixtures for VFR, B-frames, truncated files, absent duration, unusual orientation, animated-image disposal, alpha, unsupported HDR, decoder drain, seek, loop, and cancellation. A fake decoder allows precise timestamps and failure injection without relying on a particular codec build.

Model tests exercise pause-reason combinations, generation cancellation, bounded queues, dropped late frames, suspend/resume, repeated Apply, and decoder crashes. Assert queue/memory bounds, not just correct final appearance.

## Package and configuration

Validate schemas, semantic graphs, paths, hashes, budgets, unknown fields, migration backups, revision conflicts, and deterministic packing. Fuzz archive headers, manifest parser, scene graph, clip binary reader, and IPC envelope. Run fuzz jobs in isolated environments; malicious fixtures are not opened manually in the desktop shell.

Native parser dependencies require sanitizer/fuzz coverage where supported. Known-advisory scanning is an additional input, not a replacement for runtime defenses. [S62](../research/source-register.md#s62)

## Platform contracts

A fake host provides capability changes and surface failures for CI. Real-host tests verify behind-icons placement, no focus stealing, click-through, desktop menus/dragging, output changes, shell restart, lock, fullscreen, and safe shutdown. Record exact Windows build/macOS version/Linux compositor and graphics backend.

Plasma/GNOME bridge testing includes shell-main-thread responsiveness and complete disable cleanup. Virtual machines help broad coverage but do not replace physical mixed-DPI/GPU and power testing.

## UI and accessibility

Test keyboard-only first-run/import/apply/pause/export, screen-reader labels, high-contrast and scaled UI, localization overflow, unsaved changes, batch cancellation, and effective-settings explanations. A visual screenshot alone does not prove accessibility.

## Release gates

All P0 cases must pass for every claimed supported host; P1 failures are explicitly triaged with documented limitations. No release if import can write outside staging, a wallpaper steals input, crash loops block login, or the advertised offline path requires network. See [edge cases](edge-case-matrix.md) and [acceptance criteria](acceptance-criteria.md).
