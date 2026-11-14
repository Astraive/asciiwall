//! Explicit SDR transfer, alpha, and raster normalization boundaries.
//!
//! Source bytes are normalized to linear-light, premultiplied RGBA. Premultiplied sRGB input is
//! interpreted as associated in the encoded sRGB domain and unassociated before transfer decoding;
//! output pixels are always associated in linear light. Unknown source metadata is assumed to be
//! sRGB and reported as a material warning. This module does not orient, crop, resample, tone-map,
//! or perform ICC profile conversion.

use std::fmt;
use std::mem::size_of;
use std::vec::Vec;

use ascii_core::{
    AlphaMode, Budget, BudgetKind, CancellationToken, ColorDescription, ColorPrimaries, Error,
    MalformedReason, PixelChannels, RasterFrameView, RasterOrientation, TransferFunction,
    UnsupportedReason,
};

/// The exact sRGB encoded-channel breakpoint used for decoding.
pub const SRGB_DECODE_BREAKPOINT: f64 = 0.04045;
/// The standard linear-light breakpoint used for inverse sRGB encoding.
pub const SRGB_ENCODE_BREAKPOINT: f64 = 0.003_130_8;
/// Maximum absolute boundary discontinuity admitted for the rounded standard transfer thresholds.
pub const SRGB_TRANSFER_CONTINUITY_TOLERANCE: f64 = 0.000_000_1;

const SRGB_RED_LUMINANCE: f64 = 0.2126;
const SRGB_GREEN_LUMINANCE: f64 = 0.7152;
const SRGB_BLUE_LUMINANCE: f64 = 0.0722;
const NORMALIZATION_CANCELLATION_INTERVAL: usize = 256;

/// An encoded sRGB triplet in the normalized range `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EncodedSrgb {
    red: f64,
    green: f64,
    blue: f64,
}

impl EncodedSrgb {
    /// Creates a normalized encoded sRGB triplet, rejecting nonfinite or out-of-range samples.
    pub fn new(red: f64, green: f64, blue: f64) -> Result<Self, ColorNormalizationError> {
        validate_unit_sample(red)?;
        validate_unit_sample(green)?;
        validate_unit_sample(blue)?;
        Ok(Self { red, green, blue })
    }

    /// Returns the encoded red component.
    #[must_use]
    pub const fn red(self) -> f64 {
        self.red
    }

    /// Returns the encoded green component.
    #[must_use]
    pub const fn green(self) -> f64 {
        self.green
    }

    /// Returns the encoded blue component.
    #[must_use]
    pub const fn blue(self) -> f64 {
        self.blue
    }
}

/// A normalized linear-light RGB triplet. It is not sRGB encoded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearRgb {
    red: f64,
    green: f64,
    blue: f64,
}

impl LinearRgb {
    /// Linear-light black.
    pub const BLACK: Self = Self {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
    };
    /// Linear-light white.
    pub const WHITE: Self = Self {
        red: 1.0,
        green: 1.0,
        blue: 1.0,
    };

    /// Creates normalized linear-light RGB, rejecting nonfinite or out-of-range samples.
    pub fn new(red: f64, green: f64, blue: f64) -> Result<Self, ColorNormalizationError> {
        validate_unit_sample(red)?;
        validate_unit_sample(green)?;
        validate_unit_sample(blue)?;
        Ok(Self { red, green, blue })
    }

    /// Returns the linear red component.
    #[must_use]
    pub const fn red(self) -> f64 {
        self.red
    }

    /// Returns the linear green component.
    #[must_use]
    pub const fn green(self) -> f64 {
        self.green
    }

    /// Returns the linear blue component.
    #[must_use]
    pub const fn blue(self) -> f64 {
        self.blue
    }

    /// Computes standard relative luminance from normalized linear RGB.
    #[must_use]
    pub fn luminance(self) -> Luminance {
        Luminance(
            SRGB_RED_LUMINANCE * self.red
                + SRGB_GREEN_LUMINANCE * self.green
                + SRGB_BLUE_LUMINANCE * self.blue,
        )
    }
}

/// Standard relative luminance, computed from linear-light RGB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Luminance(f64);

impl Luminance {
    /// Returns the normalized relative-luminance value.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }
}

/// A normalized alpha value. Alpha is linear and is never sRGB transfer-decoded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alpha(f64);

impl Alpha {
    /// Creates an alpha value in `[0, 1]`, rejecting nonfinite and out-of-range samples.
    pub fn new(value: f64) -> Result<Self, ColorNormalizationError> {
        validate_unit_sample(value)?;
        Ok(Self(value))
    }

    /// Returns the normalized alpha value.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }
}

/// An owned normalized pixel whose linear-light RGB channels are premultiplied by alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearPremultipliedRgba {
    red: f64,
    green: f64,
    blue: f64,
    alpha: Alpha,
}

impl LinearPremultipliedRgba {
    /// Returns linear-light RGB with the pixel's alpha already multiplied into each channel.
    #[must_use]
    pub const fn premultiplied_rgb(self) -> LinearRgb {
        LinearRgb {
            red: self.red,
            green: self.green,
            blue: self.blue,
        }
    }

    /// Returns the independent alpha value.
    #[must_use]
    pub const fn alpha(self) -> Alpha {
        self.alpha
    }

    /// Returns straight (unassociated) linear RGB; transparent pixels are defined as black.
    #[must_use]
    pub fn straight_rgb(self) -> LinearRgb {
        let alpha = self.alpha.value();
        if alpha == 0.0 {
            LinearRgb::BLACK
        } else {
            LinearRgb {
                red: self.red / alpha,
                green: self.green / alpha,
                blue: self.blue / alpha,
            }
        }
    }

    /// Composites this linear-premultiplied pixel over an opaque linear-light background.
    #[must_use]
    pub fn composite_over(self, background: LinearRgb) -> LinearRgb {
        let background_weight = 1.0 - self.alpha.value();
        LinearRgb {
            red: self.red + background.red * background_weight,
            green: self.green + background.green * background_weight,
            blue: self.blue + background.blue * background_weight,
        }
    }
}

/// The currently supported explicit output encoding. Initial output is SDR sRGB, 8-bit/channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputEncoding {
    /// Standard sRGB-encoded opaque RGB, quantized to 8 bits per channel.
    Srgb8,
}

/// An RGB value encoded according to the selected output encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedOutputColor {
    encoding: OutputEncoding,
    channels: [u8; 3],
}

impl EncodedOutputColor {
    /// Returns the encoding that describes these output bytes.
    #[must_use]
    pub const fn encoding(self) -> OutputEncoding {
        self.encoding
    }

    /// Returns encoded red, green, and blue bytes.
    #[must_use]
    pub const fn channels(self) -> [u8; 3] {
        self.channels
    }
}

/// Material color-policy warning attached to a normalized raster.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorNormalizationWarning {
    /// One or both source profile fields were unknown and treated as sRGB.
    AssumedSrgbForUnknownProfile,
}

/// Typed reason that a source color description cannot be normalized by this SDR implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedColorReason {
    /// PQ or HLG requires HDR handling or an explicit tone-mapping path.
    HdrTransfer(TransferFunction),
    /// Explicit wide-gamut primaries require color-managed conversion to sRGB.
    WideGamutPrimaries(ColorPrimaries),
}

/// The invalidity of a scalar sample or a premultiplied source pixel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidColorSample {
    /// A sample was NaN or infinite.
    NonFinite,
    /// A sample was outside the normalized `[0, 1]` range.
    OutOfRange,
    /// An encoded premultiplied channel exceeded its alpha value.
    PremultipliedChannelExceedsAlpha,
}

/// A color normalization failure, preserving core validation failures and color-policy failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorNormalizationError {
    /// A dimension, layout, stride, backing, budget, cancellation, or allocation failure.
    Raster(Error),
    /// Unsupported HDR or wide-gamut source metadata.
    UnsupportedColor(UnsupportedColorReason),
    /// A nonfinite, out-of-range, or invalid premultiplied sample.
    InvalidSample(InvalidColorSample),
}

impl fmt::Display for ColorNormalizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Raster(error) => write!(formatter, "raster normalization failed: {error}"),
            Self::UnsupportedColor(UnsupportedColorReason::HdrTransfer(transfer)) => write!(
                formatter,
                "HDR transfer {transfer:?} is unsupported; provide SDR input or use an explicitly supported tone-mapping path"
            ),
            Self::UnsupportedColor(UnsupportedColorReason::WideGamutPrimaries(primaries)) => {
                write!(
                    formatter,
                    "wide-gamut primaries {primaries:?} are unsupported; convert to sRGB with tested color-managed tooling"
                )
            }
            Self::InvalidSample(InvalidColorSample::NonFinite) => {
                formatter.write_str("color sample is nonfinite")
            }
            Self::InvalidSample(InvalidColorSample::OutOfRange) => {
                formatter.write_str("normalized color sample is outside [0, 1]")
            }
            Self::InvalidSample(InvalidColorSample::PremultipliedChannelExceedsAlpha) => {
                formatter.write_str("premultiplied color channel exceeds alpha")
            }
        }
    }
}

impl std::error::Error for ColorNormalizationError {}

/// An owned, top-down raster normalized to linear-premultiplied RGBA.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedRaster {
    width: u32,
    height: u32,
    pixels: Vec<LinearPremultipliedRgba>,
    warning: Option<ColorNormalizationWarning>,
}

impl NormalizedRaster {
    /// Returns the raster width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the raster height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns all pixels in row-major order as linear-premultiplied RGBA.
    #[must_use]
    pub fn pixels(&self) -> &[LinearPremultipliedRgba] {
        &self.pixels
    }

    /// Returns the warning produced by the explicit unknown-profile sRGB assumption, if any.
    #[must_use]
    pub const fn warning(&self) -> Option<ColorNormalizationWarning> {
        self.warning
    }

    /// Iterates pixels composited in linear light over the selected opaque background.
    ///
    /// Consume this iterator before matching ordinary opaque wallpaper. No intermediate raster is
    /// allocated; transparent layered callers can instead retain [`Self::pixels`] and alpha.
    pub fn composited_over(&self, background: LinearRgb) -> impl Iterator<Item = LinearRgb> + '_ {
        self.pixels
            .iter()
            .copied()
            .map(move |pixel| pixel.composite_over(background))
    }
}

/// Decodes normalized encoded sRGB to normalized linear-light RGB.
///
/// Each channel uses `c / 12.92` at and below `0.04045`, and
/// `((c + 0.055) / 1.055)^2.4` above it. This is physical transfer conversion, not artistic gamma.
#[must_use]
pub fn decode_srgb(encoded: EncodedSrgb) -> LinearRgb {
    LinearRgb {
        red: decode_channel(encoded.red),
        green: decode_channel(encoded.green),
        blue: decode_channel(encoded.blue),
    }
}

/// Encodes normalized linear-light RGB as normalized encoded sRGB.
///
/// Each channel uses `12.92 * c` at and below `0.0031308`, and
/// `1.055 * c^(1/2.4) - 0.055` above it. No clipping is performed.
#[must_use]
pub fn encode_srgb(linear: LinearRgb) -> EncodedSrgb {
    EncodedSrgb {
        red: encode_channel(linear.red),
        green: encode_channel(linear.green),
        blue: encode_channel(linear.blue),
    }
}

/// Encodes linear RGB using an explicit output encoding.
#[must_use]
pub fn encode_output(linear: LinearRgb, encoding: OutputEncoding) -> EncodedOutputColor {
    match encoding {
        OutputEncoding::Srgb8 => {
            let encoded = encode_srgb(linear);
            EncodedOutputColor {
                encoding,
                channels: [
                    quantize_srgb(encoded.red),
                    quantize_srgb(encoded.green),
                    quantize_srgb(encoded.blue),
                ],
            }
        }
    }
}

/// Validates and normalizes an 8-bit raster to owned, linear-premultiplied RGBA.
///
/// Top-down orientation is required and is never transformed here; orientation belongs to IMG-002.
/// Supported profiles are sRGB and linear-light samples using sRGB primaries. Unknown profile
/// fields are assumed to be sRGB and return [`ColorNormalizationWarning::AssumedSrgbForUnknownProfile`].
/// Display-P3/Rec. 2020 and PQ/HLG are rejected; this function performs no clipping, tone mapping,
/// wide-gamut conversion, or ICC profile interpretation.
pub fn normalize_raster(
    view: RasterFrameView<'_>,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<NormalizedRaster, ColorNormalizationError> {
    normalize_raster_with_cancel_check(view, budget, || cancellation.is_cancelled())
}

fn normalize_raster_with_cancel_check(
    view: RasterFrameView<'_>,
    budget: &Budget,
    mut is_cancelled: impl FnMut() -> bool,
) -> Result<NormalizedRaster, ColorNormalizationError> {
    if is_cancelled() {
        return Err(Error::Cancelled.into());
    }
    if view.width() == 0 || view.height() == 0 {
        return Err(Error::Malformed(MalformedReason::EmptyDimensions).into());
    }
    if view.orientation() != RasterOrientation::TopDown {
        return Err(Error::Unsupported(UnsupportedReason::Orientation).into());
    }

    let bytes_per_pixel = match (view.format().channels(), view.format().sample_depth_bits()) {
        (PixelChannels::Gray, 8) => 1usize,
        (PixelChannels::Rgb, 8) => 3,
        (PixelChannels::Rgba, 8) => 4,
        (PixelChannels::Gray | PixelChannels::Rgb | PixelChannels::Rgba, _) => {
            return Err(Error::Unsupported(UnsupportedReason::SampleDepth).into());
        }
        (PixelChannels::Other(_), _) => {
            return Err(Error::Unsupported(UnsupportedReason::PixelFormat).into());
        }
    };
    validate_alpha_layout(view.format().channels(), view.color().alpha_mode())?;
    let (transfer, warning) = resolve_profile(view.color())?;

    let width = usize::try_from(u64::from(view.width())).map_err(|_| Error::SizeOverflow)?;
    let height = usize::try_from(u64::from(view.height())).map_err(|_| Error::SizeOverflow)?;
    let row_bytes = width
        .checked_mul(bytes_per_pixel)
        .ok_or(Error::SizeOverflow)?;
    if view.row_stride() < row_bytes {
        return Err(Error::Malformed(MalformedReason::RowStrideTooSmall).into());
    }

    let required_extent = height
        .checked_sub(1)
        .and_then(|last_row| last_row.checked_mul(view.row_stride()))
        .and_then(|prior_rows| prior_rows.checked_add(row_bytes))
        .ok_or(Error::SizeOverflow)?;
    let input_bytes = u64::try_from(required_extent).map_err(|_| Error::SizeOverflow)?;
    let pixel_count = u64::from(view.width()) * u64::from(view.height());

    if input_bytes > budget.max_raster_bytes() {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes).into());
    }
    if pixel_count > budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits).into());
    }
    if view.pixels().len() < required_extent {
        return Err(Error::Malformed(MalformedReason::RasterBackingTooShort).into());
    }

    let pixel_capacity = usize::try_from(pixel_count).map_err(|_| Error::SizeOverflow)?;
    let output_bytes = pixel_capacity
        .checked_mul(size_of::<LinearPremultipliedRgba>())
        .ok_or(Error::SizeOverflow)?;
    if output_bytes > isize::MAX as usize {
        return Err(Error::SizeOverflow.into());
    }
    let output_bytes = u64::try_from(output_bytes).map_err(|_| Error::SizeOverflow)?;
    if output_bytes > budget.max_raster_bytes() {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes).into());
    }
    if is_cancelled() {
        return Err(Error::Cancelled.into());
    }

    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_capacity)
        .map_err(|_| Error::AllocationFailed)?;

    for y in 0..height {
        if is_cancelled() {
            return Err(Error::Cancelled.into());
        }
        let row_start = y
            .checked_mul(view.row_stride())
            .ok_or(Error::SizeOverflow)?;
        let row_end = row_start
            .checked_add(row_bytes)
            .ok_or(Error::SizeOverflow)?;
        let row = &view.pixels()[row_start..row_end];
        for x in 0..width {
            if pixels.len() % NORMALIZATION_CANCELLATION_INTERVAL == 0 && is_cancelled() {
                return Err(Error::Cancelled.into());
            }
            let offset = x.checked_mul(bytes_per_pixel).ok_or(Error::SizeOverflow)?;
            let sample = &row[offset..offset + bytes_per_pixel];
            let rgba = match view.format().channels() {
                PixelChannels::Gray => [sample[0], sample[0], sample[0], u8::MAX],
                PixelChannels::Rgb => [sample[0], sample[1], sample[2], u8::MAX],
                PixelChannels::Rgba => [sample[0], sample[1], sample[2], sample[3]],
                PixelChannels::Other(_) => {
                    unreachable!("channel layout was validated before allocation")
                }
            };
            pixels.push(normalize_pixel(rgba, view.color().alpha_mode(), transfer)?);
        }
    }
    if is_cancelled() {
        return Err(Error::Cancelled.into());
    }

    Ok(NormalizedRaster {
        width: view.width(),
        height: view.height(),
        pixels,
        warning,
    })
}

fn validate_alpha_layout(
    channels: PixelChannels,
    alpha_mode: AlphaMode,
) -> Result<(), ColorNormalizationError> {
    match (channels, alpha_mode) {
        (PixelChannels::Gray | PixelChannels::Rgb, AlphaMode::Opaque)
        | (PixelChannels::Rgba, AlphaMode::Straight | AlphaMode::Premultiplied) => Ok(()),
        _ => Err(Error::Unsupported(UnsupportedReason::AlphaMode).into()),
    }
}

fn resolve_profile(
    color: ColorDescription,
) -> Result<(TransferFunction, Option<ColorNormalizationWarning>), ColorNormalizationError> {
    let primaries = color.primaries();
    let transfer = color.transfer();
    if matches!(transfer, TransferFunction::Pq | TransferFunction::Hlg) {
        return Err(ColorNormalizationError::UnsupportedColor(
            UnsupportedColorReason::HdrTransfer(transfer),
        ));
    }
    if matches!(
        primaries,
        ColorPrimaries::DisplayP3 | ColorPrimaries::Rec2020
    ) {
        return Err(ColorNormalizationError::UnsupportedColor(
            UnsupportedColorReason::WideGamutPrimaries(primaries),
        ));
    }

    let unknown_primaries = matches!(primaries, ColorPrimaries::Other(_));
    let resolved_transfer = match transfer {
        TransferFunction::Srgb | TransferFunction::Linear => transfer,
        TransferFunction::Other(_) => TransferFunction::Srgb,
        TransferFunction::Pq | TransferFunction::Hlg => unreachable!("HDR transfer rejected above"),
    };
    let warning = (unknown_primaries || matches!(transfer, TransferFunction::Other(_)))
        .then_some(ColorNormalizationWarning::AssumedSrgbForUnknownProfile);
    Ok((resolved_transfer, warning))
}

fn normalize_pixel(
    rgba: [u8; 4],
    alpha_mode: AlphaMode,
    transfer: TransferFunction,
) -> Result<LinearPremultipliedRgba, ColorNormalizationError> {
    let alpha = if alpha_mode == AlphaMode::Opaque {
        1.0
    } else {
        f64::from(rgba[3]) / f64::from(u8::MAX)
    };
    let alpha = Alpha(alpha);
    if alpha.value() == 0.0 {
        // Invisible channel bytes are not colors. In particular, never unassociate by zero.
        return Ok(LinearPremultipliedRgba {
            red: 0.0,
            green: 0.0,
            blue: 0.0,
            alpha,
        });
    }

    let red = f64::from(rgba[0]) / f64::from(u8::MAX);
    let green = f64::from(rgba[1]) / f64::from(u8::MAX);
    let blue = f64::from(rgba[2]) / f64::from(u8::MAX);
    let (red, green, blue) = match alpha_mode {
        AlphaMode::Premultiplied => {
            if red > alpha.value() || green > alpha.value() || blue > alpha.value() {
                return Err(ColorNormalizationError::InvalidSample(
                    InvalidColorSample::PremultipliedChannelExceedsAlpha,
                ));
            }
            match transfer {
                TransferFunction::Linear => (red, green, blue),
                TransferFunction::Srgb => {
                    let straight = EncodedSrgb {
                        red: red / alpha.value(),
                        green: green / alpha.value(),
                        blue: blue / alpha.value(),
                    };
                    let linear = decode_srgb(straight);
                    (
                        linear.red * alpha.value(),
                        linear.green * alpha.value(),
                        linear.blue * alpha.value(),
                    )
                }
                TransferFunction::Pq | TransferFunction::Hlg | TransferFunction::Other(_) => {
                    unreachable!("transfer metadata is resolved before pixel normalization")
                }
            }
        }
        AlphaMode::Straight | AlphaMode::Opaque => {
            let linear = match transfer {
                TransferFunction::Srgb => decode_srgb(EncodedSrgb { red, green, blue }),
                TransferFunction::Linear => LinearRgb { red, green, blue },
                TransferFunction::Pq | TransferFunction::Hlg | TransferFunction::Other(_) => {
                    unreachable!("transfer metadata is resolved before pixel normalization")
                }
            };
            (
                linear.red * alpha.value(),
                linear.green * alpha.value(),
                linear.blue * alpha.value(),
            )
        }
        AlphaMode::Other(_) => {
            return Err(Error::Unsupported(UnsupportedReason::AlphaMode).into());
        }
    };
    Ok(LinearPremultipliedRgba {
        red,
        green,
        blue,
        alpha,
    })
}

fn decode_channel(encoded: f64) -> f64 {
    if encoded == 1.0 {
        1.0
    } else if encoded <= SRGB_DECODE_BREAKPOINT {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

fn encode_channel(linear: f64) -> f64 {
    if linear == 1.0 {
        1.0
    } else if linear <= SRGB_ENCODE_BREAKPOINT {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

fn quantize_srgb(encoded: f64) -> u8 {
    (encoded * f64::from(u8::MAX)).round() as u8
}

fn validate_unit_sample(sample: f64) -> Result<(), ColorNormalizationError> {
    if !sample.is_finite() {
        return Err(ColorNormalizationError::InvalidSample(
            InvalidColorSample::NonFinite,
        ));
    }
    if !(0.0..=1.0).contains(&sample) {
        return Err(ColorNormalizationError::InvalidSample(
            InvalidColorSample::OutOfRange,
        ));
    }
    Ok(())
}

impl From<Error> for ColorNormalizationError {
    fn from(error: Error) -> Self {
        Self::Raster(error)
    }
}
#[cfg(test)]
mod tests {
    use super::{ColorNormalizationError, normalize_raster_with_cancel_check};
    use ascii_core::{
        AlphaMode, Budget, ColorDescription, Error, PixelFormat, RasterFrameView, RasterOrientation,
    };

    #[test]
    fn cancellation_is_polled_inside_a_single_wide_row() {
        let pixels = [128_u8; 512];
        let raster = RasterFrameView::new(
            512,
            1,
            512,
            PixelFormat::gray8(),
            ColorDescription::srgb(AlphaMode::Opaque),
            RasterOrientation::TopDown,
            &pixels,
        );
        let mut polls = 0;

        let result = normalize_raster_with_cancel_check(
            raster,
            &Budget::new(1 << 20, 1 << 20, 1 << 20),
            || {
                polls += 1;
                polls == 5
            },
        );

        assert_eq!(
            result,
            Err(ColorNormalizationError::Raster(Error::Cancelled))
        );
        assert_eq!(polls, 5);
    }
}
