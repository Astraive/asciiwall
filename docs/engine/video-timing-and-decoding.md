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

`ascii-media::decode_animation` supports 8-bit APNG whose default PNG image is also the first
animation frame, plus GIF89a/GIF87a animations. APNG partial rectangles are checked against the
canvas; source/source-over blending and none/background/previous disposal are composed before
publication, with PNG orientation applied once to the full canvas. GIF uses the pinned `gif` 0.14.0
low-level decoder, checks every frame rectangle, preserves palette transparency, and composes
keep/any, background-color, and previous disposal onto the logical-screen canvas. Both formats
publish straight-alpha sRGB `DecodedFrame`s, never subframes.
GIF ICC-profile and user-input-dependent variants return typed unsupported errors.

The shared `Animation` timeline floors every frame at 10 ms and rejects a normalized loop longer
than 24 hours or over 256 frames. APNG denominator zero is 100 and positive `num_plays` is total
plays. GIF delays use 10 ms units; a finite GIF repeat count is repetitions after the initial play,
while no repeat extension means one play and repeat zero means infinite. `Animation::frame_at` is
the common preview/export selection primitive and holds the last frame after finite playback.
Encoded input, GIF per-frame decoder scratch, canvas, previous-rectangle restore, and retained
full-canvas history are bounded by `DecodeLimits`, `Budget`, and `DecodeResources`; cancellation is
checked between frames, compressed-input refills, and composition/normalization rows.

GIF still-image decoding remains disabled. 16-bit and separate-default APNG, animated WebP, and all
WebP pixel decoding remain disabled; WebP's pinned lossless meta-code can allocate unbounded Huffman
groups outside the memory ledger. No CLI doctor/import surface exists yet, and the library API is
not wired to application preview/export; those acceptance items remain open. Do not infer support
from a file extension or still-image capability.

### Reproducible animation fixtures

The generated fixture inputs and generator settings are pinned by SHA-256 in
`ascii-media/tests/animation_decode.rs`:

- GIF settings `dc797df0c1e556719cea89374be0017617b45ac9abb97cd7acb4ca71f65b1092`;
  fixture `d5a5dc5f85f99530aab8862e84a9c5b4c8536dec155cda61bc1bb013edf8be29`.
- APNG settings `c4cd033e2aa48663db855d031731297fa3e857cc0efe5dabec9452d03bcff237`;
  fixture `77d6a525c74475921c8e47569d5fbb60ed90da6253ad71773e119d744492b453`.
- First-frame `Previous` disposal mutation settings
  `51679d6525c606c817699ca1203051994a9f12515add008f7f68ce2d9e1eb143`;
  fixture `7d21b8a38929c127e58a73c3d9da9d37ffd33acc4c8f7b84bbdd29ffe12a0719`.

## Hardware decoding

Begin with a well-tested CPU decode path and downsample to the needed analysis resolution. Evaluate hardware decode separately on each backend. Hardware-to-CPU readback can erase a speed gain when conversion remains CPU-based. Zero-copy interop across FFmpeg and wgpu is a future platform-specific optimization, not a version-one guarantee.

## Audio and format support

Wallpaper video is muted by default. Enabling audio playback, audio analysis, and microphone/system-audio capture are three distinct features. Initial video export can omit audio with a clear notice. Codec support comes from the exact shipped build and is reported by `doctor`; MP4/MKV/WebM are containers, not proof that every contained codec is supported.

## Security

Restrict protocol access at the FFmpeg boundary and disallow playlists/manifests that resolve extra files or network resources unless a future explicit format supports them. Protocol allowlisting alone does not constrain every local file path; use approved handles, bounded custom I/O, and worker confinement. [S32](../research/source-register.md#s32)
