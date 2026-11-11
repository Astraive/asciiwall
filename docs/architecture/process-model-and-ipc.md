# Processes, threads, and local IPC

## Production topology

The controller owns configuration, install transactions, the tray/menu UI, and child supervision. A host process owns native windows/surfaces on the required main thread and a GPU device for a compatible adapter/display group. Media workers own untrusted decoding. The CLI performs standalone conversion in its own process or sends a bounded control request to the controller.

Do not create one process per effect, nor automatically one decoder per monitor. Share an immutable decoded stream when source, playback epoch, and timing are identical; independent playback positions need independent decode state. Separate GPUs may require separate hosts. Resource budgets apply to the whole application, not only to each child.

## Thread ownership

Native event loops and AppKit objects stay on the required owning thread. Conversion runs in a bounded CPU pool; media I/O/decode runs outside the UI loop; the renderer only receives completed frames. Mouse and window callbacks copy minimal metadata into bounded queues and return. No expensive conversion, filesystem hashing, or blocking RPC occurs in an OS callback. [S04](../research/source-register.md#s04) [S52](../research/source-register.md#s52)

## Transport contract

Use Unix-domain sockets with restrictive directory/socket permissions and peer checks, or Windows named pipes restricted to the current user/session. Never open a TCP/HTTP control port. The endpoint is per interactive user and session; elevated and ordinary sessions must not accidentally share unrestricted control.

The canonical control envelope carries `protocol_version`, `session_id`, `request_id`, `generation`, `operation`, optional `expected_revision`, and `payload`, matching the [CLI IPC contract](../cli/automation-and-ipc.md). A four-byte unsigned little-endian transport prefix carries the JSON body length; `payload_length` is not a second independently trusted JSON field. Reject a length over 1 MiB before allocating the body. Generation zero denotes controller-wide commands; assignment-scoped messages carry the applicable generation.

The session ID is issued by the authenticated handshake and is not proof of identity by itself. JSON is acceptable for bounded control messages; large image buffers use a bounded shared-memory pool or explicit chunked transfer, not base64 JSON.

Frame descriptors carry slot id, generation, dimensions, stride, format, byte length, media timestamp, and ownership state. The receiver validates multiplication/offset bounds against the mapped region. A producer cannot reuse a slot until release is acknowledged. A crash invalidates the connection generation and all outstanding descriptors.

## Reliability

Commands are acknowledged with accepted/rejected status and a request id. Long operations produce progress and an explicit final result. Applying the same idempotency key twice must not install two copies or create two wallpaper hosts. A newly launched CLI cannot make a stale controller process appear current merely because an old socket file exists.

Host restart uses exponential backoff and a circuit breaker, for example at most three automatic retries in five minutes. Crash loops disable the failing assignment while preserving other displays. These numbers are policy defaults, not measured reliability thresholds.

## Security boundary

Process isolation improves crash containment, but an unrestricted child still inherits user privileges. Production worker confinement must restrict handles, filesystem roots, network access, and resource use using each OS's available facilities. A portable fallback without equivalent confinement must be labeled reduced isolation and may disable untrusted community video import until reviewed.
