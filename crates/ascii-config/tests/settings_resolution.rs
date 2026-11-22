use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Barrier,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use ascii_config::{
    ConfigError, ConstraintReason, Constraints, ControllerStatus, DocumentError,
    EffectiveConstraint, InvalidationClass, MAX_CONFIG_BYTES, MAX_NESTING_DEPTH, Registry,
    ResolveError, SemanticValidator, SettingChange, SettingDescriptor, SettingKey, SettingLayers,
    SettingPhase, SettingScope, SettingSource, SettingType, SettingValue, SettingsController,
    TypeViolation, resolve_settings,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ascii-config-{}-{sequence}", std::process::id()));
        fs::create_dir_all(&path).expect("create isolated test directory");
        Self(path)
    }

    fn config(&self) -> PathBuf {
        self.0.join("settings.toml")
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn key(value: &str) -> SettingKey {
    SettingKey::new(value).expect("valid fixture setting key")
}

fn number(value: f64) -> SettingValue {
    SettingValue::number(value).expect("finite fixture number")
}

fn descriptor(
    name: &str,
    default: SettingValue,
    value_type: SettingType,
    scope: SettingScope,
    capability: Option<&str>,
    invalidation: InvalidationClass,
    semantic_validator: SemanticValidator,
) -> SettingDescriptor {
    SettingDescriptor::new(
        key(name),
        default,
        value_type,
        scope,
        SettingPhase::V1,
        capability,
        invalidation,
        semantic_validator,
    )
    .expect("valid test descriptor")
}

fn registry() -> Arc<Registry> {
    Arc::new(
        Registry::new([
            descriptor(
                "grid.columns",
                SettingValue::Integer(240),
                SettingType::Integer { min: 32, max: 1024 },
                SettingScope::P,
                None,
                InvalidationClass::Conversion,
                SemanticValidator::None,
            ),
            descriptor(
                "playback.fps",
                SettingValue::Integer(30),
                SettingType::Integer { min: 1, max: 120 },
                SettingScope::P,
                None,
                InvalidationClass::Scheduler,
                SemanticValidator::None,
            ),
            descriptor(
                "conversion.brightness",
                number(0.0),
                SettingType::Number {
                    min: -1.0,
                    max: 1.0,
                },
                SettingScope::P,
                None,
                InvalidationClass::Conversion,
                SemanticValidator::None,
            ),
            descriptor(
                "conversion.gamma",
                number(1.0),
                SettingType::Number { min: 0.1, max: 5.0 },
                SettingScope::P,
                None,
                InvalidationClass::Conversion,
                SemanticValidator::None,
            ),
            descriptor(
                "privacy.pointer-observation",
                SettingValue::Boolean(false),
                SettingType::Boolean,
                SettingScope::A,
                Some("pointer-position"),
                InvalidationClass::Observer,
                SemanticValidator::None,
            ),
            descriptor(
                "visibility.covered-threshold",
                number(0.98),
                SettingType::Number { min: 0.5, max: 1.0 },
                SettingScope::A,
                Some("window-rectangles"),
                InvalidationClass::Runtime,
                SemanticValidator::None,
            ),
            descriptor(
                "visibility.resume-threshold",
                number(0.9),
                SettingType::Number {
                    min: 0.0,
                    max: 0.99,
                },
                SettingScope::A,
                Some("window-rectangles"),
                InvalidationClass::Runtime,
                SemanticValidator::LessThan(key("visibility.covered-threshold")),
            ),
            descriptor(
                "diagnostics.overlay",
                SettingValue::Boolean(false),
                SettingType::Boolean,
                SettingScope::S,
                None,
                InvalidationClass::Runtime,
                SemanticValidator::None,
            ),
        ])
        .expect("valid fixture registry"),
    )
}

fn read_status(path: &Path) -> ControllerStatus {
    SettingsController::open(path, registry())
        .expect("open config")
        .status()
        .expect("read controller status")
}

#[test]
fn registry_metadata_and_range_endpoints_are_exact() {
    let registry = registry();
    let columns = registry
        .get_str("grid.columns")
        .expect("columns descriptor");
    assert_eq!(columns.scope(), SettingScope::P);
    assert_eq!(columns.phase(), SettingPhase::V1);
    assert_eq!(columns.invalidation(), InvalidationClass::Conversion);
    assert_eq!(columns.default(), &SettingValue::Integer(240));
    assert_eq!(
        columns.value_type(),
        &SettingType::Integer { min: 32, max: 1024 }
    );
    let pointer = registry
        .get_str("privacy.pointer-observation")
        .expect("pointer descriptor");
    assert_eq!(pointer.capability(), Some("pointer-position"));
    assert_eq!(pointer.semantic_validator(), &SemanticValidator::None);
    let resume = registry
        .get_str("visibility.resume-threshold")
        .expect("resume threshold descriptor");
    assert_eq!(
        resume.semantic_validator(),
        &SemanticValidator::LessThan(key("visibility.covered-threshold"))
    );

    for value in [32, 1024] {
        let mut layers = SettingLayers::default();
        layers.set(
            SettingSource::Package,
            key("grid.columns"),
            SettingValue::Integer(value),
        );
        assert!(resolve_settings(&registry, &layers, &Constraints::default()).is_ok());
    }
    for value in [31, 1025, i64::MAX] {
        let mut layers = SettingLayers::default();
        layers.set(
            SettingSource::Package,
            key("grid.columns"),
            SettingValue::Integer(value),
        );
        assert!(matches!(
            resolve_settings(&registry, &layers, &Constraints::default()),
            Err(ResolveError::InvalidValue {
                violation: TypeViolation::OutOfRange,
                ..
            })
        ));
    }
    for value in -32..=1088 {
        let mut layers = SettingLayers::default();
        layers.set(
            SettingSource::Package,
            key("grid.columns"),
            SettingValue::Integer(value),
        );
        assert_eq!(
            resolve_settings(&registry, &layers, &Constraints::default()).is_ok(),
            (32..=1024).contains(&value),
        );
    }
}

#[test]
fn package_scope_cannot_enable_pointer_observation() {
    let mut layers = SettingLayers::default();
    layers.set(
        SettingSource::Package,
        key("privacy.pointer-observation"),
        SettingValue::Boolean(true),
    );
    assert!(matches!(
        resolve_settings(&registry(), &layers, &Constraints::default()),
        Err(ResolveError::UnauthorizedSource {
            source: SettingSource::Package,
            scope: SettingScope::A,
            ..
        })
    ));
}

#[test]
fn effective_constraints_preserve_the_requested_value() {
    let registry = registry();
    let mut layers = SettingLayers::default();
    layers.set(
        SettingSource::Package,
        key("playback.fps"),
        SettingValue::Integer(60),
    );
    let mut constraints = Constraints::default();
    constraints.set(
        key("playback.fps"),
        EffectiveConstraint::AtMost {
            value: SettingValue::Integer(10),
            reason: ConstraintReason::PowerPolicy,
        },
    );
    let resolved = resolve_settings(&registry, &layers, &constraints).expect("valid resolution");
    let fps = resolved.get_str("playback.fps").expect("FPS setting");
    assert_eq!(fps.requested(), &SettingValue::Integer(60));
    assert_eq!(fps.effective(), &SettingValue::Integer(10));
    assert_eq!(fps.constraint_reason(), Some(ConstraintReason::PowerPolicy));
}

#[test]
fn inverted_semantic_thresholds_are_rejected() {
    let mut layers = SettingLayers::default();
    layers.set(
        SettingSource::Application,
        key("visibility.covered-threshold"),
        number(0.9),
    );
    layers.set(
        SettingSource::Application,
        key("visibility.resume-threshold"),
        number(0.95),
    );
    assert!(matches!(
        resolve_settings(&registry(), &layers, &Constraints::default()),
        Err(ResolveError::SemanticViolation { .. })
    ));
    assert!(SettingValue::number(f64::NAN).is_err());
    assert!(SettingValue::number(f64::INFINITY).is_err());
}

#[test]
fn synthetic_migration_preserves_defaults_absence_and_backup() {
    const OLD: &[u8] = include_bytes!("fixtures/synthetic-v1-defaults-and-absent-consent.toml");
    const GOLDEN: &[u8] = include_bytes!("fixtures/synthetic-v2-defaults-and-absent-consent.toml");
    let directory = TestDirectory::new();
    let path = directory.config();
    fs::write(&path, OLD).expect("write synthetic legacy fixture");

    let controller = SettingsController::open(&path, registry()).expect("migrate fixture");
    let migrated = controller.snapshot().expect("migrated snapshot");
    assert_eq!(migrated.revision(), 5);
    assert_eq!(
        fs::read(path.with_file_name("settings.toml.bak")).unwrap(),
        OLD
    );
    assert_eq!(fs::read(&path).unwrap().as_slice(), GOLDEN);
    let golden_path = directory.0.join("golden.toml");
    fs::write(&golden_path, GOLDEN).unwrap();
    let golden = SettingsController::open(&golden_path, registry())
        .unwrap()
        .snapshot()
        .unwrap();
    assert_eq!(migrated.as_ref(), golden.as_ref());
    assert_eq!(
        migrated
            .effective()
            .get_str("privacy.pointer-observation")
            .unwrap()
            .requested(),
        &SettingValue::Boolean(false)
    );
    assert!(
        !migrated
            .overrides()
            .contains_key(&key("privacy.pointer-observation"))
    );
}

#[test]
fn explicit_synthetic_consent_and_overrides_survive_migration_and_restore() {
    const OLD: &[u8] = include_bytes!("fixtures/synthetic-v1-explicit-overrides.toml");
    let directory = TestDirectory::new();
    let path = directory.config();
    fs::write(&path, OLD).unwrap();
    let controller = SettingsController::open(&path, registry()).unwrap();
    let migrated = controller.snapshot().unwrap();
    assert_eq!(migrated.revision(), 7);
    assert_eq!(
        migrated
            .effective()
            .get_str("privacy.pointer-observation")
            .unwrap()
            .requested(),
        &SettingValue::Boolean(true)
    );
    assert_eq!(
        migrated
            .effective()
            .get_str("conversion.gamma")
            .unwrap()
            .requested(),
        &number(1.25)
    );

    let changed = controller
        .apply_batch(
            7,
            &[(key("conversion.gamma"), SettingChange::Set(number(1.5)))],
        )
        .unwrap();
    let restored = controller.restore_backup(changed.revision()).unwrap();
    assert_eq!(restored.revision(), 9);
    assert_eq!(
        restored
            .effective()
            .get_str("conversion.gamma")
            .unwrap()
            .requested(),
        &number(1.25)
    );
}

#[test]
fn batch_validation_is_atomic_and_stale_revision_returns_current_revision() {
    let directory = TestDirectory::new();
    let path = directory.config();
    let controller = SettingsController::open(&path, registry()).unwrap();
    let invalid_batch = [
        (
            key("grid.columns"),
            SettingChange::Set(SettingValue::Integer(300)),
        ),
        (
            key("playback.fps"),
            SettingChange::Set(SettingValue::Integer(0)),
        ),
    ];
    assert!(matches!(
        controller.apply_batch(0, &invalid_batch),
        Err(ConfigError::InvalidPersistentValue {
            violation: TypeViolation::OutOfRange,
            ..
        })
    ));
    assert_eq!(controller.snapshot().unwrap().revision(), 0);
    assert!(!path.exists());

    let first = controller
        .apply_batch(
            0,
            &[(key("conversion.gamma"), SettingChange::Set(number(1.25)))],
        )
        .unwrap();
    assert_eq!(first.revision(), 1);
    assert!(matches!(
        controller.apply_batch(
            0,
            &[(key("conversion.gamma"), SettingChange::Set(number(1.5)))],
        ),
        Err(ConfigError::Conflict {
            expected: 0,
            current: 1
        })
    ));
    assert_eq!(controller.snapshot().unwrap().revision(), 1);
}

#[test]
fn concurrent_same_revision_writes_commit_exactly_once() {
    let directory = TestDirectory::new();
    let controller = Arc::new(SettingsController::open(directory.config(), registry()).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for gamma in [1.25, 1.5] {
        let controller = controller.clone();
        let barrier = barrier.clone();
        workers.push(thread::spawn(move || {
            barrier.wait();
            controller.apply_batch(
                0,
                &[(key("conversion.gamma"), SettingChange::Set(number(gamma)))],
            )
        }));
    }
    barrier.wait();
    let outcomes = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(ConfigError::Conflict { current: 1, .. })))
            .count(),
        1
    );
    assert_eq!(controller.snapshot().unwrap().revision(), 1);
}

#[test]
fn independent_controllers_cannot_overwrite_the_same_revision() {
    let directory = TestDirectory::new();
    let path = directory.config();
    let first = Arc::new(SettingsController::open(&path, registry()).unwrap());
    let second = Arc::new(SettingsController::open(&path, registry()).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let first_worker = {
        let first = first.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            barrier.wait();
            first.apply_batch(
                0,
                &[(key("conversion.gamma"), SettingChange::Set(number(1.25)))],
            )
        })
    };
    let second_worker = {
        let second = second.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            barrier.wait();
            second.apply_batch(
                0,
                &[(key("conversion.gamma"), SettingChange::Set(number(1.5)))],
            )
        })
    };
    barrier.wait();
    let outcomes = [first_worker.join().unwrap(), second_worker.join().unwrap()];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(ConfigError::Conflict { current: 1, .. })))
            .count(),
        1
    );
}

#[test]
fn future_schema_and_unclassifiable_documents_are_read_only() {
    let directory = TestDirectory::new();
    let path = directory.config();
    let future_original = b"schema_version = 99\nrevision = 17\nfuture_field = true\n";
    fs::write(&path, future_original).unwrap();
    let future = SettingsController::open(&path, registry()).unwrap();
    assert!(matches!(
        future.status().unwrap(),
        ControllerStatus::ReadOnlyFutureSchema { schema_version: 99 }
    ));
    assert!(matches!(
        future.apply_batch(17, &[]),
        Err(ConfigError::ReadOnlyFutureSchema { schema_version: 99 })
    ));
    assert!(matches!(
        future.repair(17),
        Err(ConfigError::ReadOnlyFutureSchema { schema_version: 99 })
    ));
    assert!(matches!(
        future.restore_backup(17),
        Err(ConfigError::ReadOnlyFutureSchema { schema_version: 99 })
    ));
    assert_eq!(fs::read(&path).unwrap().as_slice(), &future_original[..]);

    let library_file = directory.0.join("library").join("keep.bin");
    fs::create_dir_all(library_file.parent().unwrap()).unwrap();
    fs::write(&library_file, b"immutable user library data").unwrap();
    let corrupt_path = directory.0.join("corrupt.toml");
    let original = [0xff, 0xfe];
    fs::write(&corrupt_path, original).unwrap();
    let repair = SettingsController::open(&corrupt_path, registry()).unwrap();
    assert!(matches!(
        repair.status().unwrap(),
        ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::Document(
            DocumentError::InvalidUtf8
        ))
    ));
    assert!(matches!(
        repair.repair(0),
        Err(ConfigError::ReadOnlyUnclassifiableDocument)
    ));
    assert!(matches!(
        repair.apply_batch(0, &[]),
        Err(ConfigError::ReadOnlyUnclassifiableDocument)
    ));
    assert!(matches!(
        repair.restore_backup(0),
        Err(ConfigError::ReadOnlyUnclassifiableDocument)
    ));
    assert_eq!(fs::read(&corrupt_path).unwrap().as_slice(), &original[..]);
    assert_eq!(
        fs::read(library_file).unwrap(),
        b"immutable user library data"
    );
}

#[test]
fn explicit_repair_remains_available_for_known_current_schema_data() {
    let directory = TestDirectory::new();
    let path = directory.config();
    let original = b"schema_version = 2\nrevision = 17\n[settings]\n\"grid.columns\" = 0\n";
    fs::write(&path, original).unwrap();
    let controller = SettingsController::open(&path, registry()).unwrap();

    assert!(matches!(
        controller.status().unwrap(),
        ControllerStatus::RepairRequired(ConfigError::InvalidPersistentValue { .. })
    ));
    assert_eq!(controller.snapshot().unwrap().revision(), 17);
    assert_eq!(fs::read(&path).unwrap().as_slice(), &original[..]);
    let repaired = controller.repair(17).unwrap();
    assert_eq!(repaired.revision(), 18);
    assert!(repaired.overrides().is_empty());
    assert_ne!(fs::read(&path).unwrap().as_slice(), &original[..]);
    let reopened = SettingsController::open(&path, registry()).unwrap();
    assert_eq!(reopened.status().unwrap(), ControllerStatus::Ready);
    let persisted = reopened.snapshot().unwrap();
    assert_eq!(persisted.revision(), 18);
    assert!(persisted.overrides().is_empty());
}

#[test]
fn explicit_repair_recovers_recognized_current_schema_decode_errors() {
    type RepairableCase<'a> = (&'a str, &'a [u8], u64, fn(&DocumentError) -> bool);

    let cases: &[RepairableCase<'_>] = &[
        (
            "malformed",
            b"schema_version = 2\nrevision = 17\n[settings]\n\"grid.columns\" = \n",
            17,
            |error| matches!(error, DocumentError::InvalidToml),
        ),
        (
            "missing-revision",
            b"schema_version = 2\n[settings]\n\"grid.columns\" = 240\n",
            0,
            |error| matches!(error, DocumentError::InvalidField("revision")),
        ),
        (
            "invalid-revision",
            b"schema_version = 2\nrevision = \"invalid\"\n[settings]\n\"grid.columns\" = 240\n",
            0,
            |error| matches!(error, DocumentError::InvalidField("revision")),
        ),
        (
            "overflow-revision",
            b"schema_version = 2\nrevision = 9223372036854775808\n[settings]\n\"grid.columns\" = 240\n",
            0,
            |error| matches!(error, DocumentError::InvalidToml),
        ),
    ];
    for (name, original, expected_revision, expected_error) in cases {
        let directory = TestDirectory::new();
        let path = directory.config();
        let backup_path = directory.0.join("settings.toml.bak");
        let backup = b"schema_version = 2\nrevision = 16\n[settings]\n";
        fs::write(&path, original).unwrap();
        fs::write(&backup_path, backup).unwrap();
        let controller = SettingsController::open(&path, registry()).unwrap();

        assert!(
            matches!(
                controller.status().unwrap(),
                ControllerStatus::RepairRequired(ConfigError::Document(error))
                    if expected_error(&error)
            ),
            "{name}"
        );
        assert_eq!(
            controller.snapshot().unwrap().revision(),
            *expected_revision
        );
        assert_eq!(fs::read(&path).unwrap().as_slice(), *original);
        assert_eq!(fs::read(&backup_path).unwrap().as_slice(), backup);

        let repaired = controller.repair(*expected_revision).unwrap();
        assert_eq!(repaired.revision(), *expected_revision + 1);
        assert!(repaired.overrides().is_empty());
        assert_eq!(fs::read(&backup_path).unwrap().as_slice(), backup);
        let archive = fs::read_dir(&directory.0)
            .unwrap()
            .map(Result::unwrap)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.corrupt-")
            })
            .unwrap_or_else(|| panic!("missing {name} recovery archive"));
        assert_eq!(fs::read(archive.path()).unwrap().as_slice(), *original);
    }
}

#[test]
fn unquoted_dotted_setting_path_without_revision_remains_read_only() {
    let directory = TestDirectory::new();
    let path = directory.config();
    let original = b"schema_version = 2\n[settings]\ngrid.columns = \n";
    fs::write(&path, original).unwrap();
    let controller = SettingsController::open(&path, registry()).unwrap();

    assert!(matches!(
        controller.status().unwrap(),
        ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::Document(
            DocumentError::InvalidToml
        ))
    ));
    assert_eq!(controller.snapshot().unwrap().revision(), 0);
    assert!(matches!(
        controller.repair(0),
        Err(ConfigError::ReadOnlyUnclassifiableDocument)
    ));
    assert_eq!(fs::read(&path).unwrap().as_slice(), &original[..]);
}

#[test]
fn unknown_setting_keeps_mixed_invalid_document_read_only() {
    let directory = TestDirectory::new();
    let path = directory.config();
    let original =
        b"schema_version = 2\nrevision = 17\n[settings]\n\"grid.columns\" = 0\n\"z.unknown\" = 1\n";
    fs::write(&path, original).unwrap();
    let controller = SettingsController::open(&path, registry()).unwrap();

    assert!(matches!(
        controller.status().unwrap(),
        ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::UnknownSetting(key))
            if key.as_str() == "z.unknown"
    ));
    assert_eq!(controller.snapshot().unwrap().revision(), 17);
    assert_eq!(fs::read(&path).unwrap().as_slice(), &original[..]);
    assert!(matches!(
        controller.repair(17),
        Err(ConfigError::ReadOnlyUnclassifiableDocument)
    ));
    assert_eq!(fs::read(&path).unwrap().as_slice(), &original[..]);
    let malformed_path = directory.0.join("unknown-malformed.toml");
    let malformed_original = b"schema_version = 2\nrevision = 17\n[settings]\n\"z.unknown\" = \n";
    fs::write(&malformed_path, malformed_original).unwrap();
    let malformed = SettingsController::open(&malformed_path, registry()).unwrap();
    assert!(matches!(
        malformed.status().unwrap(),
        ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::Document(
            DocumentError::InvalidToml
        ))
    ));
    assert!(matches!(
        malformed.repair(17),
        Err(ConfigError::ReadOnlyUnclassifiableDocument)
    ));
    assert_eq!(
        fs::read(&malformed_path).unwrap().as_slice(),
        &malformed_original[..]
    );
}

#[test]
fn unknown_assignment_on_malformed_line_remains_read_only() {
    let directory = TestDirectory::new();
    let path = directory.config();
    let original = b"schema_version = 2\nrevision = 17\n[settings]\n\"grid.columns\" = 240, \"z.unknown\" = 1\n";
    fs::write(&path, original).unwrap();
    let controller = SettingsController::open(&path, registry()).unwrap();

    assert!(matches!(
        controller.status().unwrap(),
        ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::Document(
            DocumentError::InvalidToml
        ))
    ));
    let revision = controller.snapshot().unwrap().revision();
    assert!(matches!(
        controller.repair(revision),
        Err(ConfigError::ReadOnlyUnclassifiableDocument)
    ));
    assert_eq!(fs::read(&path).unwrap().as_slice(), &original[..]);
}

#[test]
fn malformed_document_classes_are_rejected_before_snapshot_publication() {
    type MalformedDocumentCase<'a> = (&'a [u8], fn(&DocumentError) -> bool);

    let cases: &[MalformedDocumentCase<'_>] = &[
        (
            b"schema_version = 2\nrevision = 0\n[settings]\n\"grid.columns\" = 240\n\"grid.columns\" = 241\n",
            |error| matches!(error, DocumentError::DuplicateKey),
        ),
        (
            b"schema_version = 2\nrevision = 0\n[settings]\n\"conversion.gamma\" = nan\n",
            |error| matches!(error, DocumentError::NonFiniteNumber),
        ),
        (
            b"schema_version = 0\nrevision = 0\n[settings]\n",
            |error| matches!(error, DocumentError::UnsupportedVersion(0)),
        ),
    ];
    for (index, (bytes, expected)) in cases.iter().enumerate() {
        let directory = TestDirectory::new();
        let path = directory.0.join(format!("case-{index}.toml"));
        fs::write(&path, bytes).unwrap();
        let controller = SettingsController::open(&path, registry()).unwrap();
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::ReadOnlyUnclassifiableDocument(
                ConfigError::Document(error)
            ) if expected(&error)
        ));
        assert_eq!(controller.snapshot().unwrap().revision(), 0);
    }

    let directory = TestDirectory::new();
    let path = directory.config();
    let mut invalid_utf8 = b"schema_version = 2\nrevision = 0\n[settings]\n".to_vec();
    invalid_utf8.push(0xff);
    fs::write(&path, invalid_utf8).unwrap();
    assert!(matches!(
        read_status(&path),
        ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::Document(
            DocumentError::InvalidUtf8
        ))
    ));
}

#[test]
fn nesting_unknown_keys_overflow_and_session_persistence_are_bounded() {
    let directory = TestDirectory::new();
    let path = directory.config();
    let nested = format!(
        "schema_version = 2\nrevision = 0\n[settings]\n\"grid.columns\" = {}0{}\n",
        "[".repeat(MAX_NESTING_DEPTH + 1),
        "]".repeat(MAX_NESTING_DEPTH + 1)
    );
    fs::write(&path, nested).unwrap();
    assert!(matches!(
        read_status(&path),
        ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::Document(
            DocumentError::ExcessiveNesting { .. }
        ))
    ));

    fs::write(
        &path,
        b"schema_version = 2\nrevision = 0\n[settings]\n\"grid.colums\" = 240\n",
    )
    .unwrap();
    assert!(matches!(
        read_status(&path),
        ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::UnknownSetting(_))
    ));

    let session_controller =
        SettingsController::open(directory.0.join("session.toml"), registry()).unwrap();
    assert!(matches!(
        session_controller.apply_batch(
            0,
            &[(
                key("diagnostics.overlay"),
                SettingChange::Set(SettingValue::Boolean(true))
            )],
        ),
        Err(ConfigError::SessionSettingCannotPersist(_))
    ));

    let oversized = vec![b' '; MAX_CONFIG_BYTES + 1];
    fs::write(&path, oversized).unwrap();
    assert!(matches!(
        read_status(&path),
        ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::Document(
            DocumentError::TooLarge { .. }
        ))
    ));
}

#[test]
fn quoted_table_header_depth_is_preflighted_before_parsing() {
    let directory = TestDirectory::new();
    let path = directory.config();
    let suffix = ".a".repeat(MAX_NESTING_DEPTH);
    let deep_headers = [
        format!("[settings.\"x]\"{suffix}]"),
        format!(r#"[settings."x\"]"{suffix}]"#),
        format!("[settings.'x]'{suffix}]"),
    ];

    for header in deep_headers {
        let document = format!("schema_version = 2\nrevision = 0\n{header}\n");
        fs::write(&path, document).unwrap();
        assert!(matches!(
            read_status(&path),
            ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::Document(
                DocumentError::ExcessiveNesting {
                    limit: MAX_NESTING_DEPTH
                }
            ))
        ));
    }

    for header in [r#"["extra]"]"#, "['extra]']"] {
        let document = format!("schema_version = 2\nrevision = 0\n{header}\n");
        fs::write(&path, document).unwrap();
        assert!(matches!(
            read_status(&path),
            ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::Document(
                DocumentError::UnknownRootField(field)
            )) if field == "extra]"
        ));
    }
}

#[test]
fn oversized_registered_string_is_rejected_without_publishing_snapshot() {
    let directory = TestDirectory::new();
    let path = directory.config();
    let max_chars = MAX_CONFIG_BYTES + 1;
    let setting_key = key("application.label");
    let registry = Arc::new(
        Registry::new([descriptor(
            "application.label",
            SettingValue::text("default"),
            SettingType::String { max_chars },
            SettingScope::A,
            None,
            InvalidationClass::Controller,
            SemanticValidator::None,
        )])
        .unwrap(),
    );
    let original =
        b"schema_version = 2\nrevision = 5\n[settings]\n\"application.label\" = \"previous\"\n";
    fs::write(&path, original).unwrap();
    let controller = SettingsController::open(&path, registry).unwrap();
    let before = controller.snapshot().unwrap();
    let changes = [(
        setting_key,
        SettingChange::Set(SettingValue::text("x".repeat(max_chars))),
    )];
    let original_changes = changes.clone();

    assert!(matches!(
        controller.apply_batch(before.revision(), &changes),
        Err(ConfigError::Document(DocumentError::TooLarge {
            limit: MAX_CONFIG_BYTES
        }))
    ));
    assert_eq!(controller.snapshot().unwrap().as_ref(), before.as_ref());
    assert_eq!(changes, original_changes);
    assert_eq!(fs::read(path).unwrap().as_slice(), &original[..]);
}
