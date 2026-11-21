use ascii_runtime::storage::{
    AtomicWriteStage, DiagnosticLog, DirectoryInputs, ExportMode, ImportJournal, InstallEntry,
    InstallRegistry, JournalRecord, JournalState, LogLimits, RecoveryAction, Storage, StorageError,
    StorageRoots, ValidatedConfig, validate_application_id,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ascii-runtime-storage-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temporary test directory");
        Self(path)
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

fn linux_roots(base: &Path) -> StorageRoots {
    StorageRoots::resolve(
        "org.astraive.asciiwall",
        &DirectoryInputs::Linux {
            home: base.join("home"),
            config_home: Some(base.join("xdg-config")),
            data_home: Some(base.join("xdg-data")),
            cache_home: Some(base.join("xdg-cache")),
            runtime_dir: Some(base.join("xdg-runtime")),
        },
    )
    .expect("valid Linux roots")
}

fn validate_test_config(value: &Value) -> Result<(), &'static str> {
    value
        .is_object()
        .then_some(())
        .ok_or("configuration must be an object")
}

fn validate_theme_config(value: &Value) -> Result<(), &'static str> {
    validate_test_config(value)?;
    value
        .get("theme")
        .and_then(Value::as_str)
        .map(|_| ())
        .ok_or("theme must be text")
}

fn config(value: Value) -> ValidatedConfig {
    let bytes = serde_json::to_vec(&value).expect("serialize test config");
    ValidatedConfig::from_json(&bytes, validate_test_config).expect("valid test config")
}

fn registry(version: &str) -> InstallRegistry {
    let digest = "a".repeat(64);
    InstallRegistry {
        schema_version: 1,
        packages: [(
            "org.example.sample".to_owned(),
            InstallEntry {
                package_version: version.to_owned(),
                sha256: digest.clone(),
                relative_path: format!("packages/org.example.sample/{version}/{digest}"),
            },
        )]
        .into_iter()
        .collect(),
    }
}

fn injected_failure(
    stage_to_fail: AtomicWriteStage,
) -> impl FnMut(AtomicWriteStage) -> io::Result<()> {
    move |stage| {
        if stage == stage_to_fail {
            Err(io::Error::other("injected interruption"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn platform_directory_inputs_map_to_distinct_storage_classes() {
    let temp = TempDir::new();
    let home = temp.path().join("home");
    let fallback = StorageRoots::resolve(
        "org.example.wall",
        &DirectoryInputs::Linux {
            home: home.clone(),
            config_home: None,
            data_home: None,
            cache_home: None,
            runtime_dir: None,
        },
    )
    .expect("Linux XDG fallbacks");
    assert_eq!(fallback.config, home.join(".config/org.example.wall"));
    assert_eq!(fallback.data, home.join(".local/share/org.example.wall"));
    assert_eq!(fallback.cache, home.join(".cache/org.example.wall"));
    assert_eq!(fallback.runtime, fallback.data.join("runtime"));

    let linux = linux_roots(temp.path());
    assert_eq!(
        linux.config,
        temp.path().join("xdg-config/org.astraive.asciiwall")
    );
    assert_eq!(
        linux.data,
        temp.path().join("xdg-data/org.astraive.asciiwall")
    );
    assert_eq!(
        linux.cache,
        temp.path().join("xdg-cache/org.astraive.asciiwall")
    );
    assert_eq!(
        linux.runtime,
        temp.path().join("xdg-runtime/org.astraive.asciiwall")
    );

    let mac_home = temp.path().join("mac-home");
    let mac = StorageRoots::resolve(
        "org.example.wall",
        &DirectoryInputs::MacOs {
            home: mac_home.clone(),
        },
    )
    .expect("macOS roots");
    assert_eq!(
        mac.config,
        mac_home.join("Library/Application Support/org.example.wall/config")
    );
    assert_eq!(
        mac.data,
        mac_home.join("Library/Application Support/org.example.wall")
    );
    assert_eq!(mac.cache, mac_home.join("Library/Caches/org.example.wall"));
    assert_eq!(mac.runtime, mac.data.join("runtime"));

    let roaming = temp.path().join("windows-roaming");
    let local = temp.path().join("windows-local");
    let windows = StorageRoots::resolve(
        "org.example.wall",
        &DirectoryInputs::Windows {
            roaming_app_data: roaming.clone(),
            local_app_data: local.clone(),
        },
    )
    .expect("Windows roots");
    assert_eq!(windows.config, roaming.join("org.example.wall"));
    assert_eq!(windows.data, local.join("org.example.wall"));
    assert_eq!(windows.cache, local.join("org.example.wall/cache"));
    assert_eq!(windows.runtime, local.join("org.example.wall/runtime"));

    let shared = temp.path().join("shared-xdg");
    let coalesced = StorageRoots::resolve(
        "org.example.wall",
        &DirectoryInputs::Linux {
            home: home.clone(),
            config_home: Some(shared.clone()),
            data_home: Some(shared.clone()),
            cache_home: Some(shared.clone()),
            runtime_dir: Some(shared.clone()),
        },
    )
    .expect("overlapping XDG bases are disambiguated");
    assert_eq!(coalesced.config, shared.join("org.example.wall/config"));
    assert_eq!(coalesced.data, shared.join("org.example.wall"));
    assert_eq!(coalesced.cache, shared.join("org.example.wall/cache"));
    assert_eq!(coalesced.runtime, shared.join("org.example.wall/runtime"));

    for roots in [&fallback, &linux, &mac, &windows, &coalesced] {
        let unique: BTreeSet<_> = [
            &roots.config,
            &roots.data,
            &roots.library,
            &roots.projects,
            &roots.catalog,
            &roots.cache,
            &roots.staging,
            &roots.logs,
            &roots.runtime,
        ]
        .into_iter()
        .collect();
        assert_eq!(unique.len(), 9, "all storage classes have distinct roots");
    }
    linux.create_all().expect("create resolved directories");
    for path in [
        &linux.config,
        &linux.data,
        &linux.library,
        &linux.projects,
        &linux.catalog,
        &linux.cache,
        &linux.staging,
        &linux.logs,
        &linux.runtime,
    ] {
        assert!(path.is_dir(), "missing storage directory {path:?}");
    }
}

#[test]
fn root_resolution_separates_lexical_and_symlink_aliases() {
    let temp = TempDir::new();
    let app_id = "org.example.wall";
    let state = temp.path().join("state");
    fs::create_dir_all(&state).expect("state base");
    let lexical_alias = state.join("..").join("state");
    let config_home = temp.path().join("config-home");
    let roots = StorageRoots::resolve(
        app_id,
        &DirectoryInputs::Linux {
            home: temp.path().join("home"),
            config_home: Some(config_home.clone()),
            data_home: Some(state.clone()),
            cache_home: Some(lexical_alias.clone()),
            runtime_dir: Some(temp.path().join("runtime-home")),
        },
    )
    .expect("lexically aliased cache root is separated");
    assert_eq!(roots.cache, lexical_alias.join(app_id).join("cache"));
    assert_ne!(roots.cache, roots.data);
    assert!(
        !config_home.exists(),
        "resolving roots does not create directories"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let symlink_alias = temp.path().join("state-link");
        symlink(&state, &symlink_alias).expect("create storage-root symlink");
        let symlink_roots = StorageRoots::resolve(
            app_id,
            &DirectoryInputs::Linux {
                home: temp.path().join("home"),
                config_home: Some(temp.path().join("symlink-config")),
                data_home: Some(state.clone()),
                cache_home: Some(symlink_alias.clone()),
                runtime_dir: Some(temp.path().join("symlink-runtime")),
            },
        )
        .expect("symlink-aliased cache root is separated");
        assert_eq!(
            symlink_roots.cache,
            symlink_alias.join(app_id).join("cache")
        );
        assert_ne!(symlink_roots.cache, symlink_roots.data);

        let durable_root = state.join(app_id);
        fs::create_dir_all(durable_root.join("library")).expect("create durable library root");
        symlink(durable_root.join("library"), durable_root.join("projects"))
            .expect("create durable-root alias");
        let durable_alias = StorageRoots::resolve(
            app_id,
            &DirectoryInputs::Linux {
                home: temp.path().join("home"),
                config_home: Some(temp.path().join("durable-config")),
                data_home: Some(state.clone()),
                cache_home: Some(temp.path().join("durable-cache")),
                runtime_dir: Some(temp.path().join("durable-runtime")),
            },
        );
        assert!(matches!(
            durable_alias,
            Err(StorageError::InvalidDirectories(_))
        ));
    }
}

#[cfg(windows)]
#[test]
fn windows_roots_separate_case_aliases_when_final_directories_are_missing() {
    let temp = TempDir::new();
    let roaming = temp.path().join("State");
    let local = temp.path().join("state");
    assert!(!roaming.exists());
    assert!(!local.exists());

    let app_id = "org.example.wall";
    let roots = StorageRoots::resolve(
        app_id,
        &DirectoryInputs::Windows {
            roaming_app_data: roaming.clone(),
            local_app_data: local.clone(),
        },
    )
    .expect("case-aliased Windows roots are separated");
    assert_eq!(roots.config, roaming.join(app_id).join("config"));
    assert_eq!(roots.data, local.join(app_id));
    assert!(
        !roaming.exists(),
        "resolution does not create Windows roots"
    );
}

#[cfg(windows)]
#[test]
fn windows_filesystem_atomic_save_recovery_smoke() {
    let temp = TempDir::new();
    let roots = StorageRoots::resolve(
        "org.astraive.asciiwall",
        &DirectoryInputs::Windows {
            roaming_app_data: temp.path().join("roaming-app-data"),
            local_app_data: temp.path().join("local-app-data"),
        },
    )
    .expect("resolve native Windows roots");
    let storage = Storage::new(roots);

    let first = config(json!({"theme": "windows-first"}));
    storage
        .save_config(&first, validate_test_config)
        .expect("write config on native Windows filesystem");
    let second = config(json!({"theme": "windows-second"}));
    storage
        .save_config(&second, validate_test_config)
        .expect("replace config and establish Windows last-good backup");
    let replacement = config(json!({"theme": "windows-replacement"}));
    assert!(matches!(
        storage.save_config_with_hook(
            &replacement,
            validate_test_config,
            injected_failure(AtomicWriteStage::TargetReplaced)
        ),
        Err(StorageError::Io(_))
    ));
    assert_eq!(
        ValidatedConfig::from_json(
            &fs::read(storage.config_path()).expect("read replaced Windows config"),
            validate_test_config,
        )
        .expect("replacement config remains valid")
        .value(),
        replacement.value()
    );
    assert_eq!(
        ValidatedConfig::from_json(
            &fs::read(storage.config_backup_path()).expect("read Windows last-good backup"),
            validate_test_config,
        )
        .expect("last-good config remains valid")
        .value(),
        second.value()
    );
    let subsequent = config(json!({"theme": "windows-subsequent"}));
    storage
        .save_config(&subsequent, validate_test_config)
        .expect("subsequent Windows config write succeeds");
    assert_eq!(
        storage
            .load_config(validate_test_config)
            .expect("load Windows config")
            .expect("Windows config exists")
            .value(),
        subsequent.value()
    );

    let original_registry = registry("1.0.0");
    storage
        .save_registry(&original_registry)
        .expect("write registry on native Windows filesystem");
    let replacement_registry = registry("2.0.0");
    assert!(matches!(
        storage.save_registry_with_hook(
            &replacement_registry,
            injected_failure(AtomicWriteStage::TargetReplaced)
        ),
        Err(StorageError::Io(_))
    ));
    assert_eq!(
        storage
            .load_registry()
            .expect("load replaced Windows registry"),
        replacement_registry
    );
    storage
        .save_registry(&original_registry)
        .expect("subsequent Windows registry write succeeds");
    assert_eq!(
        storage
            .load_registry()
            .expect("load recovered Windows registry"),
        original_registry
    );
}

#[test]
fn application_identifiers_reject_path_and_windows_device_names() {
    for valid in ["org.astraive.asciiwall", "ascii-wall_2", "a"] {
        validate_application_id(valid).expect("valid identifier");
    }
    for invalid in [
        "",
        ".",
        "..",
        "../escape",
        "a/b",
        "a\\b",
        "CON",
        "con.plugins",
        "NUL",
    ] {
        assert!(
            validate_application_id(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
}

#[test]
fn registry_is_versioned_atomic_and_failed_replacement_preserves_old_state() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let path = storage.registry_path();
    let parent = path.parent().expect("registry parent");
    fs::create_dir_all(parent).expect("create registry parent");
    let stale_temp = parent.join(".ascii-runtime-install-registry.json.state.tmp");
    let unrelated_temp = parent.join("unowned.tmp");
    fs::write(&stale_temp, b"interrupted temporary write").expect("write stale temporary");
    fs::write(&unrelated_temp, b"unowned data").expect("write unrelated file");

    let previous = registry("1.0.0");
    storage
        .save_registry(&previous)
        .expect("write first registry");
    assert!(!stale_temp.exists(), "retry removes its stale temporary");
    assert_eq!(
        fs::read(&unrelated_temp)
            .expect("preserve unrelated file")
            .as_slice(),
        &b"unowned data"[..]
    );
    let original = fs::read(&path).expect("registry bytes");
    assert_eq!(storage.load_registry().expect("read registry"), previous);

    let changed = registry("2.0.0");
    for stage in [
        AtomicWriteStage::TemporaryCreated,
        AtomicWriteStage::TemporarySynced,
        AtomicWriteStage::TargetReplaced,
    ] {
        let error = storage
            .save_registry_with_hook(&changed, injected_failure(stage))
            .expect_err("injected interruption");
        assert!(matches!(error, StorageError::Io(_)));
        let expected = if stage == AtomicWriteStage::TargetReplaced {
            &changed
        } else {
            &previous
        };
        assert_eq!(
            storage.load_registry().expect("recover complete registry"),
            *expected
        );
        assert!(
            !parent
                .join(".ascii-runtime-install-registry.json.state.tmp")
                .exists(),
            "failed write removes its owned temporary"
        );
        storage
            .save_registry(&previous)
            .expect("valid write succeeds after injected failure");
        assert_eq!(
            storage.load_registry().expect("subsequent registry write"),
            previous
        );
    }
    assert_eq!(
        fs::read(&path).expect("restored original registry"),
        original
    );

    let mut invalid = changed.clone();
    invalid
        .packages
        .get_mut("org.example.sample")
        .unwrap()
        .sha256 = "not-a-digest".to_owned();
    assert!(storage.save_registry(&invalid).is_err());
    assert_eq!(
        fs::read(&path).expect("invalid registry did not replace old"),
        original
    );

    storage.save_registry(&changed).expect("replace registry");
    assert_eq!(
        storage.load_registry().expect("new registry is complete"),
        changed
    );
    let bytes = fs::read(path).expect("versioned registry JSON");
    let parsed: Value = serde_json::from_slice(&bytes).expect("valid registry JSON");
    assert_eq!(parsed["schema_version"].as_u64(), Some(1));
}

#[test]
fn configuration_validation_backup_and_interrupted_replacement_keep_valid_state() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let first = config(json!({"theme": "dark", "grid": {"width": 80}}));
    storage
        .save_config(&first, validate_test_config)
        .expect("save initial config");
    let config_path = storage.config_path();
    let backup_path = storage.config_backup_path();
    let original = fs::read(&config_path).expect("initial config bytes");

    let second = config(json!({"theme": "light", "grid": {"width": 100}}));
    assert!(
        storage
            .save_config_with_hook(
                &second,
                validate_test_config,
                injected_failure(AtomicWriteStage::TemporarySynced)
            )
            .is_err()
    );
    assert_eq!(
        fs::read(&config_path).expect("previous state survives"),
        original
    );

    let invalid_history = br#"{"cursor_history":[1,2]}"#;
    assert!(ValidatedConfig::from_json(invalid_history, |_| Ok::<(), &'static str>(())).is_err());
    let invalid_window_history = br#"{"window_geometry_history":[{"x":1}]}"#;
    assert!(
        ValidatedConfig::from_json(invalid_window_history, |_| Ok::<(), &'static str>(())).is_err()
    );
    let invalid_schema = br#"{"unexpected":true}"#;
    assert!(ValidatedConfig::from_json(invalid_schema, |_| Err("unknown field")).is_err());

    storage
        .save_config(&second, validate_test_config)
        .expect("replace config");
    assert_eq!(fs::read(&backup_path).expect("last-good backup"), original);
    assert_eq!(
        storage
            .load_config(|value| value.is_object().then_some(()).ok_or("object required"))
            .expect("load validated config")
            .unwrap()
            .value(),
        second.value()
    );

    let previous_second = fs::read(&config_path).expect("second config bytes");
    let third = config(json!({"theme": "system"}));
    assert!(
        storage
            .save_config_with_hook(
                &third,
                validate_test_config,
                injected_failure(AtomicWriteStage::TargetReplaced)
            )
            .is_err()
    );
    assert_eq!(
        fs::read(&config_path).expect("new state survives post-replacement interruption"),
        serde_json::to_vec_pretty(third.value())
            .unwrap()
            .into_iter()
            .chain(b"\n".iter().copied())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        fs::read(&backup_path).expect("last-good remains previous valid state"),
        previous_second
    );
}

#[test]
fn configuration_failures_at_each_atomic_boundary_preserve_valid_recovery() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let config_path = storage.config_path();
    let backup_path = storage.config_backup_path();
    let seed = config(json!({"theme": "seed"}));
    storage
        .save_config(&seed, validate_test_config)
        .expect("save initial config");

    for (index, stage) in [
        AtomicWriteStage::TemporaryCreated,
        AtomicWriteStage::TemporarySynced,
        AtomicWriteStage::BackupReplaced,
        AtomicWriteStage::TargetReplaced,
    ]
    .into_iter()
    .enumerate()
    {
        let baseline = config(json!({"theme": format!("baseline-{index}")}));
        storage
            .save_config(&baseline, validate_test_config)
            .expect("establish valid current and backup before injected failure");
        let baseline_bytes = fs::read(&config_path).expect("baseline current bytes");
        let previous_backup_bytes = fs::read(&backup_path).expect("previous last-good bytes");
        let candidate = config(json!({"theme": format!("candidate-{index}")}));

        let error = storage
            .save_config_with_hook(&candidate, validate_test_config, injected_failure(stage))
            .expect_err("injected interruption");
        assert!(matches!(error, StorageError::Io(_)));

        let expected_current = if stage == AtomicWriteStage::TargetReplaced {
            candidate.value()
        } else {
            baseline.value()
        };
        let current_bytes = fs::read(&config_path).expect("current config remains complete");
        let recovered_current = ValidatedConfig::from_json(&current_bytes, validate_test_config)
            .expect("current state remains valid");
        assert_eq!(recovered_current.value(), expected_current);

        let expected_backup = if matches!(
            stage,
            AtomicWriteStage::BackupReplaced | AtomicWriteStage::TargetReplaced
        ) {
            &baseline_bytes
        } else {
            &previous_backup_bytes
        };
        let backup_bytes = fs::read(&backup_path).expect("last-good backup remains present");
        assert_eq!(&backup_bytes, expected_backup);
        let recovered_backup = ValidatedConfig::from_json(&backup_bytes, validate_test_config)
            .expect("last-good state remains valid");
        assert!(recovered_backup.value().is_object());

        let parent = config_path.parent().expect("configuration directory");
        assert!(
            !parent
                .join(".ascii-runtime-settings.json.state.tmp")
                .exists(),
            "failed config write removes its owned temporary"
        );
        assert!(
            !parent
                .join(".ascii-runtime-settings.json.last-good.state.tmp")
                .exists(),
            "failed backup write leaves no owned temporary"
        );

        let subsequent = config(json!({"theme": format!("subsequent-{index}")}));
        storage
            .save_config(&subsequent, validate_test_config)
            .expect("valid config write succeeds after injected failure");
        assert_eq!(
            storage
                .load_config(validate_test_config)
                .expect("load subsequent config")
                .expect("subsequent config exists")
                .value(),
            subsequent.value()
        );
        let replaced_backup = ValidatedConfig::from_json(
            &fs::read(&backup_path).expect("subsequent write records last-good"),
            validate_test_config,
        )
        .expect("subsequent write preserves valid state");
        assert_eq!(replaced_backup.value(), expected_current);
    }
}

#[test]
fn saving_config_revalidates_against_the_active_schema_before_replacement() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let original = config(json!({"theme": "dark"}));
    storage
        .save_config(&original, validate_theme_config)
        .expect("save initial schema-valid config");
    let original_bytes = fs::read(storage.config_path()).expect("read initial config");
    let validated_by_weaker_schema = config(json!({"theme": 42}));

    assert!(matches!(
        storage.save_config(&validated_by_weaker_schema, validate_theme_config),
        Err(StorageError::InvalidConfiguration(_))
    ));
    assert_eq!(
        fs::read(storage.config_path()).expect("active config remains unchanged"),
        original_bytes
    );
}

#[test]
fn invalid_config_does_not_replace_last_good_backup_on_interruption() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let known_good = config(json!({"theme": "known-good"}));
    storage
        .save_config(&known_good, validate_theme_config)
        .expect("save known-good config");
    let known_good_bytes = fs::read(storage.config_path()).expect("read known-good config");

    storage
        .save_config(&config(json!({"theme": "current"})), validate_theme_config)
        .expect("save current config");
    let backup_path = storage.config_backup_path();
    assert_eq!(
        fs::read(&backup_path).expect("known-good backup exists"),
        known_good_bytes
    );
    fs::write(storage.config_path(), b"{malformed").expect("corrupt current config");

    let replacement = config(json!({"theme": "replacement"}));
    assert!(
        storage
            .save_config_with_hook(
                &replacement,
                validate_theme_config,
                injected_failure(AtomicWriteStage::TargetReplaced)
            )
            .is_err()
    );
    let backup = fs::read(&backup_path).expect("preserve known-good backup");
    assert_eq!(backup, known_good_bytes);
    fs::write(storage.config_path(), br#"{"theme":42}"#)
        .expect("write schema-invalid current config");
    let schema_replacement = config(json!({"theme": "schema-replacement"}));
    assert!(
        storage
            .save_config_with_hook(
                &schema_replacement,
                validate_theme_config,
                injected_failure(AtomicWriteStage::TargetReplaced),
            )
            .is_err()
    );
    assert_eq!(
        fs::read(&backup_path).expect("schema-invalid config cannot replace backup"),
        known_good_bytes
    );
    let parsed_backup = ValidatedConfig::from_json(&backup, validate_theme_config)
        .expect("prior backup remains a valid config");
    assert_eq!(parsed_backup.value(), known_good.value());
}

#[test]
fn importer_journal_persists_all_states_and_classifies_restart_recovery() {
    let temp = TempDir::new();
    let roots = linux_roots(temp.path());
    let journal = ImportJournal::new(roots.staging.clone());
    let staged = journal
        .begin("tx-staged", "org.example.package", "imports/tx-staged")
        .expect("start staged transaction");
    assert_eq!(staged.state, JournalState::Staged);
    assert_eq!(
        journal.recover("tx-staged").unwrap(),
        Some(RecoveryAction::ValidateStaged)
    );

    let restarted = ImportJournal::new(roots.staging.clone());
    assert_eq!(
        restarted.load("tx-staged").unwrap().unwrap().state,
        JournalState::Staged
    );
    let verified = restarted
        .transition("tx-staged", JournalState::Verified)
        .expect("verified transition");
    assert_eq!(verified.state, JournalState::Verified);
    let restarted = ImportJournal::new(roots.staging.clone());
    assert_eq!(
        restarted.recover("tx-staged").unwrap(),
        Some(RecoveryAction::CommitVerified)
    );
    let committed = restarted
        .transition("tx-staged", JournalState::Committed)
        .expect("committed transition");
    assert_eq!(committed.state, JournalState::Committed);
    assert_eq!(
        ImportJournal::new(roots.staging.clone())
            .recover("tx-staged")
            .unwrap(),
        Some(RecoveryAction::AlreadyCommitted)
    );
    assert!(
        restarted
            .transition("tx-staged", JournalState::Abandoned)
            .is_err()
    );

    let abandoned = restarted
        .begin(
            "tx-abandoned",
            "org.example.package",
            "imports/tx-abandoned",
        )
        .expect("start second transaction");
    assert_eq!(abandoned.state, JournalState::Staged);
    let abandoned = restarted
        .transition("tx-abandoned", JournalState::Abandoned)
        .expect("abandon staged transaction");
    assert_eq!(abandoned.state, JournalState::Abandoned);
    assert_eq!(
        ImportJournal::new(roots.staging)
            .recover("tx-abandoned")
            .unwrap(),
        Some(RecoveryAction::CleanupAbandoned)
    );
}

#[test]
fn concurrent_journal_begins_and_transitions_are_serialized() {
    let temp = TempDir::new();
    let staging = temp.path().join("staging");
    let journal = ImportJournal::new(staging.clone());
    let barrier = Arc::new(Barrier::new(2));
    let begins: Vec<_> = ["imports/first", "imports/second"]
        .into_iter()
        .map(|staging_path| {
            let journal = ImportJournal::new(staging.clone());
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                journal.begin("shared", "org.example.package", staging_path)
            })
        })
        .collect();
    let begin_results: Vec<_> = begins
        .into_iter()
        .map(|begin| begin.join().expect("begin thread"))
        .collect();
    assert_eq!(
        begin_results.iter().filter(|result| result.is_ok()).count(),
        1,
        "only one concurrent begin may create the transaction"
    );
    assert_eq!(
        journal.load("shared").unwrap().unwrap().state,
        JournalState::Staged
    );
    journal
        .transition("shared", JournalState::Verified)
        .expect("verify shared transaction");

    let barrier = Arc::new(Barrier::new(2));
    let transitions: Vec<_> = [JournalState::Committed, JournalState::Abandoned]
        .into_iter()
        .map(|next| {
            let journal = ImportJournal::new(staging.clone());
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                journal.transition("shared", next)
            })
        })
        .collect();
    let transition_results: Vec<_> = transitions
        .into_iter()
        .map(|transition| transition.join().expect("transition thread"))
        .collect();
    assert_eq!(
        transition_results
            .iter()
            .filter(|result| result.is_ok())
            .count(),
        1,
        "only one competing terminal transition may succeed"
    );
    assert!(matches!(
        journal.load("shared").unwrap().unwrap().state,
        JournalState::Committed | JournalState::Abandoned
    ));
}

#[test]
fn authoritative_documents_enforce_save_and_read_size_limits() {
    const REGISTRY_MAX_BYTES: usize = 16 * 1024 * 1024;
    const CONFIG_MAX_BYTES: usize = 8 * 1024 * 1024;
    const JOURNAL_MAX_BYTES: usize = 1024 * 1024;

    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let original_registry = registry("1.0.0");
    storage
        .save_registry(&original_registry)
        .expect("save valid registry");
    let registry_path = storage.registry_path();
    let original_registry_bytes = fs::read(&registry_path).expect("read original registry");
    let mut oversized_registry = registry("2.0.0");
    oversized_registry
        .packages
        .get_mut("org.example.sample")
        .unwrap()
        .relative_path = "x".repeat(REGISTRY_MAX_BYTES + 1);
    assert!(matches!(
        storage.save_registry(&oversized_registry),
        Err(StorageError::DocumentTooLarge)
    ));
    assert_eq!(
        fs::read(&registry_path).expect("oversized save preserves registry"),
        original_registry_bytes
    );
    fs::write(&registry_path, vec![b' '; REGISTRY_MAX_BYTES + 1])
        .expect("write oversized registry");
    assert!(matches!(
        storage.load_registry(),
        Err(StorageError::DocumentTooLarge)
    ));

    let original_config = config(json!({"theme": "dark"}));
    storage
        .save_config(&original_config, validate_test_config)
        .expect("save valid config");
    let config_path = storage.config_path();
    let original_config_bytes = fs::read(&config_path).expect("read original config");
    let large_value = json!({"payload": "x".repeat(CONFIG_MAX_BYTES - 15)});
    let compact_config = serde_json::to_vec(&large_value).expect("serialize compact config");
    assert!(compact_config.len() <= CONFIG_MAX_BYTES);
    let oversized_config = ValidatedConfig::from_json(&compact_config, |value| {
        value.is_object().then_some(()).ok_or("object required")
    })
    .expect("compact config remains within its read limit");
    assert!(matches!(
        storage.save_config(&oversized_config, validate_test_config),
        Err(StorageError::DocumentTooLarge)
    ));
    assert_eq!(
        fs::read(&config_path).expect("oversized save preserves config"),
        original_config_bytes
    );
    let recovery_config = config(json!({"theme": "recovery"}));
    storage
        .save_config(&recovery_config, validate_test_config)
        .expect("establish a last-good config before oversized-current recovery");
    let backup_path = storage.config_backup_path();
    let known_good_backup = fs::read(&backup_path).expect("retain last-good backup");
    fs::write(&config_path, vec![b' '; CONFIG_MAX_BYTES + 1]).expect("write oversized config");
    assert!(matches!(
        storage.load_config(|_| Ok::<(), &'static str>(())),
        Err(StorageError::DocumentTooLarge)
    ));
    let replacement = config(json!({"theme": "recovered"}));
    storage
        .save_config(&replacement, validate_test_config)
        .expect("replace oversized current config");
    assert_eq!(
        storage
            .load_config(validate_test_config)
            .unwrap()
            .unwrap()
            .value(),
        replacement.value()
    );
    assert_eq!(
        fs::read(backup_path).expect("oversized current did not replace last-good"),
        known_good_backup
    );

    let staging_root = temp.path().join("journal-staging");
    let journal = ImportJournal::new(staging_root.clone());
    let oversized_path = "x".repeat(JOURNAL_MAX_BYTES + 1);
    assert!(matches!(
        journal.begin("too-large", "org.example.package", &oversized_path),
        Err(StorageError::DocumentTooLarge)
    ));
    let journal_directory = staging_root.join("import-journal");
    assert!(
        !journal_directory.exists(),
        "oversized input created the journal directory"
    );
    let boundary_record_size_without_path = serde_json::to_vec_pretty(&JournalRecord {
        schema_version: 1,
        transaction_id: "boundary".to_owned(),
        package_id: "org.example.package".to_owned(),
        staging_path: String::new(),
        state: JournalState::Staged,
    })
    .expect("serialize empty-path journal record")
    .len()
        + 1;
    let boundary_path = "x".repeat(JOURNAL_MAX_BYTES - boundary_record_size_without_path);
    journal
        .begin("boundary", "org.example.package", &boundary_path)
        .expect("a journal serialized exactly at its byte limit is valid");
    assert_eq!(
        usize::try_from(
            fs::metadata(journal_directory.join("import-boundary.json"))
                .expect("read boundary journal metadata")
                .len()
        )
        .expect("journal size fits usize"),
        JOURNAL_MAX_BYTES
    );

    journal
        .begin("preserved", "org.example.package", "imports/preserved")
        .expect("save valid journal");
    assert!(journal.load("preserved").unwrap().is_some());
    let oversized_journal_path = journal_directory.join("import-too-large.json");
    assert!(!oversized_journal_path.exists());
    fs::write(&oversized_journal_path, vec![b' '; JOURNAL_MAX_BYTES + 1])
        .expect("write oversized journal");
    assert!(matches!(
        journal.load("too-large"),
        Err(StorageError::DocumentTooLarge)
    ));
}

#[test]
fn diagnostic_log_rotates_within_bounds_and_redacts_local_media_paths() {
    let temp = TempDir::new();
    let home = temp.path().join("home");
    let log = DiagnosticLog::new(
        temp.path().join("logs"),
        home.clone(),
        LogLimits::new(96, 3, 80).expect("valid bounds"),
    );
    log.append(&format!(
        "decode failed for {}/Documents/secret-frame.png and /tmp/private-clip.mov",
        home.display()
    ))
    .expect("append redacted event");
    let redacted_media = fs::read_to_string(log.current_path()).expect("redacted media record");
    assert!(!redacted_media.contains("secret-frame.png"));
    assert!(!redacted_media.contains("private-clip.mov"));
    log.append(&format!("configuration opened under {}", home.display()))
        .expect("append home-path event");
    let redacted_home = fs::read_to_string(log.current_path()).expect("redacted home record");
    assert!(!redacted_home.contains(&home.to_string_lossy().to_string()));
    log.append(&"x".repeat(300))
        .expect("append oversized record");
    let current = fs::read(log.current_path()).expect("read bounded current log");
    assert!(current.len() <= 96, "current log obeys its per-file bound");
    let current_text = std::str::from_utf8(&current).expect("UTF-8 current log");
    assert!(
        current_text.lines().all(|record| record.len() <= 80),
        "each record obeys its individual bound"
    );
    for index in 0..12 {
        log.append(&format!("event {index}: decode completed"))
            .expect("append event");
    }

    let directory = temp.path().join("logs");
    let mut count = 0;
    let mut contents = String::new();
    for entry in fs::read_dir(&directory).expect("list rotated logs") {
        let entry = entry.expect("log entry");
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let retained_log = name == "runtime.log"
            || name
                .strip_prefix("runtime.log.")
                .and_then(|suffix| suffix.parse::<usize>().ok())
                .is_some_and(|index| index > 0 && index < 3);
        if !retained_log {
            continue;
        }
        let metadata = entry.metadata().expect("log metadata");
        assert!(
            metadata.len() <= 96,
            "log exceeded configured per-file bound"
        );
        contents.push_str(&fs::read_to_string(entry.path()).expect("UTF-8 log file"));
        count += 1;
    }
    assert!(count <= 3, "rotation retained too many files");
    assert!(!contents.contains(&home.to_string_lossy().to_string()));
    assert!(!contents.contains("secret-frame.png"));
    assert!(!contents.contains("private-clip.mov"));
}

#[test]
fn huge_diagnostic_input_is_bounded_before_redaction_and_stays_private_utf8() {
    let temp = TempDir::new();
    let home = temp.path().join("home");
    let log = DiagnosticLog::new(
        temp.path().join("logs"),
        home,
        LogLimits::new(256, 2, 128).expect("valid bounds"),
    );
    let message = format!("path={}", "界".repeat(1024 * 1024));
    log.append(&message)
        .expect("append bounded diagnostic event");

    let bytes = fs::read(log.current_path()).expect("read diagnostic record");
    assert!(bytes.len() <= 128);
    assert_eq!(bytes.last(), Some(&b'\n'));
    let record = std::str::from_utf8(&bytes).expect("record remains valid UTF-8");
    assert!(!record.contains("path="));
    assert!(!record.contains('界'));
    assert!(!record.contains('/'));
}

#[test]
fn diagnostic_log_redacts_partial_filename_before_trailing_bound_space() {
    let temp = TempDir::new();
    let log = DiagnosticLog::new(
        temp.path().join("short-logs"),
        temp.path().join("home"),
        LogLimits::new(12, 2, 12).expect("valid bounds"),
    );
    log.append("file=secret family photo.jpg")
        .expect("append clipped filename");

    let bytes = fs::read(log.current_path()).expect("read clipped diagnostic");
    assert!(bytes.len() <= 12);
    let record = std::str::from_utf8(&bytes).expect("record remains valid UTF-8");
    assert!(!record.contains("secret"));
    assert!(!record.contains("photo"));
}

#[test]
fn diagnostic_log_redacts_spaced_media_name_when_record_is_truncated() {
    let temp = TempDir::new();
    let log = DiagnosticLog::new(
        temp.path().join("spaced-logs"),
        temp.path().join("home"),
        LogLimits::new(64, 2, 20).expect("valid bounds"),
    );
    log.append("file=secret family photo.jpg")
        .expect("append clipped media filename");

    let bytes = fs::read(log.current_path()).expect("read clipped diagnostic");
    assert!(bytes.len() <= 20);
    let record = std::str::from_utf8(&bytes).expect("record remains valid UTF-8");
    for private_fragment in ["secret", "family", "photo", ".jpg"] {
        assert!(
            !record.contains(private_fragment),
            "truncated media record retained {private_fragment}"
        );
    }
}

#[test]
fn diagnostic_log_instances_share_rotation_limits_and_keep_unowned_files() {
    let temp = TempDir::new();
    let directory = temp.path().join("shared-logs");
    fs::create_dir_all(&directory).expect("create shared log directory");
    let unowned_rotation = directory.join("runtime.log.99");
    fs::write(&unowned_rotation, b"unowned").expect("write unrelated file");
    let logs: Vec<_> = (0..4)
        .map(|_| {
            Arc::new(DiagnosticLog::new(
                directory.clone(),
                temp.path().join("home"),
                LogLimits::new(64, 3, 24).expect("valid bounds"),
            ))
        })
        .collect();
    let barrier = Arc::new(Barrier::new(logs.len()));
    let writers: Vec<_> = logs
        .into_iter()
        .enumerate()
        .map(|(worker, log)| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                for event in 0..100 {
                    log.append(&format!("worker-{worker} event-{event}"))
                        .expect("append concurrent event");
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().expect("logger thread");
    }

    let mut count = 0;
    for entry in fs::read_dir(&directory).expect("list shared logs") {
        let entry = entry.expect("log entry");
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let retained_log = name == "runtime.log"
            || name
                .strip_prefix("runtime.log.")
                .and_then(|suffix| suffix.parse::<usize>().ok())
                .is_some_and(|index| index > 0 && index < 3);
        if !retained_log {
            continue;
        }
        assert!(
            entry.metadata().expect("log metadata").len() <= 64,
            "concurrent logger exceeded per-file bound"
        );
        count += 1;
    }
    assert!(count <= 3, "concurrent loggers retained too many files");
    assert_eq!(
        fs::read(&unowned_rotation)
            .expect("preserve unowned file")
            .as_slice(),
        &b"unowned"[..]
    );
}

#[test]
fn concurrent_log_rotation_preserves_every_record_within_retention() {
    let temp = TempDir::new();
    let directory = temp.path().join("retained-logs");
    let logs: Vec<_> = (0..4)
        .map(|_| {
            Arc::new(DiagnosticLog::new(
                directory.clone(),
                temp.path().join("home"),
                LogLimits::new(256, 16, 24).expect("valid bounds"),
            ))
        })
        .collect();
    let barrier = Arc::new(Barrier::new(logs.len()));
    let writers: Vec<_> = logs
        .into_iter()
        .enumerate()
        .map(|(worker, log)| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                for event in 0..40 {
                    log.append(&format!("worker-{worker} event-{event:03}"))
                        .expect("append concurrent retained event");
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().expect("logger thread");
    }

    let expected: BTreeSet<_> = (0..4)
        .flat_map(|worker| (0..40).map(move |event| format!("worker-{worker} event-{event:03}")))
        .collect();
    let mut actual = BTreeSet::new();
    let mut rotated_files = 0;
    for entry in fs::read_dir(&directory).expect("list retained logs") {
        let entry = entry.expect("log entry");
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let retained_log = name == "runtime.log"
            || name
                .strip_prefix("runtime.log.")
                .and_then(|suffix| suffix.parse::<usize>().ok())
                .is_some_and(|index| (1..16).contains(&index));
        if !retained_log {
            continue;
        }
        let bytes = fs::read(entry.path()).expect("read retained log");
        assert!(bytes.len() <= 256, "rotation exceeded each file bound");
        assert_eq!(bytes.last(), Some(&b'\n'), "records remain line-complete");
        let text = std::str::from_utf8(&bytes).expect("concurrent writes remain UTF-8");
        for record in text.lines() {
            assert!(
                expected.contains(record),
                "rotation retained a corrupt or partial record: {record:?}"
            );
            assert!(
                actual.insert(record.to_owned()),
                "duplicate record {record:?}"
            );
        }
        if name != "runtime.log" {
            rotated_files += 1;
        }
    }
    assert!(
        rotated_files > 0,
        "the concurrent workload exercised rotation"
    );
    assert_eq!(
        actual, expected,
        "rotation loses no record within retention"
    );
}

#[test]
fn diagnostic_limits_reject_unbounded_configured_work() {
    assert!(LogLimits::new(usize::MAX, 1, 1).is_err());
    assert!(LogLimits::new(1024 * 1024, 1, 128 * 1024).is_err());
}

#[test]
fn portable_export_filters_machine_local_fields_and_labels_local_backup() {
    let source = json!({
        "title": "Example",
        "profile": "profile-123",
        "cursor_history": ["private-cursor-position"],
        "window_geometry_history": [{"x": 10, "y": 20}],
        "relative_path": "packages/example/1.0.0/hash",
        "source_path": "/home/alice/Movies/private.mov",
        "monitor_id": "DP-1",
        "permission_grants": ["screen-capture"],
        "app_rules": {"com.example.player": "allow"},
        "application_rules": {"com.example.second-player": "deny"},
        "ipc_endpoint": "unix:///run/user/1000/asciiwall.sock",
        "source": "file:/home/alice/private",
        "address": "localhost:1234",
        "connection": "localhost:1234",
        "note": "backend at buildhost:9000",
        "nested": {
            "safe": true,
            "machine_value": "C:\\Users\\Alice\\Videos\\private.mp4"
        }
    });
    let portable =
        ascii_runtime::storage::PortableExport::from_value(&source, ExportMode::Portable)
            .expect("bounded portable export");
    assert_eq!(portable.schema_version(), 1);
    assert_eq!(portable.mode(), "portable");
    let encoded = String::from_utf8(portable.to_json().unwrap()).unwrap();
    assert!(encoded.len() <= 1024 * 1024);
    for private_value in [
        "source",
        "address",
        "connection",
        "localhost",
        "buildhost",
        "9000",
        "file:/home/alice/private",
        "source_path",
        "relative_path",
        "monitor_id",
        "permission_grants",
        "app_rules",
        "application_rules",
        "ipc_endpoint",
        "cursor_history",
        "window_geometry_history",
        "private-cursor-position",
        "alice",
        "private.mov",
        "private.mp4",
        "DP-1",
    ] {
        assert!(
            !encoded.contains(private_value),
            "portable export retained {private_value}"
        );
    }
    assert!(encoded.contains("Example"));
    assert!(encoded.contains("\"safe\": true"));
    assert!(encoded.contains("\"profile\": \"profile-123\""));

    let backup = ascii_runtime::storage::PortableExport::from_value(
        &source,
        ExportMode::NonportableLocalBackup,
    )
    .expect("bounded local backup");
    assert_eq!(backup.mode(), "nonportable-local-backup");
    let backup_json = String::from_utf8(backup.to_json().unwrap()).unwrap();
    assert!(backup_json.contains("/home/alice/Movies/private.mov"));
    assert!(backup_json.contains("DP-1"));
    assert!(backup_json.contains("com.example.player"));
    assert!(backup_json.contains("com.example.second-player"));
    for private_history in [
        "cursor_history",
        "window_geometry_history",
        "private-cursor-position",
    ] {
        assert!(
            !backup_json.contains(private_history),
            "local backup retained forbidden history {private_history}"
        );
    }
}

#[test]
fn portable_export_redacts_user_qualified_home_paths_in_generic_strings() {
    use ascii_runtime::storage::PortableExport;

    let source = json!({
        "note": "~alice/Documents/private.mov",
        "nested": {
            "note": "~bob\\Documents\\private.mp4"
        },
        "ordinary": "ordinary note about ~visitor without a path"
    });
    let portable =
        PortableExport::from_value(&source, ExportMode::Portable).expect("portable export");
    let encoded = String::from_utf8(portable.to_json().unwrap()).unwrap();

    for private_value in ["alice", "private.mov", "bob", "private.mp4"] {
        assert!(
            !encoded.contains(private_value),
            "portable export retained {private_value}"
        );
    }
    assert_eq!(
        portable.content(),
        &json!({"ordinary": "ordinary note about ~visitor without a path"})
    );
}


#[test]
fn portable_export_filters_bare_host_field_but_preserves_local_backup() {
    let source = json!({"host": "buildhost"});
    let portable =
        ascii_runtime::storage::PortableExport::from_value(&source, ExportMode::Portable)
            .expect("bounded portable export");
    assert_eq!(portable.content(), &json!({}));

    let local_backup = ascii_runtime::storage::PortableExport::from_value(
        &source,
        ExportMode::NonportableLocalBackup,
    )
    .expect("bounded local backup");
    assert_eq!(local_backup.content(), &source);
}

#[test]
fn portable_export_rejects_over_budget_strings_depth_nodes_and_output() {
    use ascii_runtime::storage::PortableExport;

    let oversized_string = json!({"label": "private".repeat(16 * 1024 + 1)});
    assert!(matches!(
        PortableExport::from_value(&oversized_string, ExportMode::Portable),
        Err(StorageError::DocumentTooLarge)
    ));

    let mut deeply_nested = json!(true);
    for _ in 0..66 {
        deeply_nested = json!([deeply_nested]);
    }
    assert!(matches!(
        PortableExport::from_value(&deeply_nested, ExportMode::Portable),
        Err(StorageError::DocumentTooLarge)
    ));

    let too_many_nodes = Value::Array((0..20_000).map(|_| Value::Bool(true)).collect());
    assert!(matches!(
        PortableExport::from_value(&too_many_nodes, ExportMode::Portable),
        Err(StorageError::DocumentTooLarge)
    ));

    let too_much_output = Value::Array(
        (0..300)
            .map(|_| Value::String("output".repeat(1024)))
            .collect(),
    );
    assert!(matches!(
        PortableExport::from_value(&too_much_output, ExportMode::Portable),
        Err(StorageError::DocumentTooLarge)
    ));
}
