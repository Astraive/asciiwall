use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use fs2::FileExt;
use crate::document::MAX_CONFIG_BYTES;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackupMode {
    ReplaceWithPrevious,
    PreserveLastGood,
}

pub(crate) trait ConfigStore: Send {
    fn read_main(&mut self) -> Result<Option<Vec<u8>>, StoreError>;
    fn read_backup(&mut self) -> Result<Option<Vec<u8>>, StoreError>;
    fn commit(
        &mut self,
        expected_current: Option<&[u8]>,
        contents: &[u8],
        backup_mode: BackupMode,
    ) -> Result<(), StoreError>;
}

pub(crate) struct FileStore {
    path: PathBuf,
    #[cfg(test)]
    fail_at: Option<PersistStage>,
    #[cfg(test)]
    fail_disk_full: bool,
}

impl FileStore {
    pub(crate) fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            #[cfg(test)]
            fail_at: None,
            #[cfg(test)]
            fail_disk_full: false,
        }
    }

    fn backup_path(&self) -> PathBuf {
        with_suffix(&self.path, ".bak")
    }

    fn unique_corrupt_path(&self) -> PathBuf {
        unique_sibling(&self.path, "corrupt")
    }
    fn writer_lock(&self) -> Result<WriterLock, StoreError> {
        let parent = parent_directory(&self.path);
        fs::create_dir_all(&parent)
            .map_err(|error| StoreError::io(StoreOperation::CreateDirectory, error))?;
        let path = with_suffix(&self.path, ".lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&path)
            .map_err(|error| StoreError::io(StoreOperation::Lock, error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|error| StoreError::io(StoreOperation::Lock, error))?;
        }
        FileExt::lock_exclusive(&file)
            .map_err(|error| StoreError::io(StoreOperation::Lock, error))?;
        Ok(WriterLock(file))
    }

    fn read_path(&self, path: &Path) -> Result<Option<Vec<u8>>, StoreError> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(StoreError::io(StoreOperation::Read, error)),
        };
        let metadata = file
            .metadata()
            .map_err(|error| StoreError::io(StoreOperation::Read, error))?;
        if metadata.len() > MAX_CONFIG_BYTES as u64 {
            return Err(StoreError::TooLarge {
                limit: MAX_CONFIG_BYTES,
                actual: metadata.len(),
            });
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take((MAX_CONFIG_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| StoreError::io(StoreOperation::Read, error))?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(StoreError::TooLarge {
                limit: MAX_CONFIG_BYTES,
                actual: bytes.len() as u64,
            });
        }
        Ok(Some(bytes))
    }

    fn fail(&mut self, stage: PersistStage) -> Result<(), StoreError> {
        #[cfg(test)]
        if self.fail_at == Some(stage) {
            self.fail_at = None;
            return Err(StoreError::Injected(stage));
        }
        #[cfg(test)]
        if self.fail_disk_full && stage == PersistStage::TempWrite {
            self.fail_disk_full = false;
            return Err(StoreError::io(
                StoreOperation::WriteTemporary,
                std::io::Error::new(
                    std::io::ErrorKind::StorageFull,
                    "injected disk-full failure",
                ),
            ));
        }
        #[cfg(not(test))]
        let _ = stage;
        Ok(())
    }

    fn atomic_write(
        &mut self,
        target: &Path,
        bytes: &[u8],
        replace_stage: PersistStage,
    ) -> Result<(), WriteFailure> {
        let parent = parent_directory(target);
        if let Err(error) = fs::create_dir_all(&parent) {
            return Err(WriteFailure::before_replace(StoreError::io(
                StoreOperation::CreateDirectory,
                error,
            )));
        }
        let mode = match restrictive_mode(target) {
            Ok(mode) => mode,
            Err(error) => {
                return Err(WriteFailure::before_replace(StoreError::io(
                    StoreOperation::ReadPermissions,
                    error,
                )));
            }
        };
        let (temporary_path, mut temporary_file) = match self.create_temp(target, mode) {
            Ok(value) => value,
            Err(error) => return Err(WriteFailure::before_replace(error)),
        };
        let mut temporary = TempFile::new(temporary_path);

        if let Err(error) = self.fail(PersistStage::TempWrite) {
            let prefix = bytes.len() / 2;
            if let Err(write_error) = temporary_file.write_all(&bytes[..prefix]) {
                drop(temporary_file);
                return Err(WriteFailure::before_replace(StoreError::io(
                    StoreOperation::WriteTemporary,
                    write_error,
                )));
            }
            drop(temporary_file);
            return Err(WriteFailure::before_replace(error));
        }
        if let Err(error) = temporary_file.write_all(bytes) {
            drop(temporary_file);
            return Err(WriteFailure::before_replace(StoreError::io(
                StoreOperation::WriteTemporary,
                error,
            )));
        }
        if let Err(error) = self.fail(PersistStage::FileSync) {
            drop(temporary_file);
            return Err(WriteFailure::before_replace(error));
        }
        if let Err(error) = temporary_file.sync_all() {
            drop(temporary_file);
            return Err(WriteFailure::before_replace(StoreError::io(
                StoreOperation::FlushTemporary,
                error,
            )));
        }
        drop(temporary_file);

        if let Err(error) = self.fail(replace_stage) {
            return Err(WriteFailure::before_replace(error));
        }
        if let Err(error) = fs::rename(temporary.path(), target) {
            return Err(WriteFailure::before_replace(StoreError::io(
                StoreOperation::Replace,
                error,
            )));
        }
        temporary.disarm();

        if let Err(error) = self.fail(directory_sync_stage(replace_stage)) {
            return Err(WriteFailure::after_replace(error));
        }
        if let Err(error) = sync_parent(&parent) {
            return Err(WriteFailure::after_replace(StoreError::io(
                StoreOperation::FlushDirectory,
                error,
            )));
        }
        Ok(())
    }

    fn create_temp(&mut self, target: &Path, mode: u32) -> Result<(PathBuf, File), StoreError> {
        self.fail(PersistStage::TempCreate)?;
        let parent = parent_directory(target);
        for _ in 0..32 {
            let candidate = unique_sibling(target, "tmp");
            match open_restrictive(&candidate, mode) {
                Ok(file) => return Ok((candidate, file)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(StoreError::io(StoreOperation::CreateTemporary, error)),
            }
        }
        let _ = parent;
        Err(StoreError::TemporaryNameExhausted)
    }

    fn restore_previous(
        &mut self,
        previous: Option<&[u8]>,
        archived_large_file: Option<&Path>,
    ) -> Result<(), StoreError> {
        if let Some(archive) = archived_large_file {
            match fs::remove_file(&self.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(StoreError::io(StoreOperation::Rollback, error));
                }
            }
            fs::rename(archive, &self.path)
                .map_err(|error| StoreError::io(StoreOperation::Rollback, error))?;
            return sync_parent(&parent_directory(&self.path))
                .map_err(|error| StoreError::io(StoreOperation::Rollback, error));
        }
        match previous {
            Some(bytes) => self
                .atomic_write_uninjected(&self.path, bytes)
                .map_err(|error| StoreError::io(StoreOperation::Rollback, error)),
            None => {
                match fs::remove_file(&self.path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(StoreError::io(StoreOperation::Rollback, error));
                    }
                }
                sync_parent(&parent_directory(&self.path))
                    .map_err(|error| StoreError::io(StoreOperation::Rollback, error))
            }
        }
    }

    fn atomic_write_uninjected(&self, target: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let parent = parent_directory(target);
        fs::create_dir_all(&parent)?;
        let mode = restrictive_mode(target)?;
        let (path, mut file) = create_temp_uninjected(target, mode)?;
        let mut temporary = TempFile::new(path);
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temporary.path(), target)?;
        temporary.disarm();
        sync_parent(&parent)
    }

    #[cfg(test)]
    pub(crate) fn inject_failure(&mut self, stage: PersistStage) {
        self.fail_at = Some(stage);
    }
    #[cfg(test)]
    pub(crate) fn inject_disk_full(&mut self) {
        self.fail_disk_full = true;
    }
}

impl ConfigStore for FileStore {
    fn read_main(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        self.read_path(&self.path)
    }

    fn read_backup(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        self.read_path(&self.backup_path())
    }

    fn commit(
        &mut self,
        expected_current: Option<&[u8]>,
        contents: &[u8],
        backup_mode: BackupMode,
    ) -> Result<(), StoreError> {
        let _writer_lock = self.writer_lock()?;
        let (previous, oversized) = match self.read_path(&self.path) {
            Ok(value) => (value, false),
            Err(StoreError::TooLarge { .. })
                if backup_mode == BackupMode::PreserveLastGood
                    && expected_current.is_none() =>
            {
                (None, true)
            }
            Err(StoreError::TooLarge { .. }) => {
                return Err(StoreError::Stale { current: None });
            }
            Err(error) => return Err(error),
        };
        if !oversized && previous.as_deref() != expected_current {
            return Err(StoreError::Stale { current: previous });
        }

        if oversized {
            let archive = self.unique_corrupt_path();
            fs::rename(&self.path, &archive)
                .map_err(|error| StoreError::io(StoreOperation::ArchiveCorrupt, error))?;
            return match self.atomic_write(&self.path.clone(), contents, PersistStage::MainReplace) {
                Ok(()) => Ok(()),
                Err(failure) => {
                    if let Err(rollback) = self.restore_previous(None, Some(&archive)) {
                        Err(StoreError::CommitIndeterminate {
                            cause: failure.error.to_string(),
                            rollback: rollback.to_string(),
                        })
                    } else {
                        Err(failure.error)
                    }
                }
            };
        }

        match backup_mode {
            BackupMode::ReplaceWithPrevious => {
                if let Some(previous_bytes) = previous.as_deref() {
                    let backup = self.backup_path();
                    if let Err(failure) = self.atomic_write(
                        &backup,
                        previous_bytes,
                        PersistStage::BackupReplace,
                    ) {
                        return Err(failure.error);
                    }
                }
            }
            BackupMode::PreserveLastGood => {
                if let Some(previous_bytes) = previous.as_deref() {
                    let archive = self.unique_corrupt_path();
                    if let Err(failure) = self.atomic_write(
                        &archive,
                        previous_bytes,
                        PersistStage::ArchiveReplace,
                    ) {
                        return Err(failure.error);
                    }
                }
            }
        }

        match self.atomic_write(&self.path.clone(), contents, PersistStage::MainReplace) {
            Ok(()) => Ok(()),
            Err(failure) if failure.replaced => {
                if let Err(rollback) = self.restore_previous(previous.as_deref(), None) {
                    Err(StoreError::CommitIndeterminate {
                        cause: failure.error.to_string(),
                        rollback: rollback.to_string(),
                    })
                } else {
                    Err(failure.error)
                }
            }
            Err(failure) => Err(failure.error),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistStage {
    TempCreate,
    TempWrite,
    FileSync,
    BackupReplace,
    ArchiveReplace,
    MainReplace,
    BackupDirectorySync,
    ArchiveDirectorySync,
    MainDirectorySync,
}
fn directory_sync_stage(replace_stage: PersistStage) -> PersistStage {
    match replace_stage {
        PersistStage::BackupReplace => PersistStage::BackupDirectorySync,
        PersistStage::ArchiveReplace => PersistStage::ArchiveDirectorySync,
        PersistStage::MainReplace => PersistStage::MainDirectorySync,
        _ => unreachable!("only file replacement stages reach this helper"),
    }
}

#[derive(Debug)]
struct WriteFailure {
    error: StoreError,
    replaced: bool,
}

impl WriteFailure {
    fn before_replace(error: StoreError) -> Self {
        Self {
            error,
            replaced: false,
        }
    }

    fn after_replace(error: StoreError) -> Self {
        Self {
            error,
            replaced: true,
        }
    }
}

#[derive(Debug)]
struct TempFile {
    path: PathBuf,
    armed: bool,
}

impl TempFile {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}
struct WriterLock(File);

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreError {
    Io {
        operation: StoreOperation,
        kind: std::io::ErrorKind,
        message: String,
    },
    TooLarge { limit: usize, actual: u64 },
    Stale { current: Option<Vec<u8>> },
    TemporaryNameExhausted,
    CommitIndeterminate { cause: String, rollback: String },
    Injected(PersistStage),
}

impl StoreError {
    fn io(operation: StoreOperation, error: std::io::Error) -> Self {
        let mut message = error.to_string();
        if message.len() > 256 {
            let mut end = 256;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
        Self::Io {
            operation,
            kind: error.kind(),
            message,
        }
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "settings storage error: {self:?}")
    }
}

impl std::error::Error for StoreError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreOperation {
    Read,
    Lock,
    CreateDirectory,
    ReadPermissions,
    CreateTemporary,
    WriteTemporary,
    FlushTemporary,
    Replace,
    FlushDirectory,
    ArchiveCorrupt,
    Rollback,
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map_or_else(|| OsString::from("settings.toml"), OsString::from);
    name.push(suffix);
    parent_directory(path).join(name)
}

fn unique_sibling(path: &Path, role: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map_or_else(|| OsString::from("settings.toml"), OsString::from);
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    name.push(format!(".{role}-{}-{sequence}", std::process::id()));
    parent_directory(path).join(name)
}

fn parent_directory(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn restrictive_mode(path: &Path) -> std::io::Result<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match fs::metadata(path) {
            Ok(metadata) => {
                let owner_bits = metadata.permissions().mode() & 0o600;
                Ok(if owner_bits == 0 { 0o600 } else { owner_bits })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0o600),
            Err(error) => Err(error),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(0o600)
    }
}

fn open_restrictive(path: &Path, mode: u32) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(mode)) {
            drop(file);
            let _ = fs::remove_file(path);
            return Err(error);
        }
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(file)
}

fn create_temp_uninjected(target: &Path, mode: u32) -> std::io::Result<(PathBuf, File)> {
    for _ in 0..32 {
        let candidate = unique_sibling(target, "rollback");
        match open_restrictive(&candidate, mode) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate unique rollback file",
    ))
}

fn sync_parent(parent: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::{BackupMode, FileStore, PersistStage, StoreError, StoreOperation};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ascii-config-store-{}-{sequence}",
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

    #[test]
    fn injected_write_flush_backup_replace_and_main_replace_failures_keep_old_bytes() {
        for stage in [
            PersistStage::TempCreate,
            PersistStage::TempWrite,
            PersistStage::FileSync,
            PersistStage::BackupReplace,
            PersistStage::BackupDirectorySync,
            PersistStage::MainReplace,
            PersistStage::MainDirectorySync,
        ] {
            let directory = TestDirectory::new();
            let path = directory.0.join("settings.toml");
            let backup = directory.0.join("settings.toml.bak");
            fs::write(&path, b"last-valid-main").unwrap();
            fs::write(&backup, b"prior-backup").unwrap();

            let mut store = FileStore::new(&path);
            store.inject_failure(stage);
            assert!(store
                .commit(
                    Some(b"last-valid-main"),
                    b"candidate-settings",
                    BackupMode::ReplaceWithPrevious,
                )
                .is_err());
            assert_eq!(fs::read(&path).unwrap(), b"last-valid-main");
        }
    }

    #[test]
    fn repair_archive_does_not_replace_last_good_backup() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let backup = directory.0.join("settings.toml.bak");
        fs::write(&path, b"corrupt-main").unwrap();
        fs::write(&backup, b"last-good-backup").unwrap();

        let mut store = FileStore::new(&path);
        store
            .commit(
                Some(b"corrupt-main"),
                b"safe-default-settings",
                BackupMode::PreserveLastGood,
            )
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"safe-default-settings");
        assert_eq!(fs::read(&backup).unwrap(), b"last-good-backup");
        let archive = fs::read_dir(&directory.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.corrupt-")
            })
            .expect("corrupt main archived separately");
        assert_eq!(fs::read(archive.path()).unwrap(), b"corrupt-main");
    }
    #[test]
    fn repair_archive_failures_leave_corrupt_main_and_last_good_backup_usable() {
        for stage in [
            PersistStage::ArchiveReplace,
            PersistStage::ArchiveDirectorySync,
        ] {
            let directory = TestDirectory::new();
            let path = directory.0.join("settings.toml");
            let backup = directory.0.join("settings.toml.bak");
            fs::write(&path, b"corrupt-main").unwrap();
            fs::write(&backup, b"last-good-backup").unwrap();

            let mut store = FileStore::new(&path);
            store.inject_failure(stage);
            assert!(store
                .commit(
                    Some(b"corrupt-main"),
                    b"safe-default-settings",
                    BackupMode::PreserveLastGood,
                )
                .is_err());
            assert_eq!(fs::read(&path).unwrap(), b"corrupt-main");
            assert_eq!(fs::read(&backup).unwrap(), b"last-good-backup");
        }
    }
    #[test]
    fn disk_full_after_partial_temp_write_keeps_last_valid_bytes() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let backup = directory.0.join("settings.toml.bak");
        fs::write(&path, b"last-valid-main").unwrap();
        fs::write(&backup, b"prior-backup").unwrap();

        let mut store = FileStore::new(&path);
        store.inject_disk_full();
        assert!(matches!(
            store.commit(
                Some(b"last-valid-main"),
                b"candidate-settings",
                BackupMode::ReplaceWithPrevious,
            ),
            Err(StoreError::Io {
                operation: StoreOperation::WriteTemporary,
                kind: std::io::ErrorKind::StorageFull,
                ..
            })
        ));
        assert_eq!(fs::read(&path).unwrap(), b"last-valid-main");
        assert_eq!(fs::read(&backup).unwrap(), b"last-valid-main");
    }
}
