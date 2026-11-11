# Local signals and rule-based automation

## Scope

A data-driven rule system can react to time of day, local playback phase, pointer/window feeds, battery state, idle state where supported, and optional bounded system-load measurements. No cloud service is needed. Rules do not execute shell commands, access arbitrary files, or grant permissions.

Initial built-in signal types are time, pointer, button, window-coverage, fullscreen, and power. CPU/load and audio-derived signals are later optional adapters. Reading a CPU number is not required for core wallpaper playback.

## Rule contract

A rule has a stable ID, enabled flag, trigger, bounded condition set, priority, cooldown, and allowlisted action. Actions include setting an effect parameter, switching a local preset, pausing an assignment with a named reason, and choosing a local playlist entry. They cannot install packages, change system settings, or contact URLs.

Proposed evaluation order: hard safety constraints first; explicit user pause next; higher-priority matching rules next; ties break by stable ID. Actions are idempotent and carry their origin. No rule may recursively trigger itself without a bounded state change and cooldown.

## Time and playlists

Use wall clock only for schedules and monotonic time for simulation/durations. Handle DST, timezone changes, manual clock jumps, and overnight ranges. Re-evaluate current intended state after a clock change instead of replaying every missed trigger.

Playlist shuffle is seeded and avoids immediate repeats when more than one item exists. Missing/incompatible entries are skipped with a bounded retry count; an all-invalid playlist retains the last valid scene or static fallback. No endless import loop.

## Audio policy

File-based amplitude/spectrum analysis can be computed locally from an explicitly selected file in a later phase. System-loopback audio and microphone capture require separate platform research, permissions, and UI. They are not hidden dependencies of a music-reactive package. Audio playback defaults muted, with per-output duplication avoided.

Signal levels are normalized, smoothed, and clamped. Missing samples decay to neutral rather than producing maximum brightness. Cap work and ensure a paused/locked wallpaper does not keep capture active without a separately stated user policy.

## Rule editing and export

Package rules are visual suggestions operating only inside the scene. Machine-specific schedules/app matches belong to user settings and are excluded from portable exports. App matching uses locally selected identifiers, not titles or typed content.

A rule inspector shows last evaluation, active conditions, chosen action, and the constraint that overrode it. This makes full customization explainable instead of a collection of switches with hidden precedence.
