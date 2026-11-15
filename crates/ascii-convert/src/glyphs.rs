//! Immutable measurements derived from rasterized glyph coverage masks.

use ascii_core::{CancellationToken, Error};

const CANCELLATION_POLL_INTERVAL: usize = 256;

/// Integer bounds of the nonzero coverage inside a cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlyphBounds {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl GlyphBounds {
    /// Left edge of the ink bounds; zero for an empty mask.
    #[must_use]
    pub const fn x(self) -> u32 {
        self.x
    }

    /// Top edge of the ink bounds; zero for an empty mask.
    #[must_use]
    pub const fn y(self) -> u32 {
        self.y
    }

    /// Width of the ink bounds; zero for an empty mask.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Height of the ink bounds; zero for an empty mask.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

/// Font metrics expressed in raster pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphMetrics {
    advance: f32,
    line_height: f32,
    baseline: u32,
}

impl GlyphMetrics {
    pub(crate) const fn new(advance: f32, line_height: f32, baseline: u32) -> Self {
        Self {
            advance,
            line_height,
            baseline,
        }
    }
    /// Horizontal advance reported by the rasterizer.
    #[must_use]
    pub const fn advance(self) -> f32 {
        self.advance
    }

    /// Font line height reported by the rasterizer, or zero when unavailable.
    #[must_use]
    pub const fn line_height(self) -> f32 {
        self.line_height
    }

    /// Cell baseline in raster pixels, measured from the top edge.
    #[must_use]
    pub const fn baseline(self) -> u32 {
        self.baseline
    }
}

/// Coverage-weighted ink centroid in cell-pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Centroid {
    x: f32,
    y: f32,
}

impl Centroid {
    /// Horizontal centroid in pixels from the cell's left edge.
    #[must_use]
    pub const fn x(self) -> f32 {
        self.x
    }

    /// Vertical centroid in pixels from the cell's top edge.
    #[must_use]
    pub const fn y(self) -> f32 {
        self.y
    }
}

/// Normalized distribution of measured coverage-gradient orientations.
///
/// The four components are the fractions of gradient energy assigned to horizontal,
/// vertical, positive-diagonal, and negative-diagonal cell-edge bins. A blank mask has all-zero
/// components. These descriptors are derived from the actual 8-bit mask, not the code point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeOrientation {
    horizontal: f32,
    vertical: f32,
    diagonal_positive: f32,
    diagonal_negative: f32,
}

impl EdgeOrientation {
    /// Fraction of gradient energy assigned to horizontal edges.
    #[must_use]
    pub const fn horizontal(self) -> f32 {
        self.horizontal
    }

    /// Fraction of gradient energy assigned to vertical edges.
    #[must_use]
    pub const fn vertical(self) -> f32 {
        self.vertical
    }

    /// Fraction of gradient energy assigned to positive-diagonal edges.
    #[must_use]
    pub const fn diagonal_positive(self) -> f32 {
        self.diagonal_positive
    }

    /// Fraction of gradient energy assigned to negative-diagonal edges.
    #[must_use]
    pub const fn diagonal_negative(self) -> f32 {
        self.diagonal_negative
    }
}

/// Computes coverage bounds, centroid, mean coverage, and gradient-orientation bins.
///
/// The caller guarantees `mask.len() == width * height` and nonzero dimensions. The descriptor
/// passes poll cancellation every 256 pixels and allocate no memory.
pub(crate) fn describe(
    mask: &[u8],
    width: usize,
    height: usize,
    cancellation: &CancellationToken,
) -> Result<(GlyphBounds, Option<Centroid>, f32, EdgeOrientation), Error> {
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    let mut found_ink = false;
    let mut coverage_sum = 0.0f64;
    let mut centroid_x_sum = 0.0f64;
    let mut centroid_y_sum = 0.0f64;

    for (index, &coverage) in mask.iter().enumerate() {
        if index % CANCELLATION_POLL_INTERVAL == 0 && cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let x = index % width;
        let y = index / width;
        if coverage == 0 {
            continue;
        }
        found_ink = true;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        let weight = f64::from(coverage);
        coverage_sum += weight;
        centroid_x_sum += (x as f64 + 0.5) * weight;
        centroid_y_sum += (y as f64 + 0.5) * weight;
    }

    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let bounds = if found_ink {
        GlyphBounds {
            x: min_x as u32,
            y: min_y as u32,
            width: (max_x - min_x + 1) as u32,
            height: (max_y - min_y + 1) as u32,
        }
    } else {
        GlyphBounds {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }
    };

    let area = width as f64 * height as f64;
    let mean_coverage = if area == 0.0 {
        0.0
    } else {
        (coverage_sum / (area * 255.0)) as f32
    };
    let centroid = (coverage_sum > 0.0).then(|| Centroid {
        x: (centroid_x_sum / coverage_sum) as f32,
        y: (centroid_y_sum / coverage_sum) as f32,
    });

    let mut horizontal = 0.0f64;
    let mut vertical = 0.0f64;
    let mut diagonal_positive = 0.0f64;
    let mut diagonal_negative = 0.0f64;
    let mut total_energy = 0.0f64;
    for (index, &coverage) in mask.iter().enumerate() {
        if index % CANCELLATION_POLL_INTERVAL == 0 && cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let x = index % width;
        let y = index / width;
        let left = if x == 0 {
            0.0
        } else {
            f64::from(mask[index - 1])
        };
        let right = if x + 1 == width {
            0.0
        } else {
            f64::from(mask[index + 1])
        };
        let up = if y == 0 {
            0.0
        } else {
            f64::from(mask[index - width])
        };
        let down = if y + 1 == height {
            0.0
        } else {
            f64::from(mask[index + width])
        };
        let gx = if width == 1 {
            f64::from(coverage)
        } else {
            right - left
        };
        let gy = if height == 1 {
            f64::from(coverage)
        } else {
            down - up
        };
        let ax = gx.abs();
        let ay = gy.abs();
        let energy = ax + ay;
        if energy == 0.0 {
            continue;
        }
        total_energy += energy;
        if width == 1 && height == 1 {
            horizontal += ax;
            vertical += ay;
        } else if ax > ay * 2.0 {
            vertical += energy;
        } else if ay > ax * 2.0 {
            horizontal += energy;
        } else if gx * gy < 0.0 {
            diagonal_positive += energy;
        } else {
            diagonal_negative += energy;
        }
    }

    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let edges = if total_energy == 0.0 {
        EdgeOrientation::default()
    } else {
        let inverse = 1.0 / total_energy;
        EdgeOrientation {
            horizontal: (horizontal * inverse) as f32,
            vertical: (vertical * inverse) as f32,
            diagonal_positive: (diagonal_positive * inverse) as f32,
            diagonal_negative: (diagonal_negative * inverse) as f32,
        }
    };

    Ok((bounds, centroid, mean_coverage, edges))
}

#[cfg(test)]
mod tests {
    use ascii_core::CancellationToken;

    use super::{EdgeOrientation, describe};

    #[test]
    fn one_by_one_ink_has_nonzero_edge_energy_and_blank_masks_stay_zero() {
        let token = CancellationToken::new();
        let (bounds, centroid, mean, edges) =
            describe(&[255], 1, 1, &token).expect("one-pixel descriptor succeeds");
        assert_eq!((bounds.width(), bounds.height()), (1, 1));
        assert_eq!(
            centroid.map(|point| (point.x(), point.y())),
            Some((0.5, 0.5))
        );
        assert_eq!(mean, 1.0);
        assert_eq!(edges.horizontal(), 0.5);
        assert_eq!(edges.vertical(), 0.5);
        assert_eq!(edges.diagonal_positive(), 0.0);
        assert_eq!(edges.diagonal_negative(), 0.0);

        let (bounds, centroid, mean, edges) =
            describe(&[0], 1, 1, &token).expect("blank descriptor succeeds");
        assert_eq!((bounds.width(), bounds.height()), (0, 0));
        assert_eq!(centroid, None);
        assert_eq!(mean, 0.0);
        assert_eq!(edges, EdgeOrientation::default());
    }

    #[test]
    fn descriptor_cancellation_returns_no_partial_summary() {
        let token = CancellationToken::new();
        token.cancel();
        assert_eq!(
            describe(&[255], 1, 1, &token),
            Err(ascii_core::Error::Cancelled)
        );
    }
}
