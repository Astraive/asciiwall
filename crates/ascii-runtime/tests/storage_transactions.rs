use ascii_runtime::storage::{
    AtomicWriteStage, DiagnosticLog, DirectoryInputs, ExportMode, ImportJournal, InstallEntry,
    InstallRegistry, JournalRecord, JournalState, LogLimits, ParsedToml, RecoveryAction, Storage,
    StorageError, StorageRoots, validate_application_id,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, mpsc};
use std::time::Duration;
use toml::Value as TomlValue;

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

fn validate_test_config(value: &TomlValue) -> Result<(), &'static str> {
    matches!(value, TomlValue::Table(_))
        .then_some(())
        .ok_or("configuration must be a TOML table")
}

fn validate_theme_config(value: &TomlValue) -> Result<(), &'static str> {
    validate_test_config(value)?;
    value
        .get("theme")
        .and_then(TomlValue::as_str)
        .map(|_| ())
        .ok_or("theme must be text")
}

fn toml_value(value: Value) -> TomlValue {
    TomlValue::try_from(value).expect("convert test value to TOML")
}

fn config(value: Value) -> ParsedToml {
    let value = toml_value(value);
    let bytes = toml::to_string(&value).expect("serialize test TOML");
    let config = ParsedToml::from_toml(bytes.as_bytes()).expect("parse test config");
    validate_test_config(config.value()).expect("valid test config");
    config
}

fn parse_config(bytes: &[u8]) -> ParsedToml {
    let config = ParsedToml::from_toml(bytes).expect("parse TOML config");
    validate_test_config(config.value()).expect("valid config schema");
    config
}

fn save_current_config<E: std::fmt::Display>(
    storage: &Storage,
    config: &ParsedToml,
    validate: impl Fn(&TomlValue) -> Result<(), E>,
) -> Result<(), StorageError> {
    let expected_current = storage.load_config(&validate)?;
    storage.save_config(
        expected_current.as_ref().map(ParsedToml::as_bytes),
        config,
        validate,
    )
}

fn save_current_config_with_hook<E: std::fmt::Display>(
    storage: &Storage,
    config: &ParsedToml,
    validate: impl Fn(&TomlValue) -> Result<(), E>,
    hook: impl FnMut(AtomicWriteStage) -> io::Result<()>,
) -> Result<(), StorageError> {
    let expected_current = storage.load_config(&validate)?;
    storage.save_config_with_hook(
        expected_current.as_ref().map(ParsedToml::as_bytes),
        config,
        validate,
        hook,
    )
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

#[cfg(unix)]
#[test]
fn storage_roots_and_persisted_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new();
    let roots = linux_roots(temp.path());
    fs::create_dir_all(&roots.config).expect("create previously permissive config root");
    fs::set_permissions(&roots.config, fs::Permissions::from_mode(0o755))
        .expect("make existing config root broadly accessible");
    let storage = Storage::new(roots.clone());
    roots.create_all().expect("create private storage roots");
    for directory in [
        &roots.config,
        &roots.data,
        &roots.library,
        &roots.projects,
        &roots.catalog,
        &roots.cache,
        &roots.staging,
        &roots.logs,
        &roots.runtime,
    ] {
        assert_eq!(
            fs::metadata(directory)
                .expect("storage directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700,
            "storage directory is private: {directory:?}"
        );
    }

    let first_config = config(json!({"theme": "private"}));
    save_current_config_with_hook(&storage, &first_config, validate_test_config, |stage| {
        if stage == AtomicWriteStage::TemporaryCreated {
            let paths = fs::read_dir(&roots.config)?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<io::Result<Vec<_>>>()?;
            let temporary = paths
                .into_iter()
                .find(|path| {
                    path.file_name()
                        .is_some_and(|name| name.to_string_lossy().ends_with(".state.tmp"))
                })
                .ok_or_else(|| io::Error::other("atomic temporary was not created"))?;
            assert_eq!(fs::metadata(temporary)?.permissions().mode() & 0o777, 0o600);
        }
        Ok(())
    })
    .expect("write private config");
    assert_eq!(
        fs::metadata(storage.config_path())
            .expect("config metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    fs::set_permissions(storage.config_path(), fs::Permissions::from_mode(0o644))
        .expect("make old config broadly readable for replacement regression");
    save_current_config(
        &storage,
        &config(json!({"theme": "replacement"})),
        validate_test_config,
    )
    .expect("replace config without broadening permissions");
    assert_eq!(
        fs::metadata(storage.config_path())
            .expect("replacement config metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let initial_registry = registry("1.0.0");
    storage
        .save_registry_with_hook(&initial_registry, |stage| {
            if stage == AtomicWriteStage::TemporaryCreated {
                let temporary = roots
                    .data
                    .join(".ascii-runtime-install-registry.json.state.tmp");
                assert_eq!(fs::metadata(temporary)?.permissions().mode() & 0o777, 0o600);
            }
            Ok(())
        })
        .expect("write private registry");
    assert_eq!(
        fs::metadata(storage.registry_path())
            .expect("registry metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let journal = ImportJournal::new(roots.staging.clone());
    journal
        .begin("private-test", "org.example.sample", "imports/private-test")
        .expect("write private journal");
    assert_eq!(
        fs::metadata(
            roots
                .staging
                .join("import-journal/import-private-test.json")
        )
        .expect("journal metadata")
        .permissions()
        .mode()
            & 0o777,
        0o600
    );

    let log = DiagnosticLog::new(
        roots.logs.clone(),
        temp.path().join("home"),
        LogLimits::new(256, 2, 128).expect("valid bounds"),
    );
    let previous_log = log.current_path();
    fs::write(&previous_log, vec![b'x'; 240]).expect("seed existing diagnostic log");
    fs::set_permissions(&previous_log, fs::Permissions::from_mode(0o644))
        .expect("make existing log broadly readable");
    log.append("private log record").expect("write private log");
    assert_eq!(
        fs::metadata(log.current_path())
            .expect("log metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(roots.logs.join("runtime.log.1"))
            .expect("rotated log metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let isolated_roots = linux_roots(&temp.path().join("isolated"));
    ImportJournal::new(isolated_roots.staging.clone())
        .begin("first-write", "org.example.sample", "imports/first-write")
        .expect("create journal storage without prior root initialization");
    for directory in [&isolated_roots.data, &isolated_roots.staging] {
        assert_eq!(
            fs::metadata(directory)
                .expect("nested app storage directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700,
            "nested app storage directory is private: {directory:?}"
        );
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
    save_current_config(&storage, &first, validate_test_config)
        .expect("write config on native Windows filesystem");
    let second = config(json!({"theme": "windows-second"}));
    save_current_config(&storage, &second, validate_test_config)
        .expect("replace config and establish Windows last-good backup");
    let replacement = config(json!({"theme": "windows-replacement"}));
    assert!(matches!(
        save_current_config_with_hook(
            &storage,
            &replacement,
            validate_test_config,
            injected_failure(AtomicWriteStage::TargetReplaced)
        ),
        Err(StorageError::Io(_))
    ));
    assert_eq!(
        parse_config(&fs::read(storage.config_path()).expect("read replaced Windows config"))
            .value(),
        replacement.value()
    );
    assert_eq!(
        parse_config(
            &fs::read(storage.config_backup_path()).expect("read Windows last-good backup")
        )
        .value(),
        second.value()
    );
    let subsequent = config(json!({"theme": "windows-subsequent"}));
    save_current_config(&storage, &subsequent, validate_test_config)
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
fn concurrent_registry_updates_preserve_independent_package_changes() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    storage
        .save_registry(&registry("1.0.0"))
        .expect("save initial registry");
    let start = Arc::new(Barrier::new(3));
    let writers = [
        ("org.example.first", "2.0.0"),
        ("org.example.second", "3.0.0"),
    ];
    let handles: Vec<_> = writers
        .into_iter()
        .map(|(package_id, version)| {
            let storage = storage.clone();
            let start = Arc::clone(&start);
            let package_id = package_id.to_owned();
            let version = version.to_owned();
            std::thread::spawn(move || {
                let digest = "b".repeat(64);
                let entry = InstallEntry {
                    package_version: version.clone(),
                    sha256: digest.clone(),
                    relative_path: format!("packages/{package_id}/{version}/{digest}"),
                };
                start.wait();
                storage
                    .update_registry(|registry| {
                        registry.packages.insert(package_id, entry);
                    })
                    .expect("apply serialized registry update");
            })
        })
        .collect();
    start.wait();
    for handle in handles {
        handle.join().expect("registry update worker");
    }

    let updated = storage.load_registry().expect("load merged registry");
    assert!(updated.packages.contains_key("org.example.sample"));
    assert_eq!(
        updated.packages["org.example.first"].package_version,
        "2.0.0"
    );
    assert_eq!(
        updated.packages["org.example.second"].package_version,
        "3.0.0"
    );
}

#[test]
fn registry_update_parent_sync_failure_restores_exact_prior_state() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let previous = registry("1.0.0");
    storage
        .save_registry(&previous)
        .expect("save initial registry");
    let path = storage.registry_path();
    let previous_bytes = fs::read(&path).expect("capture initial registry bytes");

    let result = storage.update_registry_with_hook(
        |current| *current = registry("2.0.0"),
        injected_failure(AtomicWriteStage::ParentDirectorySync),
    );
    assert!(matches!(
        result,
        Err(StorageError::Io(error)) if error.to_string() == "injected interruption"
    ));
    assert_eq!(
        fs::read(&path).expect("read registry after failed update"),
        previous_bytes
    );
    assert_eq!(
        storage.load_registry().expect("load prior registry"),
        previous
    );
}

#[test]
fn registry_update_rollback_failure_reports_prior_state_wording() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    storage
        .save_registry(&registry("1.0.0"))
        .expect("save initial registry");

    let error = storage
        .update_registry_with_hook(
            |current| *current = registry("2.0.0"),
            |stage| match stage {
                AtomicWriteStage::ParentDirectorySync => {
                    Err(io::Error::other("injected sync failure"))
                }
                AtomicWriteStage::RecoveryRollbackRename => {
                    Err(io::Error::other("injected rollback failure"))
                }
                _ => Ok(()),
            },
        )
        .expect_err("registry rollback failure should be reported");
    assert!(matches!(
        &error,
        StorageError::RecoveryRollbackFailed {
            retained_recovery_copy: Some(_),
            ..
        }
    ));
    let display = error.to_string();
    assert!(display.contains("parent-directory sync failed (injected sync failure)"));
    assert!(display.contains("restoring prior state also failed (injected rollback failure)"));
}

#[test]
fn registry_write_rejects_newer_and_invalid_current_bytes() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let replacement = registry("2.0.0");
    storage
        .save_registry(&registry("1.0.0"))
        .expect("save supported registry");
    let path = storage.registry_path();

    let newer = br#"{"schema_version":2,"packages":{}}"#;
    fs::write(&path, newer).expect("write newer registry schema");
    assert!(matches!(
        storage.save_registry(&replacement),
        Err(StorageError::UnsupportedVersion { .. })
    ));
    assert_eq!(
        fs::read(&path).expect("newer registry remains untouched"),
        newer
    );

    let invalid = b"{not-json";
    fs::write(&path, invalid).expect("write invalid registry");
    assert!(matches!(
        storage.save_registry(&replacement),
        Err(StorageError::Json(_))
    ));
    assert_eq!(
        fs::read(&path).expect("invalid registry remains untouched"),
        invalid
    );
}

#[test]
fn duplicate_registry_package_keys_are_rejected_on_load_and_save() {
    let temp = TempDir::new();
    let roots = linux_roots(temp.path());
    roots.create_all().expect("create private storage roots");
    let storage = Storage::new(roots);
    let path = storage.registry_path();
    let first_entry = registry("1.0.0")
        .packages
        .into_iter()
        .next()
        .expect("first package entry")
        .1;
    let second_entry = registry("2.0.0")
        .packages
        .into_iter()
        .next()
        .expect("second package entry")
        .1;
    let package_id = serde_json::to_string("org.example.sample").expect("serialize package id");
    let first_entry = serde_json::to_string(&first_entry).expect("serialize first package");
    let second_entry = serde_json::to_string(&second_entry).expect("serialize second package");
    let mut duplicate = String::from(r#"{"schema_version":1,"packages":{"#);
    duplicate.push_str(&package_id);
    duplicate.push(':');
    duplicate.push_str(&first_entry);
    duplicate.push(',');
    duplicate.push_str(&package_id);
    duplicate.push(':');
    duplicate.push_str(&second_entry);
    duplicate.push_str("}}");
    let duplicate = duplicate.into_bytes();

    fs::write(&path, &duplicate).expect("write duplicate-key registry");
    assert!(matches!(
        storage.load_registry(),
        Err(StorageError::Json(_))
    ));
    assert_eq!(
        fs::read(&path).expect("rejected load preserves exact registry bytes"),
        duplicate
    );

    assert!(matches!(
        storage.save_registry(&registry("3.0.0")),
        Err(StorageError::Json(_))
    ));
    assert_eq!(
        fs::read(&path).expect("rejected save preserves exact registry bytes"),
        duplicate
    );
}

#[test]
fn configuration_validation_backup_and_interrupted_replacement_keep_valid_state() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let first = config(json!({"theme": "dark", "grid": {"width": 80}}));
    save_current_config(&storage, &first, validate_test_config).expect("save initial config");
    let config_path = storage.config_path();
    let backup_path = storage.config_backup_path();
    let original = fs::read(&config_path).expect("initial config bytes");

    let second = config(json!({"theme": "light", "grid": {"width": 100}}));
    assert!(
        save_current_config_with_hook(
            &storage,
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

    let syntax_only = ParsedToml::from_toml(b"unrecognized_setting = true")
        .expect("valid TOML syntax does not impose a settings schema");
    validate_test_config(syntax_only.value()).expect("TOML parser does not invent settings keys");
    assert!(
        storage
            .load_config(|_| Err::<(), _>("unsupported schema"))
            .is_err(),
        "caller-owned schema validator remains authoritative"
    );

    save_current_config(&storage, &second, validate_test_config).expect("replace config");
    assert_eq!(fs::read(&backup_path).expect("last-good backup"), original);
    assert_eq!(
        storage
            .load_config(validate_test_config)
            .expect("load validated config")
            .unwrap()
            .value(),
        second.value()
    );

    let previous_second = fs::read(&config_path).expect("second config bytes");
    let third = config(json!({"theme": "system"}));
    assert!(
        save_current_config_with_hook(
            &storage,
            &third,
            validate_test_config,
            injected_failure(AtomicWriteStage::TargetReplaced)
        )
        .is_err()
    );
    assert_eq!(
        fs::read(&config_path).expect("new state survives post-replacement interruption"),
        third.as_bytes().to_vec()
    );
    assert_eq!(
        fs::read(&backup_path).expect("last-good remains previous valid state"),
        previous_second
    );
}

#[test]
fn concurrent_config_writers_reject_stale_snapshots_before_mutation() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let config_path = storage.config_path();
    let backup_path = storage.config_backup_path();
    let initial = config(json!({"theme": "initial"}));
    storage
        .save_config(None, &initial, validate_test_config)
        .expect("save config from an observed-absent state");
    let initial_bytes = fs::read(&config_path).expect("initial config bytes");

    let stale_absence_write = config(json!({"theme": "stale-absence"}));
    assert!(matches!(
        storage.save_config(None, &stale_absence_write, validate_test_config),
        Err(StorageError::StaleConfiguration)
    ));
    assert_eq!(
        fs::read(&config_path).expect("stale absent-state write leaves current"),
        initial_bytes
    );
    assert!(
        !backup_path.exists(),
        "stale absent-state write does not create a backup"
    );

    let baseline = config(json!({"theme": "baseline"}));
    storage
        .save_config(Some(initial.as_bytes()), &baseline, validate_test_config)
        .expect("save against the observed current bytes");
    let expected_current = storage
        .load_config(validate_test_config)
        .expect("load current snapshot")
        .expect("current config exists");
    let current_before = fs::read(&config_path).expect("current bytes before competing writes");
    let backup_before = fs::read(&backup_path).expect("backup bytes before competing writes");
    let candidate_a = config(json!({"theme": "writer-a"}));
    let candidate_b = config(json!({"theme": "writer-b"}));
    let candidate_a_bytes = candidate_a.as_bytes().to_vec();
    let candidate_b_bytes = candidate_b.as_bytes().to_vec();
    let expected_a = expected_current.as_bytes().to_vec();
    let expected_b = expected_current.as_bytes().to_vec();
    let start = Arc::new(Barrier::new(3));
    let hook_calls_a = Arc::new(AtomicUsize::new(0));
    let hook_calls_b = Arc::new(AtomicUsize::new(0));

    let writer_a = {
        let storage = storage.clone();
        let start = Arc::clone(&start);
        let hook_calls = Arc::clone(&hook_calls_a);
        std::thread::spawn(move || {
            start.wait();
            storage.save_config_with_hook(
                Some(expected_a.as_slice()),
                &candidate_a,
                validate_test_config,
                move |_| {
                    hook_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
        })
    };
    let writer_b = {
        let storage = storage.clone();
        let start = Arc::clone(&start);
        let hook_calls = Arc::clone(&hook_calls_b);
        std::thread::spawn(move || {
            start.wait();
            storage.save_config_with_hook(
                Some(expected_b.as_slice()),
                &candidate_b,
                validate_test_config,
                move |_| {
                    hook_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
        })
    };
    start.wait();
    let result_a = writer_a.join().expect("join writer A");
    let result_b = writer_b.join().expect("join writer B");

    match (result_a, result_b) {
        (Ok(()), Err(StorageError::StaleConfiguration)) => {
            assert_eq!(hook_calls_b.load(Ordering::SeqCst), 0);
            assert!(hook_calls_a.load(Ordering::SeqCst) > 0);
        }
        (Err(StorageError::StaleConfiguration), Ok(())) => {
            assert_eq!(hook_calls_a.load(Ordering::SeqCst), 0);
            assert!(hook_calls_b.load(Ordering::SeqCst) > 0);
        }
        (result_a, result_b) => panic!(
            "expected one write and one typed stale conflict, got {result_a:?} and {result_b:?}"
        ),
    }

    let current_after = fs::read(&config_path).expect("current bytes after competing writes");
    assert!(
        current_after == candidate_a_bytes || current_after == candidate_b_bytes,
        "exactly one writer installs its candidate"
    );
    assert_eq!(
        fs::read(&backup_path).expect("backup bytes after competing writes"),
        current_before,
        "the stale writer does not replace the winner's last-good backup"
    );
    assert_eq!(backup_before, initial_bytes);
}

#[test]
fn configuration_failures_at_each_atomic_boundary_preserve_valid_recovery() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let config_path = storage.config_path();
    let backup_path = storage.config_backup_path();
    let seed = config(json!({"theme": "seed"}));
    save_current_config(&storage, &seed, validate_test_config).expect("save initial config");

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
        save_current_config(&storage, &baseline, validate_test_config)
            .expect("establish valid current and backup before injected failure");
        let baseline_bytes = fs::read(&config_path).expect("baseline current bytes");
        let previous_backup_bytes = fs::read(&backup_path).expect("previous last-good bytes");
        let candidate = config(json!({"theme": format!("candidate-{index}")}));

        let error = save_current_config_with_hook(
            &storage,
            &candidate,
            validate_test_config,
            injected_failure(stage),
        )
        .expect_err("injected interruption");
        assert!(matches!(error, StorageError::Io(_)));

        let expected_current = if stage == AtomicWriteStage::TargetReplaced {
            candidate.value()
        } else {
            baseline.value()
        };
        let current_bytes = fs::read(&config_path).expect("current config remains complete");
        let recovered_current = parse_config(&current_bytes);
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
        let recovered_backup = parse_config(&backup_bytes);
        assert!(matches!(recovered_backup.value(), TomlValue::Table(_)));

        let parent = config_path.parent().expect("configuration directory");
        assert!(
            !parent
                .join(".ascii-runtime-settings.toml.state.tmp")
                .exists(),
            "failed config write removes its owned temporary"
        );
        assert!(
            !parent
                .join(".ascii-runtime-settings.toml.last-good.state.tmp")
                .exists(),
            "failed backup write leaves no owned temporary"
        );

        let subsequent = config(json!({"theme": format!("subsequent-{index}")}));
        save_current_config(&storage, &subsequent, validate_test_config)
            .expect("valid config write succeeds after injected failure");
        assert_eq!(
            storage
                .load_config(validate_test_config)
                .expect("load subsequent config")
                .expect("subsequent config exists")
                .value(),
            subsequent.value()
        );
        let replaced_backup =
            parse_config(&fs::read(&backup_path).expect("subsequent write records last-good"));
        assert_eq!(replaced_backup.value(), expected_current);
    }
}

#[test]
fn saving_config_revalidates_against_the_active_schema_before_replacement() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let original = config(json!({"theme": "dark"}));
    save_current_config(&storage, &original, validate_theme_config)
        .expect("save initial schema-valid config");
    let original_bytes = fs::read(storage.config_path()).expect("read initial config");
    let validated_by_weaker_schema = config(json!({"theme": 42}));

    assert!(matches!(
        save_current_config(&storage, &validated_by_weaker_schema, validate_theme_config),
        Err(StorageError::InvalidConfiguration(_))
    ));
    assert_eq!(
        fs::read(storage.config_path()).expect("active config remains unchanged"),
        original_bytes
    );
}

#[test]
fn config_saves_reject_nested_history_keys_without_rotating_state() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    save_current_config(
        &storage,
        &config(json!({"theme": "previous"})),
        validate_test_config,
    )
    .expect("save previous config");
    save_current_config(
        &storage,
        &config(json!({"theme": "current"})),
        validate_test_config,
    )
    .expect("save current config and establish last-good");
    let config_path = storage.config_path();
    let backup_path = storage.config_backup_path();
    let current_before = fs::read(&config_path).expect("current config bytes");
    let backup_before = fs::read(&backup_path).expect("last-good config bytes");

    for bytes in [
        b"[ui]\ncursor_history = [\"recent\"]\n".as_slice(),
        b"[ui]\nwindow = { geometry = { window_geometry_history = [\"recent\"] } }\n",
        b"cursor.history = [\"recent\"]\n",
        b"[[profiles]]\ncursor.history = [\"recent\"]\n",
    ] {
        let forbidden = ParsedToml::from_toml(bytes).expect("parse nested history config");
        assert!(matches!(
            save_current_config(&storage, &forbidden, validate_test_config),
            Err(StorageError::InvalidConfiguration(_))
        ));
        assert_eq!(
            fs::read(&config_path).expect("current config remains unchanged"),
            current_before
        );
        assert_eq!(
            fs::read(&backup_path).expect("last-good remains unchanged"),
            backup_before
        );
    }
}

#[test]
fn config_backup_recovery_rejects_history_paths_without_mutating_state() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let previous = config(json!({"theme": "previous"}));
    save_current_config(&storage, &previous, validate_theme_config).expect("save previous config");
    let current = config(json!({"theme": "current"}));
    save_current_config(&storage, &current, validate_theme_config)
        .expect("save current config and establish last-good");
    let config_path = storage.config_path();
    let backup_path = storage.config_backup_path();
    let current_before = fs::read(&config_path).expect("capture current config");

    for backup in [
        b"theme = \"backup\"\ncursor_history = [\"recent\"]\n".as_slice(),
        b"theme = \"backup\"\n[ui]\ncursor_history = [\"recent\"]\n",
        b"theme = \"backup\"\ncursor.history = [\"recent\"]\n",
        b"theme = \"backup\"\n[ui]\ncursor.history = [\"recent\"]\n",
        b"theme = \"backup\"\n[[profiles]]\ncursor.history = [\"recent\"]\n",
    ] {
        fs::write(&backup_path, backup).expect("write forbidden last-good settings");
        let backup_before = fs::read(&backup_path).expect("capture forbidden last-good");
        assert!(matches!(
            storage.restore_config_from_backup(Some(&current_before), validate_theme_config),
            Err(StorageError::InvalidConfiguration(_))
        ));
        assert_eq!(
            fs::read(&config_path).expect("rejected recovery preserves current config"),
            current_before
        );
        assert_eq!(
            fs::read(&backup_path).expect("rejected recovery preserves last-good"),
            backup_before
        );
    }
}

#[test]
fn invalid_current_config_is_rejected_and_last_good_can_be_restored_explicitly() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let known_good = config(json!({"theme": "known-good"}));
    save_current_config(&storage, &known_good, validate_theme_config)
        .expect("save known-good config");
    let known_good_bytes = fs::read(storage.config_path()).expect("read known-good config");

    save_current_config(
        &storage,
        &config(json!({"theme": "current"})),
        validate_theme_config,
    )
    .expect("save current config");
    let config_path = storage.config_path();
    let backup_path = storage.config_backup_path();
    let known_good_backup = fs::read(&backup_path).expect("known-good backup exists");
    assert_eq!(known_good_backup, known_good_bytes);

    let malformed = b"settings = [";
    fs::write(&config_path, malformed).expect("corrupt current config");
    let replacement = config(json!({"theme": "replacement"}));
    assert!(matches!(
        storage.save_config_with_hook(
            Some(malformed),
            &replacement,
            validate_theme_config,
            injected_failure(AtomicWriteStage::TargetReplaced),
        ),
        Err(StorageError::InvalidConfiguration(_))
    ));
    assert_eq!(
        fs::read(&config_path).expect("malformed file stays untouched"),
        malformed
    );
    assert_eq!(
        fs::read(&backup_path).expect("backup stays untouched"),
        known_good_backup
    );

    let invalid_schema = b"theme = 42\n";
    fs::write(&config_path, invalid_schema).expect("write schema-invalid TOML");
    assert!(matches!(
        storage.save_config(Some(invalid_schema), &replacement, validate_theme_config),
        Err(StorageError::InvalidConfiguration(_))
    ));
    assert_eq!(
        fs::read(&config_path).expect("schema-invalid file stays untouched"),
        invalid_schema
    );
    assert_eq!(
        fs::read(&backup_path).expect("schema-invalid config cannot replace backup"),
        known_good_bytes
    );

    let expected_current = fs::read(&config_path).expect("capture current revision for recovery");
    storage
        .restore_config_from_backup(Some(&expected_current), validate_theme_config)
        .expect("explicitly restore the validated last-good config");
    assert_eq!(
        fs::read(&config_path).expect("restored config"),
        known_good_bytes
    );
    assert_eq!(
        fs::read(&backup_path).expect("explicit recovery preserves last-good"),
        known_good_bytes
    );
}

#[test]
fn recovery_sync_failure_preserves_current_and_last_good_bytes() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let known_good = config(json!({"theme": "known-good"}));
    save_current_config(&storage, &known_good, validate_test_config)
        .expect("save known-good settings");
    let current = config(json!({"theme": "current"}));
    save_current_config(&storage, &current, validate_test_config).expect("save current settings");

    let config_path = storage.config_path();
    let backup_path = storage.config_backup_path();
    let current_before = fs::read(&config_path).expect("capture current settings");
    let backup_before = fs::read(&backup_path).expect("capture last-good settings");
    let sync_hook_calls = AtomicUsize::new(0);
    let mut hook_stages = Vec::new();
    let result = storage.restore_config_from_backup_with_hook(
        Some(&current_before),
        validate_test_config,
        |stage| {
            hook_stages.push(stage);
            match stage {
                AtomicWriteStage::ParentDirectorySync => {
                    if sync_hook_calls.fetch_add(1, Ordering::Relaxed) == 0 {
                        Err(io::Error::other("injected parent-directory sync failure"))
                    } else {
                        Ok(())
                    }
                }
                AtomicWriteStage::RecoveryRollbackRename => Ok(()),
                other => panic!("unexpected recovery hook stage: {other:?}"),
            }
        },
    );

    assert!(matches!(
        result,
        Err(StorageError::Io(error))
            if error.to_string() == "injected parent-directory sync failure"
    ));
    assert_eq!(sync_hook_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        hook_stages,
        [
            AtomicWriteStage::ParentDirectorySync,
            AtomicWriteStage::RecoveryRollbackRename,
        ]
    );
    assert_eq!(
        fs::read(&config_path).expect("rolled-back current settings"),
        current_before
    );
    assert_eq!(
        fs::read(&backup_path).expect("unchanged last-good settings"),
        backup_before
    );
}

#[test]
fn recovery_rollback_rename_failure_retains_and_reports_original_bytes() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let known_good = config(json!({"theme": "known-good"}));
    save_current_config(&storage, &known_good, validate_test_config)
        .expect("save known-good settings");
    let current = config(json!({"theme": "current"}));
    save_current_config(&storage, &current, validate_test_config).expect("save current settings");

    let config_path = storage.config_path();
    let backup_path = storage.config_backup_path();
    let current_before = fs::read(&config_path).expect("capture current settings");
    let backup_before = fs::read(&backup_path).expect("capture last-good settings");
    let result = storage.restore_config_from_backup_with_hook(
        Some(&current_before),
        validate_test_config,
        |stage| match stage {
            AtomicWriteStage::ParentDirectorySync => {
                Err(io::Error::other("injected parent-directory sync failure"))
            }
            AtomicWriteStage::RecoveryRollbackRename => Err(io::Error::other(
                "injected recovery rollback rename failure",
            )),
            other => panic!("unexpected recovery hook stage: {other:?}"),
        },
    );

    let retained_copy = match result {
        Err(StorageError::RecoveryRollbackFailed {
            sync_error,
            rollback_error,
            retained_recovery_copy: Some(path),
        }) => {
            assert_eq!(
                sync_error.to_string(),
                "injected parent-directory sync failure"
            );
            assert_eq!(
                rollback_error.to_string(),
                "injected recovery rollback rename failure"
            );
            path
        }
        other => panic!("expected typed recovery rollback failure, got {other:?}"),
    };
    assert_eq!(
        fs::read(&retained_copy).expect("retained recovery copy"),
        current_before
    );
    let current_after = fs::read(&config_path).expect("current remains the restored candidate");
    assert_eq!(current_after, backup_before);
    assert_ne!(current_after, current_before);
    assert_eq!(
        fs::read(&backup_path).expect("last-good backup remains unchanged"),
        backup_before
    );
    storage
        .restore_config_from_backup(Some(&current_after), validate_test_config)
        .expect("retry settings recovery after rollback failure");
    assert_eq!(
        fs::read(&retained_copy).expect("retained recovery copy survives retry"),
        current_before
    );
}

#[test]
fn restore_config_from_backup_rejects_stale_current_revision_without_mutation() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));

    let initial = config(json!({"theme": "initial"}));
    save_current_config(&storage, &initial, validate_test_config).expect("save initial settings");
    let stale_revision = fs::read(storage.config_path()).expect("capture initial revision");
    let recovery_target = config(json!({"theme": "recovery target"}));
    save_current_config(&storage, &recovery_target, validate_test_config)
        .expect("save recovery target");
    let expected_current = fs::read(storage.config_path()).expect("capture recovery revision");
    let concurrent_winner = config(json!({"theme": "concurrent winner"}));
    save_current_config(&storage, &concurrent_winner, validate_test_config)
        .expect("write newer current settings");
    let current_before = fs::read(storage.config_path()).expect("current settings bytes");
    let backup_before = fs::read(storage.config_backup_path()).expect("last-good settings bytes");

    assert!(matches!(
        storage.restore_config_from_backup(Some(&expected_current), validate_test_config),
        Err(StorageError::StaleConfiguration)
    ));
    assert_eq!(
        fs::read(storage.config_path()).expect("stale recovery preserves current settings"),
        current_before
    );
    assert_eq!(
        fs::read(storage.config_backup_path()).expect("stale recovery preserves last-good"),
        backup_before
    );
    assert_ne!(stale_revision, expected_current);
}

#[test]
fn restore_config_from_backup_holds_directory_lock_through_compare_and_swap() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let known_good = config(json!({"theme": "known-good"}));
    save_current_config(&storage, &known_good, validate_test_config)
        .expect("save known-good settings");
    let current = config(json!({"theme": "current"}));
    save_current_config(&storage, &current, validate_test_config).expect("save current settings");
    let config_path = storage.config_path();
    let backup_path = storage.config_backup_path();
    let current_before = fs::read(&config_path).expect("current settings bytes");
    let backup_before = fs::read(&backup_path).expect("last-good settings bytes");
    let competing_write = config(json!({"theme": "competing writer"}));

    let (restore_entered_tx, restore_entered_rx) = mpsc::channel();
    let (resume_restore_tx, resume_restore_rx) = mpsc::channel();
    let restore = {
        let storage = storage.clone();
        let expected_current = current_before.clone();
        std::thread::spawn(move || {
            storage.restore_config_from_backup(Some(&expected_current), move |value| {
                restore_entered_tx
                    .send(())
                    .expect("signal restore validation");
                resume_restore_rx.recv().expect("resume restore validation");
                validate_test_config(value)
            })
        })
    };
    restore_entered_rx
        .recv()
        .expect("restore reaches schema validation");

    let (writer_ready_tx, writer_ready_rx) = mpsc::channel();
    let (continue_writer_tx, continue_writer_rx) = mpsc::channel();
    let (writer_done_tx, writer_done_rx) = mpsc::channel();
    let writer = {
        let storage = storage.clone();
        std::thread::spawn(move || {
            let validation_calls = AtomicUsize::new(0);
            let result =
                storage.save_config(Some(&current_before), &competing_write, move |value| {
                    if validation_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        writer_ready_tx
                            .send(())
                            .expect("signal competing writer validation");
                        continue_writer_rx
                            .recv()
                            .expect("continue competing writer");
                    }
                    validate_test_config(value)
                });
            writer_done_tx
                .send(result)
                .expect("report competing writer result");
        })
    };
    writer_ready_rx
        .recv()
        .expect("competing writer validates its candidate");
    continue_writer_tx
        .send(())
        .expect("allow competing writer to request the lock");

    let writer_finished_during_restore = writer_done_rx.recv_timeout(Duration::from_secs(1)).ok();
    let writer_was_blocked = writer_finished_during_restore.is_none();
    resume_restore_tx
        .send(())
        .expect("resume restore after competing writer attempt");
    let restore_result = restore.join().expect("join restore");
    let writer_result = match writer_finished_during_restore {
        Some(result) => result,
        None => writer_done_rx.recv().expect("competing writer result"),
    };
    writer.join().expect("join competing writer");

    assert!(
        writer_was_blocked,
        "competing writer completed while restore validation held the directory lock"
    );
    assert!(restore_result.is_ok(), "restore failed: {restore_result:?}");
    assert!(matches!(
        writer_result,
        Err(StorageError::StaleConfiguration)
    ));
    assert_eq!(
        fs::read(&config_path).expect("restored current settings"),
        backup_before
    );
    assert_eq!(
        fs::read(&backup_path).expect("last-good remains unchanged"),
        backup_before
    );
}

#[test]
fn newer_settings_schema_is_not_replaced_by_older_writer_or_explicit_recovery() {
    let temp = TempDir::new();
    let storage = Storage::new(linux_roots(temp.path()));
    let validate_schema_v1 = |value: &TomlValue| {
        (value.get("schema_version").and_then(TomlValue::as_integer) == Some(1))
            .then_some(())
            .ok_or("unsupported settings schema")
    };
    let supported = ParsedToml::from_toml(
        b"schema_version = 1\nrevision = 1\n[settings]\ntheme = \"known-good\"\n",
    )
    .expect("parse supported TOML");
    save_current_config(&storage, &supported, validate_schema_v1).expect("save supported schema");
    let current = ParsedToml::from_toml(
        b"schema_version = 1\nrevision = 2\n[settings]\ntheme = \"current\"\n",
    )
    .expect("parse current TOML");
    save_current_config(&storage, &current, validate_schema_v1)
        .expect("save current supported schema");
    let backup_path = storage.config_backup_path();
    let known_good_backup = fs::read(&backup_path).expect("read supported last-good");

    let newer = b"schema_version = 2\nrevision = 3\n[settings]\ntheme = \"newer\"\n";
    fs::write(storage.config_path(), newer).expect("write newer schema");
    assert!(matches!(
        storage.save_config(Some(newer), &supported, validate_schema_v1),
        Err(StorageError::InvalidConfiguration(_))
    ));
    assert_eq!(
        fs::read(storage.config_path()).expect("newer schema stays untouched"),
        newer
    );
    assert_eq!(
        fs::read(&backup_path).expect("newer current preserves last-good"),
        known_good_backup
    );
    assert!(matches!(
        storage.restore_config_from_backup(Some(&newer[..]), validate_schema_v1),
        Err(StorageError::UnsupportedVersion {
            kind: "settings configuration",
            version: 2
        })
    ));
    assert_eq!(
        fs::read(storage.config_path()).expect("future-schema settings stay untouched"),
        newer
    );
    assert_eq!(
        fs::read(&backup_path).expect("refused recovery preserves last-good"),
        known_good_backup
    );
}

#[test]
fn restore_rejects_versioned_versionless_comparisons() {
    let temp = TempDir::new();
    let roots = linux_roots(temp.path());
    roots.create_all().expect("create private storage roots");
    let storage = Storage::new(roots);

    for (current, backup) in [
        (
            b"schema_version = 2\ntheme = \"newer\"\n".as_slice(),
            b"theme = \"versionless backup\"\n".as_slice(),
        ),
        (
            b"theme = \"versionless current\"\n".as_slice(),
            b"schema_version = 1\ntheme = \"versioned backup\"\n".as_slice(),
        ),
    ] {
        fs::write(storage.config_path(), current).expect("write current settings");
        fs::write(storage.config_backup_path(), backup).expect("write last-good settings");
        let current_before = fs::read(storage.config_path()).expect("current settings bytes");
        let backup_before =
            fs::read(storage.config_backup_path()).expect("last-good settings bytes");

        assert!(matches!(
            storage.restore_config_from_backup(Some(&current_before), validate_test_config),
            Err(StorageError::InvalidConfiguration(_))
        ));
        assert_eq!(
            fs::read(storage.config_path()).expect("refused restore preserves current"),
            current_before
        );
        assert_eq!(
            fs::read(storage.config_backup_path()).expect("refused restore preserves backup"),
            backup_before
        );
    }
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
    save_current_config(&storage, &original_config, validate_test_config)
        .expect("save valid config");
    let config_path = storage.config_path();
    let prefix = b"payload = \"";
    let suffix = b"\"\n";
    let payload = "x".repeat(CONFIG_MAX_BYTES - prefix.len() - suffix.len());
    let compact_config = format!("payload = \"{payload}\"\n").into_bytes();
    assert_eq!(compact_config.len(), CONFIG_MAX_BYTES);
    let exact_limit_config =
        ParsedToml::from_toml(&compact_config).expect("exact-limit TOML is accepted");
    let boundary_temp = TempDir::new();
    let boundary_storage = Storage::new(linux_roots(boundary_temp.path()));
    save_current_config(&boundary_storage, &exact_limit_config, validate_test_config)
        .expect("save config at the inclusive size limit");
    assert_eq!(
        fs::read(boundary_storage.config_path()).expect("read exact-limit config"),
        compact_config
    );

    let oversized_current = vec![b' '; CONFIG_MAX_BYTES + 1];
    assert!(matches!(
        ParsedToml::from_toml(&oversized_current),
        Err(StorageError::DocumentTooLarge)
    ));
    let recovery_config = config(json!({"theme": "recovery"}));
    save_current_config(&storage, &recovery_config, validate_test_config)
        .expect("establish a last-good config before oversized-current recovery");
    let backup_path = storage.config_backup_path();
    let known_good_backup = fs::read(&backup_path).expect("retain last-good backup");
    fs::write(&config_path, &oversized_current).expect("write oversized config");
    assert!(matches!(
        storage.load_config(validate_test_config),
        Err(StorageError::DocumentTooLarge)
    ));
    let replacement = config(json!({"theme": "recovered"}));
    assert!(matches!(
        storage.save_config(
            Some(oversized_current.as_slice()),
            &replacement,
            validate_test_config,
        ),
        Err(StorageError::DocumentTooLarge)
    ));
    assert_eq!(
        fs::read(&config_path).expect("oversized current remains untouched"),
        oversized_current
    );
    assert_eq!(
        fs::read(&backup_path).expect("oversized current preserves last-good"),
        known_good_backup
    );
    storage
        .restore_config_from_backup(Some(&oversized_current), validate_test_config)
        .expect("explicitly restore from last-good");
    assert_eq!(
        storage
            .load_config(validate_test_config)
            .unwrap()
            .unwrap()
            .value(),
        original_config.value()
    );
    assert_eq!(
        fs::read(&backup_path).expect("recovery preserves last-good"),
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
        "punctuated_endpoint": "backend at buildhost:9000!",
        "angle_endpoint": "backend at <buildhost:9443>",
        "trailing_colon_endpoint": "backend at buildhost:9000:",
        "benign_colon_control": "status:",
        "control": "backend at buildhost:65536!",
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
    assert!(portable.content().get("punctuated_endpoint").is_none());
    assert!(portable.content().get("angle_endpoint").is_none());
    assert_eq!(
        portable.content()["control"],
        json!("backend at buildhost:65536!")
    );
    assert!(portable.content().get("trailing_colon_endpoint").is_none());
    assert_eq!(portable.content()["benign_colon_control"], json!("status:"));
    for private_value in [
        "source",
        "address",
        "connection",
        "localhost",
        "9443",
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
    assert!(backup_json.contains("backend at buildhost:9000!"));
    assert!(backup_json.contains("backend at <buildhost:9443>"));
    assert!(backup_json.contains("backend at buildhost:65536!"));
    assert!(backup_json.contains("backend at buildhost:9000:"));
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
        "prefixed_posix": "note=~alice/Documents/private.mov",
        "prefixed_windows": "note=~bob\\Documents\\private.mp4",
        "ordinary": "ordinary note about ~visitor without a path"
    });
    let portable =
        PortableExport::from_value(&source, ExportMode::Portable).expect("portable export");
    let encoded = String::from_utf8(portable.to_json().unwrap()).unwrap();
    for private_value in ["alice", "Documents", "private.mov", "bob", "private.mp4"] {
        assert!(
            !encoded.contains(private_value),
            "portable export retained {private_value}"
        );
    }
    assert_eq!(
        portable.content()["ordinary"],
        json!("ordinary note about ~visitor without a path")
    );
}

#[test]
fn portable_export_redacts_machine_paths_after_colon_delimiters() {
    let source = json!({
        "first_note": "note:~alice/Documents/private.txt",
        "second_note": "note:/home/alice/private.txt",
        "third_note": "note:~bob\\Documents\\private.txt",
        "benign_colon_text": "status:"
    });
    let portable =
        ascii_runtime::storage::PortableExport::from_value(&source, ExportMode::Portable)
            .expect("portable export");
    let encoded = String::from_utf8(portable.to_json().unwrap()).unwrap();
    for private_key in ["first_note", "second_note", "third_note"] {
        assert!(
            portable.content().get(private_key).is_none(),
            "portable export retained {private_key}"
        );
    }
    for private_value in ["alice", "bob", "Documents", "private.txt", "/home/"] {
        assert!(
            !encoded.contains(private_value),
            "portable export retained {private_value}"
        );
    }
    assert_eq!(portable.content()["benign_colon_text"], json!("status:"));
}

#[test]
fn portable_export_redacts_embedded_relative_paths_in_generic_strings() {
    use ascii_runtime::storage::PortableExport;

    let source = json!({
        "embedded_posix": "clip selected from Documents/private.mov before import",
        "embedded_windows": "clip selected from Videos\\private.mp4 before import",
        "embedded_extensionless": "clip selected from Documents/private before import",
        "embedded_unicode_extensionless": "clip selected from Documents/秘密 before import",
        "embedded_unicode_suffix": "clip selected from Documents/private.設定 before import",
        "embedded_dotfile": "credential file: Documents/.credentials",
        "harmless_slash_prose": "Choose input/output; the ratio is 3/4.",
        "harmless_supported_versions": "Supported versions v1.0/v2.0",
        "harmless_filename_prose": "The terms private.mov and Documents are examples."
    });
    let portable =
        PortableExport::from_value(&source, ExportMode::Portable).expect("portable export");

    for private_key in [
        "embedded_posix",
        "embedded_windows",
        "embedded_extensionless",
        "embedded_unicode_extensionless",
        "embedded_unicode_suffix",
        "embedded_dotfile",
    ] {
        assert!(
            portable.content().get(private_key).is_none(),
            "portable export retained {private_key}"
        );
    }
    assert_eq!(
        portable.content()["harmless_slash_prose"],
        json!("Choose input/output; the ratio is 3/4.")
    );
    assert_eq!(
        portable.content()["harmless_filename_prose"],
        json!("The terms private.mov and Documents are examples.")
    );
    assert_eq!(
        portable.content()["harmless_supported_versions"],
        json!("Supported versions v1.0/v2.0")
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
