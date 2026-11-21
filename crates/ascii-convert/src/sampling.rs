//! Shared CPU geometry and box sampling for density conversion.
//!
//! Orientation, sample format, transfer function, and alpha are normalized first by the color
//! normalizer. The resulting top-down linear-premultiplied raster is then fitted here, before any
//! artistic adjustment. Coordinates describe pixel edges. `Contain` preserves the complete source
//! in a centered display rectangle and treats letterbox pixels as the configured background;
//! `Cover` preserves aspect and crops the centered source rectangle; `CustomCrop` maps the exact
//! normalized crop rectangle to the display rectangle (independently on each axis). The returned
//! transform is the same affine mapping used for box/nearest sampling and preview/pointer mapping.
//!
//! `Box` integrates source pixel-square overlap weights over the inverse-mapped destination cell
//! or subcell rectangle. Regions outside a contain rectangle contribute background. `Nearest`
//! samples the source pixel containing the inverse-mapped sample center (`floor` on each axis;
//! exact source right/bottom edges select the last pixel). Both policies are deterministic.

use std::mem::size_of;
use std::vec::Vec;

use ascii_core::{
    Budget, BudgetKind, CancellationToken, Error, GridSize, MalformedReason, RasterFrameView,
};
use sha2::{Digest, Sha256};

use crate::color::{
    ColorNormalizationError, LinearPremultipliedRgba, LinearRgb, NormalizedRaster,
    resolve_raster_metadata,
};

const MAX_EXACT_F64_INTEGER: u64 = 1_u64 << 53;

// Bound centering error independently of content extent to avoid visible margin skew.
const MAX_CENTERING_ERROR_PIXELS: f64 = 0.125;

const MAX_PATCH_DIMENSION: u32 = 64;
const MAX_PATCH_PIXELS: u64 = 4096;
const SAMPLER_CANCEL_INTERVAL: u64 = 256;
const PATCH_INITIALIZATION_CANCEL_INTERVAL: usize = 256;

/// A pixel-edge rectangle in source or destination coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SamplingRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl SamplingRect {
    /// Creates a finite, positive-area rectangle.
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Result<Self, Error> {
        if !x.is_finite()
            || !y.is_finite()
            || !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let x = if x == 0.0 { 0.0 } else { x };
        let y = if y == 0.0 { 0.0 } else { y };
        let right = x + width;
        let bottom = y + height;
        if !right.is_finite() || !bottom.is_finite() {
            return Err(Error::SizeOverflow);
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    /// Returns the left pixel-edge coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Returns the top pixel-edge coordinate.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.y
    }

    /// Returns the horizontal extent.
    #[must_use]
    pub const fn width(self) -> f64 {
        self.width
    }

    /// Returns the vertical extent.
    #[must_use]
    pub const fn height(self) -> f64 {
        self.height
    }

    fn edges_preserve_extent_within_f64_rounding(self) -> bool {
        let right = self.right();
        let bottom = self.bottom();
        let width_error = ((right - self.x) - self.width).abs();
        let height_error = ((bottom - self.y) - self.height).abs();
        let width_tolerance = edge_rounding_tolerance(self.x, right, self.width);
        let height_tolerance = edge_rounding_tolerance(self.y, bottom, self.height);
        width_error <= width_tolerance && height_error <= height_tolerance
    }

    fn right(self) -> f64 {
        self.x + self.width
    }

    fn bottom(self) -> f64 {
        self.y + self.height
    }
}

fn centered_offset(container_extent: f64, content_extent: f64) -> Result<f64, Error> {
    let offset = ((container_extent - content_extent).max(0.0)) * 0.5;
    let end = offset + content_extent;
    let trailing_margin = (container_extent - end).max(0.0);
    let local_precision = 8.0 * f64::EPSILON * offset.abs().max(end.abs()).max(content_extent);
    let centering_tolerance = local_precision.min(MAX_CENTERING_ERROR_PIXELS);
    if (offset - trailing_margin).abs() > centering_tolerance {
        return Err(Error::SizeOverflow);
    }
    Ok(offset)
}

// Account for precision at the coordinate origin, but cap tolerance at a safe fraction of extent.

fn edge_rounding_tolerance(start: f64, end: f64, extent: f64) -> f64 {
    const MAX_EXTENT_FRACTION: f64 = 0.125;
    let local_precision = 8.0 * f64::EPSILON * start.abs().max(end.abs()).max(extent);
    local_precision.min(extent * MAX_EXTENT_FRACTION)
}

/// A normalized source crop, expressed as left/top/right/bottom fractions in `[0, 1]`.
///
/// The right and bottom edges are exclusive for pixel-area integration. The crop is mapped
/// exactly to the destination rectangle by `FitMode::CustomCrop`; callers wanting aspect-preserving
/// center crop should use `FitMode::Cover` instead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CropRect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl CropRect {
    /// Validates normalized left, top, right, and bottom crop edges.
    pub fn new(left: f64, top: f64, right: f64, bottom: f64) -> Result<Self, Error> {
        if !left.is_finite()
            || !top.is_finite()
            || !right.is_finite()
            || !bottom.is_finite()
            || left < 0.0
            || top < 0.0
            || right > 1.0
            || bottom > 1.0
            || left >= right
            || top >= bottom
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let left = if left == 0.0 { 0.0 } else { left };
        let top = if top == 0.0 { 0.0 } else { top };
        let right = if right == 0.0 { 0.0 } else { right };
        let bottom = if bottom == 0.0 { 0.0 } else { bottom };
        Ok(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    /// Returns the normalized left edge.
    #[must_use]
    pub const fn left(self) -> f64 {
        self.left
    }

    /// Returns the normalized top edge.
    #[must_use]
    pub const fn top(self) -> f64 {
        self.top
    }

    /// Returns the normalized right edge.
    #[must_use]
    pub const fn right(self) -> f64 {
        self.right
    }

    /// Returns the normalized bottom edge.
    #[must_use]
    pub const fn bottom(self) -> f64 {
        self.bottom
    }
}

/// The source-to-display fitting policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FitMode {
    /// Preserve the whole source and center it in the destination; uncovered pixels are background.
    Contain,
    /// Preserve aspect and center-crop the source until it fills the destination.
    Cover,
    /// Map this exact normalized crop to the entire destination, allowing nonuniform scale.
    CustomCrop(CropRect),
}

/// Sampling policy for each destination footprint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplingFilter {
    /// Deterministic weighted box integration over pixel-square overlap areas.
    Box,
    /// One source pixel at the inverse-mapped sample center (floor inclusion policy).
    Nearest,
}

/// Validated dimensions for retained per-cell luminance patches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatchDimensions {
    width: u32,
    height: u32,
}

impl PatchDimensions {
    /// Creates a nonempty patch no larger than 64×64 samples (4096 samples per cell).
    pub fn new(width: u32, height: u32) -> Result<Self, Error> {
        let count = u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or(Error::SizeOverflow)?;
        if width == 0
            || height == 0
            || width > MAX_PATCH_DIMENSION
            || height > MAX_PATCH_DIMENSION
            || count > MAX_PATCH_PIXELS
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(Self { width, height })
    }

    /// Returns the horizontal patch sample count.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Returns the vertical patch sample count.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }

    /// Returns the patch sample count.
    #[must_use]
    pub const fn sample_count(self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

/// Geometry and filter options shared by conversion, preview, and pointer mapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SamplingOptions {
    fit: FitMode,
    filter: SamplingFilter,
    patch: Option<PatchDimensions>,
}

impl SamplingOptions {
    /// Creates sampling options. Dimensions in an optional patch must come from
    /// [`PatchDimensions::new`].
    #[must_use]
    pub const fn new(fit: FitMode, filter: SamplingFilter, patch: Option<PatchDimensions>) -> Self {
        Self { fit, filter, patch }
    }

    /// Returns the fit/crop policy.
    #[must_use]
    pub const fn fit(self) -> FitMode {
        self.fit
    }

    /// Returns the selected source sampling filter.
    #[must_use]
    pub const fn filter(self) -> SamplingFilter {
        self.filter
    }

    /// Returns the optional retained quality-patch dimensions.
    #[must_use]
    pub const fn patch(self) -> Option<PatchDimensions> {
        self.patch
    }
}

impl Default for SamplingOptions {
    fn default() -> Self {
        Self::new(FitMode::Contain, SamplingFilter::Box, None)
    }
}

/// One invertible axis-aligned source/display mapping for fitting, preview, and pointer input.
///
/// Mapping uses pixel-edge coordinates. Points on either rectangle's boundary are accepted;
/// points in contain letterboxes or outside a cover/custom source crop are rejected with `None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitTransform {
    source_width: u32,
    source_height: u32,
    display_width: u64,
    display_height: u64,
    source_rect: SamplingRect,
    display_rect: SamplingRect,
}

impl FitTransform {
    /// Builds the transform from source and destination pixel-edge extents.
    /// Returns `SizeOverflow` when edge extents or contain letterbox margins cannot be represented
    /// within bounded local f64 rounding error.
    pub fn new(
        source_width: u32,
        source_height: u32,
        display_width: u64,
        display_height: u64,
        fit: FitMode,
    ) -> Result<Self, Error> {
        if source_width == 0 || source_height == 0 || display_width == 0 || display_height == 0 {
            return Err(Error::Malformed(MalformedReason::EmptyDimensions));
        }
        if display_width > MAX_EXACT_F64_INTEGER || display_height > MAX_EXACT_F64_INTEGER {
            return Err(Error::SizeOverflow);
        }

        let sw = f64::from(source_width);
        let sh = f64::from(source_height);
        let dw = display_width as f64;
        let dh = display_height as f64;
        let (source_rect, display_rect) = match fit {
            FitMode::Contain => {
                let scale = (dw / sw).min(dh / sh);
                if !scale.is_finite() || scale <= 0.0 {
                    return Err(Error::SizeOverflow);
                }
                let content_width = (sw * scale).min(dw);
                let content_height = (sh * scale).min(dh);
                let display_x = centered_offset(dw, content_width)?;
                let display_y = centered_offset(dh, content_height)?;
                (
                    SamplingRect::new(0.0, 0.0, sw, sh)?,
                    SamplingRect::new(display_x, display_y, content_width, content_height)?,
                )
            }
            FitMode::Cover => {
                let scale = (dw / sw).max(dh / sh);
                if !scale.is_finite() || scale <= 0.0 {
                    return Err(Error::SizeOverflow);
                }
                let crop_width = (dw / scale).min(sw);
                let crop_height = (dh / scale).min(sh);
                (
                    SamplingRect::new(
                        ((sw - crop_width).max(0.0)) * 0.5,
                        ((sh - crop_height).max(0.0)) * 0.5,
                        crop_width,
                        crop_height,
                    )?,
                    SamplingRect::new(0.0, 0.0, dw, dh)?,
                )
            }
            FitMode::CustomCrop(crop) => {
                let x = crop.left * sw;
                let y = crop.top * sh;
                let width = (crop.right - crop.left) * sw;
                let height = (crop.bottom - crop.top) * sh;
                (
                    SamplingRect::new(x, y, width, height)?,
                    SamplingRect::new(0.0, 0.0, dw, dh)?,
                )
            }
        };

        if !source_rect.edges_preserve_extent_within_f64_rounding()
            || !display_rect.edges_preserve_extent_within_f64_rounding()
        {
            return Err(Error::SizeOverflow);
        }
        Ok(Self {
            source_width,
            source_height,
            display_width,
            display_height,
            source_rect,
            display_rect,
        })
    }

    /// Returns the source dimensions in pixels.
    #[must_use]
    pub const fn source_dimensions(self) -> (u32, u32) {
        (self.source_width, self.source_height)
    }

    /// Returns the destination surface dimensions in pixels.
    #[must_use]
    pub const fn display_dimensions(self) -> (u64, u64) {
        (self.display_width, self.display_height)
    }

    /// Returns the source content rectangle used by the transform.
    #[must_use]
    pub const fn source_rect(self) -> SamplingRect {
        self.source_rect
    }

    /// Returns the display content rectangle; contain letterboxes are outside this rectangle.
    #[must_use]
    pub const fn display_rect(self) -> SamplingRect {
        self.display_rect
    }

    /// Maps a display point to source coordinates, rejecting letterbox and out-of-content points.
    #[must_use]
    pub fn map_display_to_source(self, x: f64, y: f64) -> Option<(f64, f64)> {
        if !x.is_finite()
            || !y.is_finite()
            || x < self.display_rect.x
            || y < self.display_rect.y
            || x > self.display_rect.right()
            || y > self.display_rect.bottom()
        {
            return None;
        }
        let source_x = (self.source_rect.x
            + (x - self.display_rect.x) * self.source_rect.width / self.display_rect.width)
            .clamp(self.source_rect.x, self.source_rect.right());
        let source_y = (self.source_rect.y
            + (y - self.display_rect.y) * self.source_rect.height / self.display_rect.height)
            .clamp(self.source_rect.y, self.source_rect.bottom());
        Some((source_x, source_y))
    }

    /// Maps a source point to display coordinates, rejecting pixels outside a cover/custom crop.
    #[must_use]
    pub fn map_source_to_display(self, x: f64, y: f64) -> Option<(f64, f64)> {
        if !x.is_finite()
            || !y.is_finite()
            || x < self.source_rect.x
            || y < self.source_rect.y
            || x > self.source_rect.right()
            || y > self.source_rect.bottom()
        {
            return None;
        }
        let display_x = (self.display_rect.x
            + (x - self.source_rect.x) * self.display_rect.width / self.source_rect.width)
            .clamp(self.display_rect.x, self.display_rect.right());
        let display_y = (self.display_rect.y
            + (y - self.source_rect.y) * self.display_rect.height / self.source_rect.height)
            .clamp(self.display_rect.y, self.display_rect.bottom());
        Some((display_x, display_y))
    }

    fn source_point_for_display(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.source_rect.x
                + (x - self.display_rect.x) * self.source_rect.width / self.display_rect.width,
            self.source_rect.y
                + (y - self.display_rect.y) * self.source_rect.height / self.display_rect.height,
        )
    }
}

/// Stable SHA-256 identity of the resolved crop, grid geometry, filter, and patch dimensions.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SamplingIdentity([u8; 32]);

impl SamplingIdentity {
    /// Returns the digest bytes in canonical SHA-256 order.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Per-cell luminance and mean absolute horizontal/vertical finite-difference descriptors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatchDescriptors {
    mean_luminance: f32,
    mean_absolute_dx: f32,
    mean_absolute_dy: f32,
}

impl PatchDescriptors {
    /// Returns the arithmetic mean linear-light luminance of the patch.
    #[must_use]
    pub const fn mean_luminance(self) -> f32 {
        self.mean_luminance
    }

    /// Returns mean absolute adjacent-sample difference along patch rows.
    #[must_use]
    pub const fn mean_absolute_dx(self) -> f32 {
        self.mean_absolute_dx
    }

    /// Returns mean absolute adjacent-sample difference along patch columns.
    #[must_use]
    pub const fn mean_absolute_dy(self) -> f32 {
        self.mean_absolute_dy
    }
}

/// Bounded contiguous quality patches retained in row-major output-cell order.
#[derive(Debug, PartialEq)]
pub struct QualityPatches {
    dimensions: PatchDimensions,
    luminance: Vec<f32>,
    descriptors: Vec<PatchDescriptors>,
}

impl QualityPatches {
    /// Returns the common patch dimensions.
    #[must_use]
    pub const fn dimensions(&self) -> PatchDimensions {
        self.dimensions
    }

    /// Returns the number of output cells with a retained patch.
    #[must_use]
    pub fn cell_count(&self) -> usize {
        self.descriptors.len()
    }

    /// Returns one cell's row-major patch values, or `None` for an invalid cell index.
    #[must_use]
    pub fn luminance_for_cell(&self, cell_index: usize) -> Option<&[f32]> {
        if cell_index >= self.descriptors.len() {
            return None;
        }
        let patch_len = usize::try_from(self.dimensions.sample_count()).ok()?;
        let start = cell_index.checked_mul(patch_len)?;
        self.luminance.get(start..start.checked_add(patch_len)?)
    }

    /// Returns all cell descriptors in row-major output-cell order.
    #[must_use]
    pub fn descriptors(&self) -> &[PatchDescriptors] {
        &self.descriptors
    }

    /// Returns one cell descriptor, or `None` for an invalid cell index.
    #[must_use]
    pub fn descriptor(&self, cell_index: usize) -> Option<PatchDescriptors> {
        self.descriptors.get(cell_index).copied()
    }

    fn allocate(
        dimensions: PatchDimensions,
        cell_count: usize,
        patch_value_count: usize,
        cancellation: &CancellationToken,
    ) -> Result<Self, Error> {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut luminance = Vec::new();
        luminance
            .try_reserve_exact(patch_value_count)
            .map_err(|_| Error::AllocationFailed)?;
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut descriptors = Vec::new();
        descriptors
            .try_reserve_exact(cell_count)
            .map_err(|_| Error::AllocationFailed)?;
        for index in 0..patch_value_count {
            if index.is_multiple_of(PATCH_INITIALIZATION_CANCEL_INTERVAL)
                && cancellation.is_cancelled()
            {
                return Err(Error::Cancelled);
            }
            luminance.push(0.0);
        }
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(Self {
            dimensions,
            luminance,
            descriptors,
        })
    }

    pub(crate) fn luminance_for_cell_mut(
        &mut self,
        cell_index: usize,
    ) -> Result<&mut [f32], Error> {
        let patch_len =
            usize::try_from(self.dimensions.sample_count()).map_err(|_| Error::SizeOverflow)?;
        let start = cell_index
            .checked_mul(patch_len)
            .ok_or(Error::SizeOverflow)?;
        let end = start.checked_add(patch_len).ok_or(Error::SizeOverflow)?;
        self.luminance
            .get_mut(start..end)
            .ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))
    }

    pub(crate) fn push_descriptor(&mut self, descriptor: PatchDescriptors) {
        self.descriptors.push(descriptor);
    }
}

/// Internal, fully preflighted geometry and sampling identity for one conversion.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SamplingPlan {
    transform: FitTransform,
    options: SamplingOptions,
    identity: SamplingIdentity,
}

impl SamplingPlan {
    pub(crate) const fn transform(self) -> FitTransform {
        self.transform
    }

    pub(crate) const fn identity(self) -> SamplingIdentity {
        self.identity
    }

    pub(crate) fn allocate_patches(
        self,
        cell_count: usize,
        cancellation: &CancellationToken,
    ) -> Result<Option<QualityPatches>, Error> {
        let Some(dimensions) = self.options.patch else {
            return Ok(None);
        };
        let patch_len =
            usize::try_from(dimensions.sample_count()).map_err(|_| Error::SizeOverflow)?;
        let value_count = cell_count
            .checked_mul(patch_len)
            .ok_or(Error::SizeOverflow)?;
        Ok(Some(QualityPatches::allocate(
            dimensions,
            cell_count,
            value_count,
            cancellation,
        )?))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sample_cell(
        self,
        raster: &NormalizedRaster,
        column: u32,
        row: u32,
        cell_width: u32,
        cell_height: u32,
        background: LinearRgb,
        cancellation: &CancellationToken,
        patch_output: Option<&mut [f32]>,
    ) -> Result<(f64, Option<PatchDescriptors>), Error> {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let display_x = f64::from(column) * f64::from(cell_width);
        let display_y = f64::from(row) * f64::from(cell_height);
        let display_width = f64::from(cell_width);
        let display_height = f64::from(cell_height);
        if let Some(dimensions) = self.options.patch {
            let patch_len =
                usize::try_from(dimensions.sample_count()).map_err(|_| Error::SizeOverflow)?;
            let output =
                patch_output.ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
            if output.len() != patch_len {
                return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
            }
            let patch_width = dimensions.width as usize;
            let patch_height = dimensions.height as usize;
            let patch_width_f64 = f64::from(dimensions.width);
            let patch_height_f64 = f64::from(dimensions.height);
            let mut total = 0.0;
            let mut work = 0_u64;
            for patch_y in 0..patch_height {
                for patch_x in 0..patch_width {
                    let index = patch_y
                        .checked_mul(patch_width)
                        .and_then(|start| start.checked_add(patch_x))
                        .ok_or(Error::SizeOverflow)?;
                    if (index as u64).is_multiple_of(SAMPLER_CANCEL_INTERVAL)
                        && cancellation.is_cancelled()
                    {
                        return Err(Error::Cancelled);
                    }
                    let x0 = display_x + display_width * patch_x as f64 / patch_width_f64;
                    let x1 = display_x + display_width * (patch_x + 1) as f64 / patch_width_f64;
                    let y0 = display_y + display_height * patch_y as f64 / patch_height_f64;
                    let y1 = display_y + display_height * (patch_y + 1) as f64 / patch_height_f64;
                    let value = self.sample_rect(
                        raster,
                        x0,
                        y0,
                        x1,
                        y1,
                        background,
                        cancellation,
                        &mut work,
                    )?;
                    output[index] = value as f32;
                    total += value;
                }
            }
            let mean = total / patch_len as f64;
            let descriptors = patch_descriptors(output, dimensions, cancellation)?;
            Ok((mean, Some(descriptors)))
        } else {
            let x0 = display_x;
            let y0 = display_y;
            let x1 = display_x + display_width;
            let y1 = display_y + display_height;
            let mut work = 0_u64;
            let mean =
                self.sample_rect(raster, x0, y0, x1, y1, background, cancellation, &mut work)?;
            Ok((mean, None))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn sample_rect(
        self,
        raster: &NormalizedRaster,
        display_x0: f64,
        display_y0: f64,
        display_x1: f64,
        display_y1: f64,
        background: LinearRgb,
        cancellation: &CancellationToken,
        work: &mut u64,
    ) -> Result<f64, Error> {
        match self.options.filter {
            SamplingFilter::Nearest => {
                let display_x = (display_x0 + display_x1) * 0.5;
                let display_y = (display_y0 + display_y1) * 0.5;
                let (source_x, source_y) = self
                    .transform
                    .source_point_for_display(display_x, display_y);
                if source_x < self.transform.source_rect.x
                    || source_y < self.transform.source_rect.y
                    || source_x > self.transform.source_rect.right()
                    || source_y > self.transform.source_rect.bottom()
                    || source_x < 0.0
                    || source_y < 0.0
                    || source_x > f64::from(raster.width())
                    || source_y > f64::from(raster.height())
                {
                    return Ok(background.luminance().value());
                }
                let x = if source_x == f64::from(raster.width()) {
                    raster.width() - 1
                } else {
                    source_x.floor() as u32
                };
                let y = if source_y == f64::from(raster.height()) {
                    raster.height() - 1
                } else {
                    source_y.floor() as u32
                };
                let index = pixel_index(x, y, raster.width())?;
                *work = work.checked_add(1).ok_or(Error::SizeOverflow)?;
                if (*work).is_multiple_of(SAMPLER_CANCEL_INTERVAL) && cancellation.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                Ok(pixel_luminance(raster.pixels()[index], background))
            }
            SamplingFilter::Box => {
                let source_rect = self.transform.source_rect;
                let display_rect = self.transform.display_rect;
                let source_origin_x = source_rect.x.floor() as u32;
                let source_origin_y = source_rect.y.floor() as u32;
                let source_fraction_x = source_rect.x - f64::from(source_origin_x);
                let source_fraction_y = source_rect.y - f64::from(source_origin_y);
                let source_offset_x0 =
                    (display_x0 - display_rect.x) * source_rect.width / display_rect.width;
                let source_offset_y0 =
                    (display_y0 - display_rect.y) * source_rect.height / display_rect.height;
                // Keep each positive span separate from its fractional start; adding them can
                // round an endpoint back to the start when a patch is narrower than one ulp.
                let span_x = (display_x1 - display_x0) * source_rect.width / display_rect.width;
                let span_y = (display_y1 - display_y0) * source_rect.height / display_rect.height;
                if !source_offset_x0.is_finite()
                    || !source_offset_y0.is_finite()
                    || !span_x.is_finite()
                    || span_x <= 0.0
                    || !span_y.is_finite()
                    || span_y <= 0.0
                {
                    return Err(Error::SizeOverflow);
                }
                let raster_x_extent = raster
                    .width()
                    .checked_sub(source_origin_x)
                    .ok_or(Error::SizeOverflow)?;
                let raster_y_extent = raster
                    .height()
                    .checked_sub(source_origin_y)
                    .ok_or(Error::SizeOverflow)?;
                let crop_extent_x = source_rect
                    .width
                    .min(f64::from(raster_x_extent) - source_fraction_x);
                let crop_extent_y = source_rect
                    .height
                    .min(f64::from(raster_y_extent) - source_fraction_y);
                let x_interval = box_axis_interval(
                    source_origin_x,
                    source_fraction_x,
                    source_offset_x0,
                    span_x,
                    crop_extent_x,
                    raster.width(),
                )?;
                let y_interval = box_axis_interval(
                    source_origin_y,
                    source_fraction_y,
                    source_offset_y0,
                    span_y,
                    crop_extent_y,
                    raster.height(),
                )?;
                let mut covered_weight = 0.0;
                let mut weighted_luminance = 0.0;
                if let (Some(x_interval), Some(y_interval)) = (x_interval, y_interval) {
                    let mut remaining_y = y_interval.clipped_span;
                    for y in y_interval.first_pixel..y_interval.end_pixel {
                        let overlap_y = if y == y_interval.first_pixel {
                            y_interval.first_overlap
                        } else {
                            remaining_y.min(1.0)
                        };
                        remaining_y -= overlap_y;
                        let weight_y = overlap_y / span_y;
                        if weight_y <= 0.0 {
                            continue;
                        }
                        let mut remaining_x = x_interval.clipped_span;
                        for x in x_interval.first_pixel..x_interval.end_pixel {
                            let overlap_x = if x == x_interval.first_pixel {
                                x_interval.first_overlap
                            } else {
                                remaining_x.min(1.0)
                            };
                            remaining_x -= overlap_x;
                            let weight = (overlap_x / span_x) * weight_y;
                            if weight <= 0.0 {
                                continue;
                            }
                            *work = work.checked_add(1).ok_or(Error::SizeOverflow)?;
                            if (*work).is_multiple_of(SAMPLER_CANCEL_INTERVAL)
                                && cancellation.is_cancelled()
                            {
                                return Err(Error::Cancelled);
                            }
                            let index = pixel_index(x, y, raster.width())?;
                            weighted_luminance +=
                                pixel_luminance(raster.pixels()[index], background) * weight;
                            covered_weight += weight;
                        }
                    }
                }
                let uncovered_weight = (1.0 - covered_weight).max(0.0);
                Ok(weighted_luminance + background.luminance().value() * uncovered_weight)
            }
        }
    }
}

// Preserve the rounding residual when deciding which source pixel contains a fractional start.
fn two_sum(left: f64, right: f64) -> (f64, f64) {
    let sum = left + right;
    let right_virtual = sum - left;
    let left_roundoff = left - (sum - right_virtual);
    let right_roundoff = right - right_virtual;
    (sum, left_roundoff + right_roundoff)
}

/// A clipped interval with its first pixel overlap kept separate from its total span.
#[derive(Clone, Copy, Debug)]
struct BoxAxisInterval {
    first_pixel: u32,
    end_pixel: u32,
    first_overlap: f64,
    clipped_span: f64,
}

fn box_axis_interval(
    source_origin: u32,
    source_fraction: f64,
    source_offset: f64,
    span: f64,
    crop_extent: f64,
    raster_extent: u32,
) -> Result<Option<BoxAxisInterval>, Error> {
    if !source_fraction.is_finite()
        || source_fraction < 0.0
        || source_fraction >= 1.0
        || !source_offset.is_finite()
        || !span.is_finite()
        || span <= 0.0
        || !crop_extent.is_finite()
        || crop_extent <= 0.0
        || source_origin > raster_extent
    {
        return Err(Error::SizeOverflow);
    }
    let (clipped_start, clipped_span) = if source_offset < 0.0 {
        let outside_span = -source_offset;
        if span <= outside_span {
            return Ok(None);
        }
        (0.0, (span - outside_span).min(crop_extent))
    } else {
        if source_offset >= crop_extent {
            return Ok(None);
        }
        (source_offset, span.min(crop_extent - source_offset))
    };
    if clipped_span <= 0.0 {
        return Ok(None);
    }

    let offset_integer = clipped_start.floor();
    let offset_fraction = clipped_start - offset_integer;
    let (fraction_sum, fraction_error) = two_sum(source_fraction, offset_fraction);
    let starts_after_edge = fraction_sum > 1.0 || (fraction_sum == 1.0 && fraction_error >= 0.0);
    let (pixel_offset, first_pixel_extent) = if starts_after_edge {
        let (fraction_after_edge, fraction_after_edge_error) = two_sum(fraction_sum, -1.0);
        (
            offset_integer as u64 + 1,
            1.0 - (fraction_after_edge + fraction_after_edge_error + fraction_error),
        )
    } else {
        let (distance_to_edge, distance_error) = two_sum(1.0, -source_fraction);
        let (first_pixel_extent, first_pixel_error) = two_sum(distance_to_edge, -offset_fraction);
        (
            offset_integer as u64,
            first_pixel_extent + first_pixel_error + distance_error,
        )
    };
    let raster_extent = u64::from(raster_extent);
    let first_pixel = u64::from(source_origin)
        .checked_add(pixel_offset)
        .ok_or(Error::SizeOverflow)?;
    if first_pixel >= raster_extent {
        return Err(Error::SizeOverflow);
    }
    let first_overlap = clipped_span.min(first_pixel_extent);
    if !first_overlap.is_finite() || first_overlap <= 0.0 {
        return Err(Error::SizeOverflow);
    }
    let remaining_span = clipped_span - first_overlap;
    let following_pixels = remaining_span.ceil() as u64;
    let end_pixel = first_pixel
        .checked_add(1)
        .and_then(|end| end.checked_add(following_pixels))
        .ok_or(Error::SizeOverflow)?
        .min(raster_extent);
    Ok(Some(BoxAxisInterval {
        first_pixel: first_pixel as u32,
        end_pixel: end_pixel as u32,
        first_overlap,
        clipped_span,
    }))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SamplingError {
    Core(Error),
    ColorNormalization(ColorNormalizationError),
}

impl From<Error> for SamplingError {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}

impl From<ColorNormalizationError> for SamplingError {
    fn from(error: ColorNormalizationError) -> Self {
        Self::ColorNormalization(error)
    }
}

/// Validates source orientation, sample format, and metadata before sampling geometry is prepared.
pub(crate) fn preflight_sampling_source(
    source: &RasterFrameView<'_>,
) -> Result<usize, SamplingError> {
    if source.orientation() != ascii_core::RasterOrientation::TopDown {
        return Err(Error::Unsupported(ascii_core::UnsupportedReason::Orientation).into());
    }

    let bytes_per_pixel = match (
        source.format().channels(),
        source.format().sample_depth_bits(),
    ) {
        (ascii_core::PixelChannels::Gray, 8) => 1usize,
        (ascii_core::PixelChannels::Rgb, 8) => 3,
        (ascii_core::PixelChannels::Rgba, 8) => 4,
        (
            ascii_core::PixelChannels::Gray
            | ascii_core::PixelChannels::Rgb
            | ascii_core::PixelChannels::Rgba,
            _,
        ) => {
            return Err(Error::Unsupported(ascii_core::UnsupportedReason::SampleDepth).into());
        }
        (ascii_core::PixelChannels::Other(_), _) => {
            return Err(Error::Unsupported(ascii_core::UnsupportedReason::PixelFormat).into());
        }
    };
    resolve_raster_metadata(source.format().channels(), source.color())?;
    Ok(bytes_per_pixel)
}

/// Preflights allocation, retained storage, and a conservative pixel-overlap work bound.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_sampling(
    source: &RasterFrameView<'_>,
    bytes_per_pixel: usize,
    grid: GridSize,
    cell_width: u32,
    cell_height: u32,
    options: SamplingOptions,
    matching_work: u64,
    frame_validation_work: u64,
    budget: &Budget,
) -> Result<SamplingPlan, SamplingError> {
    let source_width = source.width();
    let source_height = source.height();
    let display_width = u64::from(grid.columns())
        .checked_mul(u64::from(cell_width))
        .ok_or(Error::SizeOverflow)?;
    let display_height = u64::from(grid.rows())
        .checked_mul(u64::from(cell_height))
        .ok_or(Error::SizeOverflow)?;
    let transform = FitTransform::new(
        source_width,
        source_height,
        display_width,
        display_height,
        options.fit,
    )?;
    let cell_count = grid.cell_count();
    let patch_dimensions = options.patch;
    let (samples_per_cell, sample_span_divisor_x, sample_span_divisor_y) = match patch_dimensions {
        Some(patch) => (
            patch.sample_count(),
            u64::from(patch.width),
            u64::from(patch.height),
        ),
        None => (1, 1, 1),
    };
    let sample_count = cell_count
        .checked_mul(samples_per_cell)
        .ok_or(Error::SizeOverflow)?;
    if patch_dimensions.is_some() {
        let _patch_value_capacity =
            usize::try_from(sample_count).map_err(|_| Error::SizeOverflow)?;
        let _descriptor_capacity = usize::try_from(cell_count).map_err(|_| Error::SizeOverflow)?;
    }
    let sample_work = match options.filter {
        SamplingFilter::Nearest => sample_count,
        SamplingFilter::Box => {
            let source_rect = transform.source_rect;
            let display_rect = transform.display_rect;
            let span_x = source_rect.width * f64::from(cell_width)
                / display_rect.width
                / sample_span_divisor_x as f64;
            let span_y = source_rect.height * f64::from(cell_height)
                / display_rect.height
                / sample_span_divisor_y as f64;
            let taps_x = conservative_taps(span_x, f64::from(source_width))?;
            let taps_y = conservative_taps(span_y, f64::from(source_height))?;
            sample_count
                .checked_mul(taps_x)
                .and_then(|work| work.checked_mul(taps_y))
                .ok_or(Error::SizeOverflow)?
        }
    };
    let descriptor_work = match patch_dimensions {
        Some(_) => sample_count.checked_mul(3).ok_or(Error::SizeOverflow)?,
        None => 0,
    };
    let initialization_work = if patch_dimensions.is_some() {
        sample_count
    } else {
        0
    };
    let source_pixel_count = u64::from(source_width)
        .checked_mul(u64::from(source_height))
        .ok_or(Error::SizeOverflow)?;
    let normalization_passes =
        if source.color().alpha_mode() == ascii_core::AlphaMode::Premultiplied {
            2
        } else {
            1
        };
    let normalization_work = source_pixel_count
        .checked_mul(normalization_passes)
        .ok_or(Error::SizeOverflow)?;
    let total_work = normalization_work
        .checked_add(matching_work)
        .and_then(|work| work.checked_add(sample_work))
        .and_then(|work| work.checked_add(descriptor_work))
        .and_then(|work| work.checked_add(initialization_work))
        .and_then(|work| work.checked_add(frame_validation_work))
        .ok_or(Error::SizeOverflow)?;
    if total_work > budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits).into());
    }

    let normalized_bytes = source_pixel_count
        .checked_mul(
            u64::try_from(size_of::<LinearPremultipliedRgba>()).map_err(|_| Error::SizeOverflow)?,
        )
        .ok_or(Error::SizeOverflow)?;
    let retained_patch_bytes = if patch_dimensions.is_some() {
        let values = sample_count
            .checked_mul(u64::try_from(size_of::<f32>()).map_err(|_| Error::SizeOverflow)?)
            .ok_or(Error::SizeOverflow)?;
        let _values_allocation_layout = isize::try_from(values).map_err(|_| Error::SizeOverflow)?;
        let descriptors = cell_count
            .checked_mul(
                u64::try_from(size_of::<PatchDescriptors>()).map_err(|_| Error::SizeOverflow)?,
            )
            .ok_or(Error::SizeOverflow)?;
        let _descriptor_allocation_layout =
            isize::try_from(descriptors).map_err(|_| Error::SizeOverflow)?;
        values
            .checked_add(descriptors)
            .and_then(|bytes| bytes.checked_add(u64::try_from(size_of::<QualityPatches>()).ok()?))
            .ok_or(Error::SizeOverflow)?
    } else {
        0
    };
    let row_bytes = usize::try_from(source.width())
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(bytes_per_pixel)
        .ok_or(Error::SizeOverflow)?;
    if source.row_stride() < row_bytes {
        return Err(Error::Malformed(MalformedReason::RowStrideTooSmall).into());
    }
    let readable_extent = usize::try_from(source.height())
        .map_err(|_| Error::SizeOverflow)?
        .checked_sub(1)
        .and_then(|last_row| last_row.checked_mul(source.row_stride()))
        .and_then(|prior_rows| prior_rows.checked_add(row_bytes))
        .ok_or(Error::SizeOverflow)?;
    let readable_input_bytes = u64::try_from(readable_extent).map_err(|_| Error::SizeOverflow)?;
    let whole_raster_bytes = readable_input_bytes
        .checked_add(normalized_bytes)
        .and_then(|bytes| bytes.checked_add(retained_patch_bytes))
        .ok_or(Error::SizeOverflow)?;
    if whole_raster_bytes > budget.max_raster_bytes() {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes).into());
    }

    let identity = sampling_identity(transform, grid, cell_width, cell_height, options);
    Ok(SamplingPlan {
        transform,
        options,
        identity,
    })
}

fn conservative_taps(span: f64, source_extent: f64) -> Result<u64, Error> {
    if !span.is_finite() || span < 0.0 || !source_extent.is_finite() || source_extent <= 0.0 {
        return Err(Error::SizeOverflow);
    }
    let clipped_span = span.min(source_extent);
    let taps = clipped_span.ceil() as u64 + 1;
    Ok(taps)
}

fn sampling_identity(
    transform: FitTransform,
    grid: GridSize,
    cell_width: u32,
    cell_height: u32,
    options: SamplingOptions,
) -> SamplingIdentity {
    let mut hash = Sha256::new();
    hash.update(b"asciiwall-sampling-identity-v1\0");
    hash.update(transform.source_width.to_le_bytes());
    hash.update(transform.source_height.to_le_bytes());
    hash.update(transform.display_width.to_le_bytes());
    hash.update(transform.display_height.to_le_bytes());
    hash.update(grid.columns().to_le_bytes());
    hash.update(grid.rows().to_le_bytes());
    hash.update(cell_width.to_le_bytes());
    hash.update(cell_height.to_le_bytes());
    match options.fit {
        FitMode::Contain => hash.update([0]),
        FitMode::Cover => hash.update([1]),
        FitMode::CustomCrop(_) => hash.update([2]),
    }
    hash.update(match options.filter {
        SamplingFilter::Box => [0],
        SamplingFilter::Nearest => [1],
    });
    for value in [
        transform.source_rect.x,
        transform.source_rect.y,
        transform.source_rect.width,
        transform.source_rect.height,
        transform.display_rect.x,
        transform.display_rect.y,
        transform.display_rect.width,
        transform.display_rect.height,
    ] {
        hash.update(value.to_bits().to_le_bytes());
    }
    match options.patch {
        Some(patch) => {
            hash.update([1]);
            hash.update(patch.width.to_le_bytes());
            hash.update(patch.height.to_le_bytes());
        }
        None => hash.update([0]),
    }
    SamplingIdentity(hash.finalize().into())
}

fn pixel_index(x: u32, y: u32, width: u32) -> Result<usize, Error> {
    let row = usize::try_from(y).map_err(|_| Error::SizeOverflow)?;
    let width = usize::try_from(width).map_err(|_| Error::SizeOverflow)?;
    let column = usize::try_from(x).map_err(|_| Error::SizeOverflow)?;
    row.checked_mul(width)
        .and_then(|start| start.checked_add(column))
        .ok_or(Error::SizeOverflow)
}

fn pixel_luminance(pixel: LinearPremultipliedRgba, background: LinearRgb) -> f64 {
    pixel.composite_over(background).luminance().value()
}

fn patch_descriptors(
    values: &[f32],
    dimensions: PatchDimensions,
    cancellation: &CancellationToken,
) -> Result<PatchDescriptors, Error> {
    let width = usize::try_from(dimensions.width).map_err(|_| Error::SizeOverflow)?;
    let height = usize::try_from(dimensions.height).map_err(|_| Error::SizeOverflow)?;
    let expected = width.checked_mul(height).ok_or(Error::SizeOverflow)?;
    if values.len() != expected || values.is_empty() {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }
    let mut mean = 0.0_f64;
    for (index, value) in values.iter().enumerate() {
        if (index as u64).is_multiple_of(SAMPLER_CANCEL_INTERVAL) && cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        mean += f64::from(*value);
    }
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let mut derivative_work = 0_u64;
    let mut dx = 0.0_f64;
    let mut dx_count = 0_u64;
    for y in 0..height {
        for x in 0..width.saturating_sub(1) {
            let index = y
                .checked_mul(width)
                .and_then(|start| start.checked_add(x))
                .ok_or(Error::SizeOverflow)?;
            derivative_work = derivative_work.checked_add(1).ok_or(Error::SizeOverflow)?;
            if derivative_work.is_multiple_of(SAMPLER_CANCEL_INTERVAL)
                && cancellation.is_cancelled()
            {
                return Err(Error::Cancelled);
            }
            dx += f64::from((values[index + 1] - values[index]).abs());
            dx_count += 1;
        }
    }
    let mut dy = 0.0_f64;
    let mut dy_count = 0_u64;
    for y in 0..height.saturating_sub(1) {
        for x in 0..width {
            let index = y
                .checked_mul(width)
                .and_then(|start| start.checked_add(x))
                .ok_or(Error::SizeOverflow)?;
            let below = index.checked_add(width).ok_or(Error::SizeOverflow)?;
            derivative_work = derivative_work.checked_add(1).ok_or(Error::SizeOverflow)?;
            if derivative_work.is_multiple_of(SAMPLER_CANCEL_INTERVAL)
                && cancellation.is_cancelled()
            {
                return Err(Error::Cancelled);
            }
            dy += f64::from((values[below] - values[index]).abs());
            dy_count += 1;
        }
    }
    Ok(PatchDescriptors {
        mean_luminance: (mean / expected as f64) as f32,
        mean_absolute_dx: if dx_count == 0 {
            0.0
        } else {
            (dx / dx_count as f64) as f32
        },
        mean_absolute_dy: if dy_count == 0 {
            0.0
        } else {
            (dy / dy_count as f64) as f32
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{CropRect, FitMode, FitTransform, PatchDimensions, QualityPatches};
    use ascii_core::{CancellationToken, Error, MalformedReason};

    #[test]
    fn patch_allocation_observes_preexisting_cancellation() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            QualityPatches::allocate(
                PatchDimensions::new(1, 1).expect("one-sample patch is valid"),
                1,
                1,
                &cancellation,
            )
            .err(),
            Some(Error::Cancelled)
        );
    }

    #[test]
    fn invalid_normalized_crop_and_empty_transform_are_rejected() {
        assert_eq!(
            CropRect::new(0.5, 0.0, 0.5, 1.0),
            Err(Error::Malformed(MalformedReason::InvalidConfiguration))
        );
        assert_eq!(
            FitTransform::new(0, 10, 10, 10, FitMode::Contain),
            Err(Error::Malformed(MalformedReason::EmptyDimensions))
        );
    }
    #[test]
    fn rejects_destination_edges_outside_exact_f64_integer_range() {
        let maximum_exact = 1_u64 << 53;
        let valid =
            FitTransform::new(1, 1, maximum_exact, maximum_exact, FitMode::Contain).unwrap();
        assert_eq!(valid.display_dimensions(), (maximum_exact, maximum_exact));
        assert_eq!(
            FitTransform::new(1, 1, maximum_exact + 1, 1, FitMode::Contain),
            Err(Error::SizeOverflow)
        );
        assert_eq!(
            FitTransform::new(1, 1, 1, maximum_exact + 1, FitMode::Contain),
            Err(Error::SizeOverflow)
        );
    }
    #[test]
    fn rejects_contain_transforms_with_unrepresentable_centered_edges() {
        let maximum_exact = 1_u64 << 53;
        assert_eq!(
            FitTransform::new(1, 1, maximum_exact, 1, FitMode::Contain),
            Err(Error::SizeOverflow)
        );
        assert_eq!(
            FitTransform::new(1, 4_000_000_000, maximum_exact, 1, FitMode::Contain),
            Err(Error::SizeOverflow)
        );
    }
}
