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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use ascii_convert::{
        ConversionSettings, GlyphConfig, GlyphRepertoire, GridSizing, LinearRgb,
        NormalizedRasterResources, build_glyph_table, convert_density,
    };
    use ascii_core::{Budget, CancellationToken, FrameMetadata, GlyphTableId};
    use ascii_media::{DecodeLimits, DecodeResources, decode_image};
    use ascii_platform::{HostAdapter, WindowsHost, WindowsHostConfig};

    const MAX_FONT_BYTES: usize = 16 * 1024 * 1024;
    const MAX_ENCODED_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_RASTER_BYTES: u64 = 128 * 1024 * 1024;
    const MAX_FRAME_BYTES: u64 = 64 * 1024 * 1024;
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
    let decode_resources = DecodeResources::new(1, MAX_RASTER_BYTES);
    let decode_limits = DecodeLimits::new(
        budget,
        decode_resources,
        MAX_IMAGE_DIMENSION,
        MAX_IMAGE_DIMENSION,
        MAX_ENCODED_BYTES,
        MAX_RASTER_BYTES,
    );

    let font_bytes = read_bounded(&font_path, MAX_FONT_BYTES)?;
    let repertoire = GlyphRepertoire::new(" .:-=+*#%@")?;
    let glyph_table = Arc::new(build_glyph_table(
        &font_bytes,
        &repertoire,
        &GlyphConfig::new(16.0, 8, 16, 13)?,
        &budget,
        &cancellation,
    )?);
    drop(font_bytes);

    let mut image_file = std::fs::File::open(&image_path)?;
    let decoded = decode_image(&mut image_file, &decode_limits, &cancellation)?;
    let metadata = FrameMetadata::new(
        GlyphTableId::new(*glyph_table.fingerprint()),
        1,
        1,
        0,
        0,
    );
    let settings = ConversionSettings::new(
        GridSizing::columns(columns),
        metadata,
        &cancellation,
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )?;
    // The one-shot conversion is passed directly to the host. The runtime consumer keeps its
    // output lease private, so extracting that frame into the host's Arc would break accounting.
    let normalized_resources = NormalizedRasterResources::new(MAX_RASTER_BYTES);
    let frame = Arc::new(convert_density(
        decoded.view(),
        &glyph_table,
        &settings,
        &budget,
        &normalized_resources,
    )?);
    drop(decoded);

    // The caller supplies its own licensed font file; no font asset is bundled or redistributed.
    let mut host = pollster::block_on(WindowsHost::new(WindowsHostConfig::new(
        Arc::clone(&glyph_table),
        Arc::clone(&frame),
        budget,
        Arc::new(cancellation),
    )))?;
    let displays = host.displays()?;
    if displays.iter().next().is_none() {
        return Err(
            io::Error::new(io::ErrorKind::NotFound, "Windows reported no displays").into(),
        );

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
    Ok(())

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
}
