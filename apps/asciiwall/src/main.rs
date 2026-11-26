#[cfg(windows)]
fn main() {
    if let Err(error) = run() {
        eprintln!("asciiwall: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("asciiwall is a Windows-only host application");
    std::process::exit(1);
}

#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use std::io;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use ascii_convert::{
        GlyphConfig, GlyphRecord, GlyphRepertoire, GlyphTable, GridSizing, LinearRgb,
        NormalizedRasterResources, build_glyph_table,
    };
    use ascii_core::{AsciiFrame, Budget, CancellationToken, RasterOrientation};
    use ascii_media::{DecodeLimits, DecodeResources, DecodedFrame, decode_image};
    use ascii_platform::{HostAdapter, WindowsHost, WindowsHostConfig};
    use ascii_runtime::{
        BoundedHandoff, MemoryPermit, MemoryUsage, RasterDescriptorMetadata, RasterToAsciiConsumer,
        ResourceBudget, ResourceLimits, RuntimeError,
    };
    const MAX_FONT_BYTES: usize = 16 * 1024 * 1024;
    const MAX_ENCODED_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_RASTER_BYTES: u64 = 128 * 1024 * 1024;
    const MAX_FRAME_BYTES: u64 = 64 * 1024 * 1024;
    // One slot, one descriptor/control entry, and small config metadata; payloads are charged
    // separately, as is the retained glyph table.
    const MAX_RUNTIME_METADATA_BYTES: u64 = 4 * 1024;
    const MAX_WORK_UNITS: u64 = 1_000_000_000;
    const MAX_IMAGE_DIMENSION: u32 = 4096;
    const DEFAULT_COLUMNS: u32 = 120;

    let mut args = std::env::args_os().skip(1);
    let Some(image_path) = args.next().map(PathBuf::from) else {
        return Err(usage_error().into());
    };
    let Some(font_path) = args.next().map(PathBuf::from) else {
        return Err(usage_error().into());
    };
    let columns = match args.next() {
        Some(value) => value
            .to_str()
            .and_then(|text| text.parse::<u32>().ok())
            .filter(|value| (1..=2048).contains(value))
            .ok_or_else(usage_error)?,
        None => DEFAULT_COLUMNS,
    };
    if args.next().is_some() {
        return Err(usage_error().into());
    }

    let budget = Budget::new(MAX_FRAME_BYTES, MAX_RASTER_BYTES, MAX_WORK_UNITS);
    let cancellation = CancellationToken::new();
    let repertoire = GlyphRepertoire::new(" .:-=+*#%@")?;
    let glyph_config = GlyphConfig::new(16.0, 8, 16, 13)?;
    let (glyph_table_bytes, glyph_table_payload_bytes) =
        glyph_table_memory_bytes(&repertoire, &glyph_config)?;
    let glyph_table_overhead_bytes = glyph_table_bytes
        .checked_sub(glyph_table_payload_bytes)
        .ok_or(RuntimeError::SizeOverflow)?;
    let font_buffer_bytes = u64::try_from(
        MAX_FONT_BYTES
            .checked_add(1)
            .ok_or(RuntimeError::SizeOverflow)?,
    )
    .map_err(|_| RuntimeError::SizeOverflow)?;
    let glyph_build_memory_limit = budget
        .max_raster_bytes()
        .checked_add(glyph_table_overhead_bytes)
        .and_then(|bytes| bytes.checked_add(font_buffer_bytes))
        .and_then(|bytes| bytes.checked_add(MAX_RUNTIME_METADATA_BYTES))
        .ok_or(RuntimeError::SizeOverflow)?;
    let raster_pipeline_memory_limit = MAX_RASTER_BYTES
        .checked_mul(4)
        .and_then(|raster_bytes| {
            MAX_FRAME_BYTES
                .checked_mul(2)
                .and_then(|frame_bytes| raster_bytes.checked_add(frame_bytes))
        })
        .and_then(|bytes| bytes.checked_add(glyph_table_bytes))
        .and_then(|bytes| bytes.checked_add(MAX_RUNTIME_METADATA_BYTES))
        .ok_or(RuntimeError::SizeOverflow)?;
    let runtime_memory_limit = raster_pipeline_memory_limit.max(glyph_build_memory_limit);
    let runtime_resources =
        ResourceBudget::new(ResourceLimits::new(1, 1, 1, runtime_memory_limit, 0));
    // Charge both the retained table and read_bounded's full reserved font-buffer capacity before
    // allocation. The builder's core budget includes font length but not Vec capacity.
    let _glyph_table_memory =
        runtime_resources.try_reserve_memory(MemoryUsage::new(glyph_table_bytes, 0))?;
    let _font_buffer_memory =
        runtime_resources.try_reserve_memory(MemoryUsage::new(font_buffer_bytes, 0))?;
    let font_bytes = read_bounded(&font_path, MAX_FONT_BYTES)?;
    let font_input_bytes =
        u64::try_from(font_bytes.len()).map_err(|_| RuntimeError::SizeOverflow)?;
    let glyph_build_memory_bytes = budget
        .max_raster_bytes()
        .checked_sub(glyph_table_payload_bytes)
        .and_then(|bytes| bytes.checked_sub(font_input_bytes))
        .ok_or(RuntimeError::MemoryBudgetExceeded)?;
    let _glyph_build_memory =
        runtime_resources.try_reserve_memory(MemoryUsage::new(glyph_build_memory_bytes, 0))?;
    let glyph_table = Arc::new(build_glyph_table(
        &font_bytes,
        &repertoire,
        &glyph_config,
        &budget,
        &cancellation,
    )?);
    drop(font_bytes);
    drop(repertoire);
    drop(_glyph_build_memory);
    drop(_font_buffer_memory);

    let decode_resources = DecodeResources::new(1, MAX_RASTER_BYTES);
    let decode_limits = DecodeLimits::new(
        budget,
        decode_resources.clone(),
        MAX_IMAGE_DIMENSION,
        MAX_IMAGE_DIMENSION,
        MAX_ENCODED_BYTES,
        MAX_RASTER_BYTES,
    );

    let mut image_file = std::fs::File::open(&image_path)?;
    // DecodeResources bounds the compressed snapshot, codec scratch/temporary source, and output
    // together. Mirror that full one-shot ceiling in the process-wide ledger while decoding.
    let _decoder_memory_ceiling =
        runtime_resources.try_reserve_memory(MemoryUsage::new(MAX_RASTER_BYTES, 0))?;
    let decode_permit = runtime_resources.try_acquire_decode()?;
    let decoded = decode_image(&mut image_file, &decode_limits, &cancellation)?;
    drop(decode_permit);
    let decoded_memory = runtime_resources
        .try_reserve_memory(MemoryUsage::new(decode_resources.cpu_bytes_in_use(), 0))?;
    drop(_decoder_memory_ceiling);
    let decoded = ChargedDecodedFrame {
        frame: decoded,
        _memory_permit: decoded_memory,
    };
    let input_bytes = decoded.frame.pixels().len();
    let handoff = BoundedHandoff::new(&runtime_resources, 1, 1, input_bytes, 1, 1)?;
    handoff.submit_frame(
        RasterDescriptorMetadata {
            width: decoded.frame.width(),
            height: decoded.frame.height(),
            row_stride: decoded.frame.row_stride(),
            format: decoded.frame.format(),
            color: decoded.frame.color(),
            orientation: RasterOrientation::TopDown,
            media_timestamp_ns: 0,
        },
        decoded.frame.pixels(),
        &budget,
        &cancellation,
    )?;
    // Submission copied source bytes into the bounded pool; release the decoded raster before
    // allocating consumer scratch or converting.
    drop(decoded);
    let mut consumer = RasterToAsciiConsumer::new(
        &runtime_resources,
        input_bytes,
        GridSizing::columns(columns),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
        NormalizedRasterResources::new(MAX_RASTER_BYTES),
    )?;
    // Conversion normalizes at most one raster; reserve that independent resource ceiling in the
    // same ledger while the single consumer runs.
    let _normalized_memory =
        runtime_resources.try_reserve_memory(MemoryUsage::new(MAX_RASTER_BYTES, 0))?;

    // The consumer holds an output-count permit, but its ASCII cell allocation is only bounded by
    // the per-frame core budget. Reserve that maximum before conversion so the original output and
    // host-owned copy both stay covered by the shared CPU-memory ledger.
    let _converted_frame_memory = reserve_frame_memory(&budget, &runtime_resources, false)?;
    let converted = consumer
        .convert_next(&handoff, &glyph_table, &budget, &cancellation)?
        .ok_or_else(|| io::Error::other("bounded handoff produced no converted frame"))?;
    drop(_normalized_memory);
    drop(consumer);
    drop(handoff);

    // WindowsHost requires Arc ownership, so copy only while retaining `converted` (and its
    // acknowledged source lease/output permit) for the full host lifetime.
    let _host_frame_memory = reserve_frame_memory(&budget, &runtime_resources, true)?;
    let glyph_count = u32::try_from(glyph_table.glyphs().len())?;
    let frame = Arc::new(copy_frame_for_host(
        converted.frame(),
        glyph_count,
        &budget,
        &cancellation,
    )?);
    // The caller supplies its own licensed font file; no font asset is bundled or redistributed.
    let mut host = pollster::block_on(WindowsHost::new(WindowsHostConfig::new(
        Arc::clone(&glyph_table),
        Arc::clone(&frame),
        budget,
        Arc::new(cancellation),
    )))?;
    let displays = host.displays()?;
    if displays.iter().next().is_none() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Windows reported no displays").into());
    }

    let mut leases = Vec::new();
    for display in displays.iter() {
        leases
            .try_reserve(1)
            .map_err(|_| io::Error::other("unable to reserve display leases"))?;
        let lease = host.attach(display)?;
        leases.push(lease);
        host.present(lease, Arc::clone(&frame))?;
    }

    println!("asciiwall is presenting the selected image; press Enter to detach and exit");
    let stop = Arc::new(AtomicBool::new(false));
    let stop_reader = Arc::clone(&stop);
    let input_thread = std::thread::Builder::new()
        .name("asciiwall-console-input".to_owned())
        .spawn(move || {
            let result = wait_for_enter();
            stop_reader.store(true, Ordering::Release);
            result
        })?;

    while !stop.load(Ordering::Acquire) {
        let _events = host.poll();
        std::thread::sleep(Duration::from_millis(16));
    }
    input_thread
        .join()
        .map_err(|_| io::Error::other("console input thread panicked"))??;
    for lease in leases {
        host.detach(lease)?;
    }

    fn glyph_table_memory_bytes(
        repertoire: &GlyphRepertoire,
        config: &GlyphConfig,
    ) -> Result<(u64, u64), RuntimeError> {
        // The builder reserves the record vector and each mask from these validated sizes.
        let glyph_count =
            u64::try_from(repertoire.characters().len()).map_err(|_| RuntimeError::SizeOverflow)?;
        let mask_bytes = u64::from(config.cell_width())
            .checked_mul(u64::from(config.cell_height()))
            .and_then(|bytes| bytes.checked_mul(glyph_count))
            .ok_or(RuntimeError::SizeOverflow)?;
        let record_bytes = u64::try_from(std::mem::size_of::<GlyphRecord>())
            .map_err(|_| RuntimeError::SizeOverflow)?
            .checked_mul(glyph_count)
            .ok_or(RuntimeError::SizeOverflow)?;
        let table_payload_bytes = record_bytes
            .checked_add(mask_bytes)
            .ok_or(RuntimeError::SizeOverflow)?;
        let allocation_headers = std::mem::size_of::<GlyphTable>()
            .checked_add(std::mem::size_of::<[usize; 2]>())
            .ok_or(RuntimeError::SizeOverflow)?;
        let allocation_headers =
            u64::try_from(allocation_headers).map_err(|_| RuntimeError::SizeOverflow)?;
        let table_bytes = allocation_headers
            .checked_add(table_payload_bytes)
            .ok_or(RuntimeError::SizeOverflow)?;
        Ok((table_bytes, table_payload_bytes))
    }

    fn usage_error() -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "usage: asciiwall <image.png|jpg> <licensed-font-file> [columns: 1..2048]",
        )
    }

    fn read_bounded(path: &std::path::Path, maximum: usize) -> std::io::Result<Vec<u8>> {
        use std::io::Read as _;
        let mut file = std::fs::File::open(path)?;
        let maximum_plus_one = maximum
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("font size limit overflow"))?;
        let read_limit = u64::try_from(maximum_plus_one)
            .map_err(|_| std::io::Error::other("font size limit exceeds u64"))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(maximum_plus_one)
            .map_err(|_| std::io::Error::other("unable to reserve bounded font input"))?;
        file.by_ref().take(read_limit).read_to_end(&mut bytes)?;
        if bytes.len() > maximum {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "font exceeds the 16 MiB input limit",
            ));
        }
        Ok(bytes)
    }

    fn wait_for_enter() -> std::io::Result<()> {
        use std::io::Read as _;

        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        let mut byte = [0_u8; 1];
        loop {
            match reader.read(&mut byte) {
                Ok(0) => return Ok(()),
                Ok(_) if byte[0] == b'\n' || byte[0] == b'\r' => return Ok(()),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
    struct ChargedDecodedFrame {
        frame: DecodedFrame,
        // Keep this after `frame` so its backing pixels are freed before their runtime charge.
        _memory_permit: MemoryPermit,
    }
    fn reserve_frame_memory(
        budget: &Budget,
        resources: &ResourceBudget,
        arc_owned: bool,
    ) -> Result<ascii_runtime::MemoryPermit, RuntimeError> {
        let arc_header_bytes = if arc_owned {
            std::mem::size_of::<[usize; 2]>()
        } else {
            0
        };
        let overhead = std::mem::size_of::<AsciiFrame>()
            .checked_add(arc_header_bytes)
            .ok_or(RuntimeError::SizeOverflow)?;
        let overhead = u64::try_from(overhead).map_err(|_| RuntimeError::SizeOverflow)?;
        let bytes = budget
            .max_frame_bytes()
            .checked_add(overhead)
            .ok_or(RuntimeError::SizeOverflow)?;
        resources.try_reserve_memory(MemoryUsage::new(bytes, 0))
    }

    fn copy_frame_for_host(
        source: &AsciiFrame,
        glyph_count: u32,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<AsciiFrame, Box<dyn std::error::Error>> {
        let mut cells = Vec::new();
        cells.try_reserve_exact(source.cells().len())?;
        cells.extend_from_slice(source.cells());
        Ok(AsciiFrame::try_new(
            source.size(),
            source.metadata(),
            cells,
            glyph_count,
            budget,
            cancellation,
        )?)
    }

    Ok(())
}
