use std::mem::size_of;

use ascii_convert::{
    ColorNormalizationError, LinearPremultipliedRgba, NormalizedRasterResources, normalize_raster,
};
use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, Error, PixelFormat,
    RasterFrameView, RasterOrientation,
};

fn gray_pixel_view(pixels: &[u8]) -> RasterFrameView<'_> {
    RasterFrameView::new(
        1,
        1,
        1,
        PixelFormat::gray8(),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
        pixels,
    )
}

fn output_bytes() -> u64 {
    u64::try_from(size_of::<LinearPremultipliedRgba>()).expect("output size fits u64")
}

fn operation_budget() -> Budget {
    Budget::new(0, output_bytes() + 1, 1)
}

#[test]
fn exact_retained_output_ceiling_accepts_one_normalized_raster() {
    let pixels = [128_u8];
    let resources = NormalizedRasterResources::new(output_bytes());
    let normalized = normalize_raster(
        gray_pixel_view(&pixels),
        &operation_budget(),
        &CancellationToken::new(),
        &resources,
    )
    .expect("exact retained output capacity is sufficient");

    assert_eq!(resources.max_retained_bytes(), output_bytes());
    assert_eq!(resources.retained_bytes_in_use(), output_bytes());
    assert_eq!(normalized.pixels().len(), 1);
}

#[test]
fn second_raster_is_rejected_at_retained_boundary_and_drop_releases_capacity() {
    let pixels = [128_u8];
    let resources = NormalizedRasterResources::new(output_bytes());
    let first = normalize_raster(
        gray_pixel_view(&pixels),
        &operation_budget(),
        &CancellationToken::new(),
        &resources,
    )
    .expect("first raster fills the retained ceiling exactly");
    let sibling_resources = resources.clone();

    assert_eq!(
        normalize_raster(
            gray_pixel_view(&pixels),
            &operation_budget(),
            &CancellationToken::new(),
            &sibling_resources,
        ),
        Err(ColorNormalizationError::Raster(Error::BudgetExceeded(
            BudgetKind::RasterBytes
        )))
    );
    assert_eq!(resources.retained_bytes_in_use(), output_bytes());

    drop(first);
    assert_eq!(resources.retained_bytes_in_use(), 0);

    let next = normalize_raster(
        gray_pixel_view(&pixels),
        &operation_budget(),
        &CancellationToken::new(),
        &resources,
    )
    .expect("dropping the retained raster releases its capacity");
    assert_eq!(resources.retained_bytes_in_use(), output_bytes());
    drop(next);
    assert_eq!(resources.retained_bytes_in_use(), 0);
}

#[test]
fn pixel_validation_error_releases_reservation() {
    let pixels = [129_u8, 0, 0, 128];
    let view = RasterFrameView::new(
        1,
        1,
        4,
        PixelFormat::rgba8(),
        ColorDescription::srgb(AlphaMode::Premultiplied),
        RasterOrientation::TopDown,
        &pixels,
    );
    let retained_output_bytes = output_bytes();
    let resources = NormalizedRasterResources::new(retained_output_bytes);
    let budget = Budget::new(0, retained_output_bytes + 4, 2);

    assert_eq!(
        normalize_raster(view, &budget, &CancellationToken::new(), &resources),
        Err(ColorNormalizationError::InvalidSample(
            ascii_convert::InvalidColorSample::PremultipliedChannelExceedsAlpha
        ))
    );
    assert_eq!(resources.retained_bytes_in_use(), 0);
}
