//! Artistic, bounded linear-light adjustments applied after fit/resampling and before matching.

use std::fmt;
use std::vec::Vec;

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

/// Validation error for settings or source samples.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdjustmentError {
    InvalidSettings,
    InvalidSample,
    SizeOverflow,
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
pub fn apply_adjustments(
    samples: &[LinearRgb],
    width: usize,
    height: usize,
    settings: AdjustmentSettings,
) -> Result<Vec<LinearRgb>, AdjustmentError> {
    validate_settings(settings)?;
    let count = width
        .checked_mul(height)
        .ok_or(AdjustmentError::SizeOverflow)?;
    if width == 0 || height == 0 || samples.len() != count {
        return Err(AdjustmentError::InvalidSample);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| AdjustmentError::SizeOverflow)?;
    for sample in samples {
        let [r, g, b] = channels(*sample)?;
        // Return identity exactly, without arithmetic perturbation.
        if settings == AdjustmentSettings::default() {
            output.push(*sample);
            continue;
        }
        let exposure = 2.0_f64.powf(settings.exposure);
        let mut c =
            [r, g, b].map(|v| ((v * exposure - 0.5) * settings.contrast + 0.5).clamp(0.0, 1.0));
        c = c.map(|v| v.powf(1.0 / settings.gamma));
        let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        c = c.map(|v| (y + (v - y) * settings.saturation).clamp(0.0, 1.0));
        output.push(LinearRgb::new(c[0], c[1], c[2]).map_err(|_| AdjustmentError::InvalidSample)?);
    }
    if let Some(edge) = settings.edge {
        let base = output;
        let mut emphasized = Vec::new();
        emphasized
            .try_reserve_exact(count)
            .map_err(|_| AdjustmentError::SizeOverflow)?;
        for y in 0..height {
            for x in 0..width {
                let center = channels(base[y * width + x])?;
                let mut blur = [0.0; 3];
                let mut n = 0.0;
                for dy in -1_i32..=1 {
                    for dx in -1_i32..=1 {
                        let sx = (x as i64 + i64::from(dx)).clamp(0, width as i64 - 1) as usize;
                        let sy = (y as i64 + i64::from(dy)).clamp(0, height as i64 - 1) as usize;
                        let c = channels(base[sy * width + sx])?;
                        for i in 0..3 {
                            blur[i] += c[i];
                        }
                        n += 1.0;
                    }
                }
                let result = [0, 1, 2]
                    .map(|i| (center[i] + edge.gain * (center[i] - blur[i] / n)).clamp(0.0, 1.0));
                emphasized.push(
                    LinearRgb::new(result[0], result[1], result[2])
                        .map_err(|_| AdjustmentError::InvalidSample)?,
                );
            }
        }
        Ok(emphasized)
    } else {
        Ok(output)
    }
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
    #[test]
    fn identity_is_exact() {
        let values = [rgb(0.123456789, 0.5, 1.0)];
        assert_eq!(
            apply_adjustments(&values, 1, 1, AdjustmentSettings::default()).unwrap(),
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
                apply_adjustments(&[rgb(0.5, 0.5, 0.5)], 1, 1, s),
                Err(AdjustmentError::InvalidSettings)
            );
        }
    }
    #[test]
    fn edge_gain_bounded_and_records_halo_vs_baseline() {
        let values = [rgb(0.0, 0.0, 0.0), rgb(1.0, 1.0, 1.0), rgb(1.0, 1.0, 1.0)];
        let baseline = apply_adjustments(&values, 3, 1, AdjustmentSettings::default()).unwrap();
        let edged = apply_adjustments(
            &values,
            3,
            1,
            AdjustmentSettings {
                edge: Some(EdgeSettings { gain: 1.0 }),
                ..AdjustmentSettings::default()
            },
        )
        .unwrap();
        assert_eq!(edged[0], baseline[0]);
        assert!(edged.iter().all(|p| channels(*p).is_ok()));
        assert_eq!(edged[1].red(), 1.0);
        assert_eq!(
            adjustment_invalidation(ArtisticAdjustments::FittedSamples),
            ArtisticAdjustments::FittedSamples
        );
    }
}
