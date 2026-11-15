# Command-line interface reference

## Status and conventions

The current `asciiwall` binary parses and validates headless conversion requests, but does **not** produce conversions: image decoding, text/ANSI export, and PNG export are not integrated. Valid requests fail with a capability error without decoding the input, consuming a selected font, or producing an output. The command does not start a controller or wallpaper host.

The binary accepts global `--json`, `--quiet`, `--verbose`, `--config <path>`, `--no-color`, `--offline`, and `--help` before or after `convert`. Diagnostics are uncolored and go to stderr unless `--json` requests a structured result on stdout; help remains plain text. `--quiet` never suppresses errors. Paths are separate arguments, never shell strings. This build makes no network requests; the shared settings registry is not available, so explicitly supplying `--config` reports a capability error. Once settings are integrated, `--offline` must override any saved online preference for that invocation.

## Conversion and rendering

| Command | Purpose | Important flags |
|---|---|---|
| `asciiwall convert <image> --output <file>` | Image to text/ANSI/PNG | `--columns`, `--matcher density|structural`, `--charset`, `--color`, `--format txt|ansi|png`, `--overwrite` |
| `asciiwall preview <source>` | Normal interactive preview window | `--preset`, `--columns`, `--safe-mode` |
| `asciiwall bake <source> --output <file>` | Precompute character clip or rendered video | `--format ascii-clip|video`, `--fps`, `--duration`, `--seed`, `--width`, `--height`, `--encoder`, `--overwrite` |
| `asciiwall benchmark <source>` | Explicit local benchmark | `--preset`, `--warmup`, `--frames`, `--output`, `--json` |

File extension can suggest a format but explicit conflicting flags are rejected. `bake --format video` requires an encoder actually included in the build; print supported encoders rather than inventing availability. Audio is absent/muted unless a future explicit supported option is selected.

The current request parser defaults to 240 columns, `--matcher density`, monochrome (`--color mono`), and the literal printable ASCII repertoire ` .:-=+*#%@`. `--charset <glyphs>` supplies a literal repertoire; until the shared strict-cell repertoire is available it accepts distinct printable ASCII only and rejects controls, duplicates, and non-ASCII text. `--matcher structural` and `--color source` are recognized but report unavailable capabilities, not fallback output. There is no production built-in font: omitting `--font <path>` returns a capability error rather than silently selecting a font. Supplying a path does not yet load or consume it in `convert`; shared font geometry plus the media/export integrations are still required.

The reusable `font::load_glyph_table` helper reads an explicitly supplied local font under a 16 MiB and caller-budget bound, then delegates glyph-table construction to `ascii-convert`. It follows symlinks to regular files and rejects special-file handles before reading. Unix opens with `O_NONBLOCK` and validates the opened handle so a direct or swapped FIFO cannot block; Windows checks the native handle type and rejects pipe/device handles before synchronous reads. Cancellation is checked around file operations and between reads, but cannot interrupt an OS call already in progress. The caller must provide a validated `GlyphConfig`; the current CLI does not provide those metrics or call this helper during conversion.

For a destination other than `-`, the parser infers `.txt`, `.ans`/`.ansi`, or `.png` case-insensitively. An explicit format conflicting with a recognized extension is a usage error; an unknown extension requires `--format`. For `--output -`, omitted format means plain text. `--json --output -` is a usage error. Existing destination entries (including dangling symlinks) require `--overwrite`; that conflict takes precedence over format and charset validation. The check is advisory; publication must also enforce no-clobber at commit time.

```bash
# This request shape is accepted, but export is not integrated and creates no artifact.
asciiwall convert "mountain image.png" --font "font with spaces.ttf" --columns 180 --format txt --output "mountain output.txt"

# Plain-text output may use stdout; JSON status and artwork cannot share stdout.
asciiwall convert mountain.png --font font.ttf --format txt --output -
asciiwall --json convert mountain.png --font font.ttf --format txt --output mountain.txt
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

Text output may use `--output -`; artwork and JSON status are never combined on stdout. Progress and human diagnostics go to stderr. CLI errors use fixed messages and never interpolate input, output, font, config paths, host names, or option values; terminal controls are stripped. Redirected plain-text exports must contain no ANSI escapes, but text export is not integrated in this build.

The current CLI rejects the stdout/JSON conflict and strips terminal control sequences from diagnostics; parser failures are generic and identify the parsed command from Clap's usage context without echoing supplied values. It produces no artwork and does not read the input or selected font. Its standalone publication helper stages output in the destination directory, checks cancellation before commit, and uses a no-clobber operation unless overwrite was explicitly requested; it is not connected to an export. On filesystems without an atomic no-replace rename, the no-clobber fallback may leave an additional temporary link. Integration with the shared export transaction remains necessary.
