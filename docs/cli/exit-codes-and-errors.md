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

No success exit for a partially written archive. For batch operations, return a nonzero aggregate code if any item failed and include each result in JSON. Define deterministic error precedence: integrity/security before general validation, then resource, permission, capability, media, graphics, I/O, controller/network, internal. Cancellation before any commit returns 10; completed outputs remain listed explicitly.

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

Prefer manifest field paths and package-relative asset names over full local paths. Sanitize control characters and cap embedded text. Never echo arbitrary metadata as terminal escape sequences or rich HTML. Include native error codes only in optional redacted diagnostics, without process memory or source pixels.

## Recovery examples

Missing source: Locate file, not automatic filesystem crawl. Missing window geometry: disable that optional effect, not request screen capture. Bad signature: stop remote installation and retain current wallpaper. GPU loss: bounded reinitialization then static/preview fallback. Disk full: preserve previous file and remove incomplete staging. Unsupported package format: inspect metadata safely or upgrade manually, not reinterpret it as v1.

## Stability

The CLI schema and error taxonomy need conformance tests and a compatibility policy before release. New error codes can be added within existing categories; changing the meaning of an existing code requires a major protocol/CLI contract change. Exit-code mapping in this document is a proposed contract, not current executable behavior.
