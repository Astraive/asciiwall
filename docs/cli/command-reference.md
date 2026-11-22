# Command-line interface reference

## Status and conventions

The `asciiwall` binary converts local PNG/JPEG inputs to bounded monochrome density `.txt` and `.ans` output through the shared decoder, density matcher, and terminal-safe exporter. It does not start a controller or wallpaper host. PNG output, structural matching, source-color matching, and settings-file loading are unavailable capabilities; those requests fail closed without publishing output.
Only `asciiwall convert` is registered by the current parser. The `preview`, `bake`, `benchmark`, desktop, package/library, config-command, `doctor`, and `cache` descriptions below are planned reference only; those commands are unavailable in this binary.

The binary accepts global `--json`, `--quiet`, `--verbose`, `--config <path>`, `--no-color`, `--offline`, and `--help` before or after `convert`. Diagnostics are uncolored and go to stderr unless `--json` requests a structured result on stdout; help remains plain text. `--quiet` never suppresses errors. Paths are separate arguments, never shell strings. This build makes no application network requests, but filesystem paths can target network shares. On Windows, `--offline` rejects direct UNC, mapped remote-drive, and non-volume device-namespace roots for source, font, output, or `--config`; mapped drives are classified from their root only, without opening a share path. Relative paths are resolved against the current directory without canonicalizing or following reparse points. A second path-only pass rejects every reparse-point component, including local symlinks and junctions, without following its target. Root checks precede component metadata probes, which precede destination-conflict checks and file opens. Ordinary non-reparse local paths retain their normal behavior. The shared settings registry is unavailable, so ordinary local non-reparse `--config` requests report a capability error. No saved online preference is loaded or overridden until settings support exists.

## Conversion and rendering

| Command | Purpose | Important flags |
|---|---|---|
| `asciiwall convert <image> --output <file>` | Current executable path: monochrome density rendering to text/ANSI; PNG output is unavailable | `--columns`, `--matcher`, `--charset`, `--color`, `--format`, `--overwrite` |
| `asciiwall preview <source>` | Planned; unavailable in current binary — normal interactive preview window | `--preset`, `--columns`, `--safe-mode` |
| `asciiwall bake <source> --output <file>` | Planned; unavailable in current binary — precompute character clip or rendered video | `--format ascii-clip|video`, `--fps`, `--duration`, `--seed`, `--width`, `--height`, `--encoder`, `--overwrite` |
| `asciiwall benchmark <source>` | Planned; unavailable in current binary — explicit local benchmark | `--preset`, `--warmup`, `--frames`, `--output`, `--json` |

File extension can suggest a format for executable `convert`, but explicit conflicting flags are rejected. The `bake --format video` and audio notes describe planned behavior only: if implemented, video must require an encoder actually included in the build; print supported encoders rather than inventing availability. Audio is absent/muted unless a future explicit supported option is selected.

The request parser defaults to 240 columns, `--matcher density`, monochrome (`--color mono`), and the literal printable ASCII repertoire ` .:-=+*#%@`. `--charset <glyphs>` uses `ascii-convert`'s strict single-cell repertoire (printable ASCII, block elements, and Braille); unsupported or duplicate scalars are rejected. `--matcher structural` and `--color source` are recognized but report unavailable capabilities, not fallback output. There is no production built-in font: omitting `--font <path>` returns a capability error rather than silently selecting a font. An explicit font is measured into a shared glyph table using the current fixed geometry (8 px raster size, 8×12 cell, baseline 9). The density path decodes PNG/JPEG through `ascii-media`, calls the shared CPU density matcher, and serializes `.txt` or `.ans` through `ascii-render`; `.ans` currently uses true-color SGR. PNG rendering and structural/source-color conversion remain unavailable.

The shared font helper reads an explicitly supplied local font under a 16 MiB and caller-budget bound, then delegates glyph-table construction to `ascii-convert`. Fonts and images are opened as regular file handles; symlinks to regular files are supported and special-file handles are rejected before reading. Unix opens with `O_NONBLOCK` and validates the opened handle so a direct or swapped FIFO cannot block; Windows checks the native handle type and rejects pipe/device handles before synchronous reads. Cancellation is checked around file operations and between reads. The CLI supplies a validated fixed `GlyphConfig`, a shared strict-cell repertoire, and a bounded resource budget before attempting font loading.

For a destination other than `-`, the parser infers `.txt`, `.ans`/`.ansi`, or `.png` case-insensitively. An explicit format conflicting with a recognized extension is a usage error; an unknown extension requires `--format`. For `--output -`, omitted format means plain text. `--json --output -` is a usage error. Existing destination entries (including dangling symlinks) require `--overwrite`; that conflict takes precedence over format and charset validation. File output is staged beside the destination and published only after complete conversion; the final commit enforces no-clobber unless `--overwrite` is explicit.

```bash
# Convert a local image to plain text without starting a desktop host.
asciiwall convert "mountain image.png" --font "font with spaces.ttf" --columns 180 --format txt --output "mountain output.txt"

# Plain-text artwork may use stdout; JSON status and artwork cannot share stdout.
asciiwall convert mountain.png --font font.ttf --format txt --output -
asciiwall --json convert mountain.png --font font.ttf --format txt --output mountain.txt

```

## Planned desktop control (unavailable in current binary)

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

For the planned desktop-control commands, `--all-displays` is an explicit alternative to `--display`, not an implicit default for destructive replacement. Without a controller, mutation commands either start it through a documented `--start` option or return an unavailable-controller error. They must not silently create multiple daemon instances.

## Planned packages and library (unavailable in current binary)

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

When implemented, planned `package pack` will permit a clearly named source descriptor with computed inventory omitted; the output manifest will always contain final actual hashes/sizes. Planned `package validate` will default to no network and will not follow URLs from metadata. Download origin approval cannot be bypassed by a package.

## Planned configuration and diagnostics (unavailable in current binary)

Planned `config get <key>`, `config set <key> <value>`, `config reset <key>`, and `config explain <key>` will use the [settings registry](../settings/settings-reference.md). `--expect-revision <n>` will prevent stale writes. A batch config file will be validated atomically.

Planned `doctor` will report environment, adapter candidates, permissions, graphics capabilities, codec inventory, and safe remediation. `doctor --export <file>` will produce a reviewed redacted diagnostic report; it will never upload it. Planned `cache clear` will remove derived data only and respect active resource leases.

## Output safety

Text export contains only the validated printable glyph grid and LF row separators, with no ANSI escapes; ANSI export uses only engine-generated SGR color sequences and printable glyphs. Artwork and JSON status are never combined on stdout. Verbose progress, completion notices, and verbose target-loss notices go to stderr; `--quiet` suppresses non-error notices. CLI errors use fixed messages and never interpolate input, output, font, config paths, host names, or option values; terminal controls are stripped. PNG rendering, structural matching, source-color conversion, and settings-file loading are not available in this build.

The CLI rejects the stdout/JSON conflict, identifies parsed commands without echoing supplied values, and strips terminal controls from diagnostics. In offline mode, Windows resolves relative source, font, output, and `--config` paths lexically against the current directory. Direct UNC paths, mapped remote-drive roots, and non-volume device-namespace roots are rejected before any component metadata probe; a non-following component check then rejects every filesystem reparse point before destination-conflict metadata, file opens, staging, or publication. This intentionally rejects local symlinks and junctions too, so no reparse target is followed. Explicit source and font paths are opened through nonblocking/handle-validated file paths, with all image bytes passed to the shared bounded decoder. Text/ANSI bytes are fully materialized before publication; failed decoding, conversion, serialization, and cancellation before commit leave an existing output untouched. Publication uses a same-directory staging file and a no-clobber commit unless overwrite was explicitly requested. On filesystems without atomic no-replace rename, the no-clobber fallback may leave an additional temporary link.
