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

`ascii-media::decode_animation` currently supports APNG only: 8-bit samples whose default PNG
image is also the first animation frame. Partial frame rectangles are checked against the declared
canvas; source and source-over blending plus none/background/previous disposal are composed before
publishing a frame. The decoder applies PNG orientation once to the full canvas and retains each
frame as a budgeted `DecodedFrame` with straight-alpha sRGB metadata.

The shared `Animation` timeline assigns every frame at least 10 ms, treats APNG denominator zero
as 100, and rejects a normalized loop longer than 24 hours or over 256 frames. APNG `num_plays=0`
means infinite; positive values are total finite plays. `Animation::frame_at` is the shared
preview/export selection primitive and holds the last frame after a finite animation ends.
Encoded input, codec scratch, canvas, one previous-canvas restore buffer, and retained full-canvas
history are bounded by `DecodeLimits`, `Budget`, and `DecodeResources`; cancellation is checked
between frames and composition rows.

16-bit APNG and APNG with a separate default image, GIF, and animated WebP remain disabled and
return typed unsupported errors. WebP is also disabled for still decoding until the pinned decoder's
allocation behavior is bounded by the resource ledger. No CLI doctor/import surface exists yet, and
the library API is not currently wired to application preview/export; those acceptance items remain
open. Do not infer support from the format's extension or from still-image capabilities.
## Hardware decoding

Begin with a well-tested CPU decode path and downsample to the needed analysis resolution. Evaluate hardware decode separately on each backend. Hardware-to-CPU readback can erase a speed gain when conversion remains CPU-based. Zero-copy interop across FFmpeg and wgpu is a future platform-specific optimization, not a version-one guarantee.

## Audio and format support

Wallpaper video is muted by default. Enabling audio playback, audio analysis, and microphone/system-audio capture are three distinct features. Initial video export can omit audio with a clear notice. Codec support comes from the exact shipped build and is reported by `doctor`; MP4/MKV/WebM are containers, not proof that every contained codec is supported.

## Security

Restrict protocol access at the FFmpeg boundary and disallow playlists/manifests that resolve extra files or network resources unless a future explicit format supports them. Protocol allowlisting alone does not constrain every local file path; use approved handles, bounded custom I/O, and worker confinement. [S32](../research/source-register.md#s32)
