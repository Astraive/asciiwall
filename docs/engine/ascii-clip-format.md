# Precomputed ASCII clip format - draft version 1

This is a proposed internal portable character-animation format, separate from the outer `.asciiwall` ZIP package. Its suggested asset extension is `.ascii-clip`. It is not implemented by this documentation delivery.

## Purpose

Store timestamps and character grids without repeatedly decoding/converting the original raster video. Retain glyph identities and colors for compatible live effects. A clip does not retain the original source's spatial detail; a new sampling resolution still needs regeneration from source.

## Fixed header

All integers are little-endian; no native Rust layout is serialized. Header length is 64 bytes.

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 8 | ASCII magic `ASCIICL1` |
| 8 | 2 | format version, 1 |
| 10 | 2 | flags, zero in v1 |
| 12 | 4 | header length, 64 |
| 16 | 4 | columns |
| 20 | 4 | rows |
| 24 | 4 | glyph count |
| 28 | 4 | time-base numerator, 1 |
| 32 | 4 | time-base denominator, 1000000 |
| 36 | 8 | frame count |
| 44 | 8 | glyph-table offset |
| 52 | 8 | frame-index offset |
| 60 | 4 | reserved, zero |

Glyph-table entries are `u16 byte_length` followed by UTF-8 bytes. Version one accepts only supported single-cell glyphs, length 1-32 bytes, with no control sequences. The table must contain exactly the declared count. Font identity and license information remain in the enclosing package manifest/settings.

## Frame index

Each entry is 40 bytes: `pts_us:i64`, `duration_us:u64`, `payload_offset:u64`, `stored_length:u32`, `raw_length:u32`, `codec:u8`, `frame_kind:u8`, `reserved:u16`, `crc32:u32`. `codec=0` is uncompressed and mandatory; `codec=1` is zstd and requires the declared clip feature. `frame_kind=0` is a complete frame. Delta frames are not part of v1.

Each raw cell is 12 bytes: `glyph_index:u32`, four foreground RGBA bytes, four background RGBA bytes. `raw_length` must equal checked `columns * rows * 12`. CRC32 detects accidental corruption only; package/catalog hashes provide stronger integrity and are not author authentication by themselves.

## Validation

Require positive dimensions, positive duration, increasing presentation timestamps, valid glyph references, bounded frame/glyph counts, nonoverlapping protected header/index regions, and every offset/length inside the file. Reject integer overflow, unknown required flags, truncated data, decompression overruns, unsupported codec ids, and index/payload overlap. Bound total decoded memory independently of compressed bytes.

The baseline reader caps one frame at 1,048,576 cells and glyph tables at 4096 entries, while user-facing quality profiles are much lower. Admission also checks a whole-job memory budget; a syntactically valid clip can still be too expensive to play.

## Seeking and export

Complete-frame indexing makes seeking straightforward and corruption isolation simpler than delta chains. This trades storage for implementation safety. Build a future delta extension only after measurements justify it. Export frame `i` at a rationally computed timestamp; avoid repeatedly adding a rounded duration and accumulating drift.

A 240x68 grid uses 195,840 raw bytes per frame; 30 FPS is about 5.88 MB/s before compression. This arithmetic is not a compression benchmark, and it explains why clips are not automatically smaller than ordinary video.
