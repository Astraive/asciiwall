# Untrusted parser and worker confinement

## Worker responsibilities

The media worker owns untrusted codec parsing and frame extraction. It receives approved handles or a tightly controlled immutable package root, not arbitrary command strings. It returns bounded frame metadata/buffers validated again by the consumer. A process boundary alone is not a sandbox.

Set limits on address space where supported, committed memory, CPU time, concurrent threads, decoded dimensions, frame rate, queue depth, output bytes, and wall-clock job duration. Track global aggregate usage across workers. Kill and replace a nonresponsive worker without terminating the controller.

## Platform restriction plan

Windows: investigate restricted tokens/AppContainer-compatible helper design and Job Objects for resource/process limits. Job Objects alone are not a filesystem/network sandbox. Confirm decoder operation through supplied handles and test denied access to unrelated files/network.

macOS: design a signed restricted helper with the minimum entitlements and explicit file access. Avoid broad home-directory permissions. Validate the actual sandbox/signing behavior instead of assuming a child process inherits the intended restrictions automatically.

Linux: investigate namespaces, seccomp, and filesystem restriction mechanisms available on the supported kernel/package combination. A Flatpak package and a custom worker sandbox solve different boundaries. [S46](../research/source-register.md#s46) Absence of a required isolation primitive must be visible, not silently called secure.

These are implementation spikes with pass/fail tests, not completed sandbox code.

## FFmpeg restriction

FFmpeg supports protocols and nested input mechanisms. [S32](../research/source-register.md#s32) Use a minimal build/feature allowlist and approved custom I/O where practical. Reject playlist/demuxer formats that can reference external files unless a carefully validated package-relative resolver exists. Deny network at the OS level as well as application protocol policy.

Never pass user metadata through a shell to an FFmpeg executable. A bundled helper invoked with argument vectors still needs path/protocol/resource restrictions. Prefer pinned library integration inside the restricted worker where lifecycle and error handling can be controlled.

## Images and fonts

Check dimensions, row strides, pixel-format products, animation frame counts/durations, and allocation overflow. Limits must cover GIF/WebP animated compositing and disposal, not just the first frame. [S27](../research/source-register.md#s27) [S28](../research/source-register.md#s28) Reject unsupported color/HDR features with clear fallback rather than misreading memory layout.

Font parsing is untrusted too. Limit file size, glyph count, raster dimensions, and atlas growth. Reject custom repertoire entries with unsupported width/control behavior. Do not scan all system fonts during package import.

## Defense in depth

Validate outputs even from a worker: width/height/stride consistency, shared-memory extent, pixel format, generation, timestamp, and buffer ownership. A compromised or buggy worker must not make the host read beyond a buffer. Cap error text and never trust decoder-provided filenames.

Fuzz parsers and IPC validators, run sanitizer builds where applicable, and isolate test media. Keep unsafe code small with written invariants and review. A worker crash reports a media failure and preserves the last valid wallpaper where safe.
