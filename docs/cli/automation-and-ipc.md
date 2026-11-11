# CLI automation and local IPC protocol

## No local HTTP server

The CLI talks to the same-user controller over Windows named pipes or Unix-domain sockets, not a TCP port. Use private per-user/session endpoint discovery and restrictive permissions. A local socket is not automatically authenticated: verify peer identity and a session handshake, and avoid endpoint replacement races.

The initial controller owns a single-instance lock. A second UI/CLI instance sends an authenticated Open or command request and exits. A stale lock is repaired only after verifying the previous owner is gone; do not kill an arbitrary PID from a text file.

## Message envelope

Canonical fields: `protocol_version`, `session_id`, `request_id`, `generation`, `operation`, optional `expected_revision` when mutating, and bounded `payload`. Generation zero is for controller-wide commands; assignment-scoped messages use the applicable generation. Responses carry matching request ID, success/error, resulting revision where relevant, and result data. Negotiation rejects incompatible major versions before sending large data.

A four-byte unsigned little-endian length prefix frames each JSON body. Control bodies are capped at 1 MiB with a bounded nesting depth; reject oversized prefixes before allocation. The session ID comes from the authenticated handshake and is not authentication by itself. Large media/cell buffers travel by approved shared-memory/handle mechanisms with explicit ownership and sizes, not base64 inside JSON. Every handle exchange is checked against the connection identity and expected generation.

```json
{
  "protocol_version": 1,
  "session_id": "opaque-handshake-session",
  "request_id": "cli-42",
  "generation": 0,
  "operation": "wallpaper.pause",
  "expected_revision": 12,
  "payload": {"display_id":"local-display-1","reason":"user"}
}
```

The display ID is illustrative and local, not a portable hardware identity. A package cannot send controller commands or choose an IPC endpoint.

## Cancellation and idempotence

Long-running jobs return a job ID and progress events. Ctrl+C requests cancellation, then exits after bounded cleanup. Repeating a request with the same request ID must not duplicate an import/apply operation inside the controller's bounded deduplication window.

Commands that change settings or assignments use revisions. A conflict returns the current revision and safe explanation. Do not overwrite unrelated changes as a convenience fallback.

## Script behavior

Machine-readable mode emits one result object for ordinary commands and documented newline-delimited objects for streaming jobs. Include a schema version, command, status, error code, and result. Human progress never contaminates stdout. `--quiet` does not suppress errors or change exit status.

No command executes arbitrary package hooks. A shell script may invoke the CLI explicitly, but the scene engine does not gain shell access. Scheduled use is through the user's chosen OS scheduler or the bounded local rule system, not a cloud task service.

## Failure handling

Controller disconnect cancels or reports the job's durable state; do not blindly retry non-idempotent operations. Host disconnect transitions assignments to recovery; media-worker disconnect invalidates its generation and releases buffers. Message floods are rate-limited per client and cannot block the UI thread.

## Tests

Wrong user/session, stale endpoint, malformed lengths, unknown operation, protocol mismatch, repeated request IDs, reconnect during Apply, Ctrl+C during export, and simultaneous UI/CLI edits. A diagnostic process must not be able to invoke privileged operations just because it can read a log file.
