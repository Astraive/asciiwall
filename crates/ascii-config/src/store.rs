use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::document::MAX_CONFIG_BYTES;
use fs2::FileExt;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackupMode {
    ReplaceWithPrevious,
    PreserveLastGood,
}

pub(crate) trait ConfigStore: Send {
    fn read_main(&mut self) -> Result<Option<Vec<u8>>, StoreError>;
    /// Re-reads the main document while holding the writer lock after a stale migration CAS.
    fn read_main_locked(&mut self) -> Result<Option<Vec<u8>>, StoreError>;
    fn read_backup(&mut self) -> Result<Option<Vec<u8>>, StoreError>;
    fn commit(
        &mut self,
        expected_current: Option<&[u8]>,
        contents: &[u8],
        backup_mode: BackupMode,
    ) -> Result<(), StoreError>;
}

#[cfg(all(test, windows))]
type TargetRaceHook = Box<dyn FnOnce(&Path) + Send>;
#[cfg(all(test, windows))]
type TemporaryDaclHook = Box<dyn FnOnce(&File) + Send>;

pub(crate) struct FileStore {
    path: PathBuf,
    #[cfg(test)]
    fail_at: Option<PersistStage>,
    #[cfg(test)]
    fail_disk_full_after_temp_writes: Option<usize>,
    #[cfg(test)]
    fail_indeterminate_replace_at: Option<PersistStage>,
    #[cfg(test)]
    fail_rollback_indeterminate_replace: std::cell::Cell<bool>,
    #[cfg(all(test, windows))]
    inject_after_absent_check: Option<TargetRaceHook>,
    #[cfg(all(test, windows))]
    inspect_temporary_dacl: Option<TemporaryDaclHook>,
}

impl FileStore {
    pub(crate) fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            #[cfg(test)]
            fail_at: None,
            #[cfg(test)]
            fail_disk_full_after_temp_writes: None,
            #[cfg(test)]
            fail_indeterminate_replace_at: None,
            #[cfg(test)]
            fail_rollback_indeterminate_replace: std::cell::Cell::new(false),
            #[cfg(all(test, windows))]
            inject_after_absent_check: None,
            #[cfg(all(test, windows))]
            inspect_temporary_dacl: None,
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
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(StoreError::io(StoreOperation::Read, error)),
        };
        let metadata = file
            .metadata()
            .map_err(|error| StoreError::io(StoreOperation::Read, error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = owner_only_mode(metadata.permissions().mode());
            file.set_permissions(fs::Permissions::from_mode(mode))
                .map_err(|error| StoreError::io(StoreOperation::ReadPermissions, error))?;
        }
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
        match (stage, self.fail_disk_full_after_temp_writes) {
            (PersistStage::TempWrite, Some(0)) => {
                self.fail_disk_full_after_temp_writes = None;
                return Err(StoreError::io(
                    StoreOperation::WriteTemporary,
                    std::io::Error::new(
                        std::io::ErrorKind::StorageFull,
                        "injected disk-full failure",
                    ),
                ));
            }
            (PersistStage::TempWrite, Some(writes_before_failure)) => {
                self.fail_disk_full_after_temp_writes = Some(writes_before_failure - 1);
            }
            _ => {}
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
        target_expectation: TargetExpectation,
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
        #[cfg(all(test, windows))]
        if target_expectation == TargetExpectation::Absent
            && let Some(inject) = self.inject_after_absent_check.take()
        {
            inject(target);
        }
        if let Err(error) = prepare_target_dacl(target, &temporary_file, target_expectation) {
            drop(temporary_file);
            return Err(WriteFailure::before_replace(StoreError::io(
                StoreOperation::CreateTemporary,
                error,
            )));
        }
        #[cfg(all(test, windows))]
        if let Some(inspect) = self.inspect_temporary_dacl.take() {
            inspect(&temporary_file);
        }

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

        let replace_result =
            self.replace_temporary(temporary.path(), target, replace_stage, target_expectation);
        if let Err(failure) = replace_result {
            let ReplaceFailure {
                error: replace_error,
                replaced,
                indeterminate,
                displaced_path,
            } = failure;
            let error = StoreError::io(StoreOperation::Replace, replace_error);
            if indeterminate {
                let replacement_path = temporary.path().to_path_buf();
                temporary.disarm();
                return Err(WriteFailure::indeterminate(
                    error,
                    replacement_path,
                    displaced_path,
                ));
            }
            return Err(if replaced {
                WriteFailure::after_replace(error)
            } else {
                WriteFailure::before_replace(error)
            });
        }
        if replace_stage == PersistStage::ArchiveReplace
            && let Err(error) = fs::remove_file(temporary.path())
        {
            return Err(WriteFailure::after_replace(StoreError::io(
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

    fn replace_temporary(
        &mut self,
        temporary: &Path,
        target: &Path,
        replace_stage: PersistStage,
        target_expectation: TargetExpectation,
    ) -> Result<(), ReplaceFailure> {
        #[cfg(test)]
        if self.fail_indeterminate_replace_at == Some(replace_stage) {
            self.fail_indeterminate_replace_at = None;
            return simulated_indeterminate_replace(target);
        }

        if replace_stage == PersistStage::ArchiveReplace {
            fs::hard_link(temporary, target).map_err(|error| ReplaceFailure {
                error,
                replaced: false,
                indeterminate: false,
                displaced_path: None,
            })
        } else if target_expectation == TargetExpectation::Absent {
            publish_new_file(temporary, target)
        } else {
            replace_file(temporary, target)
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_indeterminate_replace(&mut self, stage: PersistStage) {
        self.fail_indeterminate_replace_at = Some(stage);
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
            Some(bytes) => self.atomic_write_uninjected(&self.path, bytes),
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

    fn restore_archived_if_absent(&self, archive: &Path) -> Result<(), StoreError> {
        publish_new_file(archive, &self.path)
            .map_err(|failure| StoreError::io(StoreOperation::Rollback, failure.error))?;
        sync_parent(&parent_directory(&self.path))
            .map_err(|error| StoreError::io(StoreOperation::Rollback, error))
    }

    fn atomic_write_uninjected(&self, target: &Path, bytes: &[u8]) -> Result<(), StoreError> {
        let parent = parent_directory(target);
        fs::create_dir_all(&parent)
            .map_err(|error| StoreError::io(StoreOperation::Rollback, error))?;
        let mode = restrictive_mode(target)
            .map_err(|error| StoreError::io(StoreOperation::Rollback, error))?;
        let (path, mut file) = create_temp_uninjected(target, mode)
            .map_err(|error| StoreError::io(StoreOperation::Rollback, error))?;
        let mut temporary = TempFile::new(path);
        if let Err(error) = prepare_target_dacl(target, &file, TargetExpectation::Present) {
            drop(file);
            return Err(StoreError::io(StoreOperation::Rollback, error));
        }
        if let Err(error) = file.write_all(bytes) {
            drop(file);
            return Err(StoreError::io(StoreOperation::Rollback, error));
        }
        if let Err(error) = file.sync_all() {
            drop(file);
            return Err(StoreError::io(StoreOperation::Rollback, error));
        }
        drop(file);

        #[cfg(test)]
        let replace_result = if self.fail_rollback_indeterminate_replace.replace(false) {
            simulated_indeterminate_replace(target)
        } else {
            replace_file(temporary.path(), target)
        };
        #[cfg(not(test))]
        let replace_result = replace_file(temporary.path(), target);

        if let Err(failure) = replace_result {
            let ReplaceFailure {
                error,
                indeterminate,
                displaced_path,
                ..
            } = failure;
            if indeterminate {
                let replacement_path = temporary.path().to_path_buf();
                temporary.disarm();
                return Err(indeterminate_with_artifacts(
                    StoreError::io(StoreOperation::Rollback, error),
                    &replacement_path,
                    displaced_path.as_deref(),
                ));
            }
            return Err(StoreError::io(StoreOperation::Rollback, error));
        }
        temporary.disarm();
        sync_parent(&parent).map_err(|error| StoreError::io(StoreOperation::Rollback, error))
    }

    #[cfg(test)]
    pub(crate) fn inject_failure(&mut self, stage: PersistStage) {
        self.fail_at = Some(stage);
    }
    #[cfg(test)]
    pub(crate) fn inject_disk_full_after_temp_writes(&mut self, writes: usize) {
        self.fail_disk_full_after_temp_writes = Some(writes);
    }

    #[cfg(test)]
    pub(crate) fn inject_rollback_indeterminate_replace(&mut self) {
        self.fail_rollback_indeterminate_replace.set(true);
    }
    #[cfg(all(test, windows))]
    pub(crate) fn inject_after_absent_check(
        &mut self,
        inject: impl FnOnce(&Path) + Send + 'static,
    ) {
        self.inject_after_absent_check = Some(Box::new(inject));
    }

    #[cfg(all(test, windows))]
    pub(crate) fn inspect_temporary_dacl(&mut self, inspect: impl FnOnce(&File) + Send + 'static) {
        self.inspect_temporary_dacl = Some(Box::new(inspect));
    }
}

impl ConfigStore for FileStore {
    fn read_main(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        self.read_path(&self.path)
    }
    fn read_main_locked(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        let _writer_lock = self.writer_lock()?;
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
                if backup_mode == BackupMode::PreserveLastGood && expected_current.is_none() =>
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
            self.fail(PersistStage::ArchiveReplace)?;
            archive_without_replace(&self.path, &archive)
                .map_err(|error| StoreError::io(StoreOperation::ArchiveCorrupt, error))?;
            return match self.atomic_write(
                &self.path.clone(),
                contents,
                PersistStage::MainReplace,
                TargetExpectation::Absent,
            ) {
                Ok(()) => Ok(()),
                Err(failure) if failure.indeterminate => {
                    Err(indeterminate_without_rollback(failure))
                }
                Err(failure) => {
                    let rollback = if failure.replaced {
                        self.restore_previous(None, Some(&archive))
                    } else {
                        self.restore_archived_if_absent(&archive)
                    };
                    if let Err(rollback) = rollback {
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
                    let target_expectation = initial_target_expectation(&backup)
                        .map_err(|error| StoreError::io(StoreOperation::ReadPermissions, error))?;
                    if let Err(failure) = self.atomic_write(
                        &backup,
                        previous_bytes,
                        PersistStage::BackupReplace,
                        target_expectation,
                    ) {
                        return Err(if failure.indeterminate {
                            indeterminate_without_rollback(failure)
                        } else {
                            failure.error
                        });
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
                        TargetExpectation::Absent,
                    ) {
                        return Err(if failure.indeterminate {
                            indeterminate_without_rollback(failure)
                        } else {
                            failure.error
                        });
                    }
                }
            }
        }

        let target_expectation = if previous.is_some() {
            TargetExpectation::Present
        } else {
            TargetExpectation::Absent
        };
        match self.atomic_write(
            &self.path.clone(),
            contents,
            PersistStage::MainReplace,
            target_expectation,
        ) {
            Ok(()) => Ok(()),
            Err(failure) if failure.indeterminate => Err(indeterminate_without_rollback(failure)),
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
enum TargetExpectation {
    Absent,
    Present,
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
    indeterminate: bool,
    replacement_path: Option<PathBuf>,
    displaced_path: Option<PathBuf>,
}

impl WriteFailure {
    fn before_replace(error: StoreError) -> Self {
        Self {
            error,
            replaced: false,
            indeterminate: false,
            replacement_path: None,
            displaced_path: None,
        }
    }

    fn after_replace(error: StoreError) -> Self {
        Self {
            error,
            replaced: true,
            indeterminate: false,
            replacement_path: None,
            displaced_path: None,
        }
    }

    fn indeterminate(
        error: StoreError,
        replacement_path: PathBuf,
        displaced_path: Option<PathBuf>,
    ) -> Self {
        Self {
            error,
            replaced: false,
            indeterminate: true,
            replacement_path: Some(replacement_path),
            displaced_path,
        }
    }
}

fn indeterminate_without_rollback(failure: WriteFailure) -> StoreError {
    let WriteFailure {
        error,
        replacement_path,
        displaced_path,
        ..
    } = failure;
    indeterminate_with_artifacts(
        error,
        replacement_path
            .as_deref()
            .expect("indeterminate writes retain their replacement path"),
        displaced_path.as_deref(),
    )
}

fn indeterminate_with_artifacts(
    error: StoreError,
    replacement_path: &Path,
    displaced_path: Option<&Path>,
) -> StoreError {
    StoreError::CommitIndeterminate {
        cause: error.to_string(),
        rollback: format!(
            "not attempted to avoid overwriting a potentially raced target; \
             retained replacement artifact at {:?}; displaced-original backup candidate at {:?}",
            replacement_path, displaced_path,
        ),
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

#[derive(Clone, Eq, PartialEq)]
pub enum StoreError {
    Io {
        operation: StoreOperation,
        kind: std::io::ErrorKind,
        message: String,
    },
    TooLarge {
        limit: usize,
        actual: u64,
    },
    Stale {
        current: Option<Vec<u8>>,
    },
    TemporaryNameExhausted,
    CommitIndeterminate {
        cause: String,
        rollback: String,
    },
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
impl std::fmt::Debug for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io {
                operation,
                kind,
                message,
            } => formatter
                .debug_struct("Io")
                .field("operation", operation)
                .field("kind", kind)
                .field("message", message)
                .finish(),
            Self::TooLarge { limit, actual } => formatter
                .debug_struct("TooLarge")
                .field("limit", limit)
                .field("actual", actual)
                .finish(),
            Self::Stale { current } => formatter
                .debug_struct("Stale")
                .field("current", &current.as_ref().map(|_| "[REDACTED]"))
                .finish(),
            Self::TemporaryNameExhausted => formatter.write_str("TemporaryNameExhausted"),
            Self::CommitIndeterminate { cause, rollback } => formatter
                .debug_struct("CommitIndeterminate")
                .field("cause", cause)
                .field("rollback", rollback)
                .finish(),
            Self::Injected(stage) => formatter.debug_tuple("Injected").field(stage).finish(),
        }
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommitIndeterminate { cause, rollback } => write!(
                formatter,
                "settings storage commit outcome is indeterminate: {cause}; {rollback}"
            ),
            _ => write!(formatter, "settings storage error: {self:?}"),
        }
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

fn archive_without_replace(source: &Path, archive: &Path) -> std::io::Result<()> {
    fs::hard_link(source, archive)?;
    if let Err(error) = fs::remove_file(source) {
        let _ = fs::remove_file(archive);
        return Err(error);
    }
    Ok(())
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

fn initial_target_expectation(path: &Path) -> std::io::Result<TargetExpectation> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(TargetExpectation::Present),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(TargetExpectation::Absent),
        Err(error) => Err(error),
    }
}

fn restrictive_mode(path: &Path) -> std::io::Result<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match fs::metadata(path) {
            Ok(metadata) => Ok(owner_only_mode(metadata.permissions().mode())),
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
#[cfg(unix)]
fn owner_only_mode(mode: u32) -> u32 {
    let owner_bits = mode & 0o600;
    if owner_bits == 0 { 0o600 } else { owner_bits }
}

fn open_restrictive(path: &Path, mode: u32) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use winapi::um::winnt::{GENERIC_WRITE, READ_CONTROL, WRITE_DAC};

        // Block other opens until the inherited DACL has been replaced and checked.
        options
            .access_mode(GENERIC_WRITE | READ_CONTROL | WRITE_DAC)
            .share_mode(0);
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
#[derive(Debug)]
struct ReplaceFailure {
    error: std::io::Error,
    replaced: bool,
    indeterminate: bool,
    displaced_path: Option<PathBuf>,
}

#[cfg(test)]
fn simulated_indeterminate_replace(target: &Path) -> Result<(), ReplaceFailure> {
    let displaced_path = unique_sibling(target, "replaced");
    match fs::rename(target, &displaced_path) {
        Ok(()) => Err(ReplaceFailure {
            error: std::io::Error::from_raw_os_error(1177),
            replaced: false,
            indeterminate: true,
            displaced_path: Some(displaced_path),
        }),
        Err(error) => Err(ReplaceFailure {
            error,
            replaced: false,
            indeterminate: false,
            displaced_path: None,
        }),
    }
}

#[cfg(windows)]
fn replace_file(temporary: &Path, target: &Path) -> Result<(), ReplaceFailure> {
    let backup = unique_sibling(target, "replaced");
    crate::windows_acl::replace_file(temporary, target, &backup).map_err(|failure| ReplaceFailure {
        error: failure.error,
        replaced: failure.replaced,
        indeterminate: failure.indeterminate,
        displaced_path: failure.indeterminate.then(|| backup.clone()),
    })
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, target: &Path) -> Result<(), ReplaceFailure> {
    fs::rename(temporary, target).map_err(|error| ReplaceFailure {
        error,
        replaced: false,
        indeterminate: false,
        displaced_path: None,
    })
}

#[cfg(windows)]
fn preserve_target_dacl(target: &Path, temporary: &File) -> std::io::Result<()> {
    crate::windows_acl::preserve_target_dacl(target, temporary)
}

#[cfg(windows)]
fn prepare_target_dacl(
    target: &Path,
    temporary: &File,
    target_expectation: TargetExpectation,
) -> std::io::Result<()> {
    match target_expectation {
        TargetExpectation::Absent => crate::windows_acl::set_restrictive_dacl(temporary),
        TargetExpectation::Present => preserve_target_dacl(target, temporary),
    }
}

#[cfg(not(windows))]
fn prepare_target_dacl(
    _target: &Path,
    _temporary: &File,
    _target_expectation: TargetExpectation,
) -> std::io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn publish_new_file(temporary: &Path, target: &Path) -> Result<(), ReplaceFailure> {
    crate::windows_acl::publish_new_file(temporary, target).map_err(|failure| ReplaceFailure {
        error: failure.error,
        replaced: failure.replaced,
        indeterminate: failure.indeterminate,
        displaced_path: None,
    })
}

#[cfg(not(windows))]
fn publish_new_file(temporary: &Path, target: &Path) -> Result<(), ReplaceFailure> {
    fs::hard_link(temporary, target).map_err(|error| ReplaceFailure {
        error,
        replaced: false,
        indeterminate: false,
        displaced_path: None,
    })?;
    fs::remove_file(temporary).map_err(|error| ReplaceFailure {
        error,
        replaced: true,
        indeterminate: false,
        displaced_path: None,
    })
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
    use super::{
        BackupMode, ConfigStore, FileStore, PersistStage, StoreError, StoreOperation,
        archive_without_replace,
    };
    #[cfg(windows)]
    use std::sync::{Arc, Mutex};
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
    fn corrupt_archive_name_collision_preserves_both_files() {
        let directory = TestDirectory::new();
        let source = directory.0.join("settings.toml");
        let archive = directory.0.join("settings.toml.corrupt-reused-name");
        fs::write(&source, b"corrupt-main").unwrap();
        fs::write(&archive, b"existing-archive").unwrap();

        let error = archive_without_replace(&source, &archive).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&source).unwrap(), b"corrupt-main");
        assert_eq!(fs::read(&archive).unwrap(), b"existing-archive");
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
            assert!(
                store
                    .commit(
                        Some(b"last-valid-main"),
                        b"candidate-settings",
                        BackupMode::ReplaceWithPrevious,
                    )
                    .is_err()
            );
            assert_eq!(fs::read(&path).unwrap(), b"last-valid-main");
        }
    }

    #[test]
    fn indeterminate_replace_preserves_and_reports_replacement_and_displaced_original() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        fs::write(&path, b"last-valid-main").unwrap();

        let mut store = FileStore::new(&path);
        store.inject_indeterminate_replace(PersistStage::MainReplace);
        let error = store
            .commit(
                Some(b"last-valid-main"),
                b"candidate-settings",
                BackupMode::ReplaceWithPrevious,
            )
            .unwrap_err();
        assert!(matches!(&error, StoreError::CommitIndeterminate { .. }));

        assert!(!path.exists());
        let entries = fs::read_dir(&directory.0)
            .unwrap()
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        let replacement = entries
            .iter()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.tmp-")
            })
            .expect("indeterminate replacement retained at temporary path");
        assert_eq!(fs::read(replacement.path()).unwrap(), b"candidate-settings");
        let displaced_original = entries
            .iter()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.replaced-")
            })
            .expect("displaced original retained at replacement backup path");
        assert_eq!(
            fs::read(displaced_original.path()).unwrap(),
            b"last-valid-main"
        );
        let diagnostic = error.to_string();
        assert!(diagnostic.contains(&format!("{:?}", replacement.path())));
        assert!(diagnostic.contains(&format!("{:?}", displaced_original.path())));
        let public_diagnostic = crate::ConfigError::from(error.clone()).to_string();
        assert_eq!(
            public_diagnostic,
            format!("configuration error: {diagnostic}")
        );
        assert!(public_diagnostic.contains(&format!("{:?}", replacement.path())));
        assert!(public_diagnostic.contains(&format!("{:?}", displaced_original.path())));
    }

    #[test]
    fn indeterminate_backup_replace_reports_and_preserves_both_artifacts() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let backup = directory.0.join("settings.toml.bak");
        fs::write(&path, b"last-valid-main").unwrap();
        fs::write(&backup, b"prior-backup").unwrap();

        let mut store = FileStore::new(&path);
        store.inject_indeterminate_replace(PersistStage::BackupReplace);
        let error = store
            .commit(
                Some(b"last-valid-main"),
                b"candidate-settings",
                BackupMode::ReplaceWithPrevious,
            )
            .unwrap_err();
        assert!(matches!(&error, StoreError::CommitIndeterminate { .. }));

        assert_eq!(fs::read(&path).unwrap(), b"last-valid-main");
        assert!(!backup.exists());
        let entries = fs::read_dir(&directory.0)
            .unwrap()
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        let replacement = entries
            .iter()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.bak.tmp-")
            })
            .expect("indeterminate backup replacement retained at temporary path");
        assert_eq!(fs::read(replacement.path()).unwrap(), b"last-valid-main");
        let displaced_backup = entries
            .iter()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.bak.replaced-")
            })
            .expect("displaced backup retained at replacement backup path");
        assert_eq!(fs::read(displaced_backup.path()).unwrap(), b"prior-backup");
        let diagnostic = error.to_string();
        assert!(diagnostic.contains(&format!("{:?}", replacement.path())));
        assert!(diagnostic.contains(&format!("{:?}", displaced_backup.path())));
    }

    #[test]
    fn rollback_indeterminate_replace_preserves_and_reports_both_artifacts() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        fs::write(&path, b"last-valid-main").unwrap();

        let mut store = FileStore::new(&path);
        store.inject_failure(PersistStage::MainDirectorySync);
        store.inject_rollback_indeterminate_replace();
        let error = store
            .commit(
                Some(b"last-valid-main"),
                b"candidate-settings",
                BackupMode::ReplaceWithPrevious,
            )
            .unwrap_err();
        assert!(matches!(&error, StoreError::CommitIndeterminate { .. }));

        assert!(!path.exists());
        let entries = fs::read_dir(&directory.0)
            .unwrap()
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        let replacement = entries
            .iter()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.rollback-")
            })
            .expect("indeterminate rollback replacement retained at temporary path");
        assert_eq!(fs::read(replacement.path()).unwrap(), b"last-valid-main");
        let displaced_main = entries
            .iter()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.replaced-")
            })
            .expect("current main retained at replacement backup path");
        assert_eq!(
            fs::read(displaced_main.path()).unwrap(),
            b"candidate-settings"
        );
        let diagnostic = error.to_string();
        assert!(diagnostic.contains(&format!("{:?}", replacement.path())));
        assert!(diagnostic.contains(&format!("{:?}", displaced_main.path())));
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
            assert!(
                store
                    .commit(
                        Some(b"corrupt-main"),
                        b"safe-default-settings",
                        BackupMode::PreserveLastGood,
                    )
                    .is_err()
            );
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
        store.inject_disk_full_after_temp_writes(1);
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

    #[test]
    fn stale_error_debug_and_display_redact_current_bytes() {
        let secret = b"settings-secret-marker";
        let error = StoreError::Stale {
            current: Some(secret.to_vec()),
        };
        let config_error = crate::error::ConfigError::from(error.clone());
        let raw_bytes = format!("{:?}", secret.to_vec());
        let formatted = [
            format!("{error:?}"),
            format!("{error}"),
            format!("{config_error:?}"),
            format!("{config_error}"),
        ];

        for output in &formatted {
            assert!(!output.contains("settings-secret-marker"));
            assert!(!output.contains(&raw_bytes));
            assert!(output.contains("[REDACTED]"));
        }
        assert_eq!(
            error,
            StoreError::Stale {
                current: Some(secret.to_vec())
            }
        );
    }
    #[cfg(unix)]
    #[test]
    fn reads_tighten_existing_main_and_backup_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let backup = directory.0.join("settings.toml.bak");
        fs::write(&path, b"main").unwrap();
        fs::write(&backup, b"backup").unwrap();
        for existing in [&path, &backup] {
            fs::set_permissions(existing, fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(
                fs::metadata(existing).unwrap().permissions().mode() & 0o777,
                0o644
            );
        }

        let mut store = FileStore::new(&path);
        assert_eq!(store.read_main().unwrap(), Some(b"main".to_vec()));
        assert_eq!(store.read_backup().unwrap(), Some(b"backup".to_vec()));
        for existing in [&path, &backup] {
            assert_eq!(
                fs::metadata(existing).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn oversized_repair_archive_remains_owner_only_after_rename() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        fs::write(&path, vec![b'x'; crate::document::MAX_CONFIG_BYTES + 1]).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let mut store = FileStore::new(&path);
        store
            .commit(None, b"safe-settings", BackupMode::PreserveLastGood)
            .unwrap();

        let archive = fs::read_dir(&directory.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.corrupt-")
            })
            .expect("oversized config archived separately");
        assert_eq!(
            fs::metadata(archive.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[cfg(windows)]
    #[test]
    fn replacement_preserves_existing_protected_windows_dacl() {
        use winapi::um::winnt::SE_DACL_PROTECTED;

        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        fs::write(&path, b"last-valid-main").unwrap();
        crate::windows_acl::protect_windows_dacl(&path).unwrap();
        let before = crate::windows_acl::windows_dacl_snapshot(&path).unwrap();
        assert_ne!(before.control & SE_DACL_PROTECTED, 0);

        let mut store = FileStore::new(&path);
        store
            .commit(
                Some(b"last-valid-main"),
                b"candidate-settings",
                BackupMode::ReplaceWithPrevious,
            )
            .unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"candidate-settings");
        assert_eq!(
            crate::windows_acl::windows_dacl_snapshot(&path).unwrap(),
            before
        );
    }

    #[cfg(windows)]
    #[test]
    fn first_creation_restricts_main_and_backup_windows_dacls() {
        use winapi::um::winnt::SE_DACL_PROTECTED;

        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let backup = directory.0.join("settings.toml.bak");
        let expected_dacl = crate::windows_acl::restrictive_windows_dacl_snapshot().unwrap();
        let mut store = FileStore::new(&path);

        store
            .commit(None, b"first-settings", BackupMode::ReplaceWithPrevious)
            .unwrap();
        let main_dacl = crate::windows_acl::windows_dacl_snapshot(&path).unwrap();
        assert_eq!(main_dacl, expected_dacl);
        assert_ne!(main_dacl.control & SE_DACL_PROTECTED, 0);

        store
            .commit(
                Some(b"first-settings"),
                b"second-settings",
                BackupMode::ReplaceWithPrevious,
            )
            .unwrap();

        assert_eq!(fs::read(&backup).unwrap(), b"first-settings");
        assert_eq!(
            crate::windows_acl::windows_dacl_snapshot(&path).unwrap(),
            expected_dacl
        );
        assert_eq!(
            crate::windows_acl::windows_dacl_snapshot(&backup).unwrap(),
            expected_dacl
        );
    }

    #[cfg(windows)]
    #[test]
    fn replacement_uses_dacl_from_atomic_target() {
        use std::io::Write;
        use winapi::um::winnt::SE_DACL_PROTECTED;

        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        fs::write(&path, b"old-settings").unwrap();
        crate::windows_acl::protect_windows_dacl(&path).unwrap();
        let copied_dacl = crate::windows_acl::windows_dacl_snapshot(&path).unwrap();
        assert_ne!(copied_dacl.control & SE_DACL_PROTECTED, 0);

        let temporary_path = super::unique_sibling(&path, "dacl-race");
        let mut temporary_file = super::open_restrictive(&temporary_path, 0o600).unwrap();
        crate::windows_acl::preserve_target_dacl(&path, &temporary_file).unwrap();
        temporary_file.write_all(b"new-settings").unwrap();
        temporary_file.sync_all().unwrap();
        drop(temporary_file);

        crate::windows_acl::unprotect_windows_dacl(&path).unwrap();
        let current_dacl = crate::windows_acl::windows_dacl_snapshot(&path).unwrap();
        assert_eq!(current_dacl.control & SE_DACL_PROTECTED, 0);
        assert_ne!(current_dacl, copied_dacl);

        super::replace_file(&temporary_path, &path).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new-settings");
        assert_eq!(
            crate::windows_acl::windows_dacl_snapshot(&path).unwrap(),
            current_dacl
        );
    }

    #[cfg(windows)]
    #[test]
    fn substituted_replace_backup_source_is_never_restored() {
        let directory = TestDirectory::new();
        let target = directory.0.join("settings.toml");
        let backup = directory.0.join("settings.toml.replaced");
        let displaced = directory.0.join("verified-original");
        fs::write(&target, b"original-target").unwrap();
        fs::rename(&target, &backup).unwrap();
        let verified_original = fs::read(&backup).unwrap();
        fs::rename(&backup, &displaced).unwrap();
        fs::write(&backup, b"substituted-source").unwrap();

        // The verified source path was substituted before recovery. Recovery
        // must leave both the original and the substitute untouched.
        let failure =
            crate::windows_acl::recover_replace_failure(std::io::Error::from_raw_os_error(1177));

        assert!(!failure.replaced);
        assert!(failure.indeterminate);
        assert!(!target.exists());
        assert_eq!(fs::read(&displaced).unwrap(), verified_original);
        assert_eq!(fs::read(&backup).unwrap(), b"substituted-source");
    }

    #[cfg(windows)]
    #[test]
    fn failed_replace_recovery_is_indeterminate_without_clobbering_raced_target() {
        let directory = TestDirectory::new();
        let target = directory.0.join("settings.toml");
        let backup = directory.0.join("settings.toml.replaced");
        fs::write(&target, b"original-target").unwrap();
        fs::rename(&target, &backup).unwrap();
        fs::write(&target, b"raced-target").unwrap();

        let failure =
            crate::windows_acl::recover_replace_failure(std::io::Error::from_raw_os_error(1177));

        assert!(!failure.replaced);
        assert!(failure.indeterminate);
        assert_eq!(fs::read(&target).unwrap(), b"raced-target");
        assert_eq!(fs::read(&backup).unwrap(), b"original-target");
    }
    #[cfg(windows)]
    #[test]
    fn raced_first_main_creation_keeps_target_and_temp_dacls_separate() {
        use winapi::um::winnt::SE_DACL_PROTECTED;

        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let raced_dacl = Arc::new(Mutex::new(None));
        let raced_dacl_slot = Arc::clone(&raced_dacl);
        let target_path = path.clone();
        let mut store = FileStore::new(&path);
        store.inject_after_absent_check(move |target| {
            assert_eq!(target, target_path.as_path());
            fs::write(target, b"raced-main").unwrap();
            crate::windows_acl::set_permissive_windows_dacl(target).unwrap();
            *raced_dacl_slot.lock().unwrap() =
                Some(crate::windows_acl::windows_dacl_snapshot(target).unwrap());
        });
        let temporary_dacl = Arc::new(Mutex::new(None));
        let temporary_dacl_slot = Arc::clone(&temporary_dacl);
        store.inspect_temporary_dacl(move |file| {
            *temporary_dacl_slot.lock().unwrap() =
                Some(crate::windows_acl::windows_dacl_snapshot_handle(file).unwrap());
        });

        assert!(matches!(
            store.commit(None, b"candidate-settings", BackupMode::ReplaceWithPrevious),
            Err(StoreError::Io {
                operation: StoreOperation::Replace,
                ..
            })
        ));

        assert_eq!(fs::read(&path).unwrap(), b"raced-main");
        let raced_dacl = raced_dacl.lock().unwrap().take().unwrap();
        let temporary_dacl = temporary_dacl.lock().unwrap().take().unwrap();
        let restrictive_dacl = crate::windows_acl::restrictive_windows_dacl_snapshot().unwrap();
        assert_ne!(raced_dacl.control & SE_DACL_PROTECTED, 0);
        assert_ne!(raced_dacl, restrictive_dacl);
        assert_eq!(temporary_dacl, restrictive_dacl);
        assert_eq!(
            crate::windows_acl::windows_dacl_snapshot(&path).unwrap(),
            raced_dacl
        );
        assert!(
            !fs::read_dir(&directory.0)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.tmp-"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn raced_first_backup_creation_keeps_target_and_temp_dacls_separate() {
        use winapi::um::winnt::SE_DACL_PROTECTED;

        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let backup = directory.0.join("settings.toml.bak");
        fs::write(&path, b"last-valid-main").unwrap();
        let raced_dacl = Arc::new(Mutex::new(None));
        let raced_dacl_slot = Arc::clone(&raced_dacl);
        let backup_path = backup.clone();
        let mut store = FileStore::new(&path);
        store.inject_after_absent_check(move |target| {
            assert_eq!(target, backup_path.as_path());
            fs::write(target, b"raced-backup").unwrap();
            crate::windows_acl::set_permissive_windows_dacl(target).unwrap();
            *raced_dacl_slot.lock().unwrap() =
                Some(crate::windows_acl::windows_dacl_snapshot(target).unwrap());
        });
        let temporary_dacl = Arc::new(Mutex::new(None));
        let temporary_dacl_slot = Arc::clone(&temporary_dacl);
        store.inspect_temporary_dacl(move |file| {
            *temporary_dacl_slot.lock().unwrap() =
                Some(crate::windows_acl::windows_dacl_snapshot_handle(file).unwrap());
        });

        assert!(matches!(
            store.commit(
                Some(b"last-valid-main"),
                b"candidate-settings",
                BackupMode::ReplaceWithPrevious,
            ),
            Err(StoreError::Io {
                operation: StoreOperation::Replace,
                ..
            })
        ));

        assert_eq!(fs::read(&path).unwrap(), b"last-valid-main");
        assert_eq!(fs::read(&backup).unwrap(), b"raced-backup");
        let raced_dacl = raced_dacl.lock().unwrap().take().unwrap();
        let temporary_dacl = temporary_dacl.lock().unwrap().take().unwrap();
        let restrictive_dacl = crate::windows_acl::restrictive_windows_dacl_snapshot().unwrap();
        assert_ne!(raced_dacl.control & SE_DACL_PROTECTED, 0);
        assert_ne!(raced_dacl, restrictive_dacl);
        assert_eq!(temporary_dacl, restrictive_dacl);
        assert_eq!(
            crate::windows_acl::windows_dacl_snapshot(&backup).unwrap(),
            raced_dacl
        );
        assert!(
            !fs::read_dir(&directory.0)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.bak.tmp-"))
        );
    }
}
