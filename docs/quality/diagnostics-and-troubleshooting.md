# Diagnostics and troubleshooting playbook

## Diagnostic snapshot

`asciiwall doctor --json` should report app/format/IPC versions, host adapter and support tier, graphics backend/device limits, display transforms, permission/capability states, current pause reasons, codec inventory, worker health, and bounded resource counters. It excludes source pixels, input history, window titles, and full paths by default.

A diagnostic export is saved locally after preview. It does not upload automatically. A user can redact a display name or driver detail before attaching it to an issue.

## Symptom-driven checks

| Symptom | Check first | Safe action |
|---|---|---|
| Preview works, wallpaper absent | Host capability/layering, shell version | Reattach once; otherwise preview/static fallback |
| Wallpaper above icons | Wrong native parent/level or shell regression | Detach immediately; collect safe host metadata |
| Cursor effect absent | Observer permission/capability and scope | Explain unavailable feed, not force capture |
| Window reaction absent on Wayland | Geometry bridge, not just toplevel list | Disable optional effect or install explicit supported adapter |
| Choppy video | Decode/convert/present timing and late frames | Lower density/FPS; inspect VFR scheduling |
| High GPU while static | Continuous redraw, hidden preview, bloom | Stop unnecessary loops/effects |
| Unexpected pause | Active pause-reason set | Show every reason; clear only requested one |
| Import fails | Exact validation code/path/budget | Repair package; never bypass integrity globally |
| Export differs | Font/grid/color/seed/encoder fingerprints | Compare effective settings and representation |
| Source missing | Original moved or removable disk absent | User Locate action; no filesystem crawl |
| Tray missing | Desktop tray support | Relaunch settings or CLI stop/quit |
| Catalog unavailable | Network disabled, expiration, signature, TLS | Keep local library; do not disable verification |

## Safe mode

Start with no last wallpaper, no observer feeds, no online catalog, basic UI, and the default GPU/CPU preview fallback. Safe mode must be reachable by CLI flag and a documented startup recovery path. Repeated host crashes automatically suppress autoload until the user chooses repair.

## Logs and metrics

Use structured events with stable IDs, durations, revisions, generations, and error categories. Rate-limit repetitive failures. Rotate logs within the configured budget. Avoid logging every cursor move, cell buffer, media packet, or window title.

Native verbose debugging may expose sensitive paths; gate it behind a temporary explicit session option and redact exports. Never ask a user to send a memory dump or original personal video as the default troubleshooting step.

## Recovery order

Pause the affected assignment; retain/reveal a safe background; cancel stale jobs; invalidate the failed generation; recreate only the broken component; retry with a bounded backoff; stop and explain after the circuit breaker opens. Rebooting the whole machine is not the default repair for an engine error.

## Bug reports

Include expected/actual behavior, exact host/version, reproduction steps, sanitized doctor output, smallest rights-cleared fixture, and whether the issue reproduces in preview or only as wallpaper. Distinguish a missing capability from a regression in a claimed supported capability.
