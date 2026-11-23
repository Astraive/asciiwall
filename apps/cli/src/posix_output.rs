//! POSIX same-directory staging and atomic publication.

use std::ffi::CString;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use ascii_core::CancellationToken;

use crate::output::PublishError;

#[cfg(target_os = "linux")]
const RENAME_NOREPLACE: libc::c_uint = 1;
const STAGING_ATTEMPTS: usize = 32;
const STAGING_PREFIX: &[u8] = b".asciiwall-";
const STAGING_SUFFIX: &[u8] = b".tmp";

pub(crate) fn publish(
    destination: &Path,
    overwrite: bool,
    cancellation: &CancellationToken,
    write_artifact: impl FnOnce(&mut File) -> io::Result<()>,
) -> Result<(), PublishError> {
    if cancellation.is_cancelled() {
        return Err(PublishError::Cancelled);
    }

    let parent_path = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let leaf = destination.file_name().ok_or_else(|| {
        PublishError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination does not have a file name",
        ))
    })?;
    let leaf = CString::new(leaf.as_bytes()).map_err(|_| {
        PublishError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination name contains a NUL byte",
        ))
    })?;
    let parent = File::open(parent_path).map_err(|error| io_or_cancel(error, cancellation))?;
    verify_safe_parent(&parent).map_err(|error| io_or_cancel(error, cancellation))?;
    let mut staging = create_staging(&parent).map_err(|error| io_or_cancel(error, cancellation))?;

    let result = (|| {
        if cancellation.is_cancelled() {
            return Err(PublishError::Cancelled);
        }
        write_artifact(&mut staging.file).map_err(|error| io_or_cancel(error, cancellation))?;
        staging
            .file
            .flush()
            .map_err(|error| io_or_cancel(error, cancellation))?;
        staging
            .file
            .sync_all()
            .map_err(|error| io_or_cancel(error, cancellation))?;
        if cancellation.is_cancelled() {
            return Err(PublishError::Cancelled);
        }

        commit_staging(&mut staging, &parent, &leaf, overwrite, cancellation)
    })();

    if result.is_err()
        && let Err(error) = staging.cleanup()
    {
        return Err(io_or_cancel(error, cancellation));
    }
    result
}

fn commit_staging(
    staging: &mut StagingFile<'_>,
    parent: &File,
    destination: &CString,
    overwrite: bool,
    cancellation: &CancellationToken,
) -> Result<(), PublishError> {
    if overwrite {
        let status = unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                staging.name.as_ptr(),
                parent.as_raw_fd(),
                destination.as_ptr(),
            )
        };
        if status < 0 {
            return Err(io_or_cancel(io::Error::last_os_error(), cancellation));
        }
    } else {
        #[cfg(target_os = "linux")]
        {
            let status = unsafe {
                libc::syscall(
                    libc::SYS_renameat2,
                    parent.as_raw_fd(),
                    staging.name.as_ptr(),
                    parent.as_raw_fd(),
                    destination.as_ptr(),
                    RENAME_NOREPLACE,
                )
            };
            if status < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EEXIST) {
                    return Err(PublishError::DestinationExists);
                }
                return Err(no_clobber_error(error, cancellation));
            }
        }
        #[cfg(any(target_os = "macos", target_os = "freebsd"))]
        {
            let status = unsafe {
                libc::linkat(
                    parent.as_raw_fd(),
                    staging.name.as_ptr(),
                    parent.as_raw_fd(),
                    destination.as_ptr(),
                    0,
                )
            };
            if status < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EEXIST) {
                    return Err(PublishError::DestinationExists);
                }
                return Err(no_clobber_error(error, cancellation));
            }
            // The destination is committed. A failed unlink can only leave the extra staging
            // hard link; returning an error here would report failure after publishing output.
            let _cleanup = staging.cleanup();
            return Ok(());
        }
    }
    staging.published = true;
    Ok(())
}

fn no_clobber_error(error: io::Error, cancellation: &CancellationToken) -> PublishError {
    if cancellation.is_cancelled() {
        return PublishError::Cancelled;
    }
    let code = error.raw_os_error();
    let unsupported = code == Some(libc::ENOSYS)
        || code == Some(libc::EOPNOTSUPP)
        || code == Some(libc::ENOTSUP)
        || (cfg!(target_os = "linux") && code == Some(libc::EINVAL));
    if unsupported {
        PublishError::NoClobberUnsupported
    } else {
        PublishError::Io(error)
    }
}

fn verify_safe_parent(parent: &File) -> io::Result<()> {
    let metadata = parent.metadata()?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "destination parent is not a directory",
        ));
    }

    // In sticky directories owned by root or this user, other users cannot replace this process's
    // staging entry. In a non-sticky directory, permit staging only when this user owns the
    // directory and no other user has write permission.
    let mode = metadata.mode();
    let owner = metadata.uid();
    let effective_uid = unsafe { libc::geteuid() };
    #[cfg(target_os = "linux")]
    let sticky_mask = libc::S_ISVTX;
    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    let sticky_mask = libc::S_ISVTX as u32;
    let trusted_sticky = mode & sticky_mask != 0 && (owner == 0 || owner == effective_uid);
    let owner_private = owner == effective_uid && mode & 0o022 == 0;
    if trusted_sticky || owner_private {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "destination directory does not protect staging entries from replacement",
        ))
    }
}

fn create_staging(parent: &File) -> io::Result<StagingFile<'_>> {
    for _ in 0..STAGING_ATTEMPTS {
        let name = staging_name()?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd >= 0 {
            return Ok(StagingFile {
                parent,
                name,
                file: unsafe { File::from_raw_fd(fd) },
                published: false,
                cleanup_complete: false,
            });
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EEXIST) {
            return Err(error);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot allocate a unique output staging name",
    ))
}

fn staging_name() -> io::Result<CString> {
    let mut random = [0_u8; 16];
    #[cfg(target_os = "linux")]
    {
        let mut filled = 0;
        while filled < random.len() {
            let result = unsafe {
                libc::getrandom(
                    random[filled..].as_mut_ptr().cast(),
                    random.len() - filled,
                    0,
                )
            };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if result == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "random source returned no bytes",
                ));
            }
            filled += result as usize;
        }
    }
    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    unsafe {
        libc::arc4random_buf(random.as_mut_ptr().cast(), random.len());
    }

    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut name =
        Vec::with_capacity(STAGING_PREFIX.len() + random.len() * 2 + STAGING_SUFFIX.len());
    name.extend_from_slice(STAGING_PREFIX);
    for byte in random {
        name.push(HEX[(byte >> 4) as usize]);
        name.push(HEX[(byte & 0x0f) as usize]);
    }
    name.extend_from_slice(STAGING_SUFFIX);
    CString::new(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "generated staging name contains a NUL byte",
        )
    })
}

struct StagingFile<'a> {
    parent: &'a File,
    name: CString,
    file: File,
    published: bool,
    cleanup_complete: bool,
}

impl StagingFile<'_> {
    fn cleanup(&mut self) -> io::Result<()> {
        if self.published || self.cleanup_complete {
            return Ok(());
        }

        let mut entry = std::mem::MaybeUninit::<libc::stat>::zeroed();
        let status = unsafe {
            libc::fstatat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                entry.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if status < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENOENT) {
                self.cleanup_complete = true;
                return Ok(());
            }
            return Err(error);
        }
        let entry = unsafe { entry.assume_init() };
        let file_metadata = self.file.metadata()?;
        #[cfg(target_os = "linux")]
        let (entry_device, entry_inode) = (entry.st_dev, entry.st_ino);
        #[cfg(any(target_os = "macos", target_os = "freebsd"))]
        let (entry_device, entry_inode) = (entry.st_dev as u64, entry.st_ino as u64);
        if entry_device != file_metadata.dev()
            || entry_inode != file_metadata.ino()
            || entry.st_mode & libc::S_IFMT != libc::S_IFREG
        {
            // The name no longer refers to our staging inode. Leave the replacement untouched.
            self.cleanup_complete = true;
            return Ok(());
        }

        let status = unsafe { libc::unlinkat(self.parent.as_raw_fd(), self.name.as_ptr(), 0) };
        if status < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ENOENT) {
                return Err(error);
            }
        }
        self.cleanup_complete = true;
        Ok(())
    }
}

impl Drop for StagingFile<'_> {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn io_or_cancel(error: io::Error, cancellation: &CancellationToken) -> PublishError {
    if cancellation.is_cancelled() {
        PublishError::Cancelled
    } else {
        PublishError::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    use super::{create_staging, no_clobber_error, publish};
    use crate::output::PublishError;
    use ascii_core::CancellationToken;

    #[test]
    fn cleanup_leaves_a_replacement_staging_entry_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let parent = fs::File::open(directory.path()).unwrap();
        let mut staging = create_staging(&parent).unwrap();
        staging.file.write_all(b"ours").unwrap();
        let staging_path = directory.path().join(staging.name.to_str().unwrap());
        let moved_path = directory.path().join("moved-staging-entry");
        fs::rename(&staging_path, &moved_path).unwrap();
        fs::write(&staging_path, b"replacement").unwrap();

        staging.cleanup().unwrap();
        assert_eq!(fs::read(&staging_path).unwrap(), b"replacement");
        assert_eq!(fs::read(&moved_path).unwrap(), b"ours");
    }

    #[test]
    fn unsafe_shared_parent_is_rejected_before_staging() {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o777)).unwrap();
        let destination = directory.path().join("output.txt");
        let result = publish(&destination, false, &CancellationToken::new(), |_| {
            panic!("unsafe parent must fail before staging or writing")
        });
        assert!(matches!(
            result,
            Err(PublishError::Io(error)) if error.kind() == std::io::ErrorKind::PermissionDenied
        ));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn unsupported_no_clobber_has_a_dedicated_error() {
        let error = std::io::Error::from_raw_os_error(libc::EOPNOTSUPP);
        assert!(matches!(
            no_clobber_error(error, &CancellationToken::new()),
            PublishError::NoClobberUnsupported
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn enotsup_no_clobber_has_a_dedicated_error() {
        let error = std::io::Error::from_raw_os_error(libc::ENOTSUP);
        assert!(matches!(
            no_clobber_error(error, &CancellationToken::new()),
            PublishError::NoClobberUnsupported
        ));
    }
}
