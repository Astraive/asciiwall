# Video decoding, timing, and animated image behavior

## Decoder boundary

A decoder presents stream metadata and frames with presentation timestamps, durations, color metadata, orientation, and buffer ownership. Encapsulate all FFmpeg FFI types. A media worker accepts only approved local handles or sandboxed package assets, not arbitrary URL strings.

Correct FFmpeg use requires handling send/receive backpressure, zero-or-many frames per packet, end-of-stream draining, and decoder reset. A sleep loop around an incorrect EAGAIN sequence is not a fix. [S31](../research/source-register.md#s31)

## Presentation clock

Choose a monotonic playback clock with a media epoch. Convert rational stream timestamps carefully with overflow-safe rescaling. Present the newest due frame while respecting variable frame durations. Do not derive the whole stream from a nominal FPS when timestamps are available. Missing, negative, repeated, or nonmonotonic timestamps need a documented repair policy and diagnostics.

Playback speed maps media time to runtime time; pause freezes that mapping. After resume, rebase the epoch instead of accumulating scheduling drift. A maximum output FPS controls presentation/conversion work, not necessarily all compressed-stream decode work: dependent video frames may still need decoding before a later frame is usable.

## Queues and dropping

Use small bounded queues, initially 2-3 completed frames per active stream. Drop obsolete presentation/conversion work by generation, never randomly discard compressed reference packets. Backpressure should limit decoder advance. Seek cancels pending work, seeks to an appropriate decode point, flushes decoder state, decodes to the target, and clears temporal conversion history.

Looping drains correctly and returns to the chosen loop start. A seamless-loop option cannot make discontinuous source content inherently seamless; optional crossfade uses two prepared segments and a memory budget.

## Animated images

GIF/WebP/APNG support must honor frame durations, disposal, blend behavior, loop counts, and the composed canvas rather than treating each frame as a standalone image. Extremely short/zero durations require a documented floor to prevent a runaway loop. Local preview and export should use the same normalized animation timeline.

## Hardware decoding

Begin with a well-tested CPU decode path and downsample to the needed analysis resolution. Evaluate hardware decode separately on each backend. Hardware-to-CPU readback can erase a speed gain when conversion remains CPU-based. Zero-copy interop across FFmpeg and wgpu is a future platform-specific optimization, not a version-one guarantee.

## Audio and format support

Wallpaper video is muted by default. Enabling audio playback, audio analysis, and microphone/system-audio capture are three distinct features. Initial video export can omit audio with a clear notice. Codec support comes from the exact shipped build and is reported by `doctor`; MP4/MKV/WebM are containers, not proof that every contained codec is supported.

## Security

Restrict protocol access at the FFmpeg boundary and disallow playlists/manifests that resolve extra files or network resources unless a future explicit format supports them. Protocol allowlisting alone does not constrain every local file path; use approved handles, bounded custom I/O, and worker confinement. [S32](../research/source-register.md#s32)
