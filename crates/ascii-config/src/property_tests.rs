use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use proptest::{
    prelude::*,
    test_runner::{Config as ProptestConfig, RngSeed},
};

use crate::{
    ConfigError, ControllerStatus, DocumentError, InvalidationClass, MAX_NESTING_DEPTH, Registry,
    SemanticValidator, SettingChange, SettingDescriptor, SettingKey, SettingPhase, SettingScope,
    SettingType, SettingValue, SettingsController, TypeViolation,
    document::{CURRENT_SCHEMA_VERSION, decode, encode},
    store::{BackupMode, ConfigStore, FileStore, PersistStage, StoreError, StoreOperation},
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ascii-config-property-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn property_registry() -> Arc<Registry> {
    let enabled = SettingDescriptor::new(
        SettingKey::new("application.enabled").unwrap(),
        SettingValue::Boolean(false),
        SettingType::Boolean,
        SettingScope::A,
        SettingPhase::V1,
        None,
        InvalidationClass::Controller,
        SemanticValidator::None,
    )
    .unwrap();
    let scale = SettingDescriptor::new(
        SettingKey::new("render.scale").unwrap(),
        SettingValue::Integer(1),
        SettingType::Integer { min: 1, max: 100 },
        SettingScope::P,
        SettingPhase::V1,
        None,
        InvalidationClass::Conversion,
        SemanticValidator::None,
    )
    .unwrap();
    Arc::new(Registry::new([enabled, scale]).unwrap())
}

fn write_versioned_document(path: &Path, version: i64, revision: u64) {
    fs::write(
        path,
        format!("schema_version = {version}\nrevision = {revision}\n").as_bytes(),
    )
    .unwrap();
}

fn interrupted_stage(index: u8) -> PersistStage {
    match index {
        0 => PersistStage::TempCreate,
        1 => PersistStage::TempWrite,
        2 => PersistStage::FileSync,
        3 => PersistStage::BackupReplace,
        4 => PersistStage::BackupDirectorySync,
        5 => PersistStage::MainReplace,
        _ => unreachable!("stage index is bounded by the generated strategy"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        max_local_rejects: 32,
        max_global_rejects: 32,
        max_flat_map_regens: 32,
        max_shrink_iters: 64,
        max_shrink_time: 1_000,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(0xC0F1_0010),
        ..ProptestConfig::default()
    })]

    #[test]
    fn property_integer_and_number_ranges_accept_inclusive_endpoints(
        min in -1_000_000i64..=1_000_000,
        span in 0i64..=1_000_000,
    ) {
        let max = min + span;
        let integer_range = SettingType::Integer { min, max };
        prop_assert_eq!(integer_range.validates(&SettingValue::Integer(min)), Ok(()));
        prop_assert_eq!(integer_range.validates(&SettingValue::Integer(max)), Ok(()));
        prop_assert_eq!(
            integer_range.validates(&SettingValue::Integer(min - 1)),
            Err(TypeViolation::OutOfRange),
        );
        prop_assert_eq!(
            integer_range.validates(&SettingValue::Integer(max + 1)),
            Err(TypeViolation::OutOfRange),
        );

        let float_min = min as f64;
        let float_max = max as f64;
        let number_range = SettingType::Number {
            min: float_min,
            max: float_max,
        };
        prop_assert_eq!(
            number_range.validates(&SettingValue::number(float_min).unwrap()),
            Ok(()),
        );
        prop_assert_eq!(
            number_range.validates(&SettingValue::number(float_max).unwrap()),
            Ok(()),
        );
        prop_assert_eq!(
            number_range.validates(&SettingValue::number(float_min - 1.0).unwrap()),
            Err(TypeViolation::OutOfRange),
        );
        prop_assert_eq!(
            number_range.validates(&SettingValue::number(float_max + 1.0).unwrap()),
            Err(TypeViolation::OutOfRange),
        );
    }

    #[test]
    fn property_revision_overflow_is_rejected_before_serialization(
        excess in 1u64..=1_000_000,
    ) {
        let revision = i64::MAX as u64 + excess;
        prop_assert!(matches!(
            encode(revision, &BTreeMap::new()),
            Err(DocumentError::RevisionOverflow),
        ));
    }

    #[test]
    fn property_nesting_is_bounded_before_scalar_validation(
        depth in 1usize..=MAX_NESTING_DEPTH + 2,
    ) {
        let mut document =
            String::from("schema_version = 2\nrevision = 0\n[settings]\n\"application.enabled\" = ");
        document.push_str(&"[".repeat(depth));
        document.push_str("true");
        document.push_str(&"]".repeat(depth));
        document.push('\n');

        let result = decode(document.as_bytes());
        if depth > MAX_NESTING_DEPTH {
            prop_assert!(
                matches!(
                    result,
                    Err(DocumentError::ExcessiveNesting {
                        limit: MAX_NESTING_DEPTH
                    })
                ),
                "nesting beyond the configured bound must be rejected"
            );
        } else {
            prop_assert!(
                matches!(result, Err(DocumentError::UnsupportedStructure)),
                "bounded nesting must reach scalar validation"
            );
        }
    }

    #[test]
    fn property_duplicate_settings_keys_are_rejected(
        first in 0i64..=1_000_000,
        second in 0i64..=1_000_000,
    ) {
        let document = format!(
            "schema_version = 2\nrevision = 0\n[settings]\n\"render.scale\" = {first}\n\"render.scale\" = {second}\n",
        );
        prop_assert!(matches!(decode(document.as_bytes()), Err(DocumentError::DuplicateKey)));
    }

    #[test]
    fn property_invalid_utf8_is_rejected_before_parsing(
        suffix in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let mut document = b"schema_version = 2\nrevision = 0\n".to_vec();
        document.push(0xff);
        document.extend(suffix);
        prop_assert!(matches!(decode(&document), Err(DocumentError::InvalidUtf8)));
    }

    #[test]
    fn property_nonpositive_schema_versions_are_unsupported(version in -10_000i64..=0) {
        let document = format!("schema_version = {version}\nrevision = 0\n");
        prop_assert!(matches!(
            decode(document.as_bytes()),
            Err(DocumentError::UnsupportedVersion(observed)) if observed == version,
        ));
    }

    #[test]
    fn property_future_schema_is_read_only_and_keeps_original_bytes(
        version in (CURRENT_SCHEMA_VERSION + 1)..=10_000,
        revision in 0u64..=1_000_000,
    ) {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        write_versioned_document(&path, version, revision);
        let original = fs::read(&path).unwrap();
        let controller = SettingsController::open(&path, property_registry()).unwrap();

        prop_assert_eq!(
            controller.status().unwrap(),
            ControllerStatus::ReadOnlyFutureSchema { schema_version: version },
        );
        let result = controller.apply_batch(
            revision,
            &[(
                SettingKey::new("application.enabled").unwrap(),
                SettingChange::Set(SettingValue::Boolean(true)),
            )],
        );
        prop_assert!(
            matches!(
                result,
                Err(ConfigError::ReadOnlyFutureSchema {
                    schema_version: observed
                }) if observed == version
            ),
            "writes to future schemas must be refused"
        );
        prop_assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn property_disk_loss_preserves_main_and_last_good_backup(
        previous in prop::collection::vec(any::<u8>(), 1..64),
        candidate in prop::collection::vec(any::<u8>(), 1..64),
    ) {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let backup = directory.0.join("settings.toml.bak");
        fs::write(&path, &previous).unwrap();
        let mut store = FileStore::new(&path);
        store.inject_disk_full_after_temp_writes(1);

        let result = store.commit(
            Some(&previous),
            &candidate,
            BackupMode::ReplaceWithPrevious,
        );
        prop_assert!(
            matches!(
                result,
                Err(StoreError::Io {
                    operation: StoreOperation::WriteTemporary,
                    kind: std::io::ErrorKind::StorageFull,
                    ..
                })
            ),
            "injected disk loss must fail the write"
        );
        let main_bytes = fs::read(&path).unwrap();
        let backup_bytes = fs::read(&backup).unwrap();
        prop_assert_eq!(main_bytes.as_slice(), previous.as_slice());
        prop_assert_eq!(backup_bytes.as_slice(), previous.as_slice());
    }

    #[test]
    fn property_interrupted_persistence_stages_leave_main_bytes_usable(
        stage_index in 0u8..=5,
        previous in prop::collection::vec(any::<u8>(), 1..64),
        candidate in prop::collection::vec(any::<u8>(), 1..64),
    ) {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        fs::write(&path, &previous).unwrap();
        let mut store = FileStore::new(&path);
        store.inject_failure(interrupted_stage(stage_index));

        prop_assert!(store.commit(
            Some(&previous),
            &candidate,
            BackupMode::ReplaceWithPrevious,
        ).is_err());
        prop_assert_eq!(fs::read(&path).unwrap(), previous);
    }

    #[test]
    fn property_stale_revision_rejects_the_whole_batch(
        current in 0u64..=1_000_000,
    ) {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        write_versioned_document(&path, CURRENT_SCHEMA_VERSION, current);
        let original = fs::read(&path).unwrap();
        let controller = SettingsController::open(&path, property_registry()).unwrap();
        let expected = current + 1;
        let changes = [
            (
                SettingKey::new("application.enabled").unwrap(),
                SettingChange::Set(SettingValue::Boolean(true)),
            ),
            (
                SettingKey::new("render.scale").unwrap(),
                SettingChange::Set(SettingValue::Integer(2)),
            ),
        ];

        let result = controller.apply_batch(expected, &changes);
        prop_assert!(
            matches!(
                result,
                Err(ConfigError::Conflict {
                    expected: found_expected,
                    current: found_current
                }) if found_expected == expected && found_current == current
            ),
            "stale revisions must refuse the entire batch"
        );
        prop_assert_eq!(controller.snapshot().unwrap().revision(), current);
        prop_assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn property_rollback_restores_previous_bytes_after_replacement_failure(
        previous in prop::collection::vec(any::<u8>(), 1..64),
        candidate in prop::collection::vec(any::<u8>(), 1..64),
    ) {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let backup = directory.0.join("settings.toml.bak");
        fs::write(&path, &previous).unwrap();
        let mut store = FileStore::new(&path);
        store.inject_failure(PersistStage::MainDirectorySync);

        prop_assert!(matches!(
            store.commit(
                Some(&previous),
                &candidate,
                BackupMode::ReplaceWithPrevious,
            ),
            Err(StoreError::Injected(PersistStage::MainDirectorySync)),
        ));
        let main_bytes = fs::read(&path).unwrap();
        let backup_bytes = fs::read(&backup).unwrap();
        prop_assert_eq!(main_bytes.as_slice(), previous.as_slice());
        prop_assert_eq!(backup_bytes.as_slice(), previous.as_slice());
    }
}

#[test]
fn future_documents_exceeding_limits_remain_read_only_and_unchanged() {
    use std::fmt::Write as _;

    let directory = TestDirectory::new();
    let oversized = {
        let mut bytes =
            format!("schema_version = {}\nrevision = 0\n", CURRENT_SCHEMA_VERSION + 1)
                .into_bytes();
        bytes.resize(crate::MAX_CONFIG_BYTES + 1, b' ');
        bytes
    };
    let deep = format!(
        "schema_version = {}\nrevision = 0\nvalue = {}true{}\n",
        CURRENT_SCHEMA_VERSION + 1,
        "[".repeat(MAX_NESTING_DEPTH + 1),
        "]".repeat(MAX_NESTING_DEPTH + 1),
    )
    .into_bytes();
    let mut entries = format!(
        "schema_version = {}\nrevision = 0\n[settings]\n",
        CURRENT_SCHEMA_VERSION + 1
    );
    for index in 0..crate::MAX_SETTING_ENTRIES + 2 {
        writeln!(&mut entries, "\"future.key{index:04}\" = {index}").unwrap();
    }
    let cases = [
        ("byte-limit", oversized),
        ("depth-limit", deep),
        ("entry-limit", entries.into_bytes()),
    ];

    for (name, original) in cases {
        let path = directory.0.join(format!("{name}.toml"));
        fs::write(&path, &original).unwrap();
        let controller = SettingsController::open(&path, property_registry()).unwrap();
        let before = controller.snapshot().unwrap();
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::ReadOnlyUnclassifiableDocument(_)
        ));
        assert!(matches!(
            controller.repair(0),
            Err(ConfigError::ReadOnlyUnclassifiableDocument)
        ));
        assert!(matches!(
            controller.apply_batch(
                0,
                &[(
                    SettingKey::new("application.enabled").unwrap(),
                    SettingChange::Set(SettingValue::Boolean(true)),
                )],
            ),
            Err(ConfigError::ReadOnlyUnclassifiableDocument)
        ));
        assert!(matches!(
            controller.restore_backup(0),
            Err(ConfigError::ReadOnlyUnclassifiableDocument)
        ));
        assert_eq!(controller.snapshot().unwrap(), before);
        assert_eq!(fs::read(path).unwrap(), original);
    }
}

#[test]
fn controller_revision_overflow_preserves_snapshot_and_persisted_bytes() {
    let directory = TestDirectory::new();
    let path = directory.0.join("settings.toml");
    let original = format!(
        "schema_version = {CURRENT_SCHEMA_VERSION}\nrevision = {}\n",
        i64::MAX
    );
    fs::write(&path, original.as_bytes()).unwrap();
    let controller = SettingsController::open(&path, property_registry()).unwrap();
    let before = controller.snapshot().unwrap();
    assert_eq!(before.revision(), i64::MAX as u64);

    let result = controller.apply_batch(
        before.revision(),
        &[(
            SettingKey::new("application.enabled").unwrap(),
            SettingChange::Set(SettingValue::Boolean(true)),
        )],
    );
    assert!(matches!(result, Err(ConfigError::RevisionOverflow)));
    assert_eq!(controller.snapshot().unwrap(), before);
    assert_eq!(
        fs::read(path).unwrap().as_slice(),
        original.as_bytes()
    );
}
