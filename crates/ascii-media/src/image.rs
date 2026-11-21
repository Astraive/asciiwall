//! Content-sniffed PNG/JPEG decoding into owned normalized rasters.

use std::io::{self, BufReader, Read, Seek, SeekFrom};

use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, Error, MalformedReason, PixelChannels,
    UnsupportedReason,
};
use image::metadata::Orientation;
use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::{JpegDecoder, errors::DecodeErrors};

use crate::DecodedFrame;
use crate::decoded_frame::DecodedFrameData;
use crate::resources::{DecodeResources, MemoryPermit};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const INPUT_PREFIX_BYTES: usize = 12;
/// Maximum bytes requested from caller streams and buffered between codec input checks.
const CODEC_READER_BUFFER_BYTES: usize = 128;
const NORMALIZATION_CANCEL_INTERVAL_PIXELS: usize = 128;
const PNG_FIXED_SCRATCH_BYTES: u64 = 128 * 1024;
const JPEG_FIXED_SCRATCH_BYTES: u64 = 64 * 1024;
const JPEG_EXIF_SCRATCH_BYTES: u64 = 64 * 1024;
const PNG_CRC_WORK_PER_BYTE: u64 = 8;
const PNG_SRGB_GAMMA: u32 = 45_455;
const PNG_SRGB_CHROMATICITIES: [u32; 8] = [
    31_270, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 6_000,
];

/// A caller-supplied seekable stream. The decoder never opens paths or URLs.
pub trait ReadSeek: Read + Seek {}

impl<T: Read + Seek + ?Sized> ReadSeek for T {}

/// Per-operation decoder and codec limits.
#[derive(Clone, Debug)]
pub struct DecodeLimits {
    /// Core per-operation raster and work ceiling.
    pub budget: Budget,
    /// Shared ledger; clone and reuse it across all decodes in the same job or process.
    pub resources: DecodeResources,
    /// Maximum permitted width and height after EXIF/display orientation.
    pub max_width: u32,
    pub max_height: u32,
    /// Maximum encoded stream bytes accepted from the caller's current position.
    pub max_encoded_bytes: u64,
    /// Maximum simultaneous codec scratch and temporary decoded-source bytes.
    /// The owned encoded snapshot and retained output are charged separately to `budget` and
    /// `resources`.
    pub max_decoder_bytes: u64,
}

impl DecodeLimits {
    #[must_use]
    pub const fn new(
        budget: Budget,
        resources: DecodeResources,
        max_width: u32,
        max_height: u32,
        max_encoded_bytes: u64,
        max_decoder_bytes: u64,
    ) -> Self {
        Self {
            budget,
            resources,
            max_width,
            max_height,
            max_encoded_bytes,
            max_decoder_bytes,
        }
    }
}

/// Decodes an owned, size-bounded snapshot of a PNG or JPEG stream.
///
/// PNG input is fully checked through its final IEND chunk and CRCs before publication. JPEG
/// input is parsed through EOI, then decoded by pure-Rust `zune-jpeg` with strict errors, bounded
/// dimensions, and a scan ceiling.
///
/// During image-data decoding, cancellation is checked at compressed-input refills, with at most
/// 128 compressed bytes buffered between checks. Work over already-buffered data is not
/// preempted; cancellation is also checked after each codec call. Both codecs receive preflighted
/// scratch allowances; codec crate limits are additional defense in depth.
///
/// Caller-stream snapshot reads request no more than 128 bytes per `Read` and check cancellation
/// between completed calls. A blocking caller `Read` cannot be interrupted by this API.
///
/// Normalization checks cancellation at row boundaries and every 128 pixels within each row.
///
/// Output is top-down, tightly packed 8-bit RGB or straight-alpha RGBA. Untagged input is
/// assumed sRGB. PNGs with `gAMA` or `cHRM` but no `sRGB` declaration, and embedded ICC profiles,
/// are rejected rather than silently mislabeled. JPEG EXIF display orientation is applied once.
pub fn decode_image(
    input: &mut dyn ReadSeek,
    limits: &DecodeLimits,
    cancel: &CancellationToken,
) -> Result<DecodedFrame, Error> {
    decode_image_with_jpeg_poller(input, limits, cancel, |offset| {
        check_cancel_periodically(cancel, offset)
    })
}

fn decode_image_with_jpeg_poller<F>(
    input: &mut dyn ReadSeek,
    limits: &DecodeLimits,
    cancel: &CancellationToken,
    jpeg_cancel_poller: F,
) -> Result<DecodedFrame, Error>
where
    F: FnMut(usize) -> Result<(), Error>,
{
    decode_image_with_pollers(
        input,
        limits,
        cancel,
        jpeg_cancel_poller,
        || check_cancel(cancel),
        zeroed_bytes,
    )
}

fn decode_image_with_pollers<F, G, A>(
    input: &mut dyn ReadSeek,
    limits: &DecodeLimits,
    cancel: &CancellationToken,
    mut jpeg_cancel_poller: F,
    mut normalization_cancel_poller: G,
    mut allocate_output: A,
) -> Result<DecodedFrame, Error>
where
    F: FnMut(usize) -> Result<(), Error>,
    G: FnMut() -> Result<(), Error>,
    A: FnMut(usize) -> Result<Vec<u8>, Error>,
{
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if limits.max_width == 0
        || limits.max_height == 0
        || limits.max_encoded_bytes == 0
        || limits.max_decoder_bytes == 0
    {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }

    let _decode_permit = limits.resources.try_acquire_decode()?;
    let (start, encoded_len) = inspect_extent(input, limits.max_encoded_bytes, cancel)?;
    if encoded_len == 0 {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    if encoded_len > limits.budget.max_raster_bytes() {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
    }

    let mut prefix = [0_u8; INPUT_PREFIX_BYTES];
    let prefix_len = read_prefix(input, start, encoded_len, &mut prefix, cancel)?;
    let codec = sniff_format(&prefix[..prefix_len])?;
    if limits.max_decoder_bytes < CODEC_READER_BUFFER_BYTES as u64 {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
    }
    let header_work = match codec {
        Codec::Png => encoded_len
            .checked_mul(PNG_CRC_WORK_PER_BYTE)
            .ok_or(Error::SizeOverflow)?,
        Codec::Jpeg => encoded_len,
    };
    if header_work > limits.budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }

    let snapshot_permit = limits.resources.try_reserve_memory(encoded_len)?;
    let snapshot_len = usize::try_from(encoded_len).map_err(|_| Error::SizeOverflow)?;
    let mut snapshot = zeroed_bytes(snapshot_len)?;
    snapshot[..prefix_len].copy_from_slice(&prefix[..prefix_len]);
    read_snapshot_remainder(input, &mut snapshot[prefix_len..], cancel)?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let result = match codec {
        Codec::Png => decode_png(&snapshot, limits, cancel, &mut normalization_cancel_poller),
        Codec::Jpeg => decode_jpeg_with_reader_and_pollers_and_output_allocator(
            &snapshot,
            limits,
            cancel,
            SnapshotReader::new(&snapshot, cancel),
            &mut jpeg_cancel_poller,
            &mut normalization_cancel_poller,
            &mut allocate_output,
        ),
    };
    drop(snapshot);
    drop(snapshot_permit);
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Codec {
    Png,
    Jpeg,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SampleLayout {
    Gray8,
    GrayAlpha8,
    Rgb8,
    Rgba8,
    Gray16,
    GrayAlpha16,
    Rgb16,
    Rgba16,
}

impl SampleLayout {
    const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Gray8 => 1,
            Self::GrayAlpha8 => 2,
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
            Self::Gray16 => 2,
            Self::GrayAlpha16 => 4,
            Self::Rgb16 => 6,
            Self::Rgba16 => 8,
        }
    }

    const fn has_alpha(self) -> bool {
        matches!(
            self,
            Self::GrayAlpha8 | Self::Rgba8 | Self::GrayAlpha16 | Self::Rgba16
        )
    }

    const fn is_rgb(self) -> bool {
        matches!(self, Self::Rgb8 | Self::Rgba8 | Self::Rgb16 | Self::Rgba16)
    }

    const fn is_sixteen_bit(self) -> bool {
        matches!(
            self,
            Self::Gray16 | Self::GrayAlpha16 | Self::Rgb16 | Self::Rgba16
        )
    }

    const fn output_channels(self) -> usize {
        if self.has_alpha() { 4 } else { 3 }
    }
}

#[derive(Clone, Copy, Debug)]
struct RasterPlan {
    source_width: u32,
    source_height: u32,
    output_width: u32,
    output_height: u32,
    row_stride: usize,
    source_row_stride: usize,
    source_bytes: u64,
    output_bytes: u64,
    temporary_source_bytes: u64,
    pixel_count: u64,
    layout: SampleLayout,
    orientation: Orientation,
}

fn plan_raster(
    source_width: u32,
    source_height: u32,
    layout: SampleLayout,
    orientation: Orientation,
    limits: &DecodeLimits,
) -> Result<RasterPlan, Error> {
    if source_width == 0 || source_height == 0 {
        return Err(Error::Malformed(MalformedReason::EmptyDimensions));
    }
    let (output_width, output_height) = if orientation_swaps_axes(orientation) {
        (source_height, source_width)
    } else {
        (source_width, source_height)
    };
    if output_width > limits.max_width || output_height > limits.max_height {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }

    let source_width_usize = usize::try_from(source_width).map_err(|_| Error::SizeOverflow)?;
    let source_height_usize = usize::try_from(source_height).map_err(|_| Error::SizeOverflow)?;
    let source_row_stride = source_width_usize
        .checked_mul(layout.bytes_per_pixel())
        .ok_or(Error::SizeOverflow)?;
    let source_len = source_row_stride
        .checked_mul(source_height_usize)
        .ok_or(Error::SizeOverflow)?;
    let source_layout =
        std::alloc::Layout::array::<u8>(source_len).map_err(|_| Error::SizeOverflow)?;

    let output_width_usize = usize::try_from(output_width).map_err(|_| Error::SizeOverflow)?;
    let output_height_usize = usize::try_from(output_height).map_err(|_| Error::SizeOverflow)?;
    let output_channels = layout.output_channels();
    let row_stride = output_width_usize
        .checked_mul(output_channels)
        .ok_or(Error::SizeOverflow)?;
    let output_len = row_stride
        .checked_mul(output_height_usize)
        .ok_or(Error::SizeOverflow)?;
    let output_layout =
        std::alloc::Layout::array::<u8>(output_len).map_err(|_| Error::SizeOverflow)?;
    let pixel_count = u64::from(source_width)
        .checked_mul(u64::from(source_height))
        .ok_or(Error::SizeOverflow)?;
    let source_bytes = u64::try_from(source_layout.size()).map_err(|_| Error::SizeOverflow)?;
    let output_bytes = u64::try_from(output_layout.size()).map_err(|_| Error::SizeOverflow)?;
    let direct_output = orientation == Orientation::NoTransforms
        && matches!(layout, SampleLayout::Rgb8 | SampleLayout::Rgba8)
        && source_bytes == output_bytes;

    Ok(RasterPlan {
        source_width,
        source_height,
        output_width,
        output_height,
        row_stride,
        source_row_stride,
        source_bytes,
        output_bytes,
        temporary_source_bytes: if direct_output { 0 } else { source_bytes },
        pixel_count,
        layout,
        orientation,
    })
}

fn reserve_plan(
    plan: RasterPlan,
    encoded_bytes: u64,
    scratch_bytes: u64,
    work_units: u64,
    limits: &DecodeLimits,
) -> Result<(MemoryPermit, Option<MemoryPermit>, MemoryPermit), Error> {
    let local_decoder_bytes = scratch_bytes
        .checked_add(plan.temporary_source_bytes)
        .ok_or(Error::SizeOverflow)?;
    if local_decoder_bytes > limits.max_decoder_bytes {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
    }
    let job_bytes = encoded_bytes
        .checked_add(local_decoder_bytes)
        .and_then(|bytes| bytes.checked_add(plan.output_bytes))
        .ok_or(Error::SizeOverflow)?;
    if job_bytes > limits.budget.max_raster_bytes() {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
    }
    if work_units > limits.budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }

    let scratch_permit = limits.resources.try_reserve_memory(scratch_bytes)?;
    let source_permit = if plan.temporary_source_bytes == 0 {
        None
    } else {
        Some(
            limits
                .resources
                .try_reserve_memory(plan.temporary_source_bytes)?,
        )
    };
    let output_permit = limits.resources.try_reserve_memory(plan.output_bytes)?;
    Ok((scratch_permit, source_permit, output_permit))
}

fn decode_png(
    encoded: &[u8],
    limits: &DecodeLimits,
    cancel: &CancellationToken,
    normalization_cancel_poller: &mut impl FnMut() -> Result<(), Error>,
) -> Result<DecodedFrame, Error> {
    decode_png_with_reader_and_normalization_poller(
        encoded,
        limits,
        cancel,
        SnapshotReader::new(encoded, cancel),
        normalization_cancel_poller,
    )
}

#[cfg(test)]
fn decode_png_with_reader<R: Read + Seek>(
    encoded: &[u8],
    limits: &DecodeLimits,
    cancel: &CancellationToken,
    codec_source: R,
) -> Result<DecodedFrame, Error> {
    let mut normalization_cancel_poller = || check_cancel(cancel);
    decode_png_with_reader_and_normalization_poller(
        encoded,
        limits,
        cancel,
        codec_source,
        &mut normalization_cancel_poller,
    )
}

fn decode_png_with_reader_and_normalization_poller<R, F>(
    encoded: &[u8],
    limits: &DecodeLimits,
    cancel: &CancellationToken,
    codec_source: R,
    normalization_cancel_poller: &mut F,
) -> Result<DecodedFrame, Error>
where
    R: Read + Seek,
    F: FnMut() -> Result<(), Error>,
{
    let header = inspect_png(encoded, cancel)?;
    if let Some(reason) = header.unsupported {
        return Err(Error::Unsupported(reason));
    }
    let layout = header.sample_layout()?;
    let plan = plan_raster(
        header.width,
        header.height,
        layout,
        header.orientation,
        limits,
    )?;
    let scratch_bytes = png_scratch_bound(&header, plan)?;
    let encoded_bytes = u64::try_from(encoded.len()).map_err(|_| Error::SizeOverflow)?;
    let work_units = png_work_units(encoded_bytes, plan)?;
    let (_scratch_permit, _source_permit, output_permit) =
        reserve_plan(plan, encoded_bytes, scratch_bytes, work_units, limits)?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let buffered = BufReader::with_capacity(CODEC_READER_BUFFER_BYTES, codec_source);
    let png_limits = png::Limits {
        bytes: usize::try_from(scratch_bytes).map_err(|_| Error::SizeOverflow)?,
    };
    let mut decoder = png::Decoder::new_with_limits(buffered, png_limits);
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder
        .read_info()
        .map_err(|error| map_png_error(error, cancel))?;

    let (actual_layout, actual_depth) = png_output_layout(reader.output_color_type())?;
    if actual_layout != layout
        || actual_depth != layout.is_sixteen_bit()
        || reader.info().width != header.width
        || reader.info().height != header.height
        || reader.output_buffer_size()
            != Some(usize::try_from(plan.source_bytes).map_err(|_| Error::SizeOverflow)?)
    {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }

    let mut output =
        zeroed_bytes(usize::try_from(plan.output_bytes).map_err(|_| Error::SizeOverflow)?)?;
    let mut source = if plan.temporary_source_bytes == 0 {
        None
    } else {
        Some(zeroed_bytes(
            usize::try_from(plan.source_bytes).map_err(|_| Error::SizeOverflow)?,
        )?)
    };
    let frame_info = match source.as_mut() {
        Some(source) => reader.next_frame(source),
        None => reader.next_frame(&mut output),
    }
    .map_err(|error| map_png_error(error, cancel))?;
    if frame_info.width != header.width
        || frame_info.height != header.height
        || frame_info.line_size != plan.source_row_stride
    {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    reader
        .finish()
        .map_err(|error| map_png_error(error, cancel))?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    if let Some(source) = source.as_ref() {
        normalize_pixels(source, &mut output, plan, true, normalization_cancel_poller)?;
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let (channels, alpha) = if layout.has_alpha() {
        (PixelChannels::Rgba, AlphaMode::Straight)
    } else {
        (PixelChannels::Rgb, AlphaMode::Opaque)
    };
    DecodedFrame::new(
        DecodedFrameData {
            width: plan.output_width,
            height: plan.output_height,
            row_stride: plan.row_stride,
            channels,
            alpha,
            pixels: output,
            memory_permit: output_permit,
        },
        &limits.budget,
        cancel,
    )
}

#[derive(Clone, Copy, Debug)]
struct PngHeader {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    max_non_idat_chunk_bytes: usize,
    palette_entries: usize,
    transparency_len: Option<usize>,
    orientation: Orientation,
    has_srgb: bool,
    gamma: Option<u32>,
    chromaticities: Option<[u32; 8]>,
    unsupported: Option<UnsupportedReason>,
}

impl PngHeader {
    fn sample_layout(self) -> Result<SampleLayout, Error> {
        let transparent = self.transparency_len.is_some();
        let layout = match (self.color_type, self.bit_depth, transparent) {
            (0, 1 | 2 | 4 | 8, false) => SampleLayout::Gray8,
            (0, 1 | 2 | 4 | 8, true) => SampleLayout::GrayAlpha8,
            (0, 16, false) => SampleLayout::Gray16,
            (0, 16, true) => SampleLayout::GrayAlpha16,
            (2, 8, false) => SampleLayout::Rgb8,
            (2, 8, true) => SampleLayout::Rgba8,
            (2, 16, false) => SampleLayout::Rgb16,
            (2, 16, true) => SampleLayout::Rgba16,
            (3, _, false) => SampleLayout::Rgb8,
            (3, _, true) => SampleLayout::Rgba8,
            (4, 8, _) => SampleLayout::GrayAlpha8,
            (4, 16, _) => SampleLayout::GrayAlpha16,
            (6, 8, _) => SampleLayout::Rgba8,
            (6, 16, _) => SampleLayout::Rgba16,
            _ => return Err(Error::Unsupported(UnsupportedReason::SampleDepth)),
        };
        Ok(layout)
    }
}

fn inspect_png(encoded: &[u8], cancel: &CancellationToken) -> Result<PngHeader, Error> {
    if encoded.len() < PNG_SIGNATURE.len() || &encoded[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let mut header = PngHeader {
        width: 0,
        height: 0,
        bit_depth: 0,
        color_type: 0,
        max_non_idat_chunk_bytes: 0,
        palette_entries: 0,
        transparency_len: None,
        orientation: Orientation::NoTransforms,
        has_srgb: false,
        gamma: None,
        chromaticities: None,
        unsupported: None,
    };
    let mut offset = PNG_SIGNATURE.len();
    let mut seen_ihdr = false;
    let mut seen_plte = false;
    let mut seen_trns = false;
    let mut seen_exif = false;
    let mut seen_idat = false;
    let mut idat_ended = false;
    let mut seen_iend = false;

    while offset < encoded.len() {
        check_cancel_periodically(cancel, offset)?;
        let fixed_end = offset.checked_add(12).ok_or(Error::SizeOverflow)?;
        if fixed_end > encoded.len() {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        let chunk_len_u32 = u32::from_be_bytes([
            encoded[offset],
            encoded[offset + 1],
            encoded[offset + 2],
            encoded[offset + 3],
        ]);
        let chunk_len = usize::try_from(chunk_len_u32).map_err(|_| Error::SizeOverflow)?;
        let payload_start = offset.checked_add(8).ok_or(Error::SizeOverflow)?;
        let payload_end = payload_start
            .checked_add(chunk_len)
            .ok_or(Error::SizeOverflow)?;
        let chunk_end = payload_end.checked_add(4).ok_or(Error::SizeOverflow)?;
        if chunk_end > encoded.len() {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        let chunk_type: [u8; 4] = encoded[offset + 4..offset + 8]
            .try_into()
            .map_err(|_| Error::Malformed(MalformedReason::InvalidEncodedData))?;
        if &chunk_type != b"IDAT" {
            header.max_non_idat_chunk_bytes = header.max_non_idat_chunk_bytes.max(chunk_len);
        }
        let payload = &encoded[payload_start..payload_end];
        let expected_crc = u32::from_be_bytes(
            encoded[payload_end..chunk_end]
                .try_into()
                .map_err(|_| Error::Malformed(MalformedReason::InvalidEncodedData))?,
        );
        if png_crc32(&encoded[offset + 4..payload_end], cancel)? != expected_crc {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        if !chunk_type.iter().all(u8::is_ascii_alphabetic) || chunk_type[2].is_ascii_lowercase() {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        if !seen_ihdr {
            if &chunk_type != b"IHDR" || chunk_len != 13 {
                return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
            }
            seen_ihdr = true;
        } else if &chunk_type == b"IHDR" {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        if seen_idat && &chunk_type != b"IDAT" {
            idat_ended = true;
        }
        if &chunk_type == b"IDAT" {
            if idat_ended {
                return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
            }
            seen_idat = true;
        }

        match &chunk_type {
            b"IHDR" => {
                header.width = u32::from_be_bytes(
                    payload[0..4]
                        .try_into()
                        .map_err(|_| Error::Malformed(MalformedReason::InvalidEncodedData))?,
                );
                header.height = u32::from_be_bytes(
                    payload[4..8]
                        .try_into()
                        .map_err(|_| Error::Malformed(MalformedReason::InvalidEncodedData))?,
                );
                header.bit_depth = payload[8];
                header.color_type = payload[9];
                if payload[10] != 0 || payload[11] != 0 || payload[12] > 1 {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                if header.width == 0 || header.height == 0 {
                    return Err(Error::Malformed(MalformedReason::EmptyDimensions));
                }
                if header.width > 0x7fff_ffff || header.height > 0x7fff_ffff {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                validate_png_depth(header.color_type, header.bit_depth)?;
            }
            b"PLTE" => {
                if seen_plte || seen_idat || chunk_len == 0 || chunk_len > 768 || chunk_len % 3 != 0
                {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                if matches!(header.color_type, 0 | 4) {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                seen_plte = true;
                header.palette_entries = chunk_len / 3;
            }
            b"tRNS" => {
                if seen_trns || seen_idat {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                seen_trns = true;
                header.transparency_len = Some(chunk_len);
            }
            b"gAMA" => {
                if header.gamma.is_some() || seen_idat || chunk_len != 4 {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                let gamma = u32::from_be_bytes(
                    payload
                        .try_into()
                        .map_err(|_| Error::Malformed(MalformedReason::InvalidEncodedData))?,
                );
                if gamma == 0 {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                header.gamma = Some(gamma);
            }
            b"cHRM" => {
                if header.chromaticities.is_some() || seen_idat || chunk_len != 32 {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                let mut values = [0_u32; 8];
                for (index, value) in values.iter_mut().enumerate() {
                    let start = index.checked_mul(4).ok_or(Error::SizeOverflow)?;
                    *value = u32::from_be_bytes(
                        payload[start..start + 4]
                            .try_into()
                            .map_err(|_| Error::Malformed(MalformedReason::InvalidEncodedData))?,
                    );
                }
                if values.iter().any(|value| *value > 100_000)
                    || values
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .any(|xy| xy[0] + xy[1] > 100_000)
                {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                header.chromaticities = Some(values);
            }
            b"sRGB" => {
                if header.has_srgb || seen_idat || chunk_len != 1 || payload[0] > 3 {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                header.has_srgb = true;
            }
            b"iCCP" => {
                if seen_idat {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                validate_iccp(payload)?;
                header.unsupported = Some(UnsupportedReason::ColorDescription);
            }
            b"eXIf" => {
                if seen_exif || seen_idat {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                seen_exif = true;
                if let Some(orientation) = parse_tiff_orientation(payload) {
                    header.orientation = orientation;
                }
            }
            b"acTL" | b"fcTL" | b"fdAT" => {
                header
                    .unsupported
                    .get_or_insert(UnsupportedReason::UnsupportedFormat);
            }
            b"cICP" | b"mDCv" | b"cLLi" => {
                header
                    .unsupported
                    .get_or_insert(UnsupportedReason::ColorDescription);
            }
            b"IDAT" => {}
            b"IEND" => {
                if chunk_len != 0 || !seen_idat || chunk_end != encoded.len() {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                seen_iend = true;
                offset = chunk_end;
                break;
            }
            _ => {
                if chunk_type[0].is_ascii_uppercase() {
                    header
                        .unsupported
                        .get_or_insert(UnsupportedReason::UnsupportedFormat);
                }
            }
        }
        offset = chunk_end;
    }

    if !seen_ihdr || !seen_idat || !seen_iend || offset != encoded.len() {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    if header.color_type == 3
        && (!seen_plte
            || header.palette_entries > (1_usize << header.bit_depth)
            || header
                .transparency_len
                .is_some_and(|length| length > header.palette_entries))
    {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    if let Some(length) = header.transparency_len {
        let valid = match header.color_type {
            0 => length == 2,
            2 => length == 6,
            3 => length > 0 && length <= header.palette_entries,
            _ => false,
        };
        if !valid {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
    }
    if !header.has_srgb && (header.gamma.is_some() || header.chromaticities.is_some()) {
        header
            .unsupported
            .get_or_insert(UnsupportedReason::ColorDescription);
    }
    if header.has_srgb
        && (header.gamma.is_some_and(|gamma| gamma != PNG_SRGB_GAMMA)
            || header
                .chromaticities
                .is_some_and(|values| values != PNG_SRGB_CHROMATICITIES))
    {
        header
            .unsupported
            .get_or_insert(UnsupportedReason::ColorDescription);
    }
    Ok(header)
}

fn validate_png_depth(color_type: u8, bit_depth: u8) -> Result<(), Error> {
    let supported_combination = match color_type {
        0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
        2 => matches!(bit_depth, 8 | 16),
        3 => matches!(bit_depth, 1 | 2 | 4 | 8),
        4 => matches!(bit_depth, 8 | 16),
        6 => matches!(bit_depth, 8 | 16),
        _ => false,
    };
    if supported_combination {
        Ok(())
    } else {
        Err(Error::Malformed(MalformedReason::InvalidEncodedData))
    }
}

fn validate_iccp(payload: &[u8]) -> Result<(), Error> {
    let name_end = payload
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
    if !(1..=79).contains(&name_end)
        || payload.get(name_end + 1) != Some(&0)
        || payload.len() <= name_end + 2
    {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    Ok(())
}

fn png_crc32(bytes: &[u8], cancel: &CancellationToken) -> Result<u32, Error> {
    let mut crc = !0_u32;
    for (index, byte) in bytes.iter().copied().enumerate() {
        check_cancel_periodically(cancel, index)?;
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    Ok(!crc)
}

fn png_output_layout(
    (color, depth): (png::ColorType, png::BitDepth),
) -> Result<(SampleLayout, bool), Error> {
    let layout = match (color, depth) {
        (png::ColorType::Grayscale, png::BitDepth::Eight) => SampleLayout::Gray8,
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => SampleLayout::GrayAlpha8,
        (png::ColorType::Rgb, png::BitDepth::Eight) => SampleLayout::Rgb8,
        (png::ColorType::Rgba, png::BitDepth::Eight) => SampleLayout::Rgba8,
        (png::ColorType::Grayscale, png::BitDepth::Sixteen) => SampleLayout::Gray16,
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Sixteen) => SampleLayout::GrayAlpha16,
        (png::ColorType::Rgb, png::BitDepth::Sixteen) => SampleLayout::Rgb16,
        (png::ColorType::Rgba, png::BitDepth::Sixteen) => SampleLayout::Rgba16,
        _ => return Err(Error::Unsupported(UnsupportedReason::SampleDepth)),
    };
    Ok((layout, layout.is_sixteen_bit()))
}

fn png_scratch_bound(header: &PngHeader, plan: RasterPlan) -> Result<u64, Error> {
    let encoded_samples = match header.color_type {
        0 | 3 => 1_u64,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err(Error::Malformed(MalformedReason::InvalidEncodedData)),
    };
    let raw_bits = u64::from(header.width)
        .checked_mul(encoded_samples)
        .and_then(|value| value.checked_mul(u64::from(header.bit_depth)))
        .ok_or(Error::SizeOverflow)?;
    let raw_row = raw_bits
        .checked_add(7)
        .map(|value| value / 8)
        .and_then(|value| value.checked_add(1))
        .ok_or(Error::SizeOverflow)?;
    let decoded_row = u64::try_from(plan.source_row_stride).map_err(|_| Error::SizeOverflow)?;
    let row_scratch = raw_row
        .checked_mul(2)
        .and_then(|value| value.checked_add(decoded_row.checked_mul(2)?))
        .ok_or(Error::SizeOverflow)?;
    // The PNG stream decoder buffers one complete non-IDAT chunk at a time; IDAT is streamed.
    let chunk_buffer_capacity = header
        .max_non_idat_chunk_bytes
        .max(CODEC_READER_BUFFER_BYTES)
        .checked_next_power_of_two()
        .ok_or(Error::SizeOverflow)?;
    let chunk_buffer_bytes =
        u64::try_from(chunk_buffer_capacity).map_err(|_| Error::SizeOverflow)?;
    PNG_FIXED_SCRATCH_BYTES
        .checked_add(row_scratch)
        .and_then(|value| value.checked_add(CODEC_READER_BUFFER_BYTES as u64))
        .and_then(|value| value.checked_add(chunk_buffer_bytes))
        .ok_or(Error::SizeOverflow)
}

fn png_work_units(encoded_bytes: u64, plan: RasterPlan) -> Result<u64, Error> {
    let decoded = plan.pixel_count.checked_mul(2).ok_or(Error::SizeOverflow)?;
    encoded_bytes
        .checked_mul(PNG_CRC_WORK_PER_BYTE)
        .and_then(|value| value.checked_add(decoded))
        .ok_or(Error::SizeOverflow)
}

#[derive(Clone, Copy, Debug, Default)]
struct JpegComponent {
    id: u8,
    horizontal_sampling: u8,
    vertical_sampling: u8,
}

#[derive(Clone, Copy, Debug)]
struct JpegHeader {
    width: u32,
    height: u32,
    precision: u8,
    progressive: bool,
    components: [JpegComponent; 4],
    component_count: usize,
    max_horizontal_sampling: u8,
    max_vertical_sampling: u8,
    scan_count: usize,
    first_scan_components: usize,
    scan_block_work: u64,
    exif_bytes: u64,
    orientation: Orientation,
    unsupported: Option<UnsupportedReason>,
}

#[cfg(test)]
fn decode_jpeg_with_reader_and_poller<R, F>(
    encoded: &[u8],
    limits: &DecodeLimits,
    cancel: &CancellationToken,
    codec_source: R,
    cancel_poller: &mut F,
) -> Result<DecodedFrame, Error>
where
    R: Read + Seek,
    F: FnMut(usize) -> Result<(), Error>,
{
    let mut normalization_cancel_poller = || check_cancel(cancel);
    let mut allocate_output = |len| zeroed_bytes(len);
    decode_jpeg_with_reader_and_pollers_and_output_allocator(
        encoded,
        limits,
        cancel,
        codec_source,
        cancel_poller,
        &mut normalization_cancel_poller,
        &mut allocate_output,
    )
}

fn decode_jpeg_with_reader_and_pollers_and_output_allocator<R, F, G, A>(
    encoded: &[u8],
    limits: &DecodeLimits,
    cancel: &CancellationToken,
    codec_source: R,
    cancel_poller: &mut F,
    normalization_cancel_poller: &mut G,
    allocate_output: &mut A,
) -> Result<DecodedFrame, Error>
where
    R: Read + Seek,
    F: FnMut(usize) -> Result<(), Error>,
    G: FnMut() -> Result<(), Error>,
    A: FnMut(usize) -> Result<Vec<u8>, Error>,
{
    let header = inspect_jpeg_with_poller(encoded, cancel_poller)?;
    if let Some(reason) = header.unsupported {
        return Err(Error::Unsupported(reason));
    }
    if header.precision != 8 {
        return Err(Error::Unsupported(UnsupportedReason::SampleDepth));
    }
    let layout = match header.component_count {
        1 => SampleLayout::Gray8,
        3 => SampleLayout::Rgb8,
        _ => return Err(Error::Unsupported(UnsupportedReason::PixelFormat)),
    };
    let plan = plan_raster(
        header.width,
        header.height,
        layout,
        header.orientation,
        limits,
    )?;
    let scratch_bytes = jpeg_scratch_bound(header)?;
    let encoded_bytes = u64::try_from(encoded.len()).map_err(|_| Error::SizeOverflow)?;
    let work_units = jpeg_work_units(header, plan, encoded_bytes)?;
    let (_scratch_permit, _source_permit, output_permit) =
        reserve_plan(plan, encoded_bytes, scratch_bytes, work_units, limits)?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let options = DecoderOptions::new_safe()
        .set_max_width(
            usize::try_from(limits.max_width.max(header.width)).map_err(|_| Error::SizeOverflow)?,
        )
        .set_max_height(
            usize::try_from(limits.max_height.max(header.height))
                .map_err(|_| Error::SizeOverflow)?,
        )
        .set_strict_mode(true)
        .set_use_unsafe(false)
        .jpeg_set_max_scans(header.scan_count)
        .jpeg_set_out_colorspace(if layout == SampleLayout::Gray8 {
            ColorSpace::Luma
        } else {
            ColorSpace::RGB
        });
    if header.scan_count == 0 {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let buffered = BufReader::with_capacity(CODEC_READER_BUFFER_BYTES, codec_source);
    let mut decoder = JpegDecoder::new_with_options(buffered, options);
    decoder
        .decode_headers()
        .map_err(|error| map_jpeg_error(error, cancel))?;
    if decoder.dimensions()
        != Some((
            usize::try_from(header.width).map_err(|_| Error::SizeOverflow)?,
            usize::try_from(header.height).map_err(|_| Error::SizeOverflow)?,
        ))
    {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let input_color = decoder
        .input_colorspace()
        .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
    if (header.component_count == 1 && input_color != ColorSpace::Luma)
        || (header.component_count == 3
            && !matches!(input_color, ColorSpace::RGB | ColorSpace::YCbCr))
    {
        return Err(Error::Unsupported(UnsupportedReason::PixelFormat));
    }
    let expected_color = if layout == SampleLayout::Gray8 {
        ColorSpace::Luma
    } else {
        ColorSpace::RGB
    };
    if decoder.output_colorspace() != Some(expected_color)
        || decoder.output_buffer_size()
            != Some(usize::try_from(plan.source_bytes).map_err(|_| Error::SizeOverflow)?)
    {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let mut output =
        allocate_output(usize::try_from(plan.output_bytes).map_err(|_| Error::SizeOverflow)?)?;
    let mut source = if plan.temporary_source_bytes == 0 {
        None
    } else {
        Some(zeroed_bytes(
            usize::try_from(plan.source_bytes).map_err(|_| Error::SizeOverflow)?,
        )?)
    };
    match source.as_mut() {
        Some(source) => decoder.decode_into(source),
        None => decoder.decode_into(&mut output),
    }
    .map_err(|error| map_jpeg_error(error, cancel))?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if let Some(source) = source.as_ref() {
        normalize_pixels(
            source,
            &mut output,
            plan,
            false,
            normalization_cancel_poller,
        )?;
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    DecodedFrame::new(
        DecodedFrameData {
            width: plan.output_width,
            height: plan.output_height,
            row_stride: plan.row_stride,
            channels: PixelChannels::Rgb,
            alpha: AlphaMode::Opaque,
            pixels: output,
            memory_permit: output_permit,
        },
        &limits.budget,
        cancel,
    )
}

fn inspect_jpeg_with_poller<F>(encoded: &[u8], cancel_poller: &mut F) -> Result<JpegHeader, Error>
where
    F: FnMut(usize) -> Result<(), Error>,
{
    if encoded.len() < 4 || encoded[..2] != [0xff, 0xd8] {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let mut header = JpegHeader {
        width: 0,
        height: 0,
        precision: 0,
        progressive: false,
        components: [JpegComponent::default(); 4],
        component_count: 0,
        max_horizontal_sampling: 0,
        max_vertical_sampling: 0,
        scan_count: 0,
        first_scan_components: 0,
        scan_block_work: 0,
        exif_bytes: 0,
        orientation: Orientation::NoTransforms,
        unsupported: None,
    };
    let mut offset = 2_usize;
    let mut pending_marker = None;
    let mut saw_sof = false;
    let mut saw_sos = false;
    let mut saw_exif = false;

    loop {
        cancel_poller(offset)?;
        let marker = match pending_marker.take() {
            Some(marker) => marker,
            None => read_jpeg_marker(encoded, &mut offset, cancel_poller)?,
        };
        if marker == 0xd9 {
            if !saw_sos || offset != encoded.len() {
                return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
            }
            break;
        }
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            if marker == 0xd8 {
                return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
            }
            continue;
        }
        let length_end = offset.checked_add(2).ok_or(Error::SizeOverflow)?;
        if length_end > encoded.len() {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        let segment_len = usize::from(u16::from_be_bytes([encoded[offset], encoded[offset + 1]]));
        if segment_len < 2 {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        let payload_start = length_end;
        let payload_len = segment_len - 2;
        let payload_end = payload_start
            .checked_add(payload_len)
            .filter(|end| *end <= encoded.len())
            .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
        let payload = &encoded[payload_start..payload_end];
        offset = payload_end;

        if is_jpeg_sof(marker) {
            if saw_sof {
                return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
            }
            saw_sof = true;
            parse_jpeg_sof(marker, payload, &mut header)?;
        }
        match marker {
            0xe1 => {
                if payload.starts_with(b"Exif\0\0") {
                    if saw_exif {
                        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                    }
                    saw_exif = true;
                    let tiff = &payload[6..];
                    header.exif_bytes =
                        u64::try_from(tiff.len()).map_err(|_| Error::SizeOverflow)?;
                    if let Some(orientation) = parse_tiff_orientation(tiff) {
                        header.orientation = orientation;
                    }
                } else {
                    header
                        .unsupported
                        .get_or_insert(UnsupportedReason::UnsupportedFormat);
                }
            }
            0xe2 => {
                if payload.starts_with(b"ICC_PROFILE\0") {
                    if payload.len() < 14
                        || payload[12] == 0
                        || payload[13] == 0
                        || payload[12] > payload[13]
                    {
                        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                    }
                    header
                        .unsupported
                        .get_or_insert(UnsupportedReason::ColorDescription);
                } else {
                    header
                        .unsupported
                        .get_or_insert(UnsupportedReason::UnsupportedFormat);
                }
            }
            0xed => {
                header
                    .unsupported
                    .get_or_insert(UnsupportedReason::UnsupportedFormat);
            }
            0xda => {
                if !saw_sof {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                let scan_components = parse_jpeg_sos(payload, &header)?;
                if header.scan_count == 0 {
                    header.first_scan_components = scan_components;
                }
                header.scan_count = header
                    .scan_count
                    .checked_add(1)
                    .ok_or(Error::SizeOverflow)?;
                header.scan_block_work = header
                    .scan_block_work
                    .checked_add(jpeg_scan_blocks(payload, &header)?)
                    .ok_or(Error::SizeOverflow)?;
                saw_sos = true;
                let (marker, has_entropy_data) =
                    scan_until_marker(encoded, &mut offset, cancel_poller)?;
                if !has_entropy_data {
                    return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
                }
                pending_marker = Some(marker);
            }
            0xc4 | 0xdb | 0xdd | 0xfe | 0xe0 | 0xee => {}
            _ if is_jpeg_sof(marker) => {}
            _ if (0xe0..=0xef).contains(&marker) => {
                header
                    .unsupported
                    .get_or_insert(UnsupportedReason::UnsupportedFormat);
            }
            _ => {
                header
                    .unsupported
                    .get_or_insert(UnsupportedReason::UnsupportedFormat);
            }
        }
    }
    if !saw_sof || !saw_sos {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    Ok(header)
}

fn is_jpeg_sof(marker: u8) -> bool {
    (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc)
}

fn parse_jpeg_sof(marker: u8, payload: &[u8], header: &mut JpegHeader) -> Result<(), Error> {
    if payload.len() < 6 {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    header.precision = payload[0];
    header.height = u32::from(u16::from_be_bytes([payload[1], payload[2]]));
    header.width = u32::from(u16::from_be_bytes([payload[3], payload[4]]));
    let component_count = usize::from(payload[5]);
    let required_len = 6_usize
        .checked_add(component_count.checked_mul(3).ok_or(Error::SizeOverflow)?)
        .ok_or(Error::SizeOverflow)?;
    if component_count == 0 || payload.len() != required_len {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    if header.width == 0 || header.height == 0 {
        return Err(Error::Malformed(MalformedReason::EmptyDimensions));
    }
    if marker != 0xc0 && marker != 0xc2 {
        header
            .unsupported
            .get_or_insert(UnsupportedReason::UnsupportedFormat);
    }
    header.progressive = marker == 0xc2;
    if component_count > header.components.len() {
        return Err(Error::Unsupported(UnsupportedReason::PixelFormat));
    }
    if !matches!(component_count, 1 | 3) {
        header
            .unsupported
            .get_or_insert(UnsupportedReason::PixelFormat);
    }
    if !matches!(header.precision, 8 | 12 | 16) {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    if header.precision != 8 {
        header
            .unsupported
            .get_or_insert(UnsupportedReason::SampleDepth);
    }
    header.component_count = component_count;
    for index in 0..component_count {
        let base = 6_usize
            .checked_add(index.checked_mul(3).ok_or(Error::SizeOverflow)?)
            .ok_or(Error::SizeOverflow)?;
        let id = payload[base];
        let sampling = payload[base + 1];
        let horizontal_sampling = sampling >> 4;
        let vertical_sampling = sampling & 0x0f;
        let quant_table = payload[base + 2];
        if horizontal_sampling == 0
            || horizontal_sampling > 4
            || vertical_sampling == 0
            || vertical_sampling > 4
            || quant_table > 3
            || header.components[..index]
                .iter()
                .any(|component| component.id == id)
        {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        header.components[index] = JpegComponent {
            id,
            horizontal_sampling,
            vertical_sampling,
        };
        header.max_horizontal_sampling = header.max_horizontal_sampling.max(horizontal_sampling);
        header.max_vertical_sampling = header.max_vertical_sampling.max(vertical_sampling);
    }
    Ok(())
}

fn parse_jpeg_sos(payload: &[u8], header: &JpegHeader) -> Result<usize, Error> {
    if payload.is_empty() {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    let count = usize::from(payload[0]);
    let expected_len = 4_usize
        .checked_add(count.checked_mul(2).ok_or(Error::SizeOverflow)?)
        .ok_or(Error::SizeOverflow)?;
    if count == 0 || count > header.component_count || payload.len() != expected_len {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    for index in 0..count {
        let id = payload[1 + index * 2];
        if !header.components[..header.component_count]
            .iter()
            .any(|component| component.id == id)
        {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        if payload[1..1 + index * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .any(|previous| previous[0] == id)
        {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
    }
    Ok(count)
}

fn jpeg_scan_blocks(payload: &[u8], header: &JpegHeader) -> Result<u64, Error> {
    let scan_count = usize::from(payload[0]);
    let width = u64::from(header.width);
    let height = u64::from(header.height);
    if scan_count == 1 {
        let id = payload[1];
        let component = header.components[..header.component_count]
            .iter()
            .find(|component| component.id == id)
            .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
        let columns = width.checked_add(7).ok_or(Error::SizeOverflow)? / 8;
        let rows = height.checked_add(7).ok_or(Error::SizeOverflow)? / 8;
        return columns
            .checked_mul(rows)
            .and_then(|blocks| blocks.checked_mul(u64::from(component.horizontal_sampling)))
            .and_then(|blocks| blocks.checked_mul(u64::from(component.vertical_sampling)))
            .ok_or(Error::SizeOverflow);
    }
    let mcu_width = u64::from(header.max_horizontal_sampling)
        .checked_mul(8)
        .ok_or(Error::SizeOverflow)?;
    let mcu_height = u64::from(header.max_vertical_sampling)
        .checked_mul(8)
        .ok_or(Error::SizeOverflow)?;
    let columns = width
        .checked_add(mcu_width - 1)
        .ok_or(Error::SizeOverflow)?
        / mcu_width;
    let rows = height
        .checked_add(mcu_height - 1)
        .ok_or(Error::SizeOverflow)?
        / mcu_height;
    let mcu_count = columns.checked_mul(rows).ok_or(Error::SizeOverflow)?;
    let mut blocks_per_mcu = 0_u64;
    for index in 0..scan_count {
        let id = payload[1 + index * 2];
        let component = header.components[..header.component_count]
            .iter()
            .find(|component| component.id == id)
            .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
        blocks_per_mcu = blocks_per_mcu
            .checked_add(
                u64::from(component.horizontal_sampling)
                    .checked_mul(u64::from(component.vertical_sampling))
                    .ok_or(Error::SizeOverflow)?,
            )
            .ok_or(Error::SizeOverflow)?;
    }
    mcu_count
        .checked_mul(blocks_per_mcu)
        .ok_or(Error::SizeOverflow)
}

fn jpeg_scratch_bound(header: JpegHeader) -> Result<u64, Error> {
    let mut row_scratch = 0_u64;
    let mut sampling_products = 0_u64;
    let max_sampling_ratio = u64::from(header.max_horizontal_sampling)
        .checked_mul(u64::from(header.max_vertical_sampling))
        .ok_or(Error::SizeOverflow)?;
    let mut first_component_width_stride = 0_u64;

    for (index, component) in header.components[..header.component_count]
        .iter()
        .enumerate()
    {
        let mcu_columns = u64::from(header.width)
            .checked_add(
                u64::from(header.max_horizontal_sampling)
                    .checked_mul(8)
                    .ok_or(Error::SizeOverflow)?
                    - 1,
            )
            .ok_or(Error::SizeOverflow)?
            / (u64::from(header.max_horizontal_sampling) * 8);
        let component_width_stride = mcu_columns
            .checked_mul(u64::from(component.horizontal_sampling))
            .ok_or(Error::SizeOverflow)?;
        if index == 0 {
            first_component_width_stride = component_width_stride;
        }
        let component_width = component_width_stride
            .checked_mul(8)
            .ok_or(Error::SizeOverflow)?;
        let vertical = u64::from(component.vertical_sampling);
        let component_row = component_width
            .checked_mul(vertical)
            .and_then(|value| value.checked_mul(8))
            .and_then(|value| value.checked_mul(2))
            // zune-jpeg keeps two previous/current sample rows and two
            // upsampling buffers per component while decoding subsampled JPEGs.
            .and_then(|value| {
                let row_buffers = component_width
                    .checked_mul(vertical)?
                    .checked_mul(4)?
                    .checked_add(
                        component_width
                            .checked_mul(vertical)?
                            .checked_mul(max_sampling_ratio)?
                            .checked_mul(2)?,
                    )?
                    .checked_add(
                        component_width
                            .checked_mul(max_sampling_ratio)?
                            .checked_mul(16)?,
                    )?;
                value.checked_add(row_buffers)
            })
            .ok_or(Error::SizeOverflow)?;
        row_scratch = row_scratch
            .checked_add(component_row)
            .ok_or(Error::SizeOverflow)?;
        sampling_products = sampling_products
            .checked_add(
                u64::from(component.horizontal_sampling)
                    .checked_mul(u64::from(component.vertical_sampling))
                    .ok_or(Error::SizeOverflow)?,
            )
            .ok_or(Error::SizeOverflow)?;
    }
    // Progressive or non-interleaved scans retain coefficient blocks per
    // frame. The sum of sampling products bounds all component block grids.
    let coefficient_scratch =
        if header.progressive || header.first_scan_components < header.component_count {
            let block_columns = u64::from(header.width)
                .checked_add(7)
                .ok_or(Error::SizeOverflow)?
                / 8;
            let block_rows = u64::from(header.height)
                .checked_add(7)
                .ok_or(Error::SizeOverflow)?
                / 8;
            block_columns
                .checked_mul(block_rows)
                .and_then(|blocks| blocks.checked_mul(sampling_products))
                .and_then(|blocks| blocks.checked_mul(64))
                .and_then(|values| values.checked_mul(2))
                .ok_or(Error::SizeOverflow)?
        } else {
            0
        };
    let interleaved_scan_scratch = first_component_width_stride
        .checked_mul(2)
        .ok_or(Error::SizeOverflow)?;
    JPEG_FIXED_SCRATCH_BYTES
        .checked_add(JPEG_EXIF_SCRATCH_BYTES)
        .and_then(|value| value.checked_add(CODEC_READER_BUFFER_BYTES as u64))
        .and_then(|value| value.checked_add(row_scratch))
        .and_then(|value| value.checked_add(interleaved_scan_scratch))
        .and_then(|value| value.checked_add(coefficient_scratch))
        .and_then(|value| value.checked_add(header.exif_bytes))
        .ok_or(Error::SizeOverflow)
}

fn jpeg_work_units(header: JpegHeader, plan: RasterPlan, encoded_bytes: u64) -> Result<u64, Error> {
    let scan_work = header
        .scan_block_work
        .checked_mul(64)
        .ok_or(Error::SizeOverflow)?;
    let raster_work = plan.pixel_count.checked_mul(2).ok_or(Error::SizeOverflow)?;
    scan_work
        .checked_add(raster_work)
        .and_then(|value| value.checked_add(encoded_bytes))
        .ok_or(Error::SizeOverflow)
}

fn read_jpeg_marker<F>(
    encoded: &[u8],
    offset: &mut usize,
    cancel_poller: &mut F,
) -> Result<u8, Error>
where
    F: FnMut(usize) -> Result<(), Error>,
{
    if encoded.get(*offset) != Some(&0xff) {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    while encoded.get(*offset) == Some(&0xff) {
        cancel_poller(*offset)?;
        *offset = offset.checked_add(1).ok_or(Error::SizeOverflow)?;
    }
    let marker = *encoded
        .get(*offset)
        .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
    *offset = offset.checked_add(1).ok_or(Error::SizeOverflow)?;
    if marker == 0 {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    Ok(marker)
}

fn scan_until_marker<F>(
    encoded: &[u8],
    offset: &mut usize,
    cancel_poller: &mut F,
) -> Result<(u8, bool), Error>
where
    F: FnMut(usize) -> Result<(), Error>,
{
    let mut has_entropy_data = false;
    loop {
        cancel_poller(*offset)?;
        let byte = *encoded
            .get(*offset)
            .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
        *offset = offset.checked_add(1).ok_or(Error::SizeOverflow)?;
        if byte != 0xff {
            has_entropy_data = true;
            continue;
        }
        while encoded.get(*offset) == Some(&0xff) {
            cancel_poller(*offset)?;
            *offset = offset.checked_add(1).ok_or(Error::SizeOverflow)?;
        }
        let marker = *encoded
            .get(*offset)
            .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
        *offset = offset.checked_add(1).ok_or(Error::SizeOverflow)?;
        if marker == 0 {
            has_entropy_data = true;
            continue;
        }
        if (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        return Ok((marker, has_entropy_data));
    }
}

fn parse_tiff_orientation(tiff: &[u8]) -> Option<Orientation> {
    if tiff.len() < 8 {
        return None;
    }
    let little_endian = match &tiff[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if read_tiff_u16(&tiff[2..4], little_endian) != 42 {
        return None;
    }
    let ifd_offset = usize::try_from(read_tiff_u32(&tiff[4..8], little_endian)).ok()?;
    let count_end = ifd_offset.checked_add(2)?;
    let entry_count = usize::from(read_tiff_u16(
        tiff.get(ifd_offset..count_end)?,
        little_endian,
    ));
    let entries_start = count_end;
    for index in 0..entry_count {
        let entry_start = entries_start.checked_add(index.checked_mul(12)?)?;
        let entry_end = entry_start.checked_add(12)?;
        let entry = tiff.get(entry_start..entry_end)?;
        let tag = read_tiff_u16(&entry[0..2], little_endian);
        let value_type = read_tiff_u16(&entry[2..4], little_endian);
        let value_count = read_tiff_u32(&entry[4..8], little_endian);
        if tag == 0x0112 && value_type == 3 && value_count == 1 {
            return Orientation::from_exif(
                u8::try_from(read_tiff_u16(&entry[8..10], little_endian)).ok()?,
            );
        }
    }
    None
}

fn read_tiff_u16(bytes: &[u8], little_endian: bool) -> u16 {
    let value = [bytes[0], bytes[1]];
    if little_endian {
        u16::from_le_bytes(value)
    } else {
        u16::from_be_bytes(value)
    }
}

fn read_tiff_u32(bytes: &[u8], little_endian: bool) -> u32 {
    let value = [bytes[0], bytes[1], bytes[2], bytes[3]];
    if little_endian {
        u32::from_le_bytes(value)
    } else {
        u32::from_be_bytes(value)
    }
}

fn orientation_swaps_axes(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

fn normalize_pixels<F>(
    source: &[u8],
    output: &mut [u8],
    plan: RasterPlan,
    source_big_endian: bool,
    normalization_cancel_poller: &mut F,
) -> Result<(), Error>
where
    F: FnMut() -> Result<(), Error>,
{
    let source_width = usize::try_from(plan.source_width).map_err(|_| Error::SizeOverflow)?;
    let source_height = usize::try_from(plan.source_height).map_err(|_| Error::SizeOverflow)?;
    let output_width = usize::try_from(plan.output_width).map_err(|_| Error::SizeOverflow)?;
    let output_channels = plan.layout.output_channels();
    for y in 0..source_height {
        normalization_cancel_poller()?;
        for x in 0..source_width {
            if x > 0 && x.is_multiple_of(NORMALIZATION_CANCEL_INTERVAL_PIXELS) {
                normalization_cancel_poller()?;
            }
            let source_offset = y
                .checked_mul(plan.source_row_stride)
                .and_then(|offset| {
                    offset.checked_add(x.checked_mul(plan.layout.bytes_per_pixel())?)
                })
                .ok_or(Error::SizeOverflow)?;
            let rgba = read_pixel(source, source_offset, plan.layout, source_big_endian)?;
            let (output_x, output_y) = orient_coordinate(
                u32::try_from(x).map_err(|_| Error::SizeOverflow)?,
                u32::try_from(y).map_err(|_| Error::SizeOverflow)?,
                plan,
            );
            let output_offset = usize::try_from(output_y)
                .map_err(|_| Error::SizeOverflow)?
                .checked_mul(output_width)
                .and_then(|index| index.checked_add(usize::try_from(output_x).ok()?))
                .and_then(|index| index.checked_mul(output_channels))
                .ok_or(Error::SizeOverflow)?;
            let end = output_offset
                .checked_add(output_channels)
                .ok_or(Error::SizeOverflow)?;
            let pixel = output
                .get_mut(output_offset..end)
                .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
            pixel[0] = rgba[0];
            pixel[1] = rgba[1];
            pixel[2] = rgba[2];
            if output_channels == 4 {
                pixel[3] = rgba[3];
            }
        }
    }
    Ok(())
}

fn orient_coordinate(x: u32, y: u32, plan: RasterPlan) -> (u32, u32) {
    let right = plan.source_width - 1 - x;
    let bottom = plan.source_height - 1 - y;
    match plan.orientation {
        Orientation::NoTransforms => (x, y),
        Orientation::FlipHorizontal => (right, y),
        Orientation::Rotate180 => (right, bottom),
        Orientation::FlipVertical => (x, bottom),
        Orientation::Rotate90 => (bottom, x),
        Orientation::Rotate270 => (y, right),
        Orientation::Rotate90FlipH => (y, x),
        Orientation::Rotate270FlipH => (bottom, right),
    }
}

fn read_pixel(
    source: &[u8],
    offset: usize,
    layout: SampleLayout,
    source_big_endian: bool,
) -> Result<[u8; 4], Error> {
    let end = offset
        .checked_add(layout.bytes_per_pixel())
        .ok_or(Error::SizeOverflow)?;
    let sample = source
        .get(offset..end)
        .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
    let gray = || sample_8_or_16(sample, layout.is_sixteen_bit(), 0, source_big_endian);
    let alpha = || {
        let channel = if layout.is_rgb() { 3 } else { 1 };
        sample_8_or_16(sample, layout.is_sixteen_bit(), channel, source_big_endian)
    };
    if layout.is_rgb() {
        Ok([
            sample_8_or_16(sample, layout.is_sixteen_bit(), 0, source_big_endian)?,
            sample_8_or_16(sample, layout.is_sixteen_bit(), 1, source_big_endian)?,
            sample_8_or_16(sample, layout.is_sixteen_bit(), 2, source_big_endian)?,
            if layout.has_alpha() { alpha()? } else { 255 },
        ])
    } else {
        let gray = gray()?;
        Ok([
            gray,
            gray,
            gray,
            if layout.has_alpha() { alpha()? } else { 255 },
        ])
    }
}

fn sample_8_or_16(
    sample: &[u8],
    sixteen_bit: bool,
    channel: usize,
    source_big_endian: bool,
) -> Result<u8, Error> {
    if sixteen_bit {
        let offset = channel.checked_mul(2).ok_or(Error::SizeOverflow)?;
        let bytes = sample
            .get(offset..offset + 2)
            .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
        let value = if source_big_endian {
            u16::from_be_bytes([bytes[0], bytes[1]])
        } else {
            u16::from_ne_bytes([bytes[0], bytes[1]])
        };
        Ok(((u32::from(value) + 128) / 257) as u8)
    } else {
        sample
            .get(channel)
            .copied()
            .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))
    }
}

struct SnapshotReader<'a> {
    bytes: &'a [u8],
    position: usize,
    cancel: &'a CancellationToken,
}

impl<'a> SnapshotReader<'a> {
    fn new(bytes: &'a [u8], cancel: &'a CancellationToken) -> Self {
        Self {
            bytes,
            position: 0,
            cancel,
        }
    }
}

impl Read for SnapshotReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "decode cancelled",
            ));
        }
        if output.is_empty() || self.position == self.bytes.len() {
            return Ok(0);
        }
        let count = output.len().min(self.bytes.len() - self.position);
        let end = self.position.checked_add(count).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "snapshot position overflow")
        })?;
        output[..count].copy_from_slice(&self.bytes[self.position..end]);
        self.position = end;
        Ok(count)
    }
}

impl Seek for SnapshotReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "decode cancelled",
            ));
        }
        let base = match position {
            SeekFrom::Start(_) => 0_i128,
            SeekFrom::Current(_) => i128::try_from(self.position).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "snapshot position overflow")
            })?,
            SeekFrom::End(_) => i128::try_from(self.bytes.len()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "snapshot length overflow")
            })?,
        };
        let delta = match position {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(value) | SeekFrom::End(value) => i128::from(value),
        };
        let target = if matches!(position, SeekFrom::Start(_)) {
            delta
        } else {
            base.checked_add(delta).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "snapshot seek overflow")
            })?
        };
        let target = usize::try_from(target)
            .ok()
            .filter(|target| *target <= self.bytes.len())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek outside snapshot"))?;
        self.position = target;
        u64::try_from(target)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "snapshot position overflow"))
    }
}

fn inspect_extent(
    input: &mut dyn ReadSeek,
    max_encoded_bytes: u64,
    cancel: &CancellationToken,
) -> Result<(u64, u64), Error> {
    let start = input
        .stream_position()
        .map_err(|error| map_io_error(error, cancel))?;
    let end = input
        .seek(SeekFrom::End(0))
        .map_err(|error| map_io_error(error, cancel))?;
    let length = end
        .checked_sub(start)
        .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
    input
        .seek(SeekFrom::Start(start))
        .map_err(|error| map_io_error(error, cancel))?;
    if length > max_encoded_bytes {
        return Err(Error::BudgetExceeded(BudgetKind::EncodedBytes));
    }
    Ok((start, length))
}

fn read_prefix(
    input: &mut dyn ReadSeek,
    start: u64,
    encoded_len: u64,
    prefix: &mut [u8; INPUT_PREFIX_BYTES],
    cancel: &CancellationToken,
) -> Result<usize, Error> {
    input
        .seek(SeekFrom::Start(start))
        .map_err(|error| map_io_error(error, cancel))?;
    let wanted = usize::try_from(encoded_len.min(INPUT_PREFIX_BYTES as u64))
        .map_err(|_| Error::SizeOverflow)?;
    read_exact_cancellable(input, &mut prefix[..wanted], cancel)?;
    Ok(wanted)
}

fn read_snapshot_remainder(
    input: &mut dyn ReadSeek,
    remainder: &mut [u8],
    cancel: &CancellationToken,
) -> Result<(), Error> {
    read_exact_cancellable(input, remainder, cancel)
}

fn read_exact_cancellable(
    input: &mut dyn Read,
    mut output: &mut [u8],
    cancel: &CancellationToken,
) -> Result<(), Error> {
    while !output.is_empty() {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let requested = output.len().min(CODEC_READER_BUFFER_BYTES);
        let count = match input.read(&mut output[..requested]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted && !cancel.is_cancelled() => {
                continue;
            }
            Err(error) => return Err(map_io_error(error, cancel)),
            Ok(count) => count,
        };
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if count == 0 {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        let (_, remaining) = output.split_at_mut(count);
        output = remaining;
    }
    Ok(())
}

fn sniff_format(prefix: &[u8]) -> Result<Codec, Error> {
    if prefix.len() >= PNG_SIGNATURE.len() && prefix[..PNG_SIGNATURE.len()] == *PNG_SIGNATURE {
        return Ok(Codec::Png);
    }
    if prefix.starts_with(&[0xff, 0xd8]) {
        return Ok(Codec::Jpeg);
    }
    if prefix.is_empty()
        || PNG_SIGNATURE.starts_with(prefix)
        || prefix.starts_with(b"\x89PNG")
        || prefix == [0xff]
    {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    if is_known_unsupported(prefix) {
        return Err(Error::Unsupported(UnsupportedReason::UnsupportedFormat));
    }
    Err(Error::Unsupported(UnsupportedReason::UnsupportedFormat))
}

fn is_known_unsupported(prefix: &[u8]) -> bool {
    (prefix.len() >= 2 && prefix[..2] == *b"BM")
        || (prefix.len() >= 3 && prefix[..3] == *b"GIF")
        || (prefix.len() >= 4 && (prefix[..4] == *b"II*\0" || prefix[..4] == *b"MM\0*"))
        || (prefix.len() >= 12 && prefix[..4] == *b"RIFF" && prefix[8..12] == *b"WEBP")
}

fn zeroed_bytes(len: usize) -> Result<Vec<u8>, Error> {
    if len == 0 {
        return Ok(Vec::new());
    }
    let layout = std::alloc::Layout::array::<u8>(len).map_err(|_| Error::SizeOverflow)?;
    // SAFETY: `layout` is a valid nonzero byte layout. The allocation is initialized to zero and
    // is transferred into a Vec with exactly the same length, capacity, and allocator.
    let pointer = unsafe { std::alloc::alloc_zeroed(layout) };
    if pointer.is_null() {
        return Err(Error::AllocationFailed);
    }
    // SAFETY: `pointer` was allocated with `layout`, every byte is initialized, and the Vec has
    // length/capacity `len`, matching the allocation contract.
    Ok(unsafe { Vec::from_raw_parts(pointer, len, len) })
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), Error> {
    if cancel.is_cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

fn check_cancel_periodically(cancel: &CancellationToken, offset: usize) -> Result<(), Error> {
    if offset & 0x1fff == 0 && cancel.is_cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

fn map_png_error(error: png::DecodingError, cancel: &CancellationToken) -> Error {
    match error {
        png::DecodingError::IoError(error) => map_io_error(error, cancel),
        png::DecodingError::LimitsExceeded => Error::BudgetExceeded(BudgetKind::RasterBytes),
        _ => Error::Malformed(MalformedReason::InvalidEncodedData),
    }
}

fn map_jpeg_error(error: DecodeErrors, cancel: &CancellationToken) -> Error {
    match error {
        DecodeErrors::Unsupported(_) => Error::Unsupported(UnsupportedReason::UnsupportedFormat),
        DecodeErrors::LargeDimensions(_) => Error::BudgetExceeded(BudgetKind::WorkUnits),
        DecodeErrors::IoErrors(zune_core::bytestream::ZByteIoError::StdIoError(error)) => {
            map_io_error(error, cancel)
        }
        _ => Error::Malformed(MalformedReason::InvalidEncodedData),
    }
}

fn map_io_error(error: io::Error, cancel: &CancellationToken) -> Error {
    if error.kind() == io::ErrorKind::Interrupted && cancel.is_cancelled() {
        return Error::Cancelled;
    }
    match error.kind() {
        io::ErrorKind::PermissionDenied => Error::PermissionDenied,
        io::ErrorKind::OutOfMemory => Error::AllocationFailed,
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => Error::TemporarilyUnavailable,
        _ => Error::Malformed(MalformedReason::InvalidEncodedData),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_crc_matches_the_png_reference_vector() {
        assert_eq!(
            png_crc32(b"IHDR", &CancellationToken::new()).unwrap(),
            0xa8a1_ae0a
        );
    }
}

#[cfg(test)]
mod codec_cancellation_tests {
    use std::{cell::Cell, io::Cursor};

    use super::*;

    const WIDTH: u32 = 256;
    const HEIGHT: u32 = 256;
    const TEST_INPUT_CHUNK_BYTES: usize = CODEC_READER_BUFFER_BYTES;

    // The same rights-cleared, 9x8 Adam7 fixture as the PNG integration matrix.
    const ADAM7_RGBA_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x08, 0x08, 0x06, 0x00, 0x00, 0x01, 0x5c,
        0xca, 0xe5, 0x23, 0x00, 0x00, 0x01, 0x2c, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x0d, 0xc7,
        0x4b, 0x44, 0x84, 0x51, 0x18, 0x06, 0xe0, 0x97, 0x52, 0x52, 0xca, 0x3f, 0x44, 0x51, 0xe2,
        0x54, 0xa4, 0x21, 0xa6, 0x3f, 0x52, 0x8b, 0x34, 0x1c, 0x4a, 0x74, 0x21, 0x5f, 0xad, 0x26,
        0x51, 0x1d, 0x52, 0x44, 0x17, 0x87, 0x8a, 0x34, 0xc4, 0x24, 0x52, 0x44, 0x25, 0x26, 0xd1,
        0x85, 0x8f, 0x49, 0x54, 0x9b, 0x2e, 0xa4, 0x21, 0xba, 0x90, 0x66, 0xd5, 0x50, 0x26, 0x91,
        0x22, 0x26, 0x2d, 0x6a, 0x55, 0xbe, 0x7a, 0x76, 0x0f, 0xf0, 0xef, 0x2d, 0x2f, 0x04, 0x8c,
        0x66, 0x69, 0xa0, 0x60, 0x52, 0x63, 0xe1, 0x3f, 0xa9, 0x4b, 0x37, 0x40, 0x6d, 0x9a, 0xe2,
        0x5d, 0x8f, 0x61, 0x74, 0x04, 0x0d, 0x5f, 0xcd, 0x2b, 0x46, 0x76, 0x9d, 0x42, 0xa3, 0x06,
        0x07, 0x9b, 0x0c, 0x4e, 0xda, 0x34, 0x7f, 0x77, 0x30, 0x50, 0x7e, 0x64, 0xd0, 0x7b, 0xa6,
        0x39, 0x7c, 0xa1, 0x70, 0x7f, 0x03, 0xf6, 0xc4, 0x92, 0x40, 0x61, 0x8a, 0x43, 0x9d, 0x19,
        0xae, 0x2c, 0xe6, 0x10, 0x5d, 0xe7, 0x5a, 0x81, 0xd7, 0xef, 0x52, 0x5f, 0x83, 0x23, 0xeb,
        0xcd, 0x96, 0xe2, 0xed, 0x24, 0xa8, 0x99, 0x22, 0x1a, 0x99, 0xb1, 0x12, 0x99, 0x73, 0xe8,
        0x75, 0xd1, 0x15, 0x34, 0x9c, 0x5a, 0x9a, 0x8e, 0x92, 0x1c, 0x5f, 0xba, 0xf4, 0x75, 0xeb,
        0x08, 0xd2, 0x8b, 0x1c, 0x2e, 0x51, 0x10, 0x7f, 0xa9, 0x8b, 0xae, 0x32, 0x45, 0x13, 0x5e,
        0xe2, 0x95, 0x0a, 0x2d, 0x87, 0x3e, 0x8b, 0x58, 0x95, 0xa1, 0x8f, 0xea, 0x55, 0x46, 0x6e,
        0xc0, 0xe5, 0xca, 0x6e, 0x25, 0xad, 0x3d, 0x0e, 0x06, 0x0d, 0x68, 0xb6, 0xdf, 0xf2, 0xf6,
        0xa0, 0x91, 0xe8, 0x10, 0xe1, 0x69, 0x58, 0xd3, 0xef, 0xd8, 0x31, 0xa3, 0x78, 0x8d, 0xb8,
        0x3e, 0xac, 0x25, 0xb0, 0x61, 0x31, 0xbe, 0x69, 0x68, 0x79, 0xc7, 0xe1, 0x03, 0x86, 0xdc,
        0x45, 0x5c, 0x24, 0xf7, 0x14, 0x65, 0xee, 0x3f, 0x32, 0x7c, 0x71, 0xcb, 0x2d, 0x0f, 0x46,
        0x06, 0x12, 0x84, 0xd0, 0xb3, 0xa6, 0xad, 0x17, 0x97, 0xcf, 0xdf, 0x94, 0x24, 0xde, 0x1d,
        0xfc, 0x24, 0x41, 0xf9, 0x9f, 0xc2, 0x7f, 0xec, 0x2e, 0x7c, 0xed, 0x4b, 0x2e, 0x5f, 0xcd,
        0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    struct CancelAfterCodecInput<'a> {
        input: Cursor<&'a [u8]>,
        cancellation: &'a CancellationToken,
        cancel_at: u64,
        largest_request: usize,
        furthest_read: u64,
        interrupted_reads: usize,
    }
    impl<'a> CancelAfterCodecInput<'a> {
        fn new(bytes: &'a [u8], cancellation: &'a CancellationToken, cancel_at: u64) -> Self {
            Self {
                input: Cursor::new(bytes),
                cancellation,
                cancel_at,
                largest_request: 0,
                furthest_read: 0,
                interrupted_reads: 0,
            }
        }

        fn interrupted(&mut self) -> io::Error {
            self.interrupted_reads += 1;
            io::Error::new(io::ErrorKind::Interrupted, "test cancellation")
        }
    }

    impl Read for CancelAfterCodecInput<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if output.is_empty() {
                return Ok(0);
            }
            self.largest_request = self.largest_request.max(output.len());
            if self.cancellation.is_cancelled() {
                return Err(self.interrupted());
            }
            let position = self.input.position();
            if position >= self.cancel_at {
                self.cancellation.cancel();
                return Err(self.interrupted());
            }
            let until_cancel = usize::try_from(self.cancel_at - position).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "test input offset overflow")
            })?;
            let read_limit = output.len().min(TEST_INPUT_CHUNK_BYTES).min(until_cancel);
            let count = self.input.read(&mut output[..read_limit])?;
            self.furthest_read = self.furthest_read.max(self.input.position());
            if self.input.position() >= self.cancel_at {
                self.cancellation.cancel();
            }
            Ok(count)
        }
    }

    impl Seek for CancelAfterCodecInput<'_> {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            if self.cancellation.is_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "test cancellation",
                ));
            }
            self.input.seek(position)
        }
    }

    fn limits() -> DecodeLimits {
        DecodeLimits::new(
            Budget::new(8 << 20, 8 << 20, 32 << 20),
            DecodeResources::new(1, 32 << 20),
            1024,
            1024,
            1 << 20,
            8 << 20,
        )
    }

    fn assert_cancelled_before_eof(
        result: Result<super::DecodedFrame, Error>,
        cancellation: &CancellationToken,
        source: &CancelAfterCodecInput<'_>,
        resources: &DecodeResources,
        encoded_len: usize,
    ) {
        assert!(matches!(result, Err(Error::Cancelled)));
        assert!(cancellation.is_cancelled());
        assert!(source.furthest_read >= source.cancel_at);
        assert!(source.furthest_read < u64::try_from(encoded_len).unwrap());
        assert!(source.interrupted_reads > 0);
        assert_eq!(resources.cpu_bytes_in_use(), 0);
        assert_eq!(resources.active_decodes(), 0);
    }

    fn deterministic_bytes(len: usize, seed: u32) -> Vec<u8> {
        let mut state = seed;
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            bytes.push(state.to_be_bytes()[0]);
        }
        bytes
    }

    fn random_rgb_png(width: u32, height: u32) -> Vec<u8> {
        let sample_count = usize::try_from(width.checked_mul(height).unwrap()).unwrap() * 3;
        let samples = deterministic_bytes(sample_count, 0x2c1b_3c6d);
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&samples)
                .unwrap();
        }
        bytes
    }

    fn grayscale_png(width: u32, height: u32) -> Vec<u8> {
        let sample_count = usize::try_from(width.checked_mul(height).unwrap()).unwrap();
        let samples = deterministic_bytes(sample_count, 0x1319_8a2e);
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&samples)
                .unwrap();
        }
        bytes
    }

    fn idat_payload_start(bytes: &[u8]) -> usize {
        let mut offset = PNG_SIGNATURE.len();
        while offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
            let chunk_len = usize::try_from(u32::from_be_bytes(
                bytes[offset..offset + 4].try_into().unwrap(),
            ))
            .unwrap();
            if &bytes[offset + 4..offset + 8] == b"IDAT" {
                return offset + 8;
            }
            offset = offset + 8 + chunk_len + 4;
        }
        panic!("PNG fixture has no IDAT chunk");
    }

    fn progressive_jpeg(width: u32, height: u32) -> Vec<u8> {
        let sample_count = usize::try_from(width.checked_mul(height).unwrap()).unwrap();
        let samples = deterministic_bytes(sample_count, 0x243f_6a88);
        let mut encoder = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_GRAYSCALE);
        encoder.set_size(
            usize::try_from(width).unwrap(),
            usize::try_from(height).unwrap(),
        );
        encoder.set_quality(95.0);
        encoder.set_progressive_mode();
        let mut started = encoder.start_compress(Vec::new()).unwrap();
        started.write_scanlines(&samples).unwrap();
        let bytes = started.finish().unwrap();
        assert!(bytes.windows(2).any(|marker| marker == [0xff, 0xc2]));
        bytes
    }

    fn jpeg_entropy_start(bytes: &[u8]) -> usize {
        let marker = bytes
            .windows(2)
            .position(|pair| pair == b"\xff\xda")
            .expect("JPEG has a Start of Scan marker");
        let segment_length =
            usize::from(u16::from_be_bytes([bytes[marker + 2], bytes[marker + 3]]));
        marker + 2 + segment_length
    }


    #[test]
    fn oversized_jpeg_is_rejected_before_output_allocation() {
        let bytes = include_bytes!("../tests/fixtures/ec010-oversized-budget.jpg");
        assert!(bytes.len() < 32 * 1024);

        let mut decode_limits = limits();
        decode_limits.budget = Budget::new(8 << 20, 1 << 20, 32 << 20);
        let allocation_attempted = Cell::new(None);
        let mut allocate_output = |len| {
            allocation_attempted.set(Some(len));
            Err(Error::AllocationFailed)
        };

        // The public decoder uses this same path with `zeroed_bytes`; this sentinel proves the
        // advertised raster is rejected before that output-allocation seam is reached.
        let cancellation = CancellationToken::new();
        let result = decode_image_with_pollers(
            &mut Cursor::new(bytes.as_slice()),
            &decode_limits,
            &cancellation,
            |_| Ok(()),
            || check_cancel(&cancellation),
            &mut allocate_output,
        );
        assert_eq!(
            result.unwrap_err(),
            Error::BudgetExceeded(BudgetKind::RasterBytes)
        );
        assert_eq!(
            allocation_attempted.get(),
            None,
            "oversized JPEG reached output allocation before budget rejection"
        );
    }

    #[test]
    fn cancellation_during_png_inflate_stops_at_a_bounded_input_refill() {
        let bytes = random_rgb_png(WIDTH, HEIGHT);
        let cancel_at = idat_payload_start(&bytes).checked_add(1024).unwrap();
        assert!(cancel_at < bytes.len());

        let cancellation = CancellationToken::new();
        let limits = limits();
        let mut source =
            CancelAfterCodecInput::new(&bytes, &cancellation, u64::try_from(cancel_at).unwrap());
        let result = decode_png_with_reader(&bytes, &limits, &cancellation, &mut source);

        assert_cancelled_before_eof(
            result,
            &cancellation,
            &source,
            &limits.resources,
            bytes.len(),
        );
    }

    #[test]
    fn cancellation_during_snapshot_copy_uses_bounded_reads_and_releases_reservations() {
        let bytes = random_rgb_png(WIDTH, HEIGHT);
        let cancel_at = INPUT_PREFIX_BYTES + CODEC_READER_BUFFER_BYTES * 2;
        assert!(cancel_at < bytes.len());

        let cancellation = CancellationToken::new();
        let resources = DecodeResources::new(1, 32 << 20);
        let mut decode_limits = limits();
        decode_limits.resources = resources.clone();
        let mut source =
            CancelAfterCodecInput::new(&bytes, &cancellation, u64::try_from(cancel_at).unwrap());
        let result = decode_image(&mut source, &decode_limits, &cancellation);

        assert!(matches!(result, Err(Error::Cancelled)));
        assert!(cancellation.is_cancelled());
        assert_eq!(source.furthest_read, u64::try_from(cancel_at).unwrap());
        assert!(source.furthest_read < u64::try_from(bytes.len()).unwrap());
        assert!(source.largest_request <= CODEC_READER_BUFFER_BYTES);
        assert_eq!(resources.cpu_bytes_in_use(), 0);
        assert_eq!(resources.active_decodes(), 0);
    }

    #[test]
    fn cancellation_inside_wide_png_row_releases_reservations_without_a_frame() {
        let bytes = grayscale_png(1024, 1);
        let cancellation = CancellationToken::new();
        let resources = DecodeResources::new(1, 32 << 20);
        let mut decode_limits = limits();
        decode_limits.resources = resources.clone();
        let mut input = Cursor::new(bytes.as_slice());
        let mut normalization_polls = 0;

        let result = decode_image_with_pollers(
            &mut input,
            &decode_limits,
            &cancellation,
            |_| Ok(()),
            || {
                normalization_polls += 1;
                if normalization_polls == 2 {
                    cancellation.cancel();
                }
                check_cancel(&cancellation)
            },
            zeroed_bytes,
        );

        assert!(matches!(result, Err(Error::Cancelled)));
        assert!(cancellation.is_cancelled());
        assert_eq!(normalization_polls, 2);
        assert_eq!(resources.cpu_bytes_in_use(), 0);
        assert_eq!(resources.active_decodes(), 0);
    }

    #[test]
    fn cancellation_during_adam7_png_inflate_stops_before_end_of_idat() {
        assert_eq!(ADAM7_RGBA_PNG[28], 1, "fixture must use Adam7 interlace");
        let data_start = idat_payload_start(ADAM7_RGBA_PNG);
        let cancel_at = data_start.checked_add(TEST_INPUT_CHUNK_BYTES).unwrap();
        assert!(cancel_at < ADAM7_RGBA_PNG.len());

        let cancellation = CancellationToken::new();
        let limits = limits();
        let mut source = CancelAfterCodecInput::new(
            ADAM7_RGBA_PNG,
            &cancellation,
            u64::try_from(cancel_at).unwrap(),
        );
        let result = decode_png_with_reader(ADAM7_RGBA_PNG, &limits, &cancellation, &mut source);

        assert_cancelled_before_eof(
            result,
            &cancellation,
            &source,
            &limits.resources,
            ADAM7_RGBA_PNG.len(),
        );
    }

    #[test]
    fn cancellation_during_progressive_jpeg_scan_stops_at_a_bounded_input_refill() {
        let bytes = progressive_jpeg(WIDTH, HEIGHT);
        let entropy_start = jpeg_entropy_start(&bytes);
        let cancel_at = entropy_start.checked_add(1024).unwrap();
        assert!(cancel_at < bytes.len());

        let cancellation = CancellationToken::new();
        let limits = limits();
        let mut source =
            CancelAfterCodecInput::new(&bytes, &cancellation, u64::try_from(cancel_at).unwrap());
        let mut poller = |offset| check_cancel_periodically(&cancellation, offset);
        let result = decode_jpeg_with_reader_and_poller(
            &bytes,
            &limits,
            &cancellation,
            &mut source,
            &mut poller,
        );

        assert_cancelled_before_eof(
            result,
            &cancellation,
            &source,
            &limits.resources,
            bytes.len(),
        );
    }

    const JPEG_FILL_RUN_BYTES: usize = 20_000;
    const JPEG_FILL_CANCEL_OFFSET: usize = 16_384;

    fn assert_jpeg_fill_cancellation_releases_reservations(bytes: &[u8]) {
        let mut decode_limits = limits();
        let resources = DecodeResources::new(1, 32 << 20);
        decode_limits.resources = resources.clone();
        let cancellation = CancellationToken::new();
        let last_polled_offset = Cell::new(None);

        let result = decode_image_with_jpeg_poller(
            &mut Cursor::new(bytes),
            &decode_limits,
            &cancellation,
            |offset| {
                last_polled_offset.set(Some(offset));
                if offset >= JPEG_FILL_CANCEL_OFFSET {
                    cancellation.cancel();
                }
                check_cancel_periodically(&cancellation, offset)
            },
        );

        assert!(matches!(result, Err(Error::Cancelled)));
        assert!(cancellation.is_cancelled());
        assert_eq!(
            last_polled_offset.get(),
            Some(JPEG_FILL_CANCEL_OFFSET),
            "cancellation must be observed at the configured poll inside the fill run"
        );
        assert_eq!(resources.cpu_bytes_in_use(), 0);
        assert_eq!(resources.active_decodes(), 0);
    }

    #[test]
    fn cancellation_inside_jpeg_marker_fill_loop_releases_snapshot_reservation() {
        let mut bytes = vec![0xff, 0xd8];
        bytes.resize(bytes.len() + JPEG_FILL_RUN_BYTES, 0xff);
        bytes.push(0xd9);
        assert!(bytes.len() > JPEG_FILL_CANCEL_OFFSET);

        assert_jpeg_fill_cancellation_releases_reservations(&bytes);
    }

    #[test]
    fn cancellation_inside_jpeg_entropy_fill_loop_releases_snapshot_reservation() {
        let jpeg = progressive_jpeg(WIDTH, HEIGHT);
        let entropy_start = jpeg_entropy_start(&jpeg);
        assert!(entropy_start < JPEG_FILL_CANCEL_OFFSET);

        let mut bytes = jpeg[..entropy_start].to_vec();
        bytes.resize(bytes.len() + JPEG_FILL_RUN_BYTES, 0xff);
        bytes.push(0xd9);
        assert!(bytes.len() > JPEG_FILL_CANCEL_OFFSET);

        assert_jpeg_fill_cancellation_releases_reservations(&bytes);
    }

    #[test]
    fn codec_error_mapping_does_not_mask_errors_when_cancelled() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        assert!(matches!(
            map_png_error(png::DecodingError::LimitsExceeded, &cancellation),
            Error::BudgetExceeded(BudgetKind::RasterBytes)
        ));
        assert!(matches!(
            map_jpeg_error(DecodeErrors::LargeDimensions(1), &cancellation),
            Error::BudgetExceeded(BudgetKind::WorkUnits)
        ));
        assert_eq!(
            map_jpeg_error(
                DecodeErrors::IoErrors(zune_core::bytestream::ZByteIoError::StdIoError(
                    io::Error::new(io::ErrorKind::PermissionDenied, "not cancellation"),
                )),
                &cancellation,
            ),
            Error::PermissionDenied
        );
        assert_eq!(
            map_jpeg_error(
                DecodeErrors::IoErrors(zune_core::bytestream::ZByteIoError::StdIoError(
                    io::Error::new(io::ErrorKind::Interrupted, "not cancelled"),
                )),
                &CancellationToken::new(),
            ),
            Error::TemporarilyUnavailable
        );
        assert_eq!(
            map_io_error(
                io::Error::new(io::ErrorKind::PermissionDenied, "not cancellation"),
                &cancellation,
            ),
            Error::PermissionDenied
        );
    }
}
