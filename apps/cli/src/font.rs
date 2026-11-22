//! Explicit local font loading for the shared glyph-table builder.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use ascii_convert::{GlyphConfig, GlyphRepertoire, GlyphTable, build_glyph_table};
use ascii_core::{Budget, CancellationToken, Error, GlyphFailureReason, MalformedReason};

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
            Self::TooLarge { max_bytes } => {
                write!(f, "font exceeds the {max_bytes}-byte input limit")
            }
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

/// Opens an explicit regular local input without blocking on Unix FIFOs.
///
/// The opened handle is authoritative: symlinks to regular files are supported, while pipes and
/// device handles are rejected before reads. Windows device and volume namespaces are also
/// rejected before opening and verified again against the resolved handle path.
pub(crate) fn open_regular_input_file(
    path: &Path,
    cancellation: &CancellationToken,
) -> Result<(File, u64), FontLoadError> {
    // OpenOptions follows symlinks intentionally. Unix O_NONBLOCK makes FIFO targets swapped
    // after path preflight safe; the resulting handle is authoritative before any read.
    check_cancelled(cancellation)?;
    if !is_windows_filesystem_file_path(path) {
        return Err(FontLoadError::NotRegularFile);
    }
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

    // GetFileType alone accepts raw disk and volume handles as disk files. Resolve the opened
    // handle as well and require a filesystem path with a file name before allowing reads.
    const FILE_TYPE_DISK: u32 = 0x0001;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileType(handle: *mut std::ffi::c_void) -> u32;
    }

    // SAFETY: `file` owns a valid open handle for the duration of this call.
    unsafe { GetFileType(file.as_raw_handle()) == FILE_TYPE_DISK }
        && handle_has_filesystem_file_path(file)
}

#[cfg(windows)]
fn handle_has_filesystem_file_path(file: &File) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;

    const FILE_NAME_NORMALIZED: u32 = 0;
    const VOLUME_NAME_DOS: u32 = 0;
    const PATH_FLAGS: u32 = FILE_NAME_NORMALIZED | VOLUME_NAME_DOS;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFinalPathNameByHandleW(
            handle: *mut std::ffi::c_void,
            file_path: *mut u16,
            file_path_size: u32,
            flags: u32,
        ) -> u32;
    }

    let handle = file.as_raw_handle();
    // SAFETY: A null buffer with zero capacity asks for the required buffer length.
    let required = unsafe {
        GetFinalPathNameByHandleW(handle, std::ptr::null_mut(), 0, PATH_FLAGS)
    };
    let Some(capacity) = usize::try_from(required)
        .ok()
        .and_then(|required| required.checked_add(1))
    else {
        return false;
    };
    let Ok(capacity_u32) = u32::try_from(capacity) else {
        return false;
    };
    if required == 0 {
        return false;
    }
    let mut final_path = Vec::new();
    if final_path.try_reserve_exact(capacity).is_err() {
        return false;
    }
    final_path.resize(capacity, 0);
    // SAFETY: `final_path` has `capacity_u32` writable UTF-16 code units and `handle` remains
    // valid for this call.
    let written = unsafe {
        GetFinalPathNameByHandleW(handle, final_path.as_mut_ptr(), capacity_u32, PATH_FLAGS)
    };
    if written == 0 || written as usize >= final_path.len() {
        return false;
    }
    final_path.truncate(written as usize);
    is_windows_filesystem_file_path(Path::new(std::ffi::OsStr::from_wide(&final_path)))
}

#[cfg(windows)]
fn is_windows_filesystem_file_path(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};

    let mut units = path.as_os_str().encode_wide();
    let first = units.next();
    let second = units.next();
    let third = units.next();
    let fourth = units.next();
    let is_separator = |unit| unit == b'\\' as u16 || unit == b'/' as u16;
    if matches!((first, second, third, fourth),
        (Some(a), Some(b), Some(c), Some(d))
            if is_separator(a) && b == b'?' as u16 && c == b'?' as u16 && is_separator(d)
    ) {
        return false;
    }

    let mut units = path.as_os_str().encode_wide();
    let first = units.next();
    let second = units.next();
    let third = units.next();
    let fourth = units.next();
    if matches!((first, second, third, fourth),
        (Some(a), Some(b), Some(c), Some(d))
            if is_separator(a) && is_separator(b) && c == b'.' as u16 && is_separator(d)
    ) {
        return false;
    }

    let is_filesystem_prefix = match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::DeviceNS(_) => false,
            Prefix::Verbatim(value) => is_volume_guid_prefix(value),
            Prefix::VerbatimDisk(_)
            | Prefix::VerbatimUNC(_, _)
            | Prefix::Disk(_)
            | Prefix::UNC(_, _) => true,
        },
        _ => true,
    };
    is_filesystem_prefix && path.file_name().is_some()
}

#[cfg(windows)]
fn is_volume_guid_prefix(prefix: &std::ffi::OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let mut units = prefix.encode_wide();
    for expected in "Volume{".encode_utf16() {
        let Some(actual) = units.next() else {
            return false;
        };
        let ascii_lower = |unit: u16| {
            if (b'A' as u16..=b'Z' as u16).contains(&unit) {
                unit + (b'a' - b'A') as u16
            } else {
                unit
            }
        };
        if ascii_lower(actual) != ascii_lower(expected) {
            return false;
        }
    }
    for index in 0..36 {
        let Some(actual) = units.next() else {
            return false;
        };
        if matches!(index, 8 | 13 | 18 | 23) {
            if actual != b'-' as u16 {
                return false;
            }
        } else if !matches!(actual, 48..=57 | 65..=70 | 97..=102) {
            return false;
        }
    }
    units.next() == Some(b'}' as u16) && units.next().is_none()
}

#[cfg(not(windows))]
fn is_windows_filesystem_file_path(_path: &Path) -> bool {
    true
}

#[cfg(all(test, windows))]
mod windows_path_tests {
    use std::path::Path;

    use super::is_windows_filesystem_file_path;

    #[test]
    fn rejects_windows_device_and_volume_namespace_paths() {
        for path in [
            r"\\.\PhysicalDrive0",
            r"\\.\C:",
            r"\\?\PhysicalDrive0",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1",
            r"\??\C:",
            r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}",
        ] {
            assert!(
                !is_windows_filesystem_file_path(Path::new(path)),
                "accepted device namespace path: {path}"
            );
        }
    }

    #[test]
    fn rejects_device_paths_before_opening() {
        use ascii_core::CancellationToken;

        for path in [r"\\.\PhysicalDrive0", r"\\.\C:"] {
            assert!(matches!(
                super::open_regular_input_file(Path::new(path), &CancellationToken::new()),
                Err(super::FontLoadError::NotRegularFile)
            ));
        }
    }

    #[test]
    fn accepts_ordinary_unc_and_extended_length_filesystem_paths() {
        for path in [
            r"C:\fonts\font.ttf",
            r"\\server\share\fonts\font.ttf",
            r"\\?\C:\fonts\font.ttf",
            r"\\?\UNC\server\share\fonts\font.ttf",
            r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\fonts\font.ttf",
        ] {
            assert!(
                is_windows_filesystem_file_path(Path::new(path)),
                "rejected filesystem path: {path}"
            );
        }
    }
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
    let repertoire = GlyphRepertoire::new(charset).map_err(|error| {
        FontLoadError::Engine(match error {
            Error::Glyph {
                reason: GlyphFailureReason::UnsupportedScalar | GlyphFailureReason::DuplicateScalar,
                ..
            } => Error::Malformed(MalformedReason::InvalidConfiguration),
            error => error,
        })
    })?;
    load_glyph_table_with_repertoire(path, &repertoire, config, budget, cancellation)
}

/// Loads a font using a repertoire that has already passed shared validation.
pub fn load_glyph_table_with_repertoire(
    path: &Path,
    repertoire: &GlyphRepertoire,
    config: &GlyphConfig,
    budget: &Budget,
    cancellation: &CancellationToken,
) -> Result<GlyphTable, FontLoadError> {
    check_cancelled(cancellation)?;
    if !is_windows_filesystem_file_path(path) {
        return Err(FontLoadError::NotRegularFile);
    }
    let path_metadata = fs::metadata(path).map_err(|error| io_or_cancel(error, cancellation))?;
    check_cancelled(cancellation)?;
    if !path_metadata.is_file() {
        return Err(FontLoadError::NotRegularFile);
    }
    let (mut file, initial_len) = open_regular_input_file(path, cancellation)?;
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
    bytes
        .try_reserve_exact(initial_capacity)
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
            bytes
                .try_reserve_exact(additional)
                .map_err(|_| FontLoadError::Engine(Error::AllocationFailed))?;
        }
        let start = bytes.len();
        let count = (bytes.capacity() - start)
            .min(READ_CHUNK)
            .min(limit - start);
        bytes.resize(start + count, 0);
        let read = file.read(&mut bytes[start..]);
        check_cancelled(cancellation)?;
        match read {
            Ok(0) => {
                bytes.truncate(start);
                break;
            }
            Ok(read) => bytes.truncate(start + read),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => bytes.truncate(start),
            Err(error) => return Err(io_or_cancel(error, cancellation)),
        }
    }
    if cancellation.is_cancelled() {
        return Err(FontLoadError::Engine(Error::Cancelled));
    }
    build_glyph_table(&bytes, repertoire, config, budget, cancellation)
        .map_err(FontLoadError::Engine)
}
#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::process::Command;

    use ascii_core::CancellationToken;

    use super::{FontLoadError, open_regular_input_file};

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

        let error = open_regular_input_file(&link, &CancellationToken::new()).unwrap_err();
        assert!(matches!(error, FontLoadError::NotRegularFile));
    }
}
