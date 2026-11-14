use ascii_convert::{
    Alpha, ColorNormalizationError, ColorNormalizationWarning, EncodedSrgb, InvalidColorSample,
    LinearRgb, OutputEncoding, SRGB_DECODE_BREAKPOINT, SRGB_ENCODE_BREAKPOINT,
    SRGB_TRANSFER_CONTINUITY_TOLERANCE, UnsupportedColorReason, decode_srgb, encode_output,
    encode_srgb, normalize_raster,
};
use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, ColorPrimaries, Error,
    MalformedReason, PixelFormat, RasterFrameView, RasterOrientation, TransferFunction,
};

fn budget(max_raster_bytes: u64, max_work_units: u64) -> Budget {
    Budget::new(0, max_raster_bytes, max_work_units)
}

fn raster<'a>(
    pixels: &'a [u8],
    width: u32,
    height: u32,
    row_stride: usize,
    format: PixelFormat,
    color: ColorDescription,
    orientation: RasterOrientation,
) -> RasterFrameView<'a> {
    RasterFrameView::new(
        width,
        height,
        row_stride,
        format,
        color,
        orientation,
        pixels,
    )
}

fn normalize_one_pixel(
    pixels: &[u8],
    color: ColorDescription,
) -> ascii_convert::LinearPremultipliedRgba {
    let view = raster(
        pixels,
        1,
        1,
        pixels.len(),
        PixelFormat::rgba8(),
        color,
        RasterOrientation::TopDown,
    );
    normalize_raster(view, &budget(1024, 100), &CancellationToken::new())
        .expect("synthetic pixel must normalize")
        .pixels()[0]
}

fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual:?} differs from expected {expected:?} by more than {tolerance:?}"
    );
}

fn next_down_positive(value: f64) -> f64 {
    f64::from_bits(value.to_bits() - 1)
}

fn next_up_positive(value: f64) -> f64 {
    f64::from_bits(value.to_bits() + 1)
}

#[test]
fn srgb_transfer_uses_breakpoint_neighbors_endpoints_and_round_trips() {
    let below = next_down_positive(SRGB_DECODE_BREAKPOINT);
    let above = next_up_positive(SRGB_DECODE_BREAKPOINT);
    let decoded_below = decode_srgb(EncodedSrgb::new(below, below, below).unwrap()).red();
    let decoded_at = decode_srgb(
        EncodedSrgb::new(
            SRGB_DECODE_BREAKPOINT,
            SRGB_DECODE_BREAKPOINT,
            SRGB_DECODE_BREAKPOINT,
        )
        .unwrap(),
    )
    .red();
    let decoded_above = decode_srgb(EncodedSrgb::new(above, above, above).unwrap()).red();
    assert_eq!(decoded_below, below / 12.92);
    assert_eq!(decoded_at, SRGB_DECODE_BREAKPOINT / 12.92);
    assert_eq!(decoded_above, ((above + 0.055) / 1.055).powf(2.4));
    assert_close(
        decoded_above,
        decoded_at,
        SRGB_TRANSFER_CONTINUITY_TOLERANCE,
    );

    let below_inverse = next_down_positive(SRGB_ENCODE_BREAKPOINT);
    let above_inverse = next_up_positive(SRGB_ENCODE_BREAKPOINT);
    assert_eq!(
        encode_srgb(LinearRgb::new(below_inverse, below_inverse, below_inverse).unwrap()).red(),
        12.92 * below_inverse
    );
    assert_eq!(
        encode_srgb(
            LinearRgb::new(
                SRGB_ENCODE_BREAKPOINT,
                SRGB_ENCODE_BREAKPOINT,
                SRGB_ENCODE_BREAKPOINT
            )
            .unwrap()
        )
        .red(),
        12.92 * SRGB_ENCODE_BREAKPOINT
    );
    assert_eq!(
        encode_srgb(LinearRgb::new(above_inverse, above_inverse, above_inverse).unwrap()).red(),
        1.055 * above_inverse.powf(1.0 / 2.4) - 0.055
    );
    assert_close(
        encode_srgb(LinearRgb::new(above_inverse, above_inverse, above_inverse).unwrap()).red(),
        encode_srgb(
            LinearRgb::new(
                SRGB_ENCODE_BREAKPOINT,
                SRGB_ENCODE_BREAKPOINT,
                SRGB_ENCODE_BREAKPOINT,
            )
            .unwrap(),
        )
        .red(),
        SRGB_TRANSFER_CONTINUITY_TOLERANCE,
    );

    let black = decode_srgb(EncodedSrgb::new(0.0, 0.0, 0.0).unwrap());
    let white = decode_srgb(EncodedSrgb::new(1.0, 1.0, 1.0).unwrap());
    assert_eq!(black, LinearRgb::BLACK);
    assert_eq!(white, LinearRgb::WHITE);
    assert_eq!(
        encode_srgb(LinearRgb::BLACK),
        EncodedSrgb::new(0.0, 0.0, 0.0).unwrap()
    );
    assert_eq!(
        encode_srgb(LinearRgb::WHITE),
        EncodedSrgb::new(1.0, 1.0, 1.0).unwrap()
    );

    for sample in [0.0, 0.001, SRGB_ENCODE_BREAKPOINT, 0.02, 0.18, 0.5, 1.0] {
        let original = LinearRgb::new(sample, sample, sample).unwrap();
        let round_trip = decode_srgb(encode_srgb(original));
        assert_close(round_trip.red(), sample, 2.0e-12);
        assert_close(round_trip.green(), sample, 2.0e-12);
        assert_close(round_trip.blue(), sample, 2.0e-12);
    }
}

#[test]
fn primary_linear_luminance_uses_standard_coefficients() {
    assert_eq!(
        LinearRgb::new(1.0, 0.0, 0.0).unwrap().luminance().value(),
        0.2126
    );
    assert_eq!(
        LinearRgb::new(0.0, 1.0, 0.0).unwrap().luminance().value(),
        0.7152
    );
    assert_eq!(
        LinearRgb::new(0.0, 0.0, 1.0).unwrap().luminance().value(),
        0.0722
    );
    assert_close(LinearRgb::WHITE.luminance().value(), 1.0, 1.0e-15);
}

#[test]
fn alpha_transparent_saturated_rgb_has_zero_contribution_for_both_alpha_modes() {
    let pixel = normalize_one_pixel(
        &[255, 0, 255, 0],
        ColorDescription::srgb(AlphaMode::Straight),
    );
    assert_eq!(pixel.premultiplied_rgb(), LinearRgb::BLACK);
    assert_eq!(pixel.straight_rgb(), LinearRgb::BLACK);
    assert_eq!(pixel.alpha(), Alpha::new(0.0).unwrap());

    let background = LinearRgb::new(0.15, 0.6, 0.9).unwrap();
    assert_eq!(pixel.composite_over(background), background);
    assert_eq!(pixel.premultiplied_rgb().luminance().value(), 0.0);

    let premultiplied = normalize_one_pixel(
        &[255, 0, 255, 0],
        ColorDescription::srgb(AlphaMode::Premultiplied),
    );
    assert_eq!(premultiplied.premultiplied_rgb(), LinearRgb::BLACK);
    assert_eq!(premultiplied.straight_rgb(), LinearRgb::BLACK);
    assert_eq!(premultiplied.alpha(), Alpha::new(0.0).unwrap());
    assert_eq!(premultiplied.composite_over(background), background);
}

#[test]
fn alpha_semi_transparent_red_edges_composite_in_linear_light_on_dark_and_light_backgrounds() {
    let alpha = 128.0 / 255.0;
    let straight = normalize_one_pixel(
        &[255, 0, 0, 128],
        ColorDescription::srgb(AlphaMode::Straight),
    );
    let premultiplied = normalize_one_pixel(
        &[128, 0, 0, 128],
        ColorDescription::srgb(AlphaMode::Premultiplied),
    );

    for pixel in [straight, premultiplied] {
        let dark = pixel.composite_over(LinearRgb::BLACK);
        assert_close(dark.red(), alpha, 1.0e-15);
        assert_eq!(dark.green(), 0.0);
        assert_eq!(dark.blue(), 0.0);

        let light = pixel.composite_over(LinearRgb::WHITE);
        assert_eq!(light.red(), 1.0);
        assert_close(light.green(), 1.0 - alpha, 1.0e-15);
        assert_close(light.blue(), 1.0 - alpha, 1.0e-15);
    }
    assert_eq!(
        straight.premultiplied_rgb(),
        premultiplied.premultiplied_rgb()
    );
}

#[test]
fn declared_linear_input_is_not_srgb_transfer_decoded_again() {
    let sample = 128.0 / 255.0;
    let color = ColorDescription::new(
        ColorPrimaries::Srgb,
        TransferFunction::Linear,
        AlphaMode::Opaque,
    );
    let view = raster(
        &[128, 128, 128],
        1,
        1,
        3,
        PixelFormat::rgb8(),
        color,
        RasterOrientation::TopDown,
    );
    let normalized = normalize_raster(view, &budget(1024, 100), &CancellationToken::new())
        .expect("declared linear samples are already in linear light");
    assert_eq!(
        normalized.pixels()[0].straight_rgb(),
        LinearRgb::new(sample, sample, sample).unwrap()
    );
    assert_eq!(normalized.warning(), None);
}

#[test]
fn metadata_policy_unknown_profile_is_assumed_srgb_with_a_material_warning() {
    let color = ColorDescription::new(
        ColorPrimaries::Other(7),
        TransferFunction::Other(9),
        AlphaMode::Opaque,
    );
    let view = raster(
        &[128, 64, 32],
        1,
        1,
        3,
        PixelFormat::rgb8(),
        color,
        RasterOrientation::TopDown,
    );
    let normalized = normalize_raster(view, &budget(1024, 100), &CancellationToken::new())
        .expect("unknown SDR metadata uses the documented sRGB assumption");
    assert_eq!(
        normalized.warning(),
        Some(ColorNormalizationWarning::AssumedSrgbForUnknownProfile)
    );
    let encoded = EncodedSrgb::new(128.0 / 255.0, 64.0 / 255.0, 32.0 / 255.0).unwrap();
    assert_eq!(normalized.pixels()[0].straight_rgb(), decode_srgb(encoded));
}

#[test]
fn metadata_policy_pq_and_hlg_are_rejected_with_typed_hdr_failures() {
    for transfer in [TransferFunction::Pq, TransferFunction::Hlg] {
        let color = ColorDescription::new(ColorPrimaries::Srgb, transfer, AlphaMode::Opaque);
        let view = raster(
            &[0, 0, 0],
            1,
            1,
            3,
            PixelFormat::rgb8(),
            color,
            RasterOrientation::TopDown,
        );
        let error = normalize_raster(view, &budget(1024, 100), &CancellationToken::new())
            .expect_err("unsupported HDR must not be silently clipped to SDR");
        assert_eq!(
            error,
            ColorNormalizationError::UnsupportedColor(UnsupportedColorReason::HdrTransfer(
                transfer
            ))
        );
        assert!(error.to_string().contains("HDR"));
    }
}

#[test]
fn metadata_policy_unsupported_wide_gamut_is_a_typed_failure() {
    for primaries in [ColorPrimaries::DisplayP3, ColorPrimaries::Rec2020] {
        let color = ColorDescription::new(primaries, TransferFunction::Srgb, AlphaMode::Opaque);
        let view = raster(
            &[0, 0, 0],
            1,
            1,
            3,
            PixelFormat::rgb8(),
            color,
            RasterOrientation::TopDown,
        );
        let error = normalize_raster(view, &budget(1024, 100), &CancellationToken::new())
            .expect_err("wide-gamut input requires color-managed conversion");
        assert_eq!(
            error,
            ColorNormalizationError::UnsupportedColor(UnsupportedColorReason::WideGamutPrimaries(
                primaries
            ))
        );
        assert!(error.to_string().contains("convert to sRGB"));
    }
}

#[test]
fn nonfinite_and_out_of_range_color_samples_are_rejected() {
    assert_eq!(
        EncodedSrgb::new(f64::NAN, 0.0, 0.0),
        Err(ColorNormalizationError::InvalidSample(
            InvalidColorSample::NonFinite
        ))
    );
    assert_eq!(
        LinearRgb::new(0.0, f64::INFINITY, 0.0),
        Err(ColorNormalizationError::InvalidSample(
            InvalidColorSample::NonFinite
        ))
    );
    assert_eq!(
        Alpha::new(-f64::EPSILON),
        Err(ColorNormalizationError::InvalidSample(
            InvalidColorSample::OutOfRange
        ))
    );
    assert_eq!(
        LinearRgb::new(1.0 + f64::EPSILON, 0.0, 0.0),
        Err(ColorNormalizationError::InvalidSample(
            InvalidColorSample::OutOfRange
        ))
    );
}

#[test]
fn premultiplied_channels_greater_than_alpha_are_rejected() {
    let view = raster(
        &[129, 0, 0, 128],
        1,
        1,
        4,
        PixelFormat::rgba8(),
        ColorDescription::srgb(AlphaMode::Premultiplied),
        RasterOrientation::TopDown,
    );
    let error = normalize_raster(view, &budget(1024, 100), &CancellationToken::new())
        .expect_err("a premultiplied channel cannot exceed alpha");
    assert_eq!(
        error,
        ColorNormalizationError::InvalidSample(
            InvalidColorSample::PremultipliedChannelExceedsAlpha
        )
    );
}

#[test]
fn output_encoding_is_explicit_srgb8() {
    let output = encode_output(LinearRgb::WHITE, OutputEncoding::Srgb8);
    assert_eq!(output.encoding(), OutputEncoding::Srgb8);
    assert_eq!(output.channels(), [255, 255, 255]);
}

#[test]
fn raster_budget_counts_input_and_normalized_output_together() {
    let view = raster(
        &[0, 0, 0],
        1,
        1,
        3,
        PixelFormat::rgb8(),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
    );
    let cancellation = CancellationToken::new();

    // The 3-byte input and 32-byte normalized pixel coexist.
    for max_bytes in [32, 34] {
        assert_eq!(
            normalize_raster(view, &budget(max_bytes, 1), &cancellation),
            Err(ColorNormalizationError::Raster(Error::BudgetExceeded(
                BudgetKind::RasterBytes
            )))
        );
    }
    let normalized = normalize_raster(view, &budget(35, 1), &cancellation)
        .expect("the exact combined raster budget must suffice");
    assert_eq!(normalized.pixels().len(), 1);
    assert_eq!(normalized.pixels()[0].straight_rgb(), LinearRgb::BLACK);
}

#[test]
fn raster_budget_counts_inter_row_padding_but_not_trailing_backing() {
    let pixels = [0, 0, 0, 7, 7, 7, 7, 7, 255, 255, 255, 7, 7, 7, 7, 7];
    let color = ColorDescription::srgb(AlphaMode::Opaque);
    let view = raster(
        &pixels,
        1,
        2,
        8,
        PixelFormat::rgb8(),
        color,
        RasterOrientation::TopDown,
    );
    let cancellation = CancellationToken::new();

    // The required input extent is 8 + 3 = 11, plus two 32-byte output pixels.
    assert_eq!(
        normalize_raster(view, &budget(74, 2), &cancellation),
        Err(ColorNormalizationError::Raster(Error::BudgetExceeded(
            BudgetKind::RasterBytes
        )))
    );
    let normalized = normalize_raster(view, &budget(75, 2), &cancellation)
        .expect("the exact extent plus output budget must suffice");
    assert_eq!(normalized.pixels().len(), 2);
    assert_eq!(normalized.pixels()[0].straight_rgb(), LinearRgb::BLACK);
    assert_eq!(normalized.pixels()[1].straight_rgb(), LinearRgb::WHITE);

    let short_backing = raster(
        &pixels[..10],
        1,
        2,
        8,
        PixelFormat::rgb8(),
        color,
        RasterOrientation::TopDown,
    );
    assert_eq!(
        normalize_raster(short_backing, &budget(75, 2), &cancellation),
        Err(ColorNormalizationError::Raster(Error::Malformed(
            MalformedReason::RasterBackingTooShort
        )))
    );
}

#[cfg(target_pointer_width = "64")]
#[test]
fn raster_budget_rejects_overflow_in_combined_input_and_output_size() {
    // The extent fits usize and u64, but adding two output pixels overflows u64.
    let view = raster(
        &[],
        1,
        2,
        usize::MAX - 3,
        PixelFormat::rgb8(),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
    );
    assert_eq!(
        normalize_raster(view, &budget(u64::MAX, u64::MAX), &CancellationToken::new()),
        Err(ColorNormalizationError::Raster(Error::SizeOverflow))
    );
}

#[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
#[test]
fn raster_budget_rejects_overflow_in_output_size() {
    #[cfg(target_pointer_width = "32")]
    let (width, height, stride) = (1, 1 << 27, 1);
    #[cfg(target_pointer_width = "64")]
    let (width, height, stride) = (u32::MAX, u32::MAX, usize::try_from(u32::MAX).unwrap());
    let view = raster(
        &[],
        width,
        height,
        stride,
        PixelFormat::gray8(),
        ColorDescription::srgb(AlphaMode::Opaque),
        RasterOrientation::TopDown,
    );
    assert_eq!(
        normalize_raster(view, &budget(u64::MAX, u64::MAX), &CancellationToken::new()),
        Err(ColorNormalizationError::Raster(Error::SizeOverflow))
    );
}

#[test]
fn raster_orientation_stride_budgets_and_cancellation_are_checked() {
    let srgb_opaque = ColorDescription::srgb(AlphaMode::Opaque);
    let bottom_up = raster(
        &[0, 0, 0],
        1,
        1,
        3,
        PixelFormat::rgb8(),
        srgb_opaque,
        RasterOrientation::BottomUp,
    );
    assert_eq!(
        normalize_raster(bottom_up, &budget(1024, 100), &CancellationToken::new()),
        Err(ColorNormalizationError::Raster(Error::Unsupported(
            ascii_core::UnsupportedReason::Orientation
        )))
    );

    let short_stride = raster(
        &[0; 6],
        2,
        1,
        5,
        PixelFormat::rgb8(),
        srgb_opaque,
        RasterOrientation::TopDown,
    );
    assert_eq!(
        normalize_raster(short_stride, &budget(1024, 100), &CancellationToken::new()),
        Err(ColorNormalizationError::Raster(Error::Malformed(
            MalformedReason::RowStrideTooSmall
        )))
    );

    let one_pixel = raster(
        &[0, 0, 0],
        1,
        1,
        3,
        PixelFormat::rgb8(),
        srgb_opaque,
        RasterOrientation::TopDown,
    );
    assert_eq!(
        normalize_raster(one_pixel, &budget(31, 100), &CancellationToken::new()),
        Err(ColorNormalizationError::Raster(Error::BudgetExceeded(
            BudgetKind::RasterBytes
        )))
    );

    assert_eq!(
        normalize_raster(one_pixel, &budget(1024, 0), &CancellationToken::new()),
        Err(ColorNormalizationError::Raster(Error::BudgetExceeded(
            BudgetKind::WorkUnits
        )))
    );

    let overflowing_extent = raster(
        &[],
        1,
        2,
        usize::MAX,
        PixelFormat::rgb8(),
        srgb_opaque,
        RasterOrientation::TopDown,
    );
    assert_eq!(
        normalize_raster(
            overflowing_extent,
            &budget(u64::MAX, u64::MAX),
            &CancellationToken::new()
        ),
        Err(ColorNormalizationError::Raster(Error::SizeOverflow))
    );

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        normalize_raster(one_pixel, &budget(1024, 100), &cancelled),
        Err(ColorNormalizationError::Raster(Error::Cancelled))
    );
}
