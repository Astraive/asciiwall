use ascii_runtime::cache::{
    CacheError, CacheIdentity, CellGeometry, ContentDigest, ConversionKey, CropRect, DerivedCache,
    DiskBudget, DiskRequirements, FitCrop, FitMode, FontHinting, FontRasterSettings, GpuCacheKey,
    GridSize, OutputFailureKind, OutputRegistry, PatchDimensions, Rational, Rgba8, SourceContent,
    SourceObservation, SourceStatHint, StoreOutcome, VideoCacheKey, VideoStreamSelection,
    VideoTimeRange,
};
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        loop {
            let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "asciiwall-cor009-cache-{}-{id}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create test temporary directory: {error}"),
            }
        }
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn partial_output_count(directory: &Path) -> usize {
    fs::read_dir(directory)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".asciiwall-output-")
        })
        .count()
}

fn digest(byte: u8) -> ContentDigest {
    ContentDigest([byte; 32])
}

fn conversion_key(source: SourceContent) -> ConversionKey {
    ConversionKey {
        source,
        decode_version: "decode-1".to_owned(),
        orientation_version: "orientation-1".to_owned(),
        color_version: "color-1".to_owned(),
        algorithm_version: "raster-1".to_owned(),
        font_digest: digest(2),
        font_raster_settings: FontRasterSettings {
            size_millipoints: 12_000,
            horizontal_scale_milli: 1_000,
            vertical_scale_milli: 1_000,
            hinting: FontHinting::Default,
            antialias: true,
            subpixel: false,
            extra_parameters: vec![("sdf-spread".to_owned(), 8)],
        },
        glyphs: vec![" ".to_owned(), ".".to_owned(), "#".to_owned()],
        grid: GridSize {
            columns: 80,
            rows: 24,
        },
        fit_crop: FitCrop {
            mode: FitMode::Fit,
            crop: None,
        },
        sampling_identity_digest: digest(4),
        sampling_filter: "lanczos3".to_owned(),
        cell_geometry: CellGeometry {
            cell_width_milli_px: 8_000,
            cell_height_milli_px: 16_000,
        },
        patch_dimensions: None,
        palette: vec![
            Rgba8 {
                red: 0,
                green: 0,
                blue: 0,
                alpha: 255,
            },
            Rgba8 {
                red: 255,
                green: 255,
                blue: 255,
                alpha: 255,
            },
        ],
        settings_digest: digest(3),
    }
}

fn identity(byte: u8) -> CacheIdentity {
    CacheIdentity::Conversion(conversion_key(SourceContent::Digest(digest(byte))))
}

#[test]
fn every_conversion_semantic_component_changes_identity() {
    let base = conversion_key(SourceContent::Digest(digest(1)));
    let base_identity = CacheIdentity::Conversion(base.clone());
    macro_rules! changed {
        ($field:ident, $value:expr) => {{
            let mut changed = base.clone();
            changed.$field = $value;
            assert_ne!(
                base_identity,
                CacheIdentity::Conversion(changed),
                stringify!($field)
            );
        }};
    }

    changed!(source, SourceContent::Digest(digest(9)));
    changed!(source, SourceContent::Missing);
    changed!(decode_version, "decode-2".to_owned());
    changed!(orientation_version, "orientation-2".to_owned());
    changed!(color_version, "color-2".to_owned());
    changed!(algorithm_version, "raster-2".to_owned());
    changed!(font_digest, digest(8));
    changed!(settings_digest, digest(7));
    changed!(sampling_identity_digest, digest(5));

    let mut raster = base.clone();
    raster.font_raster_settings.size_millipoints += 1;
    assert_ne!(base, raster);
    let mut raster = base.clone();
    raster.font_raster_settings.horizontal_scale_milli += 1;
    assert_ne!(base, raster);
    let mut raster = base.clone();
    raster.font_raster_settings.vertical_scale_milli += 1;
    assert_ne!(base, raster);
    let mut raster = base.clone();
    raster.font_raster_settings.hinting = FontHinting::Full;
    assert_ne!(base, raster);
    let mut raster = base.clone();
    raster.font_raster_settings.antialias = false;
    assert_ne!(base, raster);
    let mut raster = base.clone();
    raster.font_raster_settings.subpixel = true;
    assert_ne!(base, raster);
    let mut raster = base.clone();
    raster.font_raster_settings.extra_parameters[0].1 += 1;
    assert_ne!(base, raster);
    let mut sampling_filter = base.clone();
    sampling_filter.sampling_filter = "nearest".to_owned();
    assert_ne!(base, sampling_filter);
    let mut cell_geometry = base.clone();
    cell_geometry.cell_geometry.cell_width_milli_px += 1;
    assert_ne!(base, cell_geometry);
    let mut cell_geometry = base.clone();
    cell_geometry.cell_geometry.cell_height_milli_px += 1;
    assert_ne!(base, cell_geometry);
    let mut patch_dimensions = base.clone();
    patch_dimensions.patch_dimensions = Some(PatchDimensions {
        width: 16,
        height: 8,
    });
    assert_ne!(base, patch_dimensions);

    let mut glyph_order = base.clone();
    glyph_order.glyphs.reverse();
    assert_ne!(base, glyph_order);
    let mut glyph_content = base.clone();
    glyph_content.glyphs[1] = ",".to_owned();
    assert_ne!(base, glyph_content);
    let mut columns = base.clone();
    columns.grid.columns += 1;
    assert_ne!(base, columns);
    let mut rows = base.clone();
    rows.grid.rows += 1;
    assert_ne!(base, rows);
    let mut fit = base.clone();
    fit.fit_crop.mode = FitMode::Crop;
    assert_ne!(base, fit);
    let crop = CropRect {
        x: 1,
        y: 2,
        width: 3,
        height: 4,
    };
    let mut crop_key = base.clone();
    crop_key.fit_crop.crop = Some(crop);
    assert_ne!(base, crop_key);
    for field in 0..4 {
        let mut crop_key = crop_key.clone();
        let mut crop = crop_key.fit_crop.crop.expect("crop exists");
        match field {
            0 => crop.x += 1,
            1 => crop.y += 1,
            2 => crop.width += 1,
            _ => crop.height += 1,
        }
        crop_key.fit_crop.crop = Some(crop);
        assert_ne!(base, crop_key);
    }
    let mut palette_value = base.clone();
    palette_value.palette[0].red = 1;
    assert_ne!(base, palette_value);
    let mut palette_order = base.clone();
    palette_order.palette.reverse();
    assert_ne!(base, palette_order);
}

#[test]
fn video_and_gpu_semantic_components_change_identity() {
    let conversion = conversion_key(SourceContent::Digest(digest(1)));
    let video = VideoCacheKey {
        conversion: conversion.clone(),
        stream: VideoStreamSelection {
            stream_index: 0,
            stream_id: Some("video-main".to_owned()),
        },
        time_range: VideoTimeRange {
            start_ns: 0,
            end_ns: 2_000_000_000,
        },
        fps: Rational::new(30, 1).expect("valid fps"),
        temporal_algorithm_version: "temporal-1".to_owned(),
        seed: 4,
    };
    let base_video = CacheIdentity::Video(video.clone());
    macro_rules! video_changed {
        ($field:ident, $value:expr) => {{
            let mut changed = video.clone();
            changed.$field = $value;
            assert_ne!(
                base_video,
                CacheIdentity::Video(changed),
                stringify!($field)
            );
        }};
    }
    let mut stream_index = video.clone();
    stream_index.stream.stream_index += 1;
    assert_ne!(base_video, CacheIdentity::Video(stream_index));
    let mut stream_id = video.clone();
    stream_id.stream.stream_id = Some("video-alt".to_owned());
    assert_ne!(base_video, CacheIdentity::Video(stream_id));
    let mut start = video.clone();
    start.time_range.start_ns += 1;
    assert_ne!(base_video, CacheIdentity::Video(start));
    let mut end = video.clone();
    end.time_range.end_ns += 1;
    assert_ne!(base_video, CacheIdentity::Video(end));
    video_changed!(fps, Rational::new(24, 1).expect("valid fps"));
    video_changed!(temporal_algorithm_version, "temporal-2".to_owned());
    video_changed!(seed, 5);
    let mut upstream = video.clone();
    upstream.conversion.decode_version = "decode-2".to_owned();
    assert_ne!(base_video, CacheIdentity::Video(upstream));
    assert!(Rational::new(0, 1).is_err());
    assert!(Rational::new(1, 0).is_err());
    assert_eq!(
        Rational::new(30, 1).expect("valid fps"),
        Rational::new(60, 2).expect("equivalent rational fps")
    );

    let gpu = GpuCacheKey {
        upstream: Box::new(CacheIdentity::Conversion(conversion.clone())),
        backend: "vulkan".to_owned(),
        device: "device-0".to_owned(),
        shader_digest: digest(8),
    };
    let base_gpu = CacheIdentity::Gpu(gpu.clone());
    let mut backend = gpu.clone();
    backend.backend = "metal".to_owned();
    assert_ne!(base_gpu, CacheIdentity::Gpu(backend));
    let mut device = gpu.clone();
    device.device = "device-1".to_owned();
    assert_ne!(base_gpu, CacheIdentity::Gpu(device));
    let mut shader = gpu.clone();
    shader.shader_digest = digest(9);
    assert_ne!(base_gpu, CacheIdentity::Gpu(shader));
    let mut gpu_upstream = gpu;
    gpu_upstream.upstream = Box::new(CacheIdentity::Video(video));
    assert_ne!(base_gpu, CacheIdentity::Gpu(gpu_upstream));
}

#[test]
fn source_stat_and_path_are_revalidation_hints_not_identity() {
    let first = SourceObservation::available(
        "media/first.bin",
        Some(SourceStatHint {
            length_bytes: 100,
            modified_unix_ns: Some(10),
            file_id: Some(vec![1, 2]),
        }),
        digest(1),
    );
    let changed_hint = SourceObservation::available(
        "media/renamed.bin",
        Some(SourceStatHint {
            length_bytes: 101,
            modified_unix_ns: Some(11),
            file_id: Some(vec![3, 4]),
        }),
        digest(1),
    );
    assert_eq!(first.identity(), changed_hint.identity());
    assert!(changed_hint.requires_content_revalidation_from(&first));
    let key = CacheIdentity::Conversion(conversion_key(first.identity()));
    assert_eq!(
        key,
        CacheIdentity::Conversion(conversion_key(changed_hint.identity()))
    );
    let temp = TempDir::new();
    let cache = DerivedCache::open(temp.path().join("cache"), 64).unwrap();
    cache
        .store_observed(key.clone(), first.clone(), b"derived")
        .unwrap();
    assert!(matches!(
        cache.lease(&key),
        Err(CacheError::SourceObservationRequired)
    ));
    assert!(matches!(
        cache.lease_observed(&key, &changed_hint),
        Err(CacheError::SourceRevalidationRequired)
    ));
    assert_eq!(
        cache
            .store_observed(key.clone(), changed_hint.clone(), b"derived")
            .unwrap(),
        StoreOutcome::AlreadyPresent
    );
    cache
        .lease_observed(&key, &changed_hint)
        .unwrap()
        .acknowledge()
        .unwrap();
    assert!(matches!(
        cache.store_observed(
            CacheIdentity::Conversion(conversion_key(SourceContent::Digest(digest(9)))),
            first.clone(),
            b"mismatch"
        ),
        Err(CacheError::SourceIdentityMismatch)
    ));

    let rehashed = SourceObservation::available("media/renamed.bin", None, digest(2));
    assert_ne!(first.identity(), rehashed.identity());
    assert!(!rehashed.requires_content_revalidation_from(&first));
    assert!(matches!(
        cache.lease_observed(&key, &rehashed),
        Err(CacheError::SourceIdentityMismatch)
    ));
    let missing = SourceObservation::missing("media/renamed.bin", None);
    assert_eq!(missing.identity(), SourceContent::Missing);
    assert_ne!(first.identity(), missing.identity());
}

#[test]
fn byte_lru_pins_active_resources_until_explicit_acknowledgement() {
    let temp = TempDir::new();
    let cache = DerivedCache::open(temp.path().join("cache"), 8).expect("open cache");
    let a = identity(1);
    let b = identity(2);
    let c = identity(3);
    assert_eq!(
        cache.store(a.clone(), b"aaa").unwrap(),
        StoreOutcome::Inserted
    );
    assert_eq!(
        cache.store(b.clone(), b"bbb").unwrap(),
        StoreOutcome::Inserted
    );
    let lease = cache.lease(&a).expect("lease a");
    let pinned_path = lease.path().to_path_buf();
    assert!(pinned_path.exists());

    // B is the least-recently-used unpinned entry and is evicted to make room for C.
    assert_eq!(
        cache.store(c.clone(), b"cccc").unwrap(),
        StoreOutcome::Inserted
    );
    assert!(matches!(cache.lease(&b), Err(CacheError::NotFound)));
    assert!(pinned_path.exists());
    assert_eq!(cache.snapshot().unwrap().used_bytes, 7);

    // Lowering the limit evicts C but cannot remove the still-active A entry.
    assert!(matches!(
        cache.set_budget_bytes(0),
        Err(CacheError::BudgetPinned { .. })
    ));
    assert!(pinned_path.exists());
    assert_eq!(cache.snapshot().unwrap().used_bytes, 3);
    lease.acknowledge().expect("completion releases and trims");
    assert!(!pinned_path.exists());
    assert_eq!(cache.snapshot().unwrap().used_bytes, 0);
}

#[test]
fn dropping_a_lease_does_not_acknowledge_external_completion() {
    let temp = TempDir::new();
    let cache = DerivedCache::open(temp.path().join("cache"), 3).unwrap();
    let key = identity(7);
    cache.store(key.clone(), b"pin").unwrap();
    let lease = cache.lease(&key).unwrap();
    let path = lease.path().to_path_buf();
    drop(lease);

    assert!(matches!(
        cache.set_budget_bytes(0),
        Err(CacheError::BudgetPinned { .. })
    ));
    assert!(path.exists());
    assert_eq!(cache.snapshot().unwrap().active_leases, 1);
}

#[test]
fn eviction_only_deletes_generated_files_beneath_the_managed_cache_root() {
    let temp = TempDir::new();
    let cache_root = temp.path().join("cache");
    fs::create_dir(&cache_root).unwrap();
    let unmanaged_file = cache_root.join("unmanaged.bin");
    fs::write(&unmanaged_file, b"not registered by the cache").unwrap();
    let project_root = temp.path().join("projects");
    fs::create_dir(&project_root).unwrap();
    let project_file = project_root.join("user-project.bin");
    fs::write(&project_file, b"user data").unwrap();

    let cache = DerivedCache::open(&cache_root, 3).unwrap();
    cache.store(identity(1), b"one").unwrap();
    cache.store(identity(2), b"two").unwrap();
    assert!(unmanaged_file.exists());
    assert_eq!(
        fs::read(&unmanaged_file).unwrap(),
        b"not registered by the cache"
    );
    assert!(project_file.exists());
    assert_eq!(fs::read(&project_file).unwrap(), b"user data");
    let derived_files = fs::read_dir(cache.root())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("derived-"))
        .count();
    assert_eq!(derived_files, 1);
    assert_eq!(cache.snapshot().unwrap().used_bytes, 3);
}

#[test]
fn disk_reservations_count_expanded_input_scratch_output_and_safety_reserve() {
    let budget = DiskBudget::new(100, 10);
    let reservation = budget
        .reserve(DiskRequirements {
            expanded_input_bytes: 20,
            scratch_bytes: 30,
            output_bytes: 40,
        })
        .expect("exactly fits after safety reserve");
    assert_eq!(budget.snapshot().reserved_bytes, 90);
    budget.refresh_available_bytes(80);
    assert_eq!(budget.snapshot().available_bytes, 80);
    assert!(
        budget
            .reserve(DiskRequirements {
                output_bytes: 1,
                ..DiskRequirements::default()
            })
            .is_err()
    );
    drop(reservation);
    assert_eq!(budget.snapshot().reserved_bytes, 0);
    assert!(
        DiskRequirements {
            expanded_input_bytes: u64::MAX,
            scratch_bytes: 1,
            output_bytes: 0,
        }
        .total_bytes()
        .is_err()
    );
}

#[test]
fn failed_and_disk_full_output_preserve_original_and_report_unregistered_partial_temp() {
    let temp = TempDir::new();
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(1_024, 64);
    let reservation = budget
        .reserve(DiskRequirements {
            expanded_input_bytes: 100,
            scratch_bytes: 100,
            output_bytes: 100,
        })
        .unwrap();
    let failed_identity = identity(4);
    let failed_destination = temp.path().join("existing.bin");
    fs::write(&failed_destination, b"original").unwrap();
    let failed = registry.write_transactional(
        failed_identity.clone(),
        &failed_destination,
        &reservation,
        |output| {
            output.write_all(b"partial")?;
            Err(io::Error::other("injected writer failure"))
        },
    );
    let failed = failed.expect_err("injected writer failure must fail transaction");
    assert_eq!(failed.kind, OutputFailureKind::Writer);
    let partial = failed.partial_temp_path.expect("partial path is reported");
    assert_ne!(partial, failed_destination);
    assert_eq!(fs::read(partial).unwrap(), b"partial");
    assert_eq!(fs::read(&failed_destination).unwrap(), b"original");
    assert_eq!(registry.registered_path(&failed_identity), None);

    let disk_full_identity = identity(5);
    let disk_full_destination = temp.path().join("disk-full.bin");
    fs::write(&disk_full_destination, b"old output").unwrap();
    let disk_full = registry.write_transactional(
        disk_full_identity.clone(),
        &disk_full_destination,
        &reservation,
        |output| {
            output.write_all(b"written before full")?;
            Err(io::Error::new(ErrorKind::StorageFull, "injected disk full"))
        },
    );
    let disk_full = disk_full.expect_err("disk-full write must fail");
    assert_eq!(disk_full.kind, OutputFailureKind::DiskFull);
    let partial = disk_full
        .partial_temp_path
        .expect("disk-full partial path is reported");
    assert_eq!(fs::read(partial).unwrap(), b"written before full");
    assert_eq!(fs::read(&disk_full_destination).unwrap(), b"old output");
    assert_eq!(registry.registered_path(&disk_full_identity), None);
    assert_eq!(partial_output_count(temp.path()), 2);
}

#[test]
fn quota_exhaustion_is_classified_as_disk_full_without_publishing_output() {
    let temp = TempDir::new();
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(1_024, 64);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 8,
            ..DiskRequirements::default()
        })
        .unwrap();
    let key = identity(33);
    let destination = temp.path().join("quota-exhausted.bin");
    fs::write(&destination, b"original").unwrap();

    let error = registry
        .write_transactional(key.clone(), &destination, &reservation, |output| {
            output.write_all(b"part")?;
            Err(io::Error::new(
                ErrorKind::QuotaExceeded,
                "injected quota exhaustion",
            ))
        })
        .expect_err("quota exhaustion must fail the transaction");

    assert_eq!(error.kind, OutputFailureKind::DiskFull);
    assert_eq!(fs::read(&destination).unwrap(), b"original");
    assert_eq!(fs::read(error.partial_temp_path.unwrap()).unwrap(), b"part");
    assert_eq!(registry.registered_path(&key), None);
}

#[cfg(unix)]
const NOT_DISK_FULL_RAW_ERROR: i32 = 39;
#[cfg(windows)]
const NOT_DISK_FULL_RAW_ERROR: i32 = 122;

#[cfg(any(unix, windows))]
#[test]
fn unrelated_platform_error_codes_are_not_reported_as_disk_full() {
    let temp = TempDir::new();
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(1_024, 64);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 8,
            ..DiskRequirements::default()
        })
        .unwrap();
    let key = identity(32);
    let destination = temp.path().join("unrelated-os-error.bin");
    fs::write(&destination, b"original").unwrap();

    let error = registry
        .write_transactional(key.clone(), &destination, &reservation, |output| {
            output.write_all(b"part")?;
            Err(io::Error::from_raw_os_error(NOT_DISK_FULL_RAW_ERROR))
        })
        .expect_err("unrelated platform errors remain writer failures");

    assert_eq!(error.kind, OutputFailureKind::Writer);
    assert_eq!(fs::read(destination).unwrap(), b"original");
    assert!(error.partial_temp_path.unwrap().exists());
    assert_eq!(registry.registered_path(&key), None);
}

#[test]
fn successful_output_creation_registers_only_after_commit() {
    let temp = TempDir::new();
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(1_024, 64);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 32,
            ..DiskRequirements::default()
        })
        .unwrap();
    let key = identity(8);
    let destination = temp.path().join("new-output.bin");
    assert!(!destination.exists());
    let committed = registry
        .write_transactional(key.clone(), &destination, &reservation, |output| {
            output.write_all(b"created output")
        })
        .expect("new destination commits");
    assert_eq!(committed, destination);
    assert_eq!(fs::read(&committed).unwrap(), b"created output");
    assert_eq!(registry.registered_path(&key), Some(committed));
    assert_eq!(partial_output_count(temp.path()), 0);
}

#[cfg(any(unix, windows))]
#[test]
fn successful_output_replaces_existing_destination() {
    let temp = TempDir::new();
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(1_024, 64);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 32,
            ..DiskRequirements::default()
        })
        .unwrap();
    let key = identity(9);
    let destination = temp.path().join("replace-output.bin");
    fs::write(&destination, b"original output").unwrap();
    let committed = registry
        .write_transactional(key.clone(), &destination, &reservation, |output| {
            output.write_all(b"replacement")
        })
        .expect("supported targets atomically replace existing output");
    assert_eq!(fs::read(&destination).unwrap(), b"replacement");
    assert_eq!(registry.registered_path(&key), Some(committed));
    assert_eq!(partial_output_count(temp.path()), 0);
}

#[cfg(not(any(unix, windows)))]
#[test]
fn unsupported_targets_do_not_clobber_existing_destinations() {
    let temp = TempDir::new();
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(1_024, 64);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 32,
            ..DiskRequirements::default()
        })
        .unwrap();
    let key = identity(10);
    let destination = temp.path().join("no-clobber-output.bin");
    fs::write(&destination, b"original output").unwrap();
    let error = registry
        .write_transactional(key.clone(), &destination, &reservation, |output| {
            output.write_all(b"replacement")
        })
        .expect_err("unsupported targets reject replacement");
    assert_eq!(error.kind, OutputFailureKind::Commit);
    assert!(
        error
            .partial_temp_path
            .expect("staged artifact remains")
            .exists()
    );
    assert_eq!(fs::read(&destination).unwrap(), b"original output");
    assert_eq!(registry.registered_path(&key), None);
}

#[test]
fn commit_and_temp_creation_failures_preserve_recovery_boundaries() {
    let temp = TempDir::new();
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(1_024, 64);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 32,
            ..DiskRequirements::default()
        })
        .unwrap();

    let key = identity(11);
    let destination_directory = temp.path().join("destination-directory");
    fs::create_dir(&destination_directory).unwrap();
    let existing_file = destination_directory.join("keep.bin");
    fs::write(&existing_file, b"preserve this").unwrap();
    let error = registry
        .write_transactional(
            key.clone(),
            &destination_directory,
            &reservation,
            |output| output.write_all(b"not committed"),
        )
        .expect_err("a file cannot replace the non-empty destination directory");
    assert_eq!(error.kind, OutputFailureKind::Commit);
    assert!(
        error
            .partial_temp_path
            .expect("commit partial is reported")
            .exists()
    );
    assert_eq!(fs::read(&existing_file).unwrap(), b"preserve this");
    assert_eq!(registry.registered_path(&key), None);

    let missing_parent_destination = temp.path().join("absent-parent").join("output.bin");
    let temp_error = registry
        .write_transactional(
            identity(12),
            &missing_parent_destination,
            &reservation,
            |output| output.write_all(b"no stage created"),
        )
        .expect_err("temporary creation fails without its parent directory");
    assert_eq!(temp_error.kind, OutputFailureKind::TemporaryCreate);
    assert_eq!(temp_error.partial_temp_path, None);
    assert_eq!(partial_output_count(temp.path()), 1);
}

#[test]
fn reserved_output_limit_is_enforced_and_partial_is_not_registered() {
    let temp = TempDir::new();
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(100, 10);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 3,
            ..DiskRequirements::default()
        })
        .unwrap();
    let key = identity(6);
    let destination = temp.path().join("too-large.bin");
    fs::write(&destination, b"original").unwrap();
    let error = registry.write_transactional(key.clone(), &destination, &reservation, |output| {
        output.write_all(b"four")
    });
    let error = error.expect_err("output exceeding its reservation fails");
    assert_eq!(error.kind, OutputFailureKind::OutputLimitExceeded);
    let partial = error.partial_temp_path.expect("partial path is reported");
    assert!(partial.exists());
    assert_eq!(fs::read(destination).unwrap(), b"original");
    assert_eq!(registry.registered_path(&key), None);
}

#[test]
fn reopening_cache_accounts_for_generated_files_and_evicts_them_before_unmanaged_files() {
    let temp = TempDir::new();
    let cache_root = temp.path().join("cache");
    fs::create_dir(&cache_root).unwrap();
    let unmanaged_file = cache_root.join("unmanaged.bin");
    fs::write(&unmanaged_file, b"leave this alone").unwrap();

    let first_cache = DerivedCache::open(&cache_root, 16).unwrap();
    first_cache.store(identity(20), b"one").unwrap();
    first_cache.store(identity(21), b"two").unwrap();
    drop(first_cache);

    let reopened = DerivedCache::open(&cache_root, 4).unwrap();
    let snapshot = reopened.snapshot().unwrap();
    assert_eq!(snapshot.used_bytes, 3);
    assert_eq!(snapshot.entries, 0);
    let remaining_derived = fs::read_dir(&cache_root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("derived-"))
        .count();
    assert_eq!(remaining_derived, 1);
    assert_eq!(fs::read(&unmanaged_file).unwrap(), b"leave this alone");

    reopened.store(identity(22), b"zz").unwrap();
    assert_eq!(reopened.snapshot().unwrap().used_bytes, 2);
    let derived_after_store = fs::read_dir(&cache_root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("derived-"))
        .count();
    assert_eq!(derived_after_store, 1);
    assert_eq!(fs::read(&unmanaged_file).unwrap(), b"leave this alone");
}

#[test]
fn output_reservation_limits_successful_bytes_across_transactions() {
    let temp = TempDir::new();
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(100, 0);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 3,
            ..DiskRequirements::default()
        })
        .unwrap();

    let first = temp.path().join("first.bin");
    registry
        .write_transactional(identity(23), &first, &reservation, |output| {
            output.write_all(b"ab")
        })
        .unwrap();
    let second = temp.path().join("second.bin");
    registry
        .write_transactional(identity(24), &second, &reservation, |output| {
            output.write_all(b"c")
        })
        .unwrap();

    let third = temp.path().join("third.bin");
    fs::write(&third, b"original").unwrap();
    let key = identity(25);
    let error = registry
        .write_transactional(key.clone(), &third, &reservation, |output| {
            output.write_all(b"d")
        })
        .expect_err("a reused reservation cannot authorize additional output bytes");
    assert_eq!(error.kind, OutputFailureKind::OutputLimitExceeded);
    assert_eq!(fs::read(first).unwrap(), b"ab");
    assert_eq!(fs::read(second).unwrap(), b"c");
    assert_eq!(fs::read(third).unwrap(), b"original");
    assert_eq!(
        fs::read(
            error
                .partial_temp_path
                .expect("failed stage is recoverable")
        )
        .unwrap(),
        b""
    );
    assert_eq!(registry.registered_path(&key), None);
}

#[cfg(unix)]
#[test]
fn symlinked_destination_parent_spelling_removes_stale_identity_registration() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new();
    let real_parent = temp.path().join("real");
    let alias_parent = temp.path().join("alias");
    fs::create_dir(&real_parent).unwrap();
    symlink(&real_parent, &alias_parent).unwrap();

    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(1_024, 0);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 32,
            ..DiskRequirements::default()
        })
        .unwrap();
    let first_identity = identity(26);
    let second_identity = identity(27);
    let real_destination = real_parent.join("output.bin");
    let alias_destination = alias_parent.join("output.bin");

    registry
        .write_transactional(
            first_identity.clone(),
            &real_destination,
            &reservation,
            |output| output.write_all(b"first"),
        )
        .unwrap();
    registry
        .write_transactional(
            second_identity.clone(),
            &alias_destination,
            &reservation,
            |output| output.write_all(b"second"),
        )
        .unwrap();

    assert_eq!(registry.registered_path(&first_identity), None);
    assert_eq!(
        registry.registered_path(&second_identity),
        Some(alias_destination)
    );
    assert_eq!(fs::read(real_destination).unwrap(), b"second");
}

#[cfg(unix)]
#[test]
fn stage_path_substitution_before_commit_is_detected() {
    let temp = TempDir::new();
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(1_024, 0);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 32,
            ..DiskRequirements::default()
        })
        .unwrap();
    let key = identity(28);
    let destination = temp.path().join("output.bin");
    fs::write(&destination, b"original").unwrap();

    let error = registry
        .write_transactional(key.clone(), &destination, &reservation, |output| {
            output.write_all(b"trusted stage")?;
            let staged_path = fs::read_dir(temp.path())?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    path.file_name().is_some_and(|name| {
                        name.to_string_lossy().starts_with(".asciiwall-output-")
                    })
                })
                .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "staged path not found"))?;
            fs::remove_file(&staged_path)?;
            fs::write(staged_path, b"substituted")?;
            Ok(())
        })
        .expect_err("a path no longer naming the staged handle must not be committed");

    assert_eq!(error.kind, OutputFailureKind::Commit);
    let partial = error
        .partial_temp_path
        .expect("substituted stage remains reportable");
    assert_eq!(fs::read(partial).unwrap(), b"substituted");
    assert_eq!(fs::read(destination).unwrap(), b"original");
    assert_eq!(registry.registered_path(&key), None);
}

#[cfg(windows)]
#[test]
fn case_variant_regular_destination_removes_stale_identity_registration() {
    let temp = TempDir::new();
    let upper_destination = temp.path().join("Output.bin");
    let lower_destination = temp.path().join("output.bin");
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(1_024, 0);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 32,
            ..DiskRequirements::default()
        })
        .unwrap();
    let first_identity = identity(29);
    let second_identity = identity(30);

    registry
        .write_transactional(
            first_identity.clone(),
            &upper_destination,
            &reservation,
            |output| output.write_all(b"first"),
        )
        .unwrap();
    // Some Windows directories opt into case-sensitive names; only exercise the alias when
    // these spellings actually resolve to the same destination entry.
    if !lower_destination.exists() {
        return;
    }
    registry
        .write_transactional(
            second_identity.clone(),
            &lower_destination,
            &reservation,
            |output| output.write_all(b"second"),
        )
        .unwrap();

    assert_eq!(registry.registered_path(&first_identity), None);
    assert_eq!(
        registry.registered_path(&second_identity),
        Some(lower_destination)
    );
    assert_eq!(fs::read(upper_destination).unwrap(), b"second");
}

#[test]
fn failed_output_retry_preserves_previous_registration() {
    let temp = TempDir::new();
    let registry = OutputRegistry::default();
    let budget = DiskBudget::new(1_024, 0);
    let reservation = budget
        .reserve(DiskRequirements {
            output_bytes: 64,
            ..DiskRequirements::default()
        })
        .unwrap();
    let key = identity(31);
    let destination = temp.path().join("valid-output.bin");
    let committed = registry
        .write_transactional(key.clone(), &destination, &reservation, |output| {
            output.write_all(b"previous")
        })
        .unwrap();
    assert_eq!(registry.registered_path(&key), Some(committed.clone()));

    let missing_parent_destination = temp.path().join("missing-parent").join("output.bin");
    let error = registry
        .write_transactional(
            key.clone(),
            &missing_parent_destination,
            &reservation,
            |output| output.write_all(b"replacement"),
        )
        .expect_err("a retry with a missing parent must fail");

    assert_eq!(error.kind, OutputFailureKind::TemporaryCreate);
    assert_eq!(error.partial_temp_path, None);
    assert_eq!(registry.registered_path(&key), Some(committed.clone()));
    assert_eq!(fs::read(committed).unwrap(), b"previous");
}
