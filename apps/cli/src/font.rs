//! Explicit local font loading for the shared glyph-table builder.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use ascii_convert::{GlyphConfig, GlyphTable, build_glyph_table};
use ascii_core::{Budget, CancellationToken, Error};

/// Maximum encoded font bytes accepted by the CLI, independently of the caller's budget.
pub const MAX_FONT_BYTES: u64 = 16 * 1024 * 1024;
const READ_CHUNK: usize = 16 * 1024;

/// A local font read or shared glyph-table construction failure.
#[derive(Debug)]
pub enum FontLoadError {
    Io(io::Error),
    NotRegularFile,
    TooLarge { max_bytes: u64 },
    Engine(Error),
}

impl fmt::Display for FontLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot read font: {error}"),
            Self::NotRegularFile => f.write_str("font input must be a regular file"),
            Self::TooLarge { max_bytes } => write!(f, "font exceeds the {max_bytes}-byte input limit"),
            Self::Engine(error) => write!(f, "cannot build glyph table: {error}"),
    }
}
}

impl std::error::Error for FontLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Engine(error) => Some(error),
            Self::NotRegularFile | Self::TooLarge { .. } => None,
    }
}
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), FontLoadError> {
    if cancellation.is_cancelled() {
        Err(FontLoadError::Engine(Error::Cancelled))
    } else {
        Ok(())
    }
}

fn io_or_cancel(error: io::Error, cancellation: &CancellationToken) -> FontLoadError {
    if cancellation.is_cancelled() {
        FontLoadError::Engine(Error::Cancelled)
    } else {
        FontLoadError::Io(error)
    }
}

fn open_font_file(
    path: &Path,
    cancellation: &CancellationToken,
) -> Result<(File, u64), FontLoadError> {
    // OpenOptions follows symlinks intentionally. Unix O_NONBLOCK makes FIFO targets swapped
    // after path preflight safe; the resulting handle is authoritative before any read.
    check_cancelled(cancellation)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    let file = options
        .open(path)
        .map_err(|error| io_or_cancel(error, cancellation))?;
    check_cancelled(cancellation)?;
    if !is_regular_file_handle(&file) {
        return Err(FontLoadError::NotRegularFile);
    }
    let metadata = file
        .metadata()
        .map_err(|error| io_or_cancel(error, cancellation))?;
    check_cancelled(cancellation)?;
    if !metadata.is_file() {
        return Err(FontLoadError::NotRegularFile);
    }
    Ok((file, metadata.len()))
}

#[cfg(windows)]
fn is_regular_file_handle(file: &File) -> bool {
    use std::os::windows::io::AsRawHandle;

    // On Windows, std::fs::FileType also calls nondirectory, non-symlink handles files. Inspect
    // the native handle so pipes and character devices are rejected before synchronous reads.
    const FILE_TYPE_DISK: u32 = 0x0001;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileType(handle: *mut std::ffi::c_void) -> u32;
    }

    // SAFETY: `file` owns a valid open handle for the duration of this call.
    unsafe { GetFileType(file.as_raw_handle()) == FILE_TYPE_DISK }
}

#[cfg(not(windows))]
fn is_regular_file_handle(_file: &File) -> bool {
    true
}

/// No font is silently selected and no metrics are inferred here. Font input is bounded by the
/// CLI's 16 MiB limit and the supplied raster-byte budget, including when the file changes while
/// being read. Non-regular paths are rejected before opening and the opened handle is checked
/// before reading; cancellation is checked around file operations and between reads, and in the
/// shared table builder.
pub fn load_glyph_table(
    path: &Path,
    charset: &str,
    config: &GlyphConfig,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<GlyphTable, FontLoadError> {
    check_cancelled(cancellation)?;
    let path_metadata = fs::metadata(path).map_err(|error| io_or_cancel(error, cancellation))?;
    check_cancelled(cancellation)?;
    if !path_metadata.is_file() {
        return Err(FontLoadError::NotRegularFile);
    }
    let (mut file, initial_len) = open_font_file(path, cancellation)?;
    let max_bytes = MAX_FONT_BYTES.min(budget.max_raster_bytes());
    if initial_len > max_bytes {
        return Err(FontLoadError::TooLarge { max_bytes });
    }
    // MAX_FONT_BYTES fits usize on all supported targets. Keep one spare byte when possible so
    // an ordinary, unchanged file can reach EOF without a second allocation.
    let limit = max_bytes as usize;
    let initial_capacity = if initial_len == 0 {
        READ_CHUNK.min(limit)
    } else {
        (initial_len as usize).saturating_add(1).min(limit)
    };
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(initial_capacity)
        .map_err(|_| FontLoadError::Engine(Error::AllocationFailed))?;

    loop {
        if cancellation.is_cancelled() {
            return Err(FontLoadError::Engine(Error::Cancelled));
        }
        if bytes.len() == limit {
            let mut probe = [0_u8; 1];
            let read = file.read(&mut probe);
            check_cancelled(cancellation)?;
            match read {
                Ok(0) => break,
                Ok(_) => return Err(FontLoadError::TooLarge { max_bytes }),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(io_or_cancel(error, cancellation)),
            }
        }
        if bytes.capacity() == bytes.len() {
            let additional = READ_CHUNK.min(limit - bytes.len());
            bytes.try_reserve_exact(additional)
                .map_err(|_| FontLoadError::Engine(Error::AllocationFailed))?;
        }
        let start = bytes.len();
        let count = (bytes.capacity() - start).min(READ_CHUNK).min(limit - start);
        bytes.resize(start + count, 0);
        let read = file.read(&mut bytes[start..]);
        check_cancelled(cancellation)?;
        match read {
            Ok(0) => { bytes.truncate(start); break; }
            Ok(read) => bytes.truncate(start + read),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => bytes.truncate(start),
            Err(error) => return Err(io_or_cancel(error, cancellation)),
        }
    }
    if cancellation.is_cancelled() {
        return Err(FontLoadError::Engine(Error::Cancelled));
    }
    build_glyph_table(&bytes, charset, config, budget, cancellation).map_err(FontLoadError::Engine)
}
#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::process::Command;

    use ascii_core::CancellationToken;

    use super::{FontLoadError, open_font_file};

    #[test]
    fn replacing_a_preflighted_symlink_target_with_a_fifo_never_blocks_open() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("font-target");
        let link = dir.path().join("font-link");
        fs::write(&target, b"regular font").unwrap();
        symlink(&target, &link).unwrap();
        assert!(fs::metadata(&link).unwrap().is_file());

        fs::remove_file(&target).unwrap();
        let status = Command::new("mkfifo")
            .arg(&target)
            .status()
            .expect("mkfifo is available on Unix");
        assert!(status.success());

        let error = open_font_file(&link, &CancellationToken::new()).unwrap_err();
        assert!(matches!(error, FontLoadError::NotRegularFile));
    }
}
