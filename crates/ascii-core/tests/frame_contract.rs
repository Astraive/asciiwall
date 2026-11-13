use std::vec;
use std::vec::Vec;

use ascii_core::{
    AlphaMode, AsciiCell, AsciiFrame, Budget, BudgetKind, CancellationToken, ColorDescription,
    ColorPrimaries, Error, GlyphTableId, GridSize, MalformedReason, PixelChannels, PixelFormat,
    RasterFrameView, RasterOrientation, TransferFunction, UnsupportedReason, validate_frame,
    validate_grid_budget, validate_raster_frame,
};

fn budget() -> Budget {
    Budget::new(u64::MAX, u64::MAX, u64::MAX)
}

fn table_id(byte: u8) -> GlyphTableId {
    GlyphTableId::new([byte; 32])
}

fn cells(count: usize, glyph: u32) -> Vec<AsciiCell> {
    (0..count)
        .map(|_| AsciiCell::new(glyph, [1, 2, 3, 255], [4, 5, 6, 255]))
        .collect()
}

fn make_frame(
    size: GridSize,
    frame_cells: Vec<AsciiCell>,
    actual_table_id: GlyphTableId,
    expected_table_id: GlyphTableId,
    glyph_count: u32,
    limits: &Budget,
    cancellation: &CancellationToken,
) -> Result<AsciiFrame, Error> {
    let metadata = ascii_core::FrameMetadata::new(actual_table_id, 42, 7, -123_456, 33_333);
    let frame = AsciiFrame::try_new(
        size,
        metadata,
        frame_cells,
        glyph_count,
        limits,
        cancellation,
    )?;
    validate_frame(&frame, expected_table_id, glyph_count, limits, cancellation)?;
    Ok(frame)
}

fn raster<'a>(pixels: &'a [u8]) -> RasterFrameView<'a> {
    RasterFrameView::new(
        3,
        2,
        12,
        PixelFormat::rgb8(),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
        pixels,
    )
}

#[test]
fn grid_preflight_allows_allocation_and_constructs_exact_valid_frame() {
    let size = GridSize::new(4, 4);
    let limits = budget();
    let cancellation = CancellationToken::new();
    let plan = validate_grid_budget(size, &limits, &cancellation).unwrap();

    assert_eq!(plan.cell_count(), 16);
    assert_eq!(plan.cell_count_usize(), 16);
    assert_eq!(plan.work_units(), 16);
    let frame_cells = cells(plan.cell_count_usize(), 7);
    let frame = make_frame(
        size,
        frame_cells,
        table_id(1),
        table_id(1),
        8,
        &limits,
        &cancellation,
    )
    .unwrap();

    assert_eq!(frame.size(), size);
    assert_eq!(frame.cells().len(), 16);
    assert_eq!(frame.cells()[0].glyph(), 7);
    assert_eq!(frame.frame_id(), 42);
    assert_eq!(frame.generation(), 7);
    assert_eq!(frame.pts_us(), -123_456);
    assert_eq!(frame.duration_us(), 33_333);
}

#[test]
fn frame_rejects_cell_count_mismatch_and_out_of_range_glyph() {
    let limits = budget();
    let cancellation = CancellationToken::new();

    assert!(matches!(
        make_frame(
            GridSize::new(4, 4),
            cells(15, 7),
            table_id(1),
            table_id(1),
            8,
            &limits,
            &cancellation,
        ),
        Err(Error::Malformed(MalformedReason::CellCountMismatch))
    ));
    assert!(matches!(
        make_frame(
            GridSize::new(1, 1),
            cells(1, 8),
            table_id(1),
            table_id(1),
            8,
            &limits,
            &cancellation,
        ),
        Err(Error::Malformed(MalformedReason::GlyphIndexOutOfRange))
    ));
}

#[test]
fn frame_validation_binds_the_expected_glyph_table_identity() {
    let limits = budget();
    let cancellation = CancellationToken::new();
    let frame = make_frame(
        GridSize::new(1, 1),
        cells(1, 0),
        table_id(1),
        table_id(1),
        1,
        &limits,
        &cancellation,
    )
    .unwrap();

    assert!(matches!(
        validate_frame(&frame, table_id(2), 1, &limits, &cancellation),
        Err(Error::Malformed(MalformedReason::GlyphTableMismatch))
    ));
}

#[test]
fn grid_budget_rejects_frame_bytes_and_work_before_allocation() {
    let cancellation = CancellationToken::new();
    let cell_bytes = u64::try_from(std::mem::size_of::<AsciiCell>()).unwrap();

    assert!(matches!(
        validate_grid_budget(
            GridSize::new(1, 1),
            &Budget::new(cell_bytes - 1, u64::MAX, 1),
            &cancellation,
        ),
        Err(Error::BudgetExceeded(BudgetKind::FrameBytes))
    ));
    assert!(matches!(
        validate_grid_budget(
            GridSize::new(4, 4),
            &Budget::new(u64::MAX, u64::MAX, 15),
            &cancellation,
        ),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    ));

    let exact = validate_grid_budget(
        GridSize::new(1, 1),
        &Budget::new(cell_bytes, u64::MAX, 1),
        &cancellation,
    )
    .unwrap();
    assert_eq!(exact.frame_bytes(), cell_bytes);
    assert_eq!(exact.work_units(), 1);
}

#[test]
fn frame_rejects_owned_cell_capacity_over_budget() {
    let cancellation = CancellationToken::new();
    let one_cell_bytes = u64::try_from(std::mem::size_of::<AsciiCell>()).unwrap();
    let limits = Budget::new(one_cell_bytes, u64::MAX, u64::MAX);
    let mut frame_cells = Vec::with_capacity(64);
    frame_cells.push(AsciiCell::new(0, [0; 4], [0; 4]));

    assert!(matches!(
        make_frame(
            GridSize::new(1, 1),
            frame_cells,
            table_id(1),
            table_id(1),
            1,
            &limits,
            &cancellation,
        ),
        Err(Error::BudgetExceeded(BudgetKind::FrameBytes))
    ));
}

#[test]
fn grid_size_product_is_exact_but_cell_byte_size_overflow_is_rejected() {
    let size = GridSize::new(u32::MAX, u32::MAX);
    assert_eq!(size.cell_count(), u32::MAX as u64 * u32::MAX as u64);

    assert!(matches!(
        validate_grid_budget(size, &budget(), &CancellationToken::new()),
        Err(Error::SizeOverflow)
    ));
}
#[test]
fn grid_preflight_rejects_cell_storage_beyond_the_allocator_object_limit() {
    let cell_bytes = u64::try_from(std::mem::size_of::<AsciiCell>()).unwrap();
    let cells_beyond_isize = (isize::MAX as u64 / cell_bytes) + 1;
    let columns = u64::from(u32::MAX);
    let rows = cells_beyond_isize.div_ceil(columns);
    let size = GridSize::new(u32::MAX, u32::try_from(rows).unwrap());

    assert!(matches!(
        validate_grid_budget(size, &budget(), &CancellationToken::new()),
        Err(Error::SizeOverflow)
    ));
}

#[test]
fn preflight_and_frame_validation_observe_cancellation() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    assert!(matches!(
        validate_grid_budget(GridSize::new(0, 1), &budget(), &CancellationToken::new()),
        Err(Error::Malformed(MalformedReason::EmptyDimensions))
    ));
    assert!(matches!(
        validate_grid_budget(GridSize::new(1, 1), &budget(), &cancellation),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        make_frame(
            GridSize::new(1, 1),
            cells(1, 0),
            table_id(1),
            table_id(1),
            1,
            &budget(),
            &cancellation,
        ),
        Err(Error::Cancelled)
    ));
    let pixels = vec![0; 21];
    assert!(matches!(
        validate_raster_frame(raster(&pixels), &budget(), &cancellation),
        Err(Error::Cancelled)
    ));
}

#[test]
fn raster_accepts_padded_rows_and_trailing_bytes_without_copying() {
    let pixels = vec![0; 23];
    let validated =
        validate_raster_frame(raster(&pixels), &budget(), &CancellationToken::new()).unwrap();

    assert_eq!(validated.row_bytes(), 9);
    assert_eq!(validated.required_extent(), 21);
    assert_eq!(validated.pixel_count(), 6);
    assert_eq!(validated.pixels().len(), 23);
    assert_eq!(validated.readable_pixels().len(), 21);
    assert_eq!(validated.view().pixels().as_ptr(), pixels.as_ptr());
}

#[test]
fn raster_rejects_backing_one_byte_short_of_the_last_pixel() {
    let pixels = vec![0; 20];

    assert!(matches!(
        validate_raster_frame(raster(&pixels), &budget(), &CancellationToken::new()),
        Err(Error::Malformed(MalformedReason::RasterBackingTooShort))
    ));
}

#[test]
fn raster_rejects_zero_dimensions_and_a_stride_shorter_than_pixel_rows() {
    let pixels = [0; 32];
    let limits = budget();
    let cancellation = CancellationToken::new();
    let color = ColorDescription::srgb(AlphaMode::Opaque);

    assert!(matches!(
        validate_raster_frame(
            RasterFrameView::new(
                0,
                2,
                0,
                PixelFormat::rgb8(),
                color,
                RasterOrientation::TopDown,
                &pixels,
            ),
            &limits,
            &cancellation,
        ),
        Err(Error::Malformed(MalformedReason::EmptyDimensions))
    ));
    assert!(matches!(
        validate_raster_frame(
            RasterFrameView::new(
                1,
                0,
                0,
                PixelFormat::rgb8(),
                color,
                RasterOrientation::TopDown,
                &pixels,
            ),
            &limits,
            &cancellation,
        ),
        Err(Error::Malformed(MalformedReason::EmptyDimensions))
    ));
    assert!(matches!(
        validate_raster_frame(
            RasterFrameView::new(
                3,
                2,
                8,
                PixelFormat::rgb8(),
                color,
                RasterOrientation::TopDown,
                &pixels,
            ),
            &limits,
            &cancellation,
        ),
        Err(Error::Malformed(MalformedReason::RowStrideTooSmall))
    ));
}
#[test]
fn raster_rejects_overflow_in_the_last_row_extent() {
    let view = RasterFrameView::new(
        1,
        2,
        usize::MAX,
        PixelFormat::rgb8(),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
        &[],
    );

    assert!(matches!(
        validate_raster_frame(view, &budget(), &CancellationToken::new()),
        Err(Error::SizeOverflow)
    ));
}

#[test]
fn raster_rejects_unsupported_sample_depth_orientation_and_color_transfer() {
    let pixels = [0; 32];
    let limits = budget();
    let cancellation = CancellationToken::new();
    let opaque_srgb = ColorDescription::srgb(AlphaMode::Opaque);

    assert!(matches!(
        validate_raster_frame(
            RasterFrameView::new(
                1,
                1,
                3,
                PixelFormat::new(PixelChannels::Rgb, 16),
                opaque_srgb,
                RasterOrientation::TopDown,
                &pixels,
            ),
            &limits,
            &cancellation,
        ),
        Err(Error::Unsupported(UnsupportedReason::SampleDepth))
    ));
    assert!(matches!(
        validate_raster_frame(
            RasterFrameView::new(
                1,
                1,
                3,
                PixelFormat::rgb8(),
                opaque_srgb,
                RasterOrientation::BottomUp,
                &pixels,
            ),
            &limits,
            &cancellation,
        ),
        Err(Error::Unsupported(UnsupportedReason::Orientation))
    ));
    assert!(matches!(
        validate_raster_frame(
            RasterFrameView::new(
                1,
                1,
                3,
                PixelFormat::rgb8(),
                ColorDescription::new(
                    ColorPrimaries::Srgb,
                    TransferFunction::Linear,
                    AlphaMode::Opaque,
                ),
                RasterOrientation::TopDown,
                &pixels,
            ),
            &limits,
            &cancellation,
        ),
        Err(Error::Unsupported(UnsupportedReason::ColorDescription))
    ));
}

#[test]
fn raster_rejects_incompatible_channel_metadata_and_enforces_budget() {
    let pixels = [0; 32];
    let cancellation = CancellationToken::new();

    assert!(matches!(
        validate_raster_frame(
            RasterFrameView::new(
                1,
                1,
                4,
                PixelFormat::rgba8(),
                ColorDescription::srgb(AlphaMode::Opaque),
                RasterOrientation::TopDown,
                &pixels,
            ),
            &budget(),
            &cancellation,
        ),
        Err(Error::Unsupported(UnsupportedReason::AlphaMode))
    ));

    let padded = raster(&pixels);
    assert!(matches!(
        validate_raster_frame(padded, &Budget::new(u64::MAX, 20, u64::MAX), &cancellation,),
        Err(Error::BudgetExceeded(BudgetKind::RasterBytes))
    ));
    assert!(matches!(
        validate_raster_frame(padded, &Budget::new(u64::MAX, u64::MAX, 5), &cancellation,),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    ));
}
#[test]
fn frame_parts_preserve_the_cell_allocation_for_reuse() {
    let mut frame_cells = Vec::with_capacity(8);
    frame_cells.push(AsciiCell::new(0, [0; 4], [0; 4]));
    let original_capacity = frame_cells.capacity();
    let cell_bytes = u64::try_from(std::mem::size_of::<AsciiCell>()).unwrap();
    let limits = Budget::new(
        u64::try_from(original_capacity).unwrap() * cell_bytes,
        u64::MAX,
        u64::MAX,
    );
    let frame = make_frame(
        GridSize::new(1, 1),
        frame_cells,
        table_id(1),
        table_id(1),
        1,
        &limits,
        &CancellationToken::new(),
    )
    .unwrap();

    let (size, metadata, recovered_cells) = frame.into_parts();

    assert_eq!(size, GridSize::new(1, 1));
    assert_eq!(metadata.glyph_table_id(), table_id(1));
    assert_eq!(metadata.frame_id(), 42);
    assert_eq!(metadata.pts_us(), -123_456);
    assert_eq!(recovered_cells.len(), 1);
    assert_eq!(recovered_cells.capacity(), original_capacity);
}

#[test]
fn raster_accepts_exact_byte_and_work_budgets() {
    let pixels = vec![0; 21];
    let limits = Budget::new(u64::MAX, 21, 6);

    let validated =
        validate_raster_frame(raster(&pixels), &limits, &CancellationToken::new()).unwrap();

    assert_eq!(validated.required_extent(), 21);
    assert_eq!(validated.pixel_count(), 6);
}

#[test]
fn raster_rejects_unsupported_primaries_and_unrecognized_channel_layouts() {
    let pixels = [0; 4];
    let limits = budget();
    let cancellation = CancellationToken::new();

    assert!(matches!(
        validate_raster_frame(
            RasterFrameView::new(
                1,
                1,
                3,
                PixelFormat::rgb8(),
                ColorDescription::new(
                    ColorPrimaries::DisplayP3,
                    TransferFunction::Srgb,
                    AlphaMode::Opaque,
                ),
                RasterOrientation::TopDown,
                &pixels,
            ),
            &limits,
            &cancellation,
        ),
        Err(Error::Unsupported(UnsupportedReason::ColorDescription))
    ));
    assert!(matches!(
        validate_raster_frame(
            RasterFrameView::new(
                1,
                1,
                4,
                PixelFormat::new(PixelChannels::Other(4), 8),
                ColorDescription::srgb(AlphaMode::Opaque),
                RasterOrientation::TopDown,
                &pixels,
            ),
            &limits,
            &cancellation,
        ),
        Err(Error::Unsupported(UnsupportedReason::PixelFormat))
    ));
}

#[test]
fn raster_rejects_alpha_metadata_for_gray_and_rgb_channels() {
    let pixels = [0; 4];
    let limits = budget();
    let cancellation = CancellationToken::new();

    for (format, row_bytes, alpha_mode) in [
        (PixelFormat::gray8(), 1, AlphaMode::Premultiplied),
        (PixelFormat::rgb8(), 3, AlphaMode::Straight),
    ] {
        assert!(matches!(
            validate_raster_frame(
                RasterFrameView::new(
                    1,
                    1,
                    row_bytes,
                    format,
                    ColorDescription::srgb(alpha_mode),
                    RasterOrientation::TopDown,
                    &pixels,
                ),
                &limits,
                &cancellation,
            ),
            Err(Error::Unsupported(UnsupportedReason::AlphaMode))
        ));
    }
}

#[test]
fn grid_preflight_rejects_zero_rows_as_well_as_zero_columns() {
    for size in [GridSize::new(0, 1), GridSize::new(1, 0)] {
        assert!(matches!(
            validate_grid_budget(size, &budget(), &CancellationToken::new()),
            Err(Error::Malformed(MalformedReason::EmptyDimensions))
        ));
    }
}

#[test]
fn core_errors_are_displayable_and_implement_the_core_error_trait() {
    fn assert_core_error<E: core::error::Error>() {}

    assert_core_error::<Error>();
    assert_eq!(
        Error::BudgetExceeded(BudgetKind::RasterBytes).to_string(),
        "resource budget exceeded"
    );
}
