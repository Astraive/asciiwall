//! Same-directory, fail-closed publication for a completed file export.

use std::fmt;
use std::fs::File;
use std::io;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
use std::io::Write;
use std::path::Path;

use ascii_core::CancellationToken;

/// A publication failure. Until a successful commit, the original destination is untouched.
#[derive(Debug)]
pub enum PublishError {
    /// Cancellation was observed before publication.
    Cancelled,
    /// The destination was created before the final no-clobber publication.
    DestinationExists,

    /// Atomic no-clobber publication is unavailable on the destination filesystem.
    NoClobberUnsupported,

    /// Staging or publication failed.
    Io(io::Error),
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("output publication cancelled"),
            Self::DestinationExists => f.write_str("output destination already exists"),
            Self::NoClobberUnsupported => {
                f.write_str("atomic no-clobber publication is unavailable")
            }
            Self::Io(error) => write!(f, "output publication failed: {error}"),
        }
    }
}

impl std::error::Error for PublishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cancelled | Self::DestinationExists | Self::NoClobberUnsupported => None,
            Self::Io(error) => Some(error),
        }
    }
}

/// Writes a complete artifact to a private temporary file and publishes it only on success.
///
/// A no-overwrite commit uses the filesystem's atomic no-clobber operation rather than an earlier
/// existence check. Linux uses `renameat2(RENAME_NOREPLACE)` and fails without changing the
/// destination when the kernel/filesystem does not support it. POSIX staging requires a sticky
/// directory owned by root or the caller, or a caller-owned directory with no group/other write
/// permission, so cleanup cannot remove another user's replacement entry. macOS and FreeBSD use
/// `linkat` for no-clobber; if hard links are unsupported, publication fails before changing the
/// destination. A failed unlink after a successful hard-link commit can leave an extra link.
/// Other targets use tempfile's atomic publish operations.
/// Cancellation observed before the final commit discards the temporary file; after the commit
/// starts, cancellation cannot retract the published file. Staging is in the destination directory.
/// This path-based API follows normal filesystem mount resolution and is not a network-isolation
/// boundary. Offline callers must use verified handle-bound operations or fail closed before path
/// access.
pub fn publish_file(
    destination: &Path,
    overwrite: bool,
    cancellation: &CancellationToken,
    write_artifact: impl FnOnce(&mut File) -> io::Result<()>,
) -> Result<(), PublishError> {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    {
        crate::posix_output::publish(destination, overwrite, cancellation, write_artifact)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
    {
        if cancellation.is_cancelled() {
            return Err(PublishError::Cancelled);
        }
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut staging = tempfile::Builder::new()
            .prefix(".asciiwall-")
            .tempfile_in(parent)
            .map_err(|error| io_or_cancel(error, cancellation))?;
        if cancellation.is_cancelled() {
            return Err(PublishError::Cancelled);
        }
        write_artifact(staging.as_file_mut()).map_err(|error| io_or_cancel(error, cancellation))?;
        staging
            .as_file_mut()
            .flush()
            .map_err(|error| io_or_cancel(error, cancellation))?;
        staging
            .as_file()
            .sync_all()
            .map_err(|error| io_or_cancel(error, cancellation))?;
        if cancellation.is_cancelled() {
            return Err(PublishError::Cancelled);
        }
        if overwrite {
            staging
                .persist(destination)
                .map_err(|error| PublishError::Io(error.error))?;
        } else {
            staging.persist_noclobber(destination).map_err(|error| {
                if error.error.kind() == io::ErrorKind::AlreadyExists {
                    PublishError::DestinationExists
                } else {
                    PublishError::Io(error.error)
                }
            })?;
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
fn io_or_cancel(error: io::Error, cancellation: &CancellationToken) -> PublishError {
    if cancellation.is_cancelled() {
        PublishError::Cancelled
    } else {
        PublishError::Io(error)
    }
}
