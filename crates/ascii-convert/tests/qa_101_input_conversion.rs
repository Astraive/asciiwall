#[allow(dead_code)]
mod support;

use ascii_convert::{
    ConversionSettings, DensityConversion, DensityConversionError, FitMode, GlyphConfig,
    GlyphRepertoire, GlyphTable, GridSizing, LinearRgb, PatchDimensions, SamplingFilter,
    SamplingOptions, build_glyph_table, convert_density_detailed,
};
use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, ColorPrimaries, Error,
    FrameMetadata, GlyphTableId, MalformedReason, PixelFormat, RasterFrameView, RasterOrientation,
    TransferFunction,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use sha2::{Digest, Sha256};
use support::synthetic_font::synthetic_font;

struct ConversionAllocationCounter;

#[global_allocator]
static ALLOCATOR: ConversionAllocationCounter = ConversionAllocationCounter;

thread_local! {
    static TRACK_ALLOCATIONS: Cell<bool> = const { Cell::new(false) };
    static ALLOCATION_REQUESTS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for ConversionAllocationCounter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation_request();
        // SAFETY: The layout is forwarded unchanged to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation_request();
        // SAFETY: The layout is forwarded unchanged to the system allocator.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: The pointer and layout are forwarded unchanged to the system allocator.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_allocation_request();
        // SAFETY: The pointer, old layout, and new size are forwarded unchanged.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

fn record_allocation_request() {
    let tracking = TRACK_ALLOCATIONS.try_with(Cell::get).unwrap_or(false);
    if tracking {
        let _ = ALLOCATION_REQUESTS.try_with(|requests| {
            requests.set(requests.get().saturating_add(1));
        });
    }
}

struct AllocationTrackingGuard;

impl Drop for AllocationTrackingGuard {
    fn drop(&mut self) {
        let _ = TRACK_ALLOCATIONS.try_with(|tracking| tracking.set(false));
    }
}

fn count_conversion_allocations<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    TRACK_ALLOCATIONS.with(|tracking| tracking.set(false));
    ALLOCATION_REQUESTS.with(|requests| requests.set(0));
    let guard = AllocationTrackingGuard;
    TRACK_ALLOCATIONS.with(|tracking| tracking.set(true));
    let result = operation();
    TRACK_ALLOCATIONS.with(|tracking| tracking.set(false));
    let allocation_requests = ALLOCATION_REQUESTS.with(Cell::get);
    drop(guard);
    (result, allocation_requests)
}

fn report_fixture(
    name: &str,
    format: &str,
    width: u32,
    height: u32,
    row_stride: usize,
    pixels: &[u8],
) {
    let mut digest = Sha256::new();
    digest.update(b"QA-101 raster fixture v1\0");
    digest.update(format.as_bytes());
    digest.update([0]);
    digest.update(width.to_le_bytes());
    digest.update(height.to_le_bytes());
    digest.update(
        u64::try_from(row_stride)
            .expect("fixture stride fits u64")
            .to_le_bytes(),
    );
    digest.update(pixels);
    println!(
        "fixture={name} format={format} dimensions={width}x{height} row_stride={row_stride} sha256={:x}",
        digest.finalize()
    );
}

fn report_font_fixture(font: &[u8]) {
    println!(
        "fixture=rights-cleared-synthetic-font sha256={:x}",
        Sha256::digest(font)
    );
}

fn report_settings(name: &str, settings: &ConversionSettings<'_>) {
    let mut digest = Sha256::new();
    digest.update(b"QA-101 conversion settings v1\0");

    match settings.sizing() {
        GridSizing::Columns(columns) => {
            digest.update([0]);
            digest.update(columns.to_le_bytes());
        }
        GridSizing::FixedPixelSurface { width, height } => {
            digest.update([1]);
            digest.update(width.to_le_bytes());
            digest.update(height.to_le_bytes());
        }
    }

    let metadata = settings.metadata();
    digest.update(metadata.glyph_table_id().as_bytes());
    digest.update(metadata.frame_id().to_le_bytes());
    digest.update(metadata.generation().to_le_bytes());
    digest.update(metadata.pts_us().to_le_bytes());
    digest.update(metadata.duration_us().to_le_bytes());

    for channel in [
        settings.foreground().red(),
        settings.foreground().green(),
        settings.foreground().blue(),
        settings.background().red(),
        settings.background().green(),
        settings.background().blue(),
    ] {
        digest.update(channel.to_bits().to_le_bytes());
    }

    let sampling = settings.sampling();
    match sampling.fit() {
        FitMode::Contain => digest.update([0]),
        FitMode::Cover => digest.update([1]),
        FitMode::CustomCrop(crop) => {
            digest.update([2]);
            for edge in [crop.left(), crop.top(), crop.right(), crop.bottom()] {
                digest.update(edge.to_bits().to_le_bytes());
            }
        }
    }
    digest.update([match sampling.filter() {
        SamplingFilter::Box => 0,
        SamplingFilter::Nearest => 1,
    }]);
    match sampling.patch() {
        Some(patch) => {
            digest.update([1]);
            digest.update(patch.width().to_le_bytes());
            digest.update(patch.height().to_le_bytes());
        }
        None => digest.update([0]),
    }

    println!("settings={name} sha256={:x}", digest.finalize());
}

// The cancellation token is transient runtime control state, not a stable conversion setting.
const CELL_WIDTH: u32 = 8;
const CELL_HEIGHT: u32 = 12;

fn glyph_table() -> GlyphTable {
    let font = synthetic_font(false);
    report_font_fixture(&font);
    build_glyph_table(
        &font,
        &GlyphRepertoire::new("BA ").expect("fixture repertoire is supported"),
        &GlyphConfig::new(8.0, CELL_WIDTH, CELL_HEIGHT, 9)
            .expect("fixture glyph geometry is valid"),
        &Budget::new(1_000_000, 2_000_000, 2_000_000),
        &CancellationToken::new(),
    )
    .expect("rights-cleared synthetic font builds a measured table")
}

fn budget() -> Budget {
    Budget::new(8_000_000, 64_000_000, 16_000_000)
}

fn metadata(table: &GlyphTable) -> FrameMetadata {
    FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 19, 7, -123, 41_000)
}

fn settings<'a>(
    table: &GlyphTable,
    cancellation: &'a CancellationToken,
    sizing: GridSizing,
    foreground: LinearRgb,
    background: LinearRgb,
) -> ConversionSettings<'a> {
    ConversionSettings::new(
        sizing,
        metadata(table),
        cancellation,
        foreground,
        background,
    )
    .expect("fixture conversion settings are valid")
}

fn gray_raster(pixels: &[u8], width: u32, height: u32, row_stride: usize) -> RasterFrameView<'_> {
    RasterFrameView::new(
        width,
        height,
        row_stride,
        PixelFormat::gray8(),
        ColorDescription::new(
            ColorPrimaries::Srgb,
            TransferFunction::Linear,
            AlphaMode::Opaque,
        ),
        RasterOrientation::TopDown,
        pixels,
    )
}

fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {actual} to be within {tolerance} of {expected}"
    );
}

fn glyph_slot(table: &GlyphTable, character: char) -> u32 {
    u32::try_from(
        table
            .glyphs()
            .iter()
            .position(|glyph| glyph.character() == character)
            .expect("fixture repertoire contains the character"),
    )
    .expect("fixture table position fits u32")
}

#[test]
fn ec_001_zero_and_overflow_geometry_precede_budget_and_output_allocations() {
    let table = glyph_table();
    let cancellation = CancellationToken::new();
    let zero_budget = Budget::new(0, 0, 0);
    let zero_settings = settings(
        &table,
        &cancellation,
        GridSizing::columns(1),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );
    report_settings("ec001-zero-dimensions", &zero_settings);

    for (name, zero_dimension) in [
        ("ec001-zero-width", gray_raster(&[], 0, 1, 0)),
        ("ec001-zero-height", gray_raster(&[], 1, 0, 0)),
    ] {
        report_fixture(
            name,
            "gray8-linear-opaque",
            zero_dimension.width(),
            zero_dimension.height(),
            0,
            &[],
        );
        let (result, allocation_requests) = count_conversion_allocations(|| {
            convert_density_detailed(zero_dimension, &table, &zero_settings, &zero_budget)
        });
        assert_eq!(
            result,
            Err(DensityConversionError::Core(Error::Malformed(
                MalformedReason::EmptyDimensions
            )))
        );
        assert_eq!(
            allocation_requests, 0,
            "zero geometry must be rejected before any output allocation request"
        );
    }

    // This exact empty-backed u32::MAX square plus maximum column count produces a grid whose
    // checked `cell_count * size_of::<AsciiCell>()` byte product overflows. SizeOverflow must win
    // over the zero output-byte budget and malformed backing extent.
    let overflowing_grid = gray_raster(&[], u32::MAX, u32::MAX, 0);
    report_fixture(
        "ec001-overflow-grid",
        "gray8-linear-opaque",
        u32::MAX,
        u32::MAX,
        0,
        &[],
    );
    let overflow_settings = settings(
        &table,
        &cancellation,
        GridSizing::columns(u32::MAX),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );
    report_settings("ec001-overflow-grid", &overflow_settings);
    let (overflow_result, overflow_allocations) = count_conversion_allocations(|| {
        convert_density_detailed(overflowing_grid, &table, &overflow_settings, &zero_budget)
    });
    assert_eq!(
        overflow_result,
        Err(DensityConversionError::Core(Error::SizeOverflow))
    );
    assert_eq!(
        overflow_allocations, 0,
        "overflowed output geometry must be rejected before allocation"
    );

    // This fully backed 1x2 view has a 129-byte readable extent, beyond the 64-byte raster
    // budget. Sampling preflight must reject that extent before normalized-raster/output storage.
    let padded_source = [0_u8; 129];
    let bounded_input = gray_raster(&padded_source, 1, 2, 128);
    report_fixture(
        "ec001-over-budget-readable-extent",
        "gray8-linear-opaque",
        1,
        2,
        128,
        &padded_source,
    );
    let bounded_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(CELL_WIDTH, CELL_HEIGHT),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    );
    report_settings("ec001-over-budget-readable-extent", &bounded_settings);
    let bounded_budget = Budget::new(1024, 64, 1024);
    let (bounded_result, bounded_allocations) = count_conversion_allocations(|| {
        convert_density_detailed(bounded_input, &table, &bounded_settings, &bounded_budget)
    });
    assert_eq!(
        bounded_result,
        Err(DensityConversionError::Core(Error::BudgetExceeded(
            BudgetKind::RasterBytes
        )))
    );
    assert_eq!(
        bounded_allocations, 0,
        "over-budget source extent must be rejected before normalized/output allocation"
    );
}

#[test]
fn ec_002_tiny_raster_resamples_into_larger_grid_without_reading_past_source() {
    let table = glyph_table();
    let cancellation = CancellationToken::new();
    let sampling = SamplingOptions::new(
        FitMode::Contain,
        SamplingFilter::Box,
        Some(PatchDimensions::new(1, 1).expect("one-sample patch is valid")),
    );
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(32, 12),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(sampling);
    report_settings("ec002-tiny-upsample", &conversion_settings);
    // Exactly two backing bytes for a 2x1 source; the 4-cell destination forces upsampling.
    let source = [0, u8::MAX];
    report_fixture(
        "ec002-tiny-upsample",
        "gray8-linear-opaque",
        2,
        1,
        2,
        &source,
    );

    let first = convert_density_detailed(
        gray_raster(&source, 2, 1, 2),
        &table,
        &conversion_settings,
        &budget(),
    )
    .expect("small source converts to a larger requested grid");
    let repeated = convert_density_detailed(
        gray_raster(&source, 2, 1, 2),
        &table,
        &conversion_settings,
        &budget(),
    )
    .expect("repeated small-source conversion succeeds");

    assert_eq!(first.frame().cells().len(), 4);
    let expected_glyphs = [
        glyph_slot(&table, ' '),
        glyph_slot(&table, ' '),
        glyph_slot(&table, 'A'),
        glyph_slot(&table, 'A'),
    ];
    let actual_glyphs = first
        .frame()
        .cells()
        .iter()
        .map(|cell| cell.glyph())
        .collect::<Vec<_>>();
    assert_eq!(actual_glyphs, expected_glyphs);
    assert_eq!(first.frame().cells(), repeated.frame().cells());
    assert_eq!(first.patches(), repeated.patches());
    let patches = first.patches().expect("quality patches were requested");
    let expected = [0.0, 0.0, 1.0, 0.5];
    for (index, expected_luminance) in expected.into_iter().enumerate() {
        let sample = patches
            .luminance_for_cell(index)
            .expect("each destination cell has a bounded sample")[0];
        assert_close(f64::from(sample), expected_luminance, 1.0e-6);
    }
}

fn converted_extreme_raster(
    table: &GlyphTable,
    source: &[u8],
    width: u32,
    height: u32,
    fit: FitMode,
) -> DensityConversion {
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        table,
        &cancellation,
        GridSizing::fixed_pixel_surface(CELL_WIDTH, CELL_HEIGHT),
        LinearRgb::WHITE,
        LinearRgb::BLACK,
    )
    .with_sampling(SamplingOptions::new(
        fit,
        SamplingFilter::Box,
        Some(PatchDimensions::new(1, 1).expect("one-sample patch is valid")),
    ));
    report_settings(
        match fit {
            FitMode::Contain => "ec003-contain",
            FitMode::Cover => "ec003-cover",
            FitMode::CustomCrop(_) => "ec003-custom-crop",
        },
        &conversion_settings,
    );
    let row_stride = usize::try_from(width).expect("fixture row stride fits usize");
    let result = convert_density_detailed(
        gray_raster(source, width, height, row_stride),
        table,
        &conversion_settings,
        &budget(),
    );
    result.unwrap_or_else(|error| {
        panic!("extreme-aspect source {width}x{height} fails with {fit:?}: {error:?}")
    })
}

#[test]
fn ec_003_extreme_portrait_and_wide_sources_preserve_aspect_and_centered_crop_output() {
    let table = glyph_table();
    let target_aspect = f64::from(CELL_WIDTH) / f64::from(CELL_HEIGHT);
    for (width, height) in [(1_u32, 16_u32), (16, 1)] {
        let mut source = vec![0; usize::try_from(width * height).expect("fixture size fits usize")];
        let center = source.len() / 2;
        source[center - 1..center + 1].fill(u8::MAX);
        report_fixture(
            if width == 1 {
                "ec003-extreme-portrait"
            } else {
                "ec003-extreme-wide"
            },
            "gray8-linear-opaque",
            width,
            height,
            usize::try_from(width).expect("fixture row stride fits usize"),
            &source,
        );

        let source_aspect = f64::from(width) / f64::from(height);
        let contain = converted_extreme_raster(&table, &source, width, height, FitMode::Contain);
        let contain_patch = contain
            .patches()
            .expect("quality patches were requested")
            .luminance_for_cell(0)
            .expect("the one-cell output has one patch sample");
        let pixel_scale = if width < height {
            f64::from(CELL_HEIGHT) / f64::from(height)
        } else {
            f64::from(CELL_WIDTH) / f64::from(width)
        };
        let expected_contain_sample =
            2.0 * pixel_scale * pixel_scale / (f64::from(CELL_WIDTH) * f64::from(CELL_HEIGHT));
        assert_close(f64::from(contain_patch[0]), expected_contain_sample, 1.0e-6);
        assert_eq!(contain.frame().cells()[0].glyph(), glyph_slot(&table, ' '));

        // Consumer-visible sample and glyph assertions prove the crop is actually used; a
        // transform that only reports centered geometry but samples the uncropped source fails.
        let cover = converted_extreme_raster(&table, &source, width, height, FitMode::Cover);
        let cover_patch = cover
            .patches()
            .expect("quality patches were requested")
            .luminance_for_cell(0)
            .expect("the one-cell output has one patch sample");
        assert_close(f64::from(cover_patch[0]), 1.0, 1.0e-6);
        assert_eq!(cover.frame().cells()[0].glyph(), glyph_slot(&table, 'A'));

        let contain_transform = contain.transform();
        assert_eq!(contain_transform.source_dimensions(), (width, height));
        let contain_source = contain_transform.source_rect();
        let contain_display = contain_transform.display_rect();
        assert_eq!(contain_source.width(), f64::from(width));
        assert_eq!(contain_source.height(), f64::from(height));
        assert_close(
            contain_display.width() / contain_display.height(),
            source_aspect,
            source_aspect * 1.0e-12,
        );

        let cover_transform = cover.transform();
        let cover_source = cover_transform.source_rect();
        let cover_display = cover_transform.display_rect();
        let display_aspect = cover_display.width() / cover_display.height();
        assert_close(
            cover_source.width() / cover_source.height(),
            display_aspect,
            display_aspect * 1.0e-12,
        );
        let (expected_x, expected_y, expected_width, expected_height) = if width > height {
            (
                (f64::from(width) - target_aspect) * 0.5,
                0.0,
                target_aspect,
                1.0,
            )
        } else {
            (
                0.0,
                (f64::from(height) - 1.0 / target_aspect) * 0.5,
                1.0,
                1.0 / target_aspect,
            )
        };
        assert_close(cover_source.x(), expected_x, 1.0e-12);
        assert_close(cover_source.y(), expected_y, 1.0e-12);
        assert_close(cover_source.width(), expected_width, 1.0e-12);
        assert_close(cover_source.height(), expected_height, 1.0e-12);
    }
}

#[test]
fn ec_005_contrasting_hidden_colors_do_not_create_an_alpha_halo() {
    let table = glyph_table();
    let cancellation = CancellationToken::new();
    let conversion_settings = settings(
        &table,
        &cancellation,
        GridSizing::fixed_pixel_surface(CELL_WIDTH, CELL_HEIGHT),
        LinearRgb::BLACK,
        LinearRgb::WHITE,
    )
    .with_sampling(SamplingOptions::new(
        FitMode::Cover,
        SamplingFilter::Box,
        Some(PatchDimensions::new(1, 1).expect("one-sample patch is valid")),
    ));
    report_settings("ec005-transparent-edge", &conversion_settings);
    // Two saturated, contradictory hidden RGB values share the same alpha-zero edge and opaque
    // yellow neighbor. Correct premultiplication makes their converted output identical.
    let hidden_blue = [0, 0, 255, 0, 255, 255, 0, 255];
    let hidden_magenta = [255, 0, 255, 0, 255, 255, 0, 255];
    report_fixture(
        "ec005-transparent-blue-hidden",
        "rgba8-linear-straight",
        2,
        1,
        8,
        &hidden_blue,
    );
    report_fixture(
        "ec005-transparent-magenta-hidden",
        "rgba8-linear-straight",
        2,
        1,
        8,
        &hidden_magenta,
    );

    let convert_edge = |source: &[u8]| {
        let raster = RasterFrameView::new(
            2,
            1,
            8,
            PixelFormat::rgba8(),
            ColorDescription::new(
                ColorPrimaries::Srgb,
                TransferFunction::Linear,
                AlphaMode::Straight,
            ),
            RasterOrientation::TopDown,
            source,
        );
        convert_density_detailed(raster, &table, &conversion_settings, &budget())
            .expect("transparent colored edge converts")
    };
    let blue_hidden = convert_edge(&hidden_blue);
    let magenta_hidden = convert_edge(&hidden_magenta);
    let expected_luminance = LinearRgb::new(1.0, 1.0, 0.5)
        .expect("expected composite is in range")
        .luminance()
        .value();

    for converted in [&blue_hidden, &magenta_hidden] {
        let patch = converted
            .patches()
            .expect("quality patch records the composited edge")
            .luminance_for_cell(0)
            .expect("the one-cell output has one patch sample");
        assert_close(f64::from(patch[0]), expected_luminance, 1.0e-6);
        assert!(
            f64::from(patch[0]) > 0.85,
            "fully transparent hidden RGB must not darken the opaque yellow edge over white"
        );
        assert_eq!(
            converted.frame().cells()[0].glyph(),
            glyph_slot(&table, ' ')
        );
    }
    assert_eq!(blue_hidden.frame().cells(), magenta_hidden.frame().cells());
    assert_eq!(blue_hidden.patches(), magenta_hidden.patches());
}
