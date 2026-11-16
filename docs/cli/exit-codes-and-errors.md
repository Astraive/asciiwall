# Exit codes and structured errors

## Exit status contract

| Code | Symbol | Meaning |
|---:|---|---|
| 0 | ok | Completed successfully; warnings may still exist |
| 2 | usage | Invalid flags/arguments or incompatible output options |
| 3 | validation | Invalid config, package, scene, or unsupported format syntax |
| 4 | capability | Required feature/host/codec unavailable |
| 5 | permission | Access denied or required consent absent |
| 6 | media | Decode/encode/content error |
| 7 | graphics | GPU/surface/presentation failure |
| 8 | io | File read/write, disk-full, or path error |
| 9 | controller | Local controller/IPC unavailable or protocol failure |
| 10 | cancelled | User-requested cancellation after cleanup |
| 11 | resource-limit | Valid-looking input exceeds explicit resource budget |
| 12 | conflict | Revision or package identity conflict |
| 13 | integrity | Hash/signature/provenance validation failure |
| 14 | network | Explicit optional download failed |
| 15 | internal | Unexpected invariant failure; safe diagnostic ID supplied |

The current `convert` command emits usage `2`, validation `3`, capability `4`, permission `5`, media `6`, I/O `8`, cancelled `10`, and resource-limit `11` results. Density-mode text and ANSI conversions can complete without a desktop host. Missing font, PNG export, structural matching, source-color conversion, and settings loading remain explicit capability errors. Existing-output conflicts take precedence over format/charset checks and engine-capability errors, and remain usage `2`.

Cancellation before destination commit returns `10`; decode, conversion, export, or publication failures before commit leave an existing destination unchanged. The output helper stages in the destination directory, checks cancellation before commit, and publishes with a no-clobber operation unless overwrite was explicitly requested.

Successful `--json` conversion writes one schema-versioned status object on stdout and keeps progress/target-loss diagnostics on stderr. The object reports only command, format, and grid dimensions, not local paths. Error JSON also uses `schema_version: 1`, with a stable category/code and a null result.

## Structured result example

```json
{
  "schema_version": 1,
  "command": "package.import",
  "status": "error",
  "error": {
    "code": "package.path-traversal",
    "category": "validation",
    "message": "An archive entry is outside the permitted package path policy.",
    "field": "assets[2].path",
    "retryable": false
  }
}
```

Error strings are human-readable and may evolve/localize. Scripts depend on `code`, category, schema version, and exit code, not matching message text. Unknown error codes remain safely interpretable through the category.

## Error location and redaction

Prefer manifest field paths and package-relative asset names over full local paths. Sanitize control characters and cap embedded text. CLI parser failures use a generic message rather than echoing supplied option values or local paths. Never echo arbitrary metadata as terminal escape sequences or rich HTML. Include native error codes only in optional redacted diagnostics, without process memory or source pixels.

## Recovery examples

Missing source: Locate file, not automatic filesystem crawl. Missing window geometry: disable that optional effect, not request screen capture. Bad signature: stop remote installation and retain current wallpaper. GPU loss: bounded reinitialization then static/preview fallback. Disk full: preserve previous file and remove incomplete staging. Unsupported package format: inspect metadata safely or upgrade manually, not reinterpret it as v1.

## Stability

The CLI schema and error taxonomy need conformance tests and a compatibility policy before release. New error codes can be added within existing categories; changing the meaning of an existing code requires a major protocol/CLI contract change. Exit-code mapping in this document is a proposed contract, not current executable behavior.
