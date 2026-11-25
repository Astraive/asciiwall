//! Artistic, bounded linear-light adjustments applied after fit/resampling and before matching.

use std::fmt;
use std::mem::size_of;
use std::vec::Vec;

use ascii_core::{Budget, BudgetKind, CancellationToken, Error};

use crate::color::LinearRgb;

/// Artistic settings are normalized only after validation; gamma is not source transfer decoding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdjustmentSettings {
    /// Exposure stops in `[-8, 8]`.
    pub exposure: f64,
    /// Contrast multiplier in `[0, 4]`, centered at linear-light 0.5.
    pub contrast: f64,
    /// Artistic power exponent in `[0.1, 4]`; one is identity.
    pub gamma: f64,
    /// Saturation multiplier in `[0, 4]`.
    pub saturation: f64,
    /// Bounded unsharp-mask settings; `None` disables edge emphasis.
    pub edge: Option<EdgeSettings>,
}

impl Default for AdjustmentSettings {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            saturation: 1.0,
            edge: None,
        }
    }
}

/// Bounded 3x3 unsharp mask. Radius-one halo is recorded by comparison to disabled settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeSettings {
    /// Unsharp gain in `[0, 1]`.
    pub gain: f64,
}

/// Validation, resource, or cancellation error for an adjustment operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdjustmentError {
    InvalidSettings,
    InvalidSample,
    SizeOverflow,
    /// A configured resource limit, allocation, or cancellation failure.
    Operation(Error),
}
impl fmt::Display for AdjustmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for AdjustmentError {}

/// Describes which reusable stage a settings change invalidates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtisticAdjustments {
    NormalizedRaster,
    FittedSamples,
    QuantizedGlyphs,
}

/// The conversion boundary has no application cache owner; this function states pipeline reuse semantics.
#[must_use]
pub const fn adjustment_invalidation(change: ArtisticAdjustments) -> ArtisticAdjustments {
    change
}

/// Apply exposure, contrast, artistic gamma, saturation, then bounded edge emphasis to fitted samples.
/// Input/output are row-major normalized linear RGB. The caller must pass samples after fitting.
/// `budget` covers the borrowed sample bytes and owned output bytes, and charges one work unit per
/// sample (nine per sample with edge emphasis). `cancellation` is checked at bounded intervals.
pub fn apply_adjustments(
    samples: &[LinearRgb],
    width: usize,
    height: usize,
    settings: AdjustmentSettings,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<Vec<LinearRgb>, AdjustmentError> {
    apply_adjustments_with_cancel_check(samples, width, height, settings, budget, || {
        cancellation.is_cancelled()
    })
}

const CANCELLATION_SAMPLE_INTERVAL: usize = 256;

fn apply_adjustments_with_cancel_check(
    samples: &[LinearRgb],
    width: usize,
    height: usize,
    settings: AdjustmentSettings,
    budget: &Budget,
    mut is_cancelled: impl FnMut() -> bool,
) -> Result<Vec<LinearRgb>, AdjustmentError> {
    validate_settings(settings)?;
    let count = width
        .checked_mul(height)
        .ok_or(AdjustmentError::SizeOverflow)?;
    if width == 0 || height == 0 || samples.len() != count {
        return Err(AdjustmentError::InvalidSample);
    }
    if is_cancelled() {
        return Err(AdjustmentError::Operation(Error::Cancelled));
    }

    let sample_bytes = count
        .checked_mul(size_of::<LinearRgb>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(AdjustmentError::SizeOverflow)?;
    let operation_bytes = sample_bytes
        .checked_add(sample_bytes)
        .ok_or(AdjustmentError::SizeOverflow)?;
    if operation_bytes > budget.max_raster_bytes() {
        return Err(AdjustmentError::Operation(Error::BudgetExceeded(
            BudgetKind::RasterBytes,
        )));
    }

    let work_units = u64::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(if settings.edge.is_some() { 9 } else { 1 }))
        .ok_or(AdjustmentError::SizeOverflow)?;
    if work_units > budget.max_work_units() {
        return Err(AdjustmentError::Operation(Error::BudgetExceeded(
            BudgetKind::WorkUnits,
        )));
    }

    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| AdjustmentError::Operation(Error::AllocationFailed))?;
    if settings == AdjustmentSettings::default() {
        for (index, sample) in samples.iter().enumerate() {
            if index.is_multiple_of(CANCELLATION_SAMPLE_INTERVAL) && is_cancelled() {
                return Err(AdjustmentError::Operation(Error::Cancelled));
            }
            channels(*sample)?;
            output.push(*sample);
        }
        if is_cancelled() {
            return Err(AdjustmentError::Operation(Error::Cancelled));
        }
        return Ok(output);
    }

    if let Some(edge) = settings.edge {
        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                if index.is_multiple_of(CANCELLATION_SAMPLE_INTERVAL) && is_cancelled() {
                    return Err(AdjustmentError::Operation(Error::Cancelled));
                }
                let mut center = [0.0; 3];
                let mut blur = [0.0; 3];
                let mut n = 0.0;
                for dy in -1_i32..=1 {
                    for dx in -1_i32..=1 {
                        let sx = if dx < 0 {
                            x.saturating_sub(1)
                        } else if dx > 0 {
                            x.saturating_add(1).min(width - 1)
                        } else {
                            x
                        };
                        let sy = if dy < 0 {
                            y.saturating_sub(1)
                        } else if dy > 0 {
                            y.saturating_add(1).min(height - 1)
                        } else {
                            y
                        };
                        let neighbor = adjusted_channels(samples[sy * width + sx], settings)?;
                        if dx == 0 && dy == 0 {
                            center = neighbor;
                        }
                        for channel in 0..3 {
                            blur[channel] += neighbor[channel];
                        }
                        n += 1.0;
                    }
                }
                let result = [0, 1, 2].map(|channel| {
                    (center[channel] + edge.gain * (center[channel] - blur[channel] / n))
                        .clamp(0.0, 1.0)
                });
                output.push(
                    LinearRgb::new(result[0], result[1], result[2])
                        .map_err(|_| AdjustmentError::InvalidSample)?,
                );
            }
        }
    } else {
        for (index, sample) in samples.iter().enumerate() {
            if index.is_multiple_of(CANCELLATION_SAMPLE_INTERVAL) && is_cancelled() {
                return Err(AdjustmentError::Operation(Error::Cancelled));
            }
            let c = adjusted_channels(*sample, settings)?;
            output.push(
                LinearRgb::new(c[0], c[1], c[2]).map_err(|_| AdjustmentError::InvalidSample)?,
            );
        }
    }
    if is_cancelled() {
        return Err(AdjustmentError::Operation(Error::Cancelled));
    }
    Ok(output)
}

fn adjusted_channels(
    sample: LinearRgb,
    settings: AdjustmentSettings,
) -> Result<[f64; 3], AdjustmentError> {
    let [r, g, b] = channels(sample)?;
    let exposure = 2.0_f64.powf(settings.exposure);
    let mut c = [r, g, b].map(|v| ((v * exposure - 0.5) * settings.contrast + 0.5).clamp(0.0, 1.0));
    c = c.map(|v| v.powf(1.0 / settings.gamma));
    let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    c = c.map(|v| (y + (v - y) * settings.saturation).clamp(0.0, 1.0));
    Ok(c)
}

fn validate_settings(s: AdjustmentSettings) -> Result<(), AdjustmentError> {
    if !s.exposure.is_finite()
        || !(-8.0..=8.0).contains(&s.exposure)
        || !s.contrast.is_finite()
        || !(0.0..=4.0).contains(&s.contrast)
        || !s.gamma.is_finite()
        || !(0.1..=4.0).contains(&s.gamma)
        || !s.saturation.is_finite()
        || !(0.0..=4.0).contains(&s.saturation)
        || s.edge
            .is_some_and(|e| !e.gain.is_finite() || !(0.0..=1.0).contains(&e.gain))
    {
        return Err(AdjustmentError::InvalidSettings);
    }
    Ok(())
}
fn channels(c: LinearRgb) -> Result<[f64; 3], AdjustmentError> {
    let values = [c.red(), c.green(), c.blue()];
    if values
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        Err(AdjustmentError::InvalidSample)
    } else {
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: f64, g: f64, b: f64) -> LinearRgb {
        LinearRgb::new(r, g, b).unwrap()
    }

    fn budget_for(samples: usize) -> Budget {
        Budget::new(
            u64::MAX,
            (samples * size_of::<LinearRgb>() * 2) as u64,
            u64::MAX,
        )
    }

    fn apply(
        samples: &[LinearRgb],
        width: usize,
        height: usize,
        settings: AdjustmentSettings,
    ) -> Result<Vec<LinearRgb>, AdjustmentError> {
        apply_adjustments(
            samples,
            width,
            height,
            settings,
            &budget_for(samples.len()),
            &CancellationToken::new(),
        )
    }

    #[test]
    fn identity_is_exact() {
        let values = [rgb(0.123456789, 0.5, 1.0)];
        assert_eq!(
            apply(&values, 1, 1, AdjustmentSettings::default()).unwrap(),
            values
        );
    }

    #[test]
    fn rejects_nonfinite_and_out_of_range_settings() {
        for value in [f64::NAN, f64::INFINITY, -9.0, 9.0] {
            let s = AdjustmentSettings {
                exposure: value,
                ..AdjustmentSettings::default()
            };
            assert_eq!(
                apply(&[rgb(0.5, 0.5, 0.5)], 1, 1, s),
                Err(AdjustmentError::InvalidSettings)
            );
        }
    }

    #[test]
    fn edge_response_is_bounded_and_quantifies_halo_against_baseline() {
        let values = (0..15)
            .map(|index| {
                let value = if index % 5 < 2 { 0.25 } else { 0.75 };
                rgb(value, value, value)
            })
            .collect::<Vec<_>>();
        let baseline = apply(&values, 5, 3, AdjustmentSettings::default()).unwrap();
        let edged = apply(
            &values,
            5,
            3,
            AdjustmentSettings {
                edge: Some(EdgeSettings { gain: 1.0 }),
                ..AdjustmentSettings::default()
            },
        )
        .unwrap();
        assert!(edged.iter().all(|p| channels(*p).is_ok()));
        assert!((edged[1].red() - baseline[1].red() + 1.0 / 6.0).abs() < 1e-12);
        assert!((edged[2].red() - baseline[2].red() - 1.0 / 6.0).abs() < 1e-12);
        assert_eq!(edged[0], baseline[0]);
        assert_eq!(edged[3], baseline[3]);
        assert_eq!(edged[4], baseline[4]);
        assert_eq!(
            adjustment_invalidation(ArtisticAdjustments::FittedSamples),
            ArtisticAdjustments::FittedSamples
        );
    }

    #[test]
    fn rejects_raster_and_work_budget_exhaustion() {
        let values = [rgb(0.5, 0.5, 0.5); 2];
        let too_small_raster =
            Budget::new(u64::MAX, (size_of::<LinearRgb>() * 4 - 1) as u64, u64::MAX);
        assert_eq!(
            apply_adjustments(
                &values,
                2,
                1,
                AdjustmentSettings::default(),
                &too_small_raster,
                &CancellationToken::new(),
            ),
            Err(AdjustmentError::Operation(Error::BudgetExceeded(
                BudgetKind::RasterBytes
            )))
        );

        let too_small_work = Budget::new(u64::MAX, u64::MAX, 1);
        assert_eq!(
            apply_adjustments(
                &values,
                2,
                1,
                AdjustmentSettings::default(),
                &too_small_work,
                &CancellationToken::new(),
            ),
            Err(AdjustmentError::Operation(Error::BudgetExceeded(
                BudgetKind::WorkUnits
            )))
        );
    }

    #[test]
    fn cancellation_drops_partial_output_and_later_call_succeeds() {
        let values = [rgb(0.5, 0.5, 0.5); CANCELLATION_SAMPLE_INTERVAL * 2];
        let budget = budget_for(values.len());
        let token = CancellationToken::new();
        token.cancel();
        assert_eq!(
            apply_adjustments(
                &values,
                values.len(),
                1,
                AdjustmentSettings::default(),
                &budget,
                &token,
            ),
            Err(AdjustmentError::Operation(Error::Cancelled))
        );
        let mut checks = 0;
        let cancelled = apply_adjustments_with_cancel_check(
            &values,
            values.len(),
            1,
            AdjustmentSettings {
                exposure: 1.0,
                ..AdjustmentSettings::default()
            },
            &budget,
            || {
                checks += 1;
                checks == 3
            },
        );
        assert_eq!(cancelled, Err(AdjustmentError::Operation(Error::Cancelled)));
        assert_eq!(
            apply_adjustments(
                &values[..1],
                1,
                1,
                AdjustmentSettings::default(),
                &budget_for(1),
                &CancellationToken::new(),
            ),
            Ok(vec![values[0]])
        );
    }
}
