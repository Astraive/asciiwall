# Command-line interface reference

## Status and conventions

These commands are the proposed stable CLI contract, not commands available from an already implemented executable. All work is local unless a command explicitly downloads catalog/package data and network consent is enabled. Headless conversion/export does not require a wallpaper host.

Global flags: `--json`, `--quiet`, `--verbose`, `--config <path>`, `--no-color`, `--offline`, and `--help`. Paths are arguments, never interpolated into shell command strings. `--offline` overrides any saved online preference for that invocation.

## Conversion and rendering

| Command | Purpose | Important flags |
|---|---|---|
| `asciiwall convert <image> --output <file>` | Image to text/ANSI/PNG | `--columns`, `--matcher density|structural`, `--charset`, `--color`, `--format txt|ansi|png`, `--overwrite` |
| `asciiwall preview <source>` | Normal interactive preview window | `--preset`, `--columns`, `--safe-mode` |
| `asciiwall bake <source> --output <file>` | Precompute character clip or rendered video | `--format ascii-clip|video`, `--fps`, `--duration`, `--seed`, `--width`, `--height`, `--encoder`, `--overwrite` |
| `asciiwall benchmark <source>` | Explicit local benchmark | `--preset`, `--warmup`, `--frames`, `--output`, `--json` |

File extension can suggest a format but explicit conflicting flags are rejected. `bake --format video` requires an encoder actually included in the build; print supported encoders rather than inventing availability. Audio is absent/muted unless a future explicit supported option is selected.

```bash
asciiwall convert mountain.png --columns 180 --matcher structural --format txt --output mountain.txt
asciiwall convert mountain.png --color source --format png --output mountain-ascii.png
asciiwall bake waves.mp4 --format ascii-clip --fps 30 --duration 12s --output waves.ascii-clip
asciiwall bake scene.json --format video --fps 30 --duration 10s --seed 42 --output rain.webm
```

## Desktop control

| Command | Purpose |
|---|---|
| `asciiwall wallpaper apply <installed-id-or-package> --display <id>` | Validate/prepare/apply to explicit display |
| `asciiwall wallpaper pause --display <id>` | Add user pause reason |
| `asciiwall wallpaper resume --display <id>` | Clear user pause only, not lock/power reasons |
| `asciiwall wallpaper stop --display <id>` | Detach owned wallpaper and reveal existing background |
| `asciiwall wallpaper status` | Assignments, capabilities, effective state |
| `asciiwall displays list` | Local output identifiers and geometry |
| `asciiwall refresh --scope redraw|source|scene|desktop|library` | Explicit refresh action |
| `asciiwall quit` | Graceful controller/host shutdown |

`--all-displays` is an explicit alternative to `--display`, not an implicit default for destructive replacement. Without a controller, mutation commands either start it through a documented `--start` option or return an unavailable-controller error. They must not silently create multiple daemon instances.

## Packages and library

| Command | Purpose |
|---|---|
| `asciiwall package inspect <archive>` | Bounded metadata inspection, no activation |
| `asciiwall package validate <archive>` | Full static validation, optional bounded decode checks |
| `asciiwall package pack <source-directory> --output <archive>` | Build inventory and strict manifest from approved build inputs |
| `asciiwall package import <archive>` | Validate and install immutable local package |
| `asciiwall package export <installed-id> --output <archive>` | Portable resolved variant, rights/privacy review |
| `asciiwall library list` | Installed local metadata |
| `asciiwall library search <terms>` | Search local or previously fetched catalog |
| `asciiwall library catalog refresh` | Explicit network operation if enabled |
| `asciiwall library download <id> --version <version>` | Explicit verified download, not auto-Apply |
| `asciiwall library remove <id> --version <version>` | Remove managed package after dependency checks |

`package pack` permits a clearly named source descriptor with computed inventory omitted; the output manifest always contains final actual hashes/sizes. `package validate` defaults to no network and does not follow URLs from metadata. Download origin approval cannot be bypassed by a package.

## Configuration and diagnostics

`config get <key>`, `config set <key> <value>`, `config reset <key>`, and `config explain <key>` use the [settings registry](../settings/settings-reference.md). `--expect-revision <n>` prevents stale writes. A batch config file is validated atomically.

`doctor` reports environment, adapter candidates, permissions, graphics capabilities, codec inventory, and safe remediation. `doctor --export <file>` produces a reviewed redacted diagnostic report; it never uploads it. `cache clear` removes derived data only and respects active resource leases.

## Output safety

Plain text may go to stdout with `--output -`; binary output and JSON status cannot share stdout. Reject `--json --output -` when the output is artwork bytes. Progress and diagnostics go to stderr. No ANSI escape output when redirected unless `--format ansi` was explicitly chosen. Exported plain text strips terminal control sequences from metadata/custom glyph input.
