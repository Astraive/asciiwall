//! Explicit native GPU evidence procedure. These tests never stand in for host-surface evidence.
#[path = "../../ascii-convert/tests/support/synthetic_font.rs"]
mod synthetic_font;

use std::{sync::Arc, time::Instant};

use ascii_convert::{GlyphConfig, GlyphRepertoire, GlyphTable, build_glyph_table};
use ascii_core::{
    AsciiCell, AsciiFrame, Budget, CancellationToken, FrameMetadata, GlyphTableId, GridSize,
    MalformedReason,
};
use ascii_render::{
    DeviceStatus, Error, PresentOutcome, RecoverableReason, RenderTarget, Renderer,
    SHADER_REVISION, SkippedReason,
};
use sha2::{Digest, Sha256};

fn native_device() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
    let backends = match std::env::var("ASCIIWALL_WGPU_BACKEND").as_deref() {
        Ok("vulkan") => wgpu::Backends::VULKAN,
        Ok("dx12") => wgpu::Backends::DX12,
        Ok("metal") => wgpu::Backends::METAL,
        Ok("gl") => wgpu::Backends::GL,
        Ok(other) => panic!("unsupported ASCIIWALL_WGPU_BACKEND: {other}"),
        Err(std::env::VarError::NotPresent) => wgpu::Backends::all(),
        Err(error) => panic!("invalid ASCIIWALL_WGPU_BACKEND: {error}"),
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..Default::default()
    });
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("native evidence requires an available adapter; unavailable is not a pass");
    let info = adapter.get_info();
    println!(
        "adapter={info:?}; shader={SHADER_REVISION}; shader_sha256={:x}; os={} arch={}",
        Sha256::digest(include_bytes!("../src/presenter.wgsl")),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("ascii presenter evidence"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .expect("native evidence requires a compatible device");
    (Arc::new(device), Arc::new(queue))
}

fn fixtures() -> (Arc<GlyphTable>, Budget, Arc<CancellationToken>) {
    let font = synthetic_font::synthetic_font(false);
    let budget = Budget::new(32 * 1024 * 1024, 32 * 1024 * 1024, 20_000_000);
    let token = Arc::new(CancellationToken::new());
    let config = GlyphConfig::new(8.0, 8, 12, 9).unwrap();
    let repertoire = GlyphRepertoire::new(" AB").unwrap();
    let table = build_glyph_table(&font, &repertoire, &config, &budget, &token)
        .expect("pinned synthetic font builds");
    println!(
        "font_sha256={:x}; table_fingerprint={:02x?}; raster=8 cell=8x12 baseline=9 repertoire=space,A,B",
        Sha256::digest(&font),
        table.fingerprint()
    );
    (Arc::new(table), budget, token)
}

fn frame(
    table: &GlyphTable,
    size: GridSize,
    budget: &Budget,
    token: &CancellationToken,
) -> AsciiFrame {
    let cells = (0..size.cell_count())
        .map(|index| {
            let fg = match index % 4 {
                0 => [255, 50, 20, 255],
                1 => [0, 255, 80, 128],
                2 => [20, 40, 255, 0],
                _ => [220, 180, 40, 255],
            };
            let bg = match (index / 4) % 4 {
                0 => [0, 0, 0, 0],
                1 => [20, 60, 100, 255],
                2 => [160, 30, 200, 128],
                _ => [0, 0, 0, 255],
            };
            AsciiCell::new((index % table.glyphs().len() as u64) as u32, fg, bg)
        })
        .collect();
    let metadata = FrameMetadata::new(GlyphTableId::new(*table.fingerprint()), 1, 1, 0, 0);
    AsciiFrame::try_new(
        size,
        metadata,
        cells,
        table.glyphs().len() as u32,
        budget,
        token,
    )
    .unwrap()
}

fn texture(device: &wgpu::Device, size: GridSize) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native evidence target"),
        size: wgpu::Extent3d {
            width: size.columns() * 8,
            height: size.rows() * 12,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn map_buffer(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let slice = buffer.slice(..);
    let (send, receive) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        send.send(result).unwrap();
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    receive
        .recv()
        .unwrap()
        .expect("GPU readback mapping succeeds");
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

fn read_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> (Vec<u8>, usize) {
    let stride = (texture.width() * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only image readback"),
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("test image copy"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(texture.height()),
            },
        },
        texture.size(),
    );
    queue.submit(Some(encoder.finish()));
    (map_buffer(device, &buffer), stride as usize)
}

fn linear(x: f32) -> f32 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
fn srgb(x: f32) -> f32 {
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}
fn unorm(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}

// Reference includes quantization between the background and glyph passes in the sRGB target.
fn reference_pixel(cell: AsciiCell, coverage: u8) -> [u8; 4] {
    let fg = cell.fg_srgba8();
    let bg = cell.bg_srgba8();
    let fa = f32::from(fg[3]) / 255.0 * f32::from(coverage) / 255.0;
    let ba = f32::from(bg[3]) / 255.0;
    let mut out = [0; 4];
    for c in 0..3 {
        let bg_stored = unorm(srgb(linear(f32::from(bg[c]) / 255.0) * ba));
        let background = linear(f32::from(bg_stored) / 255.0);
        let foreground = linear(f32::from(fg[c]) / 255.0) * fa;
        out[c] = unorm(srgb(foreground + background * (1.0 - fa)));
    }
    out[3] = unorm(fa + ba * (1.0 - fa));
    out
}

fn compare_golden(frame: &AsciiFrame, table: &GlyphTable, pixels: &[u8], stride: usize) {
    let width = frame.size().columns() as usize * 8;
    let height = frame.size().rows() as usize * 12;
    let mut max_delta = 0u8;
    let mut sum_delta = 0u64;
    let mut weighted_sum = 0.0f64;
    let mut weighted_max = 0.0f64;
    for y in 0..height {
        for x in 0..width {
            let index = (y / 12) * frame.size().columns() as usize + x / 8;
            let cell = frame.cells()[index];
            let coverage = table.glyphs()[cell.glyph() as usize].mask()[(y % 12) * 8 + x % 8];
            let expected = reference_pixel(cell, coverage);
            let actual = &pixels[y * stride + x * 4..y * stride + x * 4 + 4];
            let delta = |channel: usize| f64::from(actual[channel].abs_diff(expected[channel]));
            let weighted =
                (0.2126 * delta(0).powi(2) + 0.7152 * delta(1).powi(2) + 0.0722 * delta(2).powi(2))
                    .sqrt();
            weighted_sum += weighted;
            weighted_max = weighted_max.max(weighted);
            for channel in 0..4 {
                let delta = actual[channel].abs_diff(expected[channel]);
                max_delta = max_delta.max(delta);
                sum_delta += u64::from(delta);
            }
        }
    }
    let mean = sum_delta as f64 / (width * height * 4) as f64;
    let weighted_mean = weighted_sum / (width * height) as f64;
    println!(
        "golden={}x{} max_channel_delta={max_delta} mean_channel_delta={mean:.4} weighted_max={weighted_max:.4} weighted_mean={weighted_mean:.4}",
        frame.size().columns(),
        frame.size().rows()
    );
    assert!(
        max_delta <= 4 && mean <= 1.0 && weighted_max <= 4.0 && weighted_mean <= 1.0,
        "sRGB perceptually weighted golden tolerance exceeded"
    );
}

#[test]
#[ignore = "requires a real wgpu adapter; record backend/driver/OS and fixture identity"]
fn pinned_golden_and_static_invalidation() {
    let (device, queue) = native_device();
    let (table, budget, token) = fixtures();
    let mut renderer = Renderer::new(
        device.clone(),
        queue.clone(),
        table.clone(),
        budget,
        token.clone(),
        DeviceStatus::new(),
    );
    for (identity, size) in [(1, GridSize::new(4, 4)), (2, GridSize::new(240, 68))] {
        let frame = frame(&table, size, &budget, &token);
        let output = texture(&device, size);
        let target = RenderTarget::texture(&output, identity, 1);
        assert_eq!(
            renderer.present(&frame, &target).unwrap(),
            PresentOutcome::Presented { suboptimal: false }
        );
        let after_first = renderer.stats();
        assert_eq!(
            renderer.present(&frame, &target).unwrap(),
            PresentOutcome::Skipped(SkippedReason::Unchanged)
        );
        assert_eq!(
            renderer.stats(),
            after_first,
            "unchanged scene performs no uploads, allocations or draws"
        );
        let (pixels, stride) = read_pixels(&device, &queue, &output);
        compare_golden(&frame, &table, &pixels, stride);
        let invalidated = RenderTarget::texture(&output, identity, 2);
        assert_eq!(
            renderer.present(&frame, &invalidated).unwrap(),
            PresentOutcome::Presented { suboptimal: false }
        );
        let redrawn = renderer.stats();
        assert_eq!(
            redrawn.uploads, after_first.uploads,
            "damage only redraws cached input"
        );
        assert_eq!(redrawn.draw_calls, after_first.draw_calls + 2);
        let (size, metadata, mut cells) = frame.into_parts();
        cells[0] = AsciiCell::new(1, [17, 45, 99, 255], [0, 0, 0, 255]);
        let changed = AsciiFrame::try_new(
            size,
            metadata,
            cells,
            table.glyphs().len() as u32,
            &budget,
            &token,
        )
        .unwrap();
        assert_eq!(
            renderer.present(&changed, &invalidated).unwrap(),
            PresentOutcome::Presented { suboptimal: false }
        );
        assert_eq!(
            renderer.stats().uploads,
            redrawn.uploads + 1,
            "changed content uploads even with identical metadata"
        );
    }
    let stats = renderer.stats();
    assert_eq!(stats.atlas_textures, 1);
    println!("presenter_stats={stats:?}; presenter-owned full-resolution/effect targets=0");
}

#[test]
#[ignore = "requires a real wgpu adapter for complex-outline golden comparisons"]
fn complex_outline_masks_render_without_crossing_glyph_table_identity() {
    let (device, queue) = native_device();
    let (baseline_table, budget, token) = fixtures();
    let config = GlyphConfig::new(8.0, 8, 12, 9).unwrap();
    let repertoire = GlyphRepertoire::new(" AB").unwrap();
    let size = GridSize::new(4, 4);
    let output = texture(&device, size);
    let target = RenderTarget::texture(&output, 1, 1);
    let mut baseline_renderer = Renderer::new(
        device.clone(),
        queue.clone(),
        baseline_table.clone(),
        budget,
        token.clone(),
        DeviceStatus::new(),
    );
    type FontFixture = (&'static str, fn() -> Vec<u8>);
    let fonts: [FontFixture; 3] = [
        ("diagonal", || synthetic_font::diagonal_font(false)),
        ("quadratic", synthetic_font::quadratic_font),
        ("composite", synthetic_font::composite_font),
    ];
    for (name, build_font) in fonts {
        let font = build_font();
        let table = Arc::new(
            build_glyph_table(&font, &repertoire, &config, &budget, &token)
                .expect("complex outline fixture builds"),
        );
        assert_ne!(table.fingerprint(), baseline_table.fingerprint());
        let glyph = table
            .glyphs()
            .iter()
            .find(|glyph| glyph.character() == 'A')
            .expect("complex outline glyph is present");
        assert!(
            glyph.mask().iter().any(|&coverage| coverage != 0),
            "{name} outline must have visible coverage"
        );
        let frame = frame(&table, size, &budget, &token);
        assert_eq!(
            baseline_renderer.present(&frame, &target),
            Err(Error::Core(ascii_core::Error::Malformed(
                MalformedReason::GlyphTableMismatch
            )))
        );
        assert_eq!(
            baseline_renderer.stats(),
            ascii_render::PresenterStats::default()
        );
        let mut renderer = Renderer::new(
            device.clone(),
            queue.clone(),
            table.clone(),
            budget,
            token.clone(),
            DeviceStatus::new(),
        );
        println!(
            "fixture={name}; font_sha256={:x}; table_fingerprint={:02x?}",
            Sha256::digest(&font),
            table.fingerprint()
        );
        assert_eq!(
            renderer.present(&frame, &target),
            Ok(PresentOutcome::Presented { suboptimal: false })
        );
        let after_first = renderer.stats();
        assert_eq!(
            renderer.present(&frame, &target),
            Ok(PresentOutcome::Skipped(SkippedReason::Unchanged))
        );
        assert_eq!(renderer.stats(), after_first);
        let (pixels, stride) = read_pixels(&device, &queue, &output);
        compare_golden(&frame, &table, &pixels, stride);
    }
}

#[test]
#[ignore = "requires a real wgpu adapter to exercise presenter failure and recovery"]
fn invalid_frame_and_host_loss_do_not_submit() {
    let (device, queue) = native_device();
    let (table, budget, token) = fixtures();
    let status = DeviceStatus::new();
    let size = GridSize::new(4, 4);
    let output = texture(&device, size);
    let target = RenderTarget::texture(&output, 1, 1);
    let mut renderer = Renderer::new(
        device,
        queue,
        table.clone(),
        budget,
        token.clone(),
        status.clone(),
    );
    let id = GlyphTableId::new(*table.fingerprint());
    let bad_cells = vec![AsciiCell::new(table.glyphs().len() as u32, [255; 4], [0; 4]); 16];
    let invalid = AsciiFrame::try_new(
        size,
        FrameMetadata::new(id, 1, 1, 0, 0),
        bad_cells,
        table.glyphs().len() as u32 + 1,
        &budget,
        &token,
    )
    .unwrap();
    assert_eq!(
        renderer.present(&invalid, &target),
        Err(Error::Core(ascii_core::Error::Malformed(
            MalformedReason::GlyphIndexOutOfRange
        )))
    );
    assert_eq!(renderer.stats().submissions, 0);
    assert_eq!(renderer.stats().uploads, 0);
    let valid = frame(&table, size, &budget, &token);
    assert_eq!(
        renderer.present(&valid, &target),
        Ok(PresentOutcome::Presented { suboptimal: false })
    );
    let before = renderer.stats();
    status.mark_lost();
    assert_eq!(
        renderer.present(&valid, &target),
        Ok(PresentOutcome::Recoverable(RecoverableReason::DeviceLost))
    );
    assert_eq!(renderer.stats(), before);
    token.cancel();
    assert_eq!(
        renderer.present(&valid, &target),
        Err(Error::Core(ascii_core::Error::Cancelled))
    );
    assert_eq!(renderer.stats(), before);
}

#[test]
#[ignore = "requires a real wgpu adapter to exercise the padded atlas copy"]
fn narrow_atlas_budget_boundary_and_gpu_copy() {
    let (device, queue) = native_device();
    let (table, budget, token) = fixtures();
    let size = GridSize::new(4, 4);
    let frame = frame(&table, size, &budget, &token);
    let output = texture(&device, size);
    let target = RenderTarget::texture(&output, 1, 1);
    let width = table.glyphs().len() as u64 * u64::from(table.cell_width());
    let height = u64::from(table.cell_height());
    let alignment = u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let staging_bytes = width.div_ceil(alignment) * alignment * height;
    let raster_bytes = width * height + staging_bytes;
    let make_budget =
        |raster| Budget::new(budget.max_frame_bytes(), raster, budget.max_work_units());
    let mut rejected = Renderer::new(
        device.clone(),
        queue.clone(),
        table.clone(),
        make_budget(raster_bytes - 1),
        token.clone(),
        DeviceStatus::new(),
    );
    assert_eq!(
        rejected.present(&frame, &target),
        Err(Error::Core(ascii_core::Error::BudgetExceeded(
            ascii_core::BudgetKind::RasterBytes
        )))
    );
    let stats = rejected.stats();
    assert_eq!(stats.atlas_textures, 0);
    assert_eq!(stats.cell_buffers, 0);
    assert_eq!(stats.uploads, 0);
    assert_eq!(stats.submissions, 0);

    let required_work = u64::from(output.width()) * u64::from(output.height()) * 2
        + width * height * 2
        + staging_bytes
        + size.cell_count();
    let mut work_rejected = Renderer::new(
        device.clone(),
        queue.clone(),
        table.clone(),
        Budget::new(budget.max_frame_bytes(), raster_bytes, required_work - 1),
        token.clone(),
        DeviceStatus::new(),
    );
    assert_eq!(
        work_rejected.present(&frame, &target),
        Err(Error::Core(ascii_core::Error::BudgetExceeded(
            ascii_core::BudgetKind::WorkUnits
        )))
    );
    assert_eq!(
        work_rejected.stats(),
        ascii_render::PresenterStats::default()
    );

    let mut accepted = Renderer::new(
        device.clone(),
        queue.clone(),
        table.clone(),
        Budget::new(budget.max_frame_bytes(), raster_bytes, required_work),
        token,
        DeviceStatus::new(),
    );
    assert_eq!(
        accepted.present(&frame, &target),
        Ok(PresentOutcome::Presented { suboptimal: false })
    );
    let (pixels, stride) = read_pixels(&device, &queue, &output);
    compare_golden(&frame, &table, &pixels, stride);
}

#[test]
#[ignore = "requires a real wgpu adapter; reports a measurement rather than a support claim"]
fn balanced_p95_measurement() {
    let (device, queue) = native_device();
    let (table, budget, token) = fixtures();
    let size = GridSize::new(240, 68);
    let frame = frame(&table, size, &budget, &token);
    let output = texture(&device, size);
    let mut renderer = Renderer::new(
        device.clone(),
        queue,
        table,
        budget,
        token,
        DeviceStatus::new(),
    );
    let target = RenderTarget::texture(&output, 1, 1);
    renderer.present(&frame, &target).unwrap();
    let _ = device.poll(wgpu::Maintain::Wait);
    let mut micros = Vec::with_capacity(100);
    for _ in 0..100 {
        renderer.invalidate();
        let start = Instant::now();
        renderer.present(&frame, &target).unwrap();
        let _ = device.poll(wgpu::Maintain::Wait);
        micros.push(start.elapsed().as_micros());
    }
    micros.sort_unstable();
    let p95_us = micros[94];
    println!(
        "balanced 240x68; 1920x816; 100 warm redraws; host wall time including GPU completion; p95_us={p95_us}; provisional_4ms_target_met={}",
        p95_us <= 4_000
    );
}

#[test]
#[ignore = "requires a real wgpu adapter to read shader storage layout"]
fn shader_cell_layout_roundtrip_and_mismatched_stride() {
    let (device, queue) = native_device();
    // Reuse the shipped WGSL Cell declaration and binding, not a separately invented layout.
    let source = format!(
        "{}\n@group(1) @binding(0) var<storage, read_write> observed: array<vec4<u32>>;\n@compute @workgroup_size(1) fn inspect(@builtin(global_invocation_id) id: vec3<u32>) {{ let c = cells[id.x]; observed[id.x] = vec4<u32>(c.glyph, c.foreground, c.background, c.flags); }}\n",
        include_str!("../src/presenter.wgsl")
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("storage roundtrip"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("layout inspection"),
        layout: None,
        module: &shader,
        entry_point: "inspect",
    });
    let input = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("layout input"),
        size: 32,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("layout output"),
        size: 32,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("layout readback"),
        size: 32,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let input_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 1,
            resource: input.as_entire_binding(),
        }],
    });
    let output_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(1),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: output.as_entire_binding(),
        }],
    });
    let run = |data: &[u8]| {
        queue.write_buffer(&input, 0, data);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("inspect layout"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &input_group, &[]);
            pass.set_bind_group(1, &output_group, &[]);
            pass.dispatch_workgroups(2, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 32);
        queue.submit(Some(encoder.finish()));
        map_buffer(&device, &readback)
    };
    let words = [
        3u32, 0x12345678, 0xaabbccdd, 0x55667788, 7, 0x10203040, 0x90807060, 0x13579bdf,
    ];
    let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    assert_eq!(
        run(&bytes),
        bytes,
        "all glyph/foreground/background/flags words survive the GPU"
    );
    let mut bad = vec![0u8; 32];
    bad[..12].copy_from_slice(&bytes[..12]);
    bad[12..24].copy_from_slice(&bytes[16..28]);
    assert_ne!(
        run(&bad),
        bytes,
        "a mistaken 12-byte stride must not satisfy the layout"
    );
}
