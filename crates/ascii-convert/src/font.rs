//! Font parsing, bounded rasterization, and stable table identity.

use crate::font_preflight::preflight_font;

extern crate alloc;

use alloc::vec::Vec;
use core::num::NonZeroU32;

use ascii_core::{
    Budget, BudgetKind, CancellationToken, Error, MalformedReason, UnsupportedReason,
};
use fontdue::{Font, FontSettings};
use sha2::{Digest, Sha256};

use crate::glyphs::{Centroid, EdgeOrientation, GlyphBounds, GlyphMetrics, describe};

const MAX_FONT_BYTES: usize = 16 * 1024 * 1024;
const MAX_GLYPHS: usize = 4096;
const MAX_GLYPH_RASTER_PIXELS: usize = 16 * 1024 * 1024;
const MAX_GLYPH_RASTER_DIMENSION: usize = 16 * 1024;
const MAX_CHARSET_BYTES: usize = MAX_GLYPHS * 4;
const DESCRIPTOR_VERSION: u32 = 1;
const RENDERING_MODE: &[u8] = b"fontdue-0.9.3-gray8-no-gsub-v1";

/// Validated geometry and raster size for a single-cell glyph table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphConfig {
    raster_size: f32,
    cell_width: NonZeroU32,
    cell_height: NonZeroU32,
    baseline: u32,
}

impl GlyphConfig {
    /// Creates a glyph configuration after validating raster and cell geometry.
    pub fn new(
        raster_size: f32,
        cell_width: u32,
        cell_height: u32,
        baseline: u32,
    ) -> Result<Self, Error> {
        if !raster_size.is_finite()
            || raster_size <= 0.0
            || raster_size > 4096.0
            || cell_width == 0
            || cell_height == 0
            || baseline > cell_height
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }

        Ok(Self {
            raster_size,
            cell_width: NonZeroU32::new(cell_width)
                .ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?,
            cell_height: NonZeroU32::new(cell_height)
                .ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?,
            baseline,
        })
    }

    #[must_use]
    pub const fn raster_size(self) -> f32 {
        self.raster_size
    }

    #[must_use]
    pub const fn cell_width(self) -> u32 {
        self.cell_width.get()
    }

    #[must_use]
    pub const fn cell_height(self) -> u32 {
        self.cell_height.get()
    }

    #[must_use]
    pub const fn baseline(self) -> u32 {
        self.baseline
    }
}

/// Measured glyph data. All access is read-only after table construction.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphRecord {
    character: char,
    glyph_index: u16,
    metrics: GlyphMetrics,
    bounds: GlyphBounds,
    centroid: Option<Centroid>,
    mean_coverage: f32,
    edges: EdgeOrientation,
    mask: Vec<u8>,
}

impl GlyphRecord {
    #[must_use]
    pub const fn character(&self) -> char {
        self.character
    }

    #[must_use]
    pub const fn glyph_index(&self) -> u16 {
        self.glyph_index
    }

    #[must_use]
    pub const fn metrics(&self) -> GlyphMetrics {
        self.metrics
    }

    #[must_use]
    pub const fn bounds(&self) -> GlyphBounds {
        self.bounds
    }

    #[must_use]
    pub const fn centroid(&self) -> Option<Centroid> {
        self.centroid
    }

    #[must_use]
    pub const fn mean_coverage(&self) -> f32 {
        self.mean_coverage
    }

    #[must_use]
    pub const fn edge_orientation(&self) -> EdgeOrientation {
        self.edges
    }

    #[must_use]
    pub fn mask(&self) -> &[u8] {
        &self.mask
    }
}

/// Immutable glyph lookup table identified by its complete rasterization input fingerprint.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphTable {
    fingerprint: [u8; 32],
    cell_width: u32,
    cell_height: u32,
    glyphs: Vec<GlyphRecord>,
}

impl GlyphTable {
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    #[must_use]
    pub const fn cell_width(&self) -> u32 {
        self.cell_width
    }

    #[must_use]
    pub const fn cell_height(&self) -> u32 {
        self.cell_height
    }

    /// Glyphs are ordered by measured coverage, then glyph index, then Unicode scalar.
    #[must_use]
    pub fn glyphs(&self) -> &[GlyphRecord] {
        &self.glyphs
    }
}

/// Builds a fully measured, cell-clipped table. No table is published unless every glyph succeeds.
pub fn build_glyph_table(
    font_bytes: &[u8],
    charset: &str,
    config: &GlyphConfig,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<GlyphTable, Error> {
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if font_bytes.is_empty() {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    if font_bytes.len() > MAX_FONT_BYTES {
        return Err(Error::BudgetExceeded(BudgetKind::EncodedBytes));
    }
    if charset.len() > MAX_CHARSET_BYTES {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }
    let glyph_count = charset.chars().count();
    if glyph_count == 0 {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }
    if glyph_count > MAX_GLYPHS {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }
    if charset.chars().any(char::is_control) {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }

    let glyph_count_u64 = u64::try_from(glyph_count).map_err(|_| Error::SizeOverflow)?;
    let font_bytes_len = u64::try_from(font_bytes.len()).map_err(|_| Error::SizeOverflow)?;
    let width = usize::try_from(config.cell_width()).map_err(|_| Error::SizeOverflow)?;
    let height = usize::try_from(config.cell_height()).map_err(|_| Error::SizeOverflow)?;
    let mask_len = width.checked_mul(height).ok_or(Error::SizeOverflow)?;
    let _mask_layout = isize::try_from(mask_len).map_err(|_| Error::SizeOverflow)?;
    let mask_bytes = u64::try_from(mask_len)
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(glyph_count_u64)
        .ok_or(Error::SizeOverflow)?;
    let records_bytes = u64::try_from(core::mem::size_of::<GlyphRecord>())
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(glyph_count_u64)
        .ok_or(Error::SizeOverflow)?;
    let characters_bytes = u64::try_from(core::mem::size_of::<char>())
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(glyph_count_u64)
        .ok_or(Error::SizeOverflow)?;
    let glyph_indices_bytes = u64::try_from(core::mem::size_of::<u16>())
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(glyph_count_u64)
        .ok_or(Error::SizeOverflow)?;
    let base_bytes = font_bytes_len
        .checked_add(mask_bytes)
        .and_then(|value| value.checked_add(records_bytes))
        .and_then(|value| value.checked_add(characters_bytes))
        .and_then(|value| value.checked_add(glyph_indices_bytes))
        .ok_or(Error::SizeOverflow)?;
    if base_bytes > budget.max_raster_bytes() {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
    }

    let font_settings = FontSettings {
        load_substitutions: false,
        ..FontSettings::default()
    };
    let parser_scale = font_settings.scale;
    let table_work = u64::try_from(mask_len)
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(glyph_count_u64)
        .and_then(|value| value.checked_add(font_bytes_len))
        .ok_or(Error::SizeOverflow)?;
    let font_scan_work = font_bytes_len
        .checked_mul(glyph_count_u64)
        .ok_or(Error::SizeOverflow)?;
    let preparse_work = table_work
        .checked_add(font_scan_work)
        .ok_or(Error::SizeOverflow)?;
    let parser_raster_budget = budget
        .max_raster_bytes()
        .checked_sub(base_bytes)
        .ok_or(Error::BudgetExceeded(BudgetKind::RasterBytes))?;
    let parser_work_budget = budget
        .max_work_units()
        .checked_sub(preparse_work)
        .ok_or(Error::BudgetExceeded(BudgetKind::WorkUnits))?;
    let parser_preflight = preflight_font(
        font_bytes,
        font_settings.collection_index,
        parser_scale,
        parser_raster_budget,
        parser_work_budget,
    )?;
    let base_bytes = base_bytes
        .checked_add(parser_preflight.parser_bytes)
        .ok_or(Error::SizeOverflow)?;
    if base_bytes > budget.max_raster_bytes() {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
    }
    let preparse_work = preparse_work
        .checked_add(parser_preflight.work_units)
        .ok_or(Error::SizeOverflow)?;
    if preparse_work > budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }

    let mut characters = Vec::new();
    characters
        .try_reserve_exact(glyph_count)
        .map_err(|_| Error::AllocationFailed)?;
    characters.extend(charset.chars());
    characters.sort_unstable();
    if characters.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }

    let font = Font::from_bytes(font_bytes, font_settings)
        .map_err(|_| Error::Malformed(MalformedReason::InvalidEncodedData))?;
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let line_height = font
        .horizontal_line_metrics(config.raster_size())
        .map_or(0.0, |metrics| metrics.new_line_size);
    if !line_height.is_finite() || line_height < 0.0 {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }

    let mut glyph_indices = Vec::new();
    glyph_indices
        .try_reserve_exact(glyph_count)
        .map_err(|_| Error::AllocationFailed)?;
    let mut max_bitmap_area = 0usize;
    let mut raster_work = 0u64;
    for &character in &characters {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let glyph_index = lookup_glyph_index(&font, character)
            .ok_or(Error::Unsupported(UnsupportedReason::GlyphUnavailable))?;
        let metrics = font.metrics_indexed(glyph_index, config.raster_size());
        validate_metrics(metrics.advance_width, line_height)?;
        if metrics.width > MAX_GLYPH_RASTER_DIMENSION || metrics.height > MAX_GLYPH_RASTER_DIMENSION
        {
            return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
        }
        let bitmap_area = metrics
            .width
            .checked_mul(metrics.height)
            .ok_or(Error::SizeOverflow)?;
        if bitmap_area > MAX_GLYPH_RASTER_PIXELS {
            return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
        }
        max_bitmap_area = max_bitmap_area.max(bitmap_area);
        let glyph_work = u64::try_from(bitmap_area)
            .map_err(|_| Error::SizeOverflow)?
            .checked_add(u64::try_from(metrics.width).map_err(|_| Error::SizeOverflow)?)
            .and_then(|value| value.checked_add(u64::try_from(metrics.height).ok()?))
            .ok_or(Error::SizeOverflow)?;
        raster_work = raster_work
            .checked_add(glyph_work)
            .ok_or(Error::SizeOverflow)?;
        glyph_indices.push(glyph_index);
    }
    let cmap_count = u64::try_from(font.chars().len().max(1)).map_err(|_| Error::SizeOverflow)?;
    let lookup_work = glyph_count_u64
        .checked_mul(cmap_count)
        .ok_or(Error::SizeOverflow)?;

    // fontdue's scalar rasterizer keeps an f32 coverage accumulator and a u8 bitmap at once.
    let raster_scratch = u64::try_from(max_bitmap_area)
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(
            u64::try_from(core::mem::size_of::<f32>() + core::mem::size_of::<u8>())
                .map_err(|_| Error::SizeOverflow)?,
        )
        .and_then(|value| value.checked_add(u64::try_from(3 * core::mem::size_of::<f32>()).ok()?))
        .ok_or(Error::SizeOverflow)?;
    let total_bytes = base_bytes
        .checked_add(raster_scratch)
        .ok_or(Error::SizeOverflow)?;
    if total_bytes > budget.max_raster_bytes() {
        return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
    }
    let total_work = table_work
        .checked_add(lookup_work)
        .and_then(|value| value.checked_add(raster_work))
        .and_then(|value| value.checked_add(font_scan_work))
        .and_then(|value| value.checked_add(parser_preflight.work_units))
        .ok_or(Error::SizeOverflow)?;
    if total_work > budget.max_work_units() {
        return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
    }

    let mut fingerprint = Sha256::new();
    fingerprint.update(b"asciiwall-glyph-table\0");
    fingerprint.update(DESCRIPTOR_VERSION.to_le_bytes());
    fingerprint.update(font_bytes_len.to_le_bytes());
    fingerprint.update(font_bytes);
    fingerprint.update(config.raster_size().to_bits().to_le_bytes());
    fingerprint.update(config.cell_width().to_le_bytes());
    fingerprint.update(config.cell_height().to_le_bytes());
    fingerprint.update(config.baseline().to_le_bytes());
    fingerprint.update(RENDERING_MODE);
    fingerprint.update(font_settings.collection_index.to_le_bytes());
    fingerprint.update(parser_scale.to_bits().to_le_bytes());
    fingerprint.update([u8::from(font_settings.load_substitutions)]);
    fingerprint.update(
        u32::try_from(glyph_count)
            .map_err(|_| Error::SizeOverflow)?
            .to_le_bytes(),
    );
    for character in &characters {
        fingerprint.update((*character as u32).to_le_bytes());
    }

    let mut glyphs = Vec::new();
    glyphs
        .try_reserve_exact(glyph_count)
        .map_err(|_| Error::AllocationFailed)?;
    for (&character, &glyph_index) in characters.iter().zip(&glyph_indices) {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let (metrics, bitmap) = font.rasterize_indexed(glyph_index, config.raster_size());
        validate_metrics(metrics.advance_width, line_height)?;
        let bitmap_len = metrics
            .width
            .checked_mul(metrics.height)
            .ok_or(Error::SizeOverflow)?;
        if bitmap.len() != bitmap_len {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }

        let mut mask = Vec::new();
        mask.try_reserve_exact(mask_len)
            .map_err(|_| Error::AllocationFailed)?;
        mask.resize(mask_len, 0);
        let x_origin = ((f64::from(config.cell_width()) - f64::from(metrics.advance_width)) * 0.5
            + f64::from(metrics.xmin))
        .floor();
        let y_origin =
            f64::from(config.baseline()) - (f64::from(metrics.ymin) + metrics.height as f64);
        if !x_origin.is_finite() || !y_origin.is_finite() {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        let fully_clipped = x_origin + metrics.width as f64 <= 0.0
            || x_origin >= f64::from(config.cell_width())
            || y_origin + metrics.height as f64 <= 0.0
            || y_origin >= f64::from(config.cell_height());
        if !fully_clipped
            && (x_origin < -(MAX_GLYPH_RASTER_DIMENSION as f64)
                || x_origin > f64::from(config.cell_width())
                || y_origin < -(MAX_GLYPH_RASTER_DIMENSION as f64)
                || y_origin > f64::from(config.cell_height()))
        {
            return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
        }
        let x_origin = x_origin as i64;
        let y_origin = y_origin.floor() as i64;
        for source_y in 0..metrics.height {
            if cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            for source_x in 0..metrics.width {
                let target_x = x_origin
                    .checked_add(i64::try_from(source_x).map_err(|_| Error::SizeOverflow)?)
                    .ok_or(Error::SizeOverflow)?;
                let target_y = y_origin
                    .checked_add(i64::try_from(source_y).map_err(|_| Error::SizeOverflow)?)
                    .ok_or(Error::SizeOverflow)?;
                if target_x < 0
                    || target_y < 0
                    || target_x >= i64::from(config.cell_width())
                    || target_y >= i64::from(config.cell_height())
                {
                    continue;
                }
                let source_index = source_y
                    .checked_mul(metrics.width)
                    .and_then(|value| value.checked_add(source_x))
                    .ok_or(Error::SizeOverflow)?;
                let target_index = usize::try_from(target_y)
                    .map_err(|_| Error::SizeOverflow)?
                    .checked_mul(width)
                    .and_then(|value| value.checked_add(usize::try_from(target_x).ok()?))
                    .ok_or(Error::SizeOverflow)?;
                let coverage = bitmap
                    .get(source_index)
                    .copied()
                    .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))?;
                *mask
                    .get_mut(target_index)
                    .ok_or(Error::Malformed(MalformedReason::InvalidEncodedData))? = coverage;
            }
        }

        let (bounds, centroid, mean_coverage, edges) = describe(&mask, width, height);
        glyphs.push(GlyphRecord {
            character,
            glyph_index,
            metrics: GlyphMetrics::new(metrics.advance_width, line_height, config.baseline()),
            bounds,
            centroid,
            mean_coverage,
            edges,
            mask,
        });
    }

    glyphs.sort_unstable_by(|left, right| {
        left.mean_coverage
            .total_cmp(&right.mean_coverage)
            .then_with(|| left.glyph_index.cmp(&right.glyph_index))
            .then_with(|| left.character.cmp(&right.character))
    });

    Ok(GlyphTable {
        fingerprint: fingerprint.finalize().into(),
        cell_width: config.cell_width(),
        cell_height: config.cell_height(),
        glyphs,
    })
}

fn lookup_glyph_index(font: &Font, character: char) -> Option<u16> {
    font.chars()
        .get(&character)
        .copied()
        .map(|index| index.get())
}

fn validate_metrics(advance_width: f32, line_height: f32) -> Result<(), Error> {
    if !advance_width.is_finite() || advance_width < 0.0 || !line_height.is_finite() {
        return Err(Error::Malformed(MalformedReason::InvalidEncodedData));
    }
    Ok(())
}
