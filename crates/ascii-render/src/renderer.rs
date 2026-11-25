//! Validated, bounded wgpu glyph-atlas presentation.

use ascii_convert::GlyphTable;
use ascii_core::{
    AsciiCell, AsciiFrame, Budget, BudgetKind, CancellationToken, GlyphTableId, GridSize,
    MalformedReason, validate_frame,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::surface::{
    PresentOutcome, RecoverableReason, RenderTarget, SkippedReason, TargetKey, TargetKind,
    cached_outcome, surface_error,
};

/// Revision of the engine-owned shader, for evidence records.
pub const SHADER_REVISION: &str = "ascii-presenter-2";
const CELL_BYTES: u64 = 16;
const PARAM_BYTES: u64 = 32;
const ATLAS_PADDING: u32 = 1;
const POLL_INTERVAL: usize = 1024;

/// Presenter failures, preserving portable core failures and explicit GPU boundary failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// A frame, budget, allocation, or cancellation failure from the core contract.
    Core(ascii_core::Error),
    /// The requested resources exceed the indicated enabled device limit.
    DeviceLimit(&'static str),
    /// The target format or texture properties are outside the baseline contract.
    UnsupportedTarget,
    /// Surface acquisition reported GPU memory exhaustion.
    OutOfMemory,
}

impl From<ascii_core::Error> for Error {
    fn from(value: ascii_core::Error) -> Self {
        Self::Core(value)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core(error) => write!(f, "{error}"),
            Self::DeviceLimit(limit) => write!(f, "device limit exceeded or unavailable: {limit}"),
            Self::UnsupportedTarget => f.write_str("unsupported render target"),
            Self::OutOfMemory => f.write_str("surface acquisition exhausted GPU memory"),
        }
    }
}
impl std::error::Error for Error {}

/// Host-updated device-loss signal. Install the host's wgpu lost callback with a clone of this
/// value, and call `mark_lost`; the renderer does not replace the host's callback.
#[derive(Clone, Debug, Default)]
pub struct DeviceStatus(Arc<AtomicBool>);
impl DeviceStatus {
    /// Creates a live device status.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Permanently marks this device as lost. Use a new status for a replacement device.
    pub fn mark_lost(&self) {
        self.0.store(true, Ordering::Release);
    }
    /// Returns whether loss has been signalled by the host.
    #[must_use]
    pub fn is_lost(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Counts atlas-cache hits, API resource creation and submitted work, not driver allocations or timings.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PresenterStats {
    pub atlas_textures: u64,
    pub atlas_cache_hits: u64,
    pub cell_buffers: u64,
    pub pipelines: u64,
    pub uploads: u64,
    pub uploaded_bytes: u64,
    pub submissions: u64,
    pub draw_calls: u64,
}

// WGSL Cell has @align(16), four u32 members, and a 16-byte array stride. Serialization below
// deliberately does not transmute either this struct or the separate 12-byte core AsciiCell.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GpuCell {
    glyph: u32,
    foreground: u32,
    background: u32,
    flags: u32,
}
impl GpuCell {
    fn from_cell(cell: AsciiCell) -> Self {
        Self {
            glyph: cell.glyph(),
            foreground: u32::from_le_bytes(cell.fg_srgba8()),
            background: u32::from_le_bytes(cell.bg_srgba8()),
            flags: 0,
        }
    }
    fn bytes(self) -> [u8; 16] {
        let mut output = [0; 16];
        output[0..4].copy_from_slice(&self.glyph.to_le_bytes());
        output[4..8].copy_from_slice(&self.foreground.to_le_bytes());
        output[8..12].copy_from_slice(&self.background.to_le_bytes());
        output[12..16].copy_from_slice(&self.flags.to_le_bytes());
        output
    }
}

#[derive(Clone, Copy, Debug)]
struct Plan {
    count: u32,
    cell_bytes: u64,
    atlas_width: u32,
    atlas_height: u32,
    atlas_columns: u32,
    atlas_tile_width: u32,
    atlas_tile_height: u32,
    atlas_bytes: u64,
    atlas_row_pitch: u32,
    atlas_staging_bytes: u64,
}

#[derive(Clone, Copy)]
struct PlanInput {
    size: GridSize,
    glyph_count: u32,
    glyph_width: u32,
    glyph_height: u32,
    target_size: (u32, u32),
}

fn plan(input: PlanInput, limits: &wgpu::Limits, budget: &Budget) -> Result<Plan, Error> {
    let PlanInput {
        size,
        glyph_count,
        glyph_width,
        glyph_height,
        target_size,
    } = input;
    let count = u32::try_from(size.cell_count()).map_err(|_| ascii_core::Error::SizeOverflow)?;
    let cell_bytes = u64::from(count)
        .checked_mul(CELL_BYTES)
        .ok_or(ascii_core::Error::SizeOverflow)?;
    let _ = isize::try_from(cell_bytes).map_err(|_| ascii_core::Error::SizeOverflow)?;
    if limits.max_bind_groups < 1
        || limits.max_bindings_per_bind_group < 3
        || limits.max_non_sampler_bindings < 3
        || limits.max_storage_buffers_per_shader_stage < 1
        || limits.max_uniform_buffers_per_shader_stage < 1
        || limits.max_sampled_textures_per_shader_stage < 1
        || limits.max_inter_stage_shader_components < 3
        || u64::from(limits.max_uniform_buffer_binding_size) < PARAM_BYTES
        || limits.max_buffer_size < PARAM_BYTES
    {
        return Err(Error::DeviceLimit("required shader bindings"));
    }
    if cell_bytes > limits.max_buffer_size
        || cell_bytes > u64::from(limits.max_storage_buffer_binding_size)
    {
        return Err(Error::DeviceLimit("cell storage buffer"));
    }
    if glyph_count == 0 || glyph_width == 0 || glyph_height == 0 {
        return Err(ascii_core::Error::Malformed(MalformedReason::InvalidConfiguration).into());
    }
    let max_dim = limits.max_texture_dimension_2d;
    let atlas_tile_width = glyph_width
        .checked_add(ATLAS_PADDING * 2)
        .ok_or(ascii_core::Error::SizeOverflow)?;
    let atlas_tile_height = glyph_height
        .checked_add(ATLAS_PADDING * 2)
        .ok_or(ascii_core::Error::SizeOverflow)?;
    if target_size.0 > max_dim || target_size.1 > max_dim {
        return Err(Error::DeviceLimit("2D texture dimensions"));
    }
    if atlas_tile_width > max_dim || atlas_tile_height > max_dim {
        return Err(Error::DeviceLimit("padded glyph dimensions"));
    }
    let atlas_columns = (max_dim / atlas_tile_width).min(glyph_count);
    if atlas_columns == 0 {
        return Err(Error::DeviceLimit("atlas width"));
    }
    let atlas_rows = glyph_count.div_ceil(atlas_columns);
    let atlas_width = atlas_columns
        .checked_mul(atlas_tile_width)
        .ok_or(ascii_core::Error::SizeOverflow)?;
    let atlas_height = atlas_rows
        .checked_mul(atlas_tile_height)
        .ok_or(ascii_core::Error::SizeOverflow)?;
    if atlas_height > max_dim {
        return Err(Error::DeviceLimit("atlas height"));
    }
    let atlas_len = u64::from(atlas_width)
        .checked_mul(u64::from(atlas_height))
        .ok_or(ascii_core::Error::SizeOverflow)?;
    // Use our own mapped COPY_SRC buffer rather than Queue::write_texture, whose internal
    // staging pitch can be device-dependent (including Vulkan's optimal row-pitch alignment).
    let alignment = u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let row_pitch = u64::from(atlas_width)
        .div_ceil(alignment)
        .checked_mul(alignment)
        .ok_or(ascii_core::Error::SizeOverflow)?;
    let atlas_row_pitch = u32::try_from(row_pitch).map_err(|_| ascii_core::Error::SizeOverflow)?;
    let atlas_staging_bytes = row_pitch
        .checked_mul(u64::from(atlas_height))
        .ok_or(ascii_core::Error::SizeOverflow)?;
    let _ = isize::try_from(atlas_staging_bytes).map_err(|_| ascii_core::Error::SizeOverflow)?;
    if atlas_staging_bytes > limits.max_buffer_size {
        return Err(Error::DeviceLimit("atlas staging buffer"));
    }
    // The R8 texture and one padded, directly mapped staging buffer coexist during upload.
    if atlas_len
        .checked_add(atlas_staging_bytes)
        .ok_or(ascii_core::Error::SizeOverflow)?
        > budget.max_raster_bytes()
    {
        return Err(ascii_core::Error::BudgetExceeded(BudgetKind::RasterBytes).into());
    }
    if cell_bytes
        .checked_mul(3)
        .ok_or(ascii_core::Error::SizeOverflow)?
        > budget.max_frame_bytes()
    {
        return Err(ascii_core::Error::BudgetExceeded(BudgetKind::FrameBytes).into());
    }
    let pixels = u64::from(target_size.0)
        .checked_mul(u64::from(target_size.1))
        .ok_or(ascii_core::Error::SizeOverflow)?;
    // A presentation validates cells, may compare a cached copy, then serializes changed cells.
    // Preflight all three worst-case passes before the conditional comparison and serialization.
    let cell_work = u64::from(count)
        .checked_mul(3)
        .ok_or(ascii_core::Error::SizeOverflow)?;
    let work = pixels
        .checked_mul(2)
        .and_then(|v| v.checked_add(atlas_len))
        .and_then(|v| v.checked_add(atlas_len))
        // wgpu zero-initializes the whole mapped staging buffer, including row padding.
        .and_then(|v| v.checked_add(atlas_staging_bytes))
        .and_then(|v| v.checked_add(cell_work))
        .ok_or(ascii_core::Error::SizeOverflow)?;
    if work > budget.max_work_units() {
        return Err(ascii_core::Error::BudgetExceeded(BudgetKind::WorkUnits).into());
    }
    Ok(Plan {
        count,
        cell_bytes,
        atlas_width,
        atlas_height,
        atlas_columns,
        atlas_tile_width,
        atlas_tile_height,
        atlas_bytes: atlas_len,
        atlas_row_pitch,
        atlas_staging_bytes,
    })
}
fn ensure_frame_budget(
    current_cpu_capacity: u64,
    current_gpu_capacity: u64,
    requested: u64,
    budget: &Budget,
) -> Result<(), Error> {
    let retained_cpu = current_cpu_capacity.max(requested);
    let retained_gpu = current_gpu_capacity.max(requested);
    // A growing Vec may briefly retain both the old packed storage and its replacement.
    let cpu_growth_overlap = if current_cpu_capacity < requested {
        current_cpu_capacity
    } else {
        0
    };
    // The replacement GPU buffer and the upload staging bytes coexist with the old buffer.
    let gpu_growth_overlap = if current_gpu_capacity < requested {
        current_gpu_capacity
    } else {
        0
    };
    let frame_bytes = retained_cpu
        .checked_add(retained_gpu)
        .and_then(|bytes| bytes.checked_add(cpu_growth_overlap))
        .and_then(|bytes| bytes.checked_add(gpu_growth_overlap))
        .and_then(|bytes| bytes.checked_add(requested))
        .ok_or(ascii_core::Error::SizeOverflow)?;
    if frame_bytes > budget.max_frame_bytes() {
        return Err(ascii_core::Error::BudgetExceeded(BudgetKind::FrameBytes).into());
    }
    Ok(())
}

struct Atlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum AtlasSamplingMode {
    IntegerTextureLoad,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum AtlasFormat {
    R8Unorm,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct AtlasKey {
    fingerprint: [u8; 32],
    atlas_width: u32,
    atlas_height: u32,
    atlas_columns: u32,
    atlas_tile_width: u32,
    atlas_tile_height: u32,
    cell_width: u32,
    cell_height: u32,
    padding: u32,
    sampling_mode: AtlasSamplingMode,
    format: AtlasFormat,
}

struct AtlasCacheEntries {
    atlases: HashMap<AtlasKey, Arc<Atlas>>,
    bytes: u64,
}

/// Same-device owner of immutable glyph atlases. Construct one per host device and share its
/// `Arc` between renderers for preview and wallpaper targets.
pub struct AtlasCache {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    max_bytes: u64,
    entries: Mutex<AtlasCacheEntries>,
}

impl AtlasCache {
    /// Creates an empty cache bounded by the host's atlas byte budget.
    #[must_use]
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, max_bytes: u64) -> Self {
        Self {
            device,
            queue,
            max_bytes,
            entries: Mutex::new(AtlasCacheEntries {
                atlases: HashMap::new(),
                bytes: 0,
            }),
        }
    }

    /// Returns retained immutable texture bytes, excluding staging and backend allocations.
    #[must_use]
    pub fn resident_bytes(&self) -> u64 {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .bytes
    }

    fn acquire(
        &self,
        table: &GlyphTable,
        plan: Plan,
        cancellation: &CancellationToken,
    ) -> Result<(Arc<Atlas>, bool), Error> {
        let key = AtlasKey {
            fingerprint: *table.fingerprint(),
            atlas_width: plan.atlas_width,
            atlas_height: plan.atlas_height,
            atlas_columns: plan.atlas_columns,
            atlas_tile_width: plan.atlas_tile_width,
            atlas_tile_height: plan.atlas_tile_height,
            cell_width: table.cell_width(),
            cell_height: table.cell_height(),
            padding: ATLAS_PADDING,
            sampling_mode: AtlasSamplingMode::IntegerTextureLoad,
            format: AtlasFormat::R8Unorm,
        };
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(atlas) = entries.atlases.get(&key) {
            return Ok((atlas.clone(), false));
        }

        // Decide whether unreferenced generations suffice before evicting any of them. A
        // failed budget request therefore leaves both the cache and all active renderer refs intact.
        let retained_bytes = entries
            .atlases
            .values()
            .filter(|atlas| Arc::strong_count(atlas) > 1)
            .try_fold(0u64, |bytes, atlas| bytes.checked_add(atlas.bytes))
            .ok_or(ascii_core::Error::SizeOverflow)?;
        let required_bytes = retained_bytes
            .checked_add(plan.atlas_bytes)
            .ok_or(ascii_core::Error::SizeOverflow)?;
        if plan.atlas_bytes > self.max_bytes || required_bytes > self.max_bytes {
            return Err(ascii_core::Error::BudgetExceeded(BudgetKind::RasterBytes).into());
        }
        entries
            .atlases
            .try_reserve(1)
            .map_err(|_| ascii_core::Error::AllocationFailed)?;
        entries
            .atlases
            .retain(|_, atlas| Arc::strong_count(atlas) > 1);
        entries.bytes = entries
            .atlases
            .values()
            .try_fold(0u64, |bytes, atlas| bytes.checked_add(atlas.bytes))
            .ok_or(ascii_core::Error::SizeOverflow)?;

        if cancellation.is_cancelled() {
            return Err(ascii_core::Error::Cancelled.into());
        }
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ascii glyph atlas staging"),
            size: plan.atlas_staging_bytes,
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        {
            let mut data = staging.slice(..).get_mapped_range_mut();
            Renderer::fill_atlas_data(table, cancellation, plan, &mut data)?;
        }
        staging.unmap();
        if cancellation.is_cancelled() {
            return Err(ascii_core::Error::Cancelled.into());
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ascii glyph masks"),
            size: wgpu::Extent3d {
                width: plan.atlas_width,
                height: plan.atlas_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas = Arc::new(Atlas {
            texture,
            view,
            bytes: plan.atlas_bytes,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ascii glyph atlas upload"),
            });
        encoder.copy_buffer_to_texture(
            wgpu::ImageCopyBuffer {
                buffer: &staging,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(plan.atlas_row_pitch),
                    rows_per_image: Some(plan.atlas_height),
                },
            },
            atlas.texture.as_image_copy(),
            wgpu::Extent3d {
                width: plan.atlas_width,
                height: plan.atlas_height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        let queued_atlas = atlas.clone();
        self.queue.on_submitted_work_done(move || {
            drop(staging);
            drop(queued_atlas);
        });
        entries.bytes = entries
            .bytes
            .checked_add(plan.atlas_bytes)
            .ok_or(ascii_core::Error::SizeOverflow)?;
        entries.atlases.insert(key, atlas.clone());
        Ok((atlas, true))
    }
}
struct GpuResources {
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    cells: wgpu::Buffer,
    capacity: u64,
    group: wgpu::BindGroup,
    background: wgpu::RenderPipeline,
    glyph: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
}

struct InFlight {
    submission: wgpu::SubmissionIndex,
}

/// A renderer bound to one host device, immutable font table, atlas cache and budget.
///
/// No device, queue, surface, full-resolution intermediate target or readback is created here.
/// Share one `AtlasCache` between compatible targets and create one for each device.
pub struct Renderer {
    atlas_cache: Arc<AtlasCache>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    table: Arc<GlyphTable>,
    budget: Budget,
    cancellation: Arc<CancellationToken>,
    status: DeviceStatus,
    atlas: Option<Arc<Atlas>>,
    resources: Option<GpuResources>,
    packed: Vec<u8>,
    packed_valid: bool,
    uniform_size: Option<GridSize>,
    last: Option<(TargetKey, GridSize)>,
    in_flight: Option<InFlight>,
    stats: PresenterStats,
}

impl Renderer {
    /// Creates a lazy renderer with a renderer-local atlas cache.
    #[must_use]
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        table: Arc<GlyphTable>,
        budget: Budget,
        cancellation: Arc<CancellationToken>,
        status: DeviceStatus,
    ) -> Self {
        let cache = Arc::new(AtlasCache::new(device, queue, budget.max_raster_bytes()));
        Self::new_with_cache(cache, table, budget, cancellation, status)
    }

    /// Creates a lazy renderer using a host-shared same-device atlas cache.
    #[must_use]
    pub fn new_with_cache(
        atlas_cache: Arc<AtlasCache>,
        table: Arc<GlyphTable>,
        budget: Budget,
        cancellation: Arc<CancellationToken>,
        status: DeviceStatus,
    ) -> Self {
        Self {
            device: atlas_cache.device.clone(),
            queue: atlas_cache.queue.clone(),
            atlas_cache,
            table,
            budget,
            cancellation,
            status,
            atlas: None,
            resources: None,
            packed: Vec::new(),
            packed_valid: false,
            uniform_size: None,
            last: None,
            in_flight: None,
            stats: PresenterStats::default(),
        }
    }

    /// Forces the next valid presentation to redraw without discarding the atlas or cell buffer.
    pub fn invalidate(&mut self) {
        self.last = None;
    }
    /// Returns cumulative API-level instrumentation.
    #[must_use]
    pub const fn stats(&self) -> PresenterStats {
        self.stats
    }

    /// Validates and submits the baseline background and instanced-glyph passes.
    pub fn present(
        &mut self,
        frame: &AsciiFrame,
        target: &RenderTarget<'_>,
    ) -> Result<PresentOutcome, Error> {
        if self.cancellation.is_cancelled() {
            return Err(ascii_core::Error::Cancelled.into());
        }
        if self.status.is_lost() {
            self.last = None;
            return Ok(PresentOutcome::Recoverable(RecoverableReason::DeviceLost));
        }
        if let Some(outcome) = cached_outcome(false, target.surface_status, &mut self.last) {
            return Ok(outcome);
        }
        let glyph_count = u32::try_from(self.table.glyphs().len())
            .map_err(|_| ascii_core::Error::SizeOverflow)?;
        validate_frame(
            frame,
            GlyphTableId::new(*self.table.fingerprint()),
            glyph_count,
            &self.budget,
            &self.cancellation,
        )?;
        if target.width == 0 || target.height == 0 {
            self.last = None;
            return Ok(PresentOutcome::Skipped(SkippedReason::ZeroSized));
        }
        let plan = plan(
            PlanInput {
                size: frame.size(),
                glyph_count,
                glyph_width: self.table.cell_width(),
                glyph_height: self.table.cell_height(),
                target_size: (target.width, target.height),
            },
            &self.device.limits(),
            &self.budget,
        )?;
        if !matches!(
            target.format,
            wgpu::TextureFormat::Rgba8UnormSrgb | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            return Err(Error::UnsupportedTarget);
        }
        if let TargetKind::Texture(texture) = &target.kind
            && (texture.dimension() != wgpu::TextureDimension::D2
                || texture.depth_or_array_layers() != 1
                || texture.mip_level_count() != 1
                || texture.sample_count() != 1
                || !texture
                    .usage()
                    .contains(wgpu::TextureUsages::RENDER_ATTACHMENT))
        {
            return Err(Error::UnsupportedTarget);
        }
        let mut same_cells = self.packed_valid && self.packed.len() == plan.cell_bytes as usize;
        if same_cells {
            for (index, (cell, bytes)) in frame
                .cells()
                .iter()
                .zip(self.packed.as_chunks::<16>().0)
                .enumerate()
            {
                if index.is_multiple_of(POLL_INTERVAL) && self.cancellation.is_cancelled() {
                    return Err(ascii_core::Error::Cancelled.into());
                }
                if GpuCell::from_cell(*cell).bytes() != *bytes {
                    same_cells = false;
                    break;
                }
            }
        }
        if self.cancellation.is_cancelled() {
            return Err(ascii_core::Error::Cancelled.into());
        }
        let unchanged = same_cells && self.last == Some((target.key(), frame.size()));
        if let Some(outcome) = cached_outcome(unchanged, None, &mut self.last) {
            return Ok(outcome);
        }

        // Bounds include retained capacity when grids shrink. A submission's staging bytes may
        // coexist with retained allocations, but never with another submission.
        let current_cpu_capacity =
            u64::try_from(self.packed.capacity()).map_err(|_| ascii_core::Error::SizeOverflow)?;
        let current_gpu_capacity = self
            .resources
            .as_ref()
            .map_or(0, |resources| resources.capacity);
        ensure_frame_budget(
            current_cpu_capacity,
            current_gpu_capacity,
            plan.cell_bytes,
            &self.budget,
        )?;
        if !same_cells {
            self.last = None;
            self.packed_valid = false;
            self.packed.clear();
            let bytes =
                usize::try_from(plan.cell_bytes).map_err(|_| ascii_core::Error::SizeOverflow)?;
            self.packed
                .try_reserve_exact(bytes)
                .map_err(|_| ascii_core::Error::AllocationFailed)?;
            for (index, cell) in frame.cells().iter().enumerate() {
                if index.is_multiple_of(POLL_INTERVAL) && self.cancellation.is_cancelled() {
                    return Err(ascii_core::Error::Cancelled.into());
                }
                self.packed
                    .extend_from_slice(&GpuCell::from_cell(*cell).bytes());
            }
        }
        if let Some(InFlight { submission }) = self.in_flight.take() {
            let _ = self
                .device
                .poll(wgpu::Maintain::WaitForSubmissionIndex(submission));
        }
        if self.cancellation.is_cancelled() {
            return Err(ascii_core::Error::Cancelled.into());
        }
        if self.status.is_lost() {
            self.last = None;
            return Ok(PresentOutcome::Recoverable(RecoverableReason::DeviceLost));
        }
        if self.atlas.is_none() {
            let (atlas, created) =
                self.atlas_cache
                    .acquire(&self.table, plan, &self.cancellation)?;
            self.atlas = Some(atlas);
            if created {
                self.stats.atlas_textures += 1;
                self.stats.uploads += 1;
                self.stats.uploaded_bytes += plan.atlas_bytes;
                self.stats.submissions += 1;
            } else {
                self.stats.atlas_cache_hits += 1;
            }
        }
        let surface_frame = match &target.kind {
            TargetKind::Surface(surface) => match surface.get_current_texture() {
                Ok(texture) => {
                    if texture.texture.width() != target.width
                        || texture.texture.height() != target.height
                        || texture.texture.format() != target.format
                    {
                        self.last = None;
                        return Ok(PresentOutcome::Recoverable(
                            RecoverableReason::SurfaceOutdated,
                        ));
                    }
                    Some(texture)
                }
                Err(error) => {
                    self.last = None;
                    return surface_error(error);
                }
            },
            TargetKind::Texture(_) => None,
        };

        self.ensure_resources(plan.cell_bytes, target.format);
        // Resource creation can take time; do not enqueue writes after cancellation or known loss.
        if self.cancellation.is_cancelled() || self.status.is_lost() {
            self.last = None;
            if self.cancellation.is_cancelled() {
                return Err(ascii_core::Error::Cancelled.into());
            }
            return Ok(PresentOutcome::Recoverable(RecoverableReason::DeviceLost));
        }
        let resources = self.resources.as_ref().expect("resources just created");
        if !same_cells {
            self.queue.write_buffer(&resources.cells, 0, &self.packed);
            self.stats.uploads += 1;
            self.stats.uploaded_bytes += plan.cell_bytes;
        }
        if self.uniform_size != Some(frame.size()) {
            let mut params = [0u8; 32];
            for (index, value) in [
                frame.size().columns(),
                frame.size().rows(),
                plan.atlas_columns,
                self.table.cell_width(),
                self.table.cell_height(),
                plan.atlas_tile_width,
                plan.atlas_tile_height,
                ATLAS_PADDING,
            ]
            .into_iter()
            .enumerate()
            {
                params[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            self.queue.write_buffer(&resources.uniform, 0, &params);
            self.stats.uploads += 1;
            self.stats.uploaded_bytes += PARAM_BYTES;
            self.uniform_size = Some(frame.size());
        }
        let texture = match (&target.kind, &surface_frame) {
            (TargetKind::Texture(texture), _) => *texture,
            (TargetKind::Surface(_), Some(frame)) => &frame.texture,
            (TargetKind::Surface(_), None) => unreachable!("surface frame was acquired"),
        };
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ascii presentation"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ascii background and glyphs"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &resources.group, &[]);
            pass.set_pipeline(&resources.background);
            pass.draw(0..6, 0..plan.count);
            pass.set_pipeline(&resources.glyph);
            pass.draw(0..6, 0..plan.count);
        }
        let submission = self.queue.submit(Some(encoder.finish()));
        let queued_atlas = self.atlas.as_ref().expect("atlas exists").clone();
        self.queue
            .on_submitted_work_done(move || drop(queued_atlas));
        self.in_flight = Some(InFlight { submission });
        self.stats.submissions += 1;
        self.stats.draw_calls += 2;
        let suboptimal = surface_frame.as_ref().is_some_and(|frame| frame.suboptimal);
        if let Some(frame) = surface_frame {
            frame.present();
        }
        self.packed_valid = true;
        self.last = Some((target.key(), frame.size()));
        Ok(PresentOutcome::Presented { suboptimal })
    }

    fn fill_atlas_data(
        table: &GlyphTable,
        cancellation: &CancellationToken,
        plan: Plan,
        data: &mut [u8],
    ) -> Result<(), Error> {
        let width = table.cell_width() as usize;
        let height = table.cell_height() as usize;
        let stride = plan.atlas_row_pitch as usize;
        let tile_width = plan.atlas_tile_width as usize;
        let tile_height = plan.atlas_tile_height as usize;
        let padding = ATLAS_PADDING as usize;
        for (index, glyph) in table.glyphs().iter().enumerate() {
            if cancellation.is_cancelled() {
                return Err(ascii_core::Error::Cancelled.into());
            }
            if glyph.mask().len()
                != width
                    .checked_mul(height)
                    .ok_or(ascii_core::Error::SizeOverflow)?
            {
                return Err(
                    ascii_core::Error::Malformed(MalformedReason::InvalidConfiguration).into(),
                );
            }
            let x = index % plan.atlas_columns as usize * tile_width + padding;
            let y = index / plan.atlas_columns as usize * tile_height + padding;
            for row in 0..height {
                if row.is_multiple_of(POLL_INTERVAL) && cancellation.is_cancelled() {
                    return Err(ascii_core::Error::Cancelled.into());
                }
                let dst = (y + row) * stride + x;
                let src = row * width;
                data[dst..dst + width].copy_from_slice(&glyph.mask()[src..src + width]);
            }
        }
        Ok(())
    }

    fn ensure_resources(&mut self, cell_bytes: u64, format: wgpu::TextureFormat) {
        let atlas = self.atlas.as_ref().expect("atlas exists");
        if self.resources.is_none() {
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("ascii bindings"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                    ],
                });
            let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ascii geometry"),
                size: PARAM_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let cells = create_cell_buffer(&self.device, cell_bytes);
            let group = create_group(&self.device, &layout, &uniform, &cells, &atlas.view);
            let (background, glyph) = create_pipelines(&self.device, &layout, format);
            self.stats.cell_buffers += 1;
            self.stats.pipelines += 2;
            self.resources = Some(GpuResources {
                layout,
                uniform,
                cells,
                capacity: cell_bytes,
                group,
                background,
                glyph,
                format,
            });
            self.uniform_size = None;
        } else if let Some(resources) = self.resources.as_mut() {
            if resources.capacity < cell_bytes {
                resources.cells = create_cell_buffer(&self.device, cell_bytes);
                resources.capacity = cell_bytes;
                resources.group = create_group(
                    &self.device,
                    &resources.layout,
                    &resources.uniform,
                    &resources.cells,
                    &atlas.view,
                );
                self.stats.cell_buffers += 1;
            }
            if resources.format != format {
                (resources.background, resources.glyph) =
                    create_pipelines(&self.device, &resources.layout, format);
                resources.format = format;
                self.stats.pipelines += 2;
            }
        }
    }
}

fn create_cell_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ascii cells"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
fn create_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    cells: &wgpu::Buffer,
    atlas: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ascii resources"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: cells.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(atlas),
            },
        ],
    })
}
fn create_pipelines(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(SHADER_REVISION),
        source: wgpu::ShaderSource::Wgsl(include_str!("presenter.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("ascii pipeline layout"),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    let create = |entry_point, blend| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(entry_point),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vertex_main",
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point,
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
        })
    };
    (
        create("background_main", None),
        create(
            "glyph_main",
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    mod synthetic_font {
        include!("../../ascii-convert/tests/support/synthetic_font.rs");
    }

    #[test]
    fn gpu_record_layout_and_explicit_roundtrip() {
        assert_eq!(std::mem::size_of::<AsciiCell>(), 12);
        assert_eq!(std::mem::size_of::<GpuCell>(), 16);
        assert_eq!(std::mem::align_of::<GpuCell>(), 16);
        assert_eq!(std::mem::offset_of!(GpuCell, glyph), 0);
        assert_eq!(std::mem::offset_of!(GpuCell, foreground), 4);
        assert_eq!(std::mem::offset_of!(GpuCell, background), 8);
        assert_eq!(std::mem::offset_of!(GpuCell, flags), 12);
        let record = GpuCell {
            glyph: 0x12345678,
            foreground: 0x11223344,
            background: 0xaabbccdd,
            flags: 0x01234567,
        };
        let bytes = record.bytes();
        let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        assert_eq!(
            (word(0), word(4), word(8), word(12)),
            (
                record.glyph,
                record.foreground,
                record.background,
                record.flags
            )
        );
        let packed_12 = &bytes[..12];
        assert_ne!(packed_12.len(), std::mem::size_of::<GpuCell>());
    }

    #[test]
    fn invalid_cells_are_rejected_without_gpu_resources() {
        let budget = Budget::new(10_000, 10_000, 10_000);
        let token = CancellationToken::new();
        let id = GlyphTableId::new([1; 32]);
        let metadata = ascii_core::FrameMetadata::new(id, 1, 1, 0, 0);
        let mut cells = vec![AsciiCell::new(0, [0; 4], [0; 4]); 16];
        cells[15] = AsciiCell::new(8, [0; 4], [0; 4]);
        let frame = AsciiFrame::try_new(
            GridSize::new(4, 4),
            metadata,
            cells.clone(),
            9,
            &budget,
            &token,
        )
        .unwrap();
        assert_eq!(
            validate_frame(&frame, id, 8, &budget, &token),
            Err(ascii_core::Error::Malformed(
                MalformedReason::GlyphIndexOutOfRange
            ))
        );
        assert_eq!(
            AsciiFrame::try_new(
                GridSize::new(4, 4),
                metadata,
                cells[..15].to_vec(),
                8,
                &budget,
                &token
            ),
            Err(ascii_core::Error::Malformed(
                MalformedReason::CellCountMismatch
            ))
        );
        assert_eq!(
            ascii_core::validate_grid_budget(GridSize::new(u32::MAX, u32::MAX), &budget, &token),
            Err(ascii_core::Error::SizeOverflow)
        );
        token.cancel();
        assert_eq!(
            validate_frame(&frame, id, 8, &budget, &token),
            Err(ascii_core::Error::Cancelled)
        );
    }

    #[test]
    fn limits_and_budget_fail_before_allocation() {
        let generous = Budget::new(100_000, 100_000, 100_000);
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffer_binding_size = 64;
        let input = PlanInput {
            size: GridSize::new(4, 4),
            glyph_count: 1,
            glyph_width: 8,
            glyph_height: 12,
            target_size: (32, 48),
        };
        assert!(matches!(
            plan(input, &limits, &generous),
            Err(Error::DeviceLimit("cell storage buffer"))
        ));
        limits.max_storage_buffer_binding_size = 1_000;
        let small = Budget::new(100_000, 1, 100_000);
        assert_eq!(
            plan(input, &limits, &small).unwrap_err(),
            Error::Core(ascii_core::Error::BudgetExceeded(BudgetKind::RasterBytes))
        );
    }

    #[test]
    fn cell_buffer_growth_and_cpu_reallocation_overlap_are_charged_at_frame_budget_boundary() {
        let old_capacity = 16;
        let requested = 32;
        // Retained CPU/GPU buffers plus upload staging coexist with both old buffers during growth.
        let exact_budget = requested * 3 + old_capacity * 2;
        assert_eq!(exact_budget, 128);
        let make_budget = |max_frame| Budget::new(max_frame, 10_000, 10_000);

        assert_eq!(
            ensure_frame_budget(
                old_capacity,
                old_capacity,
                requested,
                &make_budget(exact_budget - 1),
            ),
            Err(Error::Core(ascii_core::Error::BudgetExceeded(
                BudgetKind::FrameBytes
            )))
        );
        assert_eq!(
            ensure_frame_budget(
                old_capacity,
                old_capacity,
                requested,
                &make_budget(exact_budget),
            ),
            Ok(())
        );
    }

    #[test]
    #[ignore = "requires an offscreen wgpu adapter"]
    fn present_charges_cpu_growth_peak_before_vec_reallocation() {
        let font_bytes = synthetic_font::synthetic_font(false);
        let table_budget = Budget::new(32 * 1024 * 1024, 32 * 1024 * 1024, 20_000_000);
        let cancellation = Arc::new(CancellationToken::new());
        let repertoire = ascii_convert::GlyphRepertoire::new(" AB").unwrap();
        let config = ascii_convert::GlyphConfig::new(8.0, 8, 12, 9).unwrap();
        let table = Arc::new(
            ascii_convert::build_glyph_table(
                &font_bytes,
                &repertoire,
                &config,
                &table_budget,
                &cancellation,
            )
            .unwrap(),
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            ..Default::default()
        });
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("offscreen presenter boundary test requires an adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("presenter frame-budget boundary"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
            },
            None,
        ))
        .expect("offscreen presenter boundary test requires a compatible device");
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let table_id = GlyphTableId::new(*table.fingerprint());
        let frame_budget = Budget::new(128, 32 * 1024 * 1024, 20_000_000);
        let one_cell = AsciiFrame::try_new(
            GridSize::new(1, 1),
            ascii_core::FrameMetadata::new(table_id, 1, 1, 0, 0),
            vec![AsciiCell::new(0, [255; 4], [0; 4])],
            table.glyphs().len() as u32,
            &frame_budget,
            &cancellation,
        )
        .unwrap();
        let two_cells = AsciiFrame::try_new(
            GridSize::new(2, 1),
            ascii_core::FrameMetadata::new(table_id, 1, 1, 0, 0),
            vec![
                AsciiCell::new(1, [255; 4], [0; 4]),
                AsciiCell::new(2, [0; 4], [255; 4]),
            ],
            table.glyphs().len() as u32,
            &frame_budget,
            &cancellation,
        )
        .unwrap();
        let target_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("presenter frame-budget target"),
            size: wgpu::Extent3d {
                width: 16,
                height: 12,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let target = RenderTarget::texture(&target_texture, 1, 1);
        let renderer_budget =
            |max_frame_bytes| Budget::new(max_frame_bytes, 32 * 1024 * 1024, 20_000_000);
        let mut rejected = Renderer::new(
            device.clone(),
            queue.clone(),
            table.clone(),
            renderer_budget(127),
            cancellation.clone(),
            DeviceStatus::new(),
        );
        assert_eq!(
            rejected.present(&one_cell, &target),
            Ok(crate::surface::PresentOutcome::Presented { suboptimal: false })
        );
        assert_eq!(rejected.packed.capacity(), 16);
        assert_eq!(rejected.resources.as_ref().unwrap().capacity, 16);
        let stats_before_growth = rejected.stats();
        assert_eq!(
            rejected.present(&two_cells, &target),
            Err(Error::Core(ascii_core::Error::BudgetExceeded(
                BudgetKind::FrameBytes
            )))
        );
        assert_eq!(rejected.packed.capacity(), 16);
        assert_eq!(rejected.resources.as_ref().unwrap().capacity, 16);
        assert_eq!(rejected.stats(), stats_before_growth);

        let mut accepted = Renderer::new(
            device,
            queue,
            table,
            renderer_budget(128),
            cancellation,
            DeviceStatus::new(),
        );
        assert_eq!(
            accepted.present(&one_cell, &target),
            Ok(crate::surface::PresentOutcome::Presented { suboptimal: false })
        );
        assert_eq!(accepted.packed.capacity(), 16);
        assert_eq!(accepted.resources.as_ref().unwrap().capacity, 16);
        assert_eq!(
            accepted.present(&two_cells, &target),
            Ok(crate::surface::PresentOutcome::Presented { suboptimal: false })
        );
        assert_eq!(accepted.packed.capacity(), 32);
        assert_eq!(accepted.resources.as_ref().unwrap().capacity, 32);
    }

    #[test]
    fn padded_atlas_staging_is_preflighted_at_exact_budget_and_device_limits() {
        let input = PlanInput {
            size: GridSize::new(1, 1),
            glyph_count: 1,
            glyph_width: 1,
            glyph_height: 2,
            target_size: (1, 1),
        };
        let mut limits = wgpu::Limits::downlevel_defaults();
        let too_small = Budget::new(10_000, 513, 10_000);
        assert_eq!(
            plan(input, &limits, &too_small).unwrap_err(),
            Error::Core(ascii_core::Error::BudgetExceeded(BudgetKind::RasterBytes))
        );
        let exact = Budget::new(10_000, 1036, 10_000);
        let narrow = plan(input, &limits, &exact).unwrap();
        assert_eq!(narrow.atlas_bytes, 12);
        assert_eq!(narrow.atlas_row_pitch, 256);
        assert_eq!(narrow.atlas_staging_bytes, 1024);

        limits.max_buffer_size = 511;
        assert!(matches!(
            plan(input, &limits, &exact),
            Err(Error::DeviceLimit("atlas staging buffer"))
        ));
    }

    #[test]
    fn padded_atlas_initialization_and_three_cell_passes_are_charged_before_allocation() {
        let mut input = PlanInput {
            size: GridSize::new(1, 1),
            glyph_count: 1,
            glyph_width: 1,
            glyph_height: 2,
            target_size: (1, 1),
        };
        let mut limits = wgpu::Limits::downlevel_defaults();
        // Two target passes + two padded-atlas copies + 1024 staging bytes + three cell traversals.
        let too_little_work = Budget::new(10_000, 10_000, 1052);
        assert_eq!(
            plan(input, &limits, &too_little_work).unwrap_err(),
            Error::Core(ascii_core::Error::BudgetExceeded(BudgetKind::WorkUnits))
        );
        let exact_work = Budget::new(10_000, 10_000, 1053);
        assert_eq!(
            plan(input, &limits, &exact_work)
                .unwrap()
                .atlas_staging_bytes,
            1024
        );

        input.target_size = (u32::MAX, u32::MAX);
        limits.max_texture_dimension_2d = u32::MAX;
        let generous = Budget::new(u64::MAX, u64::MAX, u64::MAX);
        assert_eq!(
            plan(input, &limits, &generous).unwrap_err(),
            Error::Core(ascii_core::Error::SizeOverflow)
        );
    }

    #[test]
    fn atlas_pitch_rounds_each_row_and_rejects_unrepresentable_layouts() {
        let mut input = PlanInput {
            size: GridSize::new(1, 1),
            glyph_count: 1,
            glyph_width: 256,
            glyph_height: 2,
            target_size: (1, 1),
        };
        let generous = Budget::new(10_000, 10_000, 10_000);
        let mut limits = wgpu::Limits::downlevel_defaults();
        let aligned = plan(input, &limits, &generous).unwrap();
        assert_eq!(
            (aligned.atlas_row_pitch, aligned.atlas_staging_bytes),
            (512, 2048)
        );
        input.glyph_width = 257;
        let padded = plan(input, &limits, &generous).unwrap();
        assert_eq!(
            (padded.atlas_row_pitch, padded.atlas_staging_bytes),
            (512, 2048)
        );

        input.glyph_width = u32::MAX;
        limits.max_texture_dimension_2d = u32::MAX;
        assert_eq!(
            plan(input, &limits, &generous).unwrap_err(),
            Error::Core(ascii_core::Error::SizeOverflow)
        );
    }

    #[test]
    fn padded_atlas_contains_the_immutable_glyph_masks_without_row_bleed() {
        let builders: [fn() -> Vec<u8>; 5] = [
            || synthetic_font::synthetic_font(false),
            || synthetic_font::diagonal_font(false),
            synthetic_font::quadratic_font,
            synthetic_font::composite_font,
            || synthetic_font::legacy_format2_font(false),
        ];
        for build_font in builders {
            assert_padded_atlas(&build_font());
        }
    }

    fn assert_padded_atlas(font: &[u8]) {
        let budget = Budget::new(32 * 1024 * 1024, 32 * 1024 * 1024, 20_000_000);
        let cancellation = CancellationToken::new();
        let config = ascii_convert::GlyphConfig::new(4.0, 8, 4, 3).unwrap();
        let repertoire = ascii_convert::GlyphRepertoire::new(" AB").unwrap();
        let table =
            ascii_convert::build_glyph_table(font, &repertoire, &config, &budget, &cancellation)
                .unwrap();
        assert!(
            table
                .glyphs()
                .iter()
                .any(|glyph| glyph.mask().iter().any(|&coverage| coverage != 0))
        );
        let fingerprint = *table.fingerprint();
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_texture_dimension_2d = 24;
        let planned = plan(
            PlanInput {
                size: GridSize::new(1, 1),
                glyph_count: table.glyphs().len() as u32,
                glyph_width: table.cell_width(),
                glyph_height: table.cell_height(),
                target_size: (1, 1),
            },
            &limits,
            &budget,
        )
        .unwrap();
        assert_eq!((planned.atlas_columns, planned.atlas_height), (2, 12));
        assert_eq!(planned.atlas_row_pitch, 256);
        let mut data = vec![0; planned.atlas_staging_bytes as usize];
        Renderer::fill_atlas_data(&table, &cancellation, planned, &mut data).unwrap();
        for (index, glyph) in table.glyphs().iter().enumerate() {
            let tile_x = index % 2 * planned.atlas_tile_width as usize;
            let tile_y = index / 2 * planned.atlas_tile_height as usize;
            for row in 0..table.cell_height() as usize {
                let start =
                    (tile_y + row + ATLAS_PADDING as usize) * 256 + tile_x + ATLAS_PADDING as usize;
                assert_eq!(
                    &data[start..start + table.cell_width() as usize],
                    &glyph.mask()[row * table.cell_width() as usize
                        ..(row + 1) * table.cell_width() as usize]
                );
            }
            for row in 0..planned.atlas_tile_height as usize {
                for col in 0..planned.atlas_tile_width as usize {
                    let is_glyph = (ATLAS_PADDING as usize
                        ..ATLAS_PADDING as usize + table.cell_width() as usize)
                        .contains(&col)
                        && (ATLAS_PADDING as usize
                            ..ATLAS_PADDING as usize + table.cell_height() as usize)
                            .contains(&row);
                    if !is_glyph {
                        assert_eq!(data[(tile_y + row) * 256 + tile_x + col], 0);
                    }
                }
            }
        }
        assert_eq!(*table.fingerprint(), fingerprint);
        cancellation.cancel();
        assert_eq!(
            Renderer::fill_atlas_data(&table, &cancellation, planned, &mut data),
            Err(Error::Core(ascii_core::Error::Cancelled))
        );
    }
}
