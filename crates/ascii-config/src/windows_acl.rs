use std::{
    fs::{self, File},
    path::Path,
};
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "ReplaceFileW"]
    fn replace_file_w(
        replaced: *const u16,
        replacement: *const u16,
        backup: *const u16,
        flags: u32,
        exclude: *mut std::ffi::c_void,
        reserved: *mut std::ffi::c_void,
    ) -> i32;
}

#[cfg(windows)]
pub(super) struct ReplaceFailure {
    pub(super) error: std::io::Error,
    pub(super) replaced: bool,
    pub(super) indeterminate: bool,
}

#[cfg(windows)]
fn windows_path(path: &Path) -> std::io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;

    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    for unit in &mut wide {
        if *unit == b'/' as u16 {
            *unit = b'\\' as u16;
        }
    }

    let mut extended = Vec::with_capacity(wide.len() + 8);
    if wide.starts_with(&[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16])
        || wide.starts_with(&[b'\\' as u16, b'\\' as u16, b'.' as u16, b'\\' as u16])
    {
        extended.extend(wide);
    } else if wide.starts_with(&[b'\\' as u16, b'\\' as u16]) {
        extended.extend("\\\\?\\UNC\\".encode_utf16());
        extended.extend_from_slice(&wide[2..]);
    } else {
        extended.extend("\\\\?\\".encode_utf16());
        extended.extend(wide);
    }
    extended.push(0);
    Ok(extended)
}

#[cfg(windows)]
pub(super) fn recover_replace_failure(error: std::io::Error) -> ReplaceFailure {
    const ERROR_UNABLE_TO_MOVE_REPLACEMENT_2: i32 = 1177;

    // ReplaceFileW may have moved the original to its backup path. Do not
    // follow that path after failure; without a pinned handle, only report the
    // outcome as indeterminate and leave all candidate files untouched.
    ReplaceFailure {
        indeterminate: error.raw_os_error() == Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT_2),
        error,
        replaced: false,
    }
}
#[cfg(windows)]
#[derive(Clone, Copy)]
struct WindowsDacl {
    control: u16,
    acl: winapi::um::winnt::PACL,
}

#[cfg(all(windows, test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct WindowsDaclSnapshot {
    pub(super) control: u16,
    pub(super) acl: Option<Vec<u8>>,
}

#[cfg(windows)]
struct OwnedWindowsSecurityDescriptor(winapi::um::winnt::PSECURITY_DESCRIPTOR);

#[cfg(windows)]
impl Drop for OwnedWindowsSecurityDescriptor {
    fn drop(&mut self) {
        use winapi::{shared::minwindef::HLOCAL, um::winbase::LocalFree};

        unsafe {
            LocalFree(self.0 as HLOCAL);
        }
    }
}

#[cfg(windows)]
fn read_windows_dacl(
    path: &Path,
) -> std::io::Result<(OwnedWindowsSecurityDescriptor, WindowsDacl)> {
    use winapi::um::{
        accctrl::SE_FILE_OBJECT,
        aclapi::GetNamedSecurityInfoW,
        winnt::{DACL_SECURITY_INFORMATION, PACL, PSECURITY_DESCRIPTOR},
    };

    let mut path = windows_path(path)?;
    let mut acl: PACL = std::ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            path.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    let descriptor = OwnedWindowsSecurityDescriptor(descriptor);
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32));
    }
    if descriptor.0.is_null() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Windows returned no security descriptor",
        ));
    }
    let control = windows_dacl_control(descriptor.0)?;
    Ok((descriptor, WindowsDacl { control, acl }))
}

#[cfg(windows)]
fn read_windows_dacl_handle(
    file: &File,
) -> std::io::Result<(OwnedWindowsSecurityDescriptor, WindowsDacl)> {
    use std::os::windows::io::AsRawHandle;
    use winapi::um::{
        accctrl::SE_FILE_OBJECT,
        aclapi::GetSecurityInfo,
        winnt::{DACL_SECURITY_INFORMATION, HANDLE, PACL, PSECURITY_DESCRIPTOR},
    };

    let mut acl: PACL = std::ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle() as HANDLE,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    let descriptor = OwnedWindowsSecurityDescriptor(descriptor);
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32));
    }
    if descriptor.0.is_null() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Windows returned no security descriptor",
        ));
    }
    let control = windows_dacl_control(descriptor.0)?;
    Ok((descriptor, WindowsDacl { control, acl }))
}

#[cfg(windows)]
fn windows_dacl_control(
    descriptor: winapi::um::winnt::PSECURITY_DESCRIPTOR,
) -> std::io::Result<u16> {
    use winapi::{
        shared::minwindef::DWORD,
        um::{
            securitybaseapi::GetSecurityDescriptorControl,
            winnt::{SE_DACL_PRESENT, SE_DACL_PROTECTED, SECURITY_DESCRIPTOR_CONTROL},
        },
    };

    let mut control: SECURITY_DESCRIPTOR_CONTROL = 0;
    let mut revision: DWORD = 0;
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Windows may normalize defaulted and auto-inheritance metadata when setting an ACL.
    Ok(control & (SE_DACL_PRESENT | SE_DACL_PROTECTED))
}

#[cfg(windows)]
fn windows_dacls_equal(left: WindowsDacl, right: WindowsDacl) -> std::io::Result<bool> {
    use winapi::um::winnt::ACL;

    if left.control != right.control {
        return Ok(false);
    }
    if left.acl.is_null() || right.acl.is_null() {
        return Ok(left.acl.is_null() && right.acl.is_null());
    }
    let left_length = unsafe { usize::from((*left.acl).AclSize) };
    let right_length = unsafe { usize::from((*right.acl).AclSize) };
    let minimum_length = std::mem::size_of::<ACL>();
    if left_length < minimum_length || right_length < minimum_length {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Windows returned an invalid ACL size",
        ));
    }
    if left_length != right_length {
        return Ok(false);
    }
    Ok(unsafe {
        std::slice::from_raw_parts(left.acl.cast::<u8>(), left_length)
            == std::slice::from_raw_parts(right.acl.cast::<u8>(), right_length)
    })
}

#[cfg(windows)]
fn restrictive_creation_dacl() -> std::io::Result<(OwnedWindowsSecurityDescriptor, WindowsDacl)> {
    use winapi::{
        shared::{
            minwindef::BOOL,
            sddl::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        },
        um::{
            securitybaseapi::GetSecurityDescriptorDacl,
            winnt::{PACL, PSECURITY_DESCRIPTOR},
        },
    };

    // OWNER RIGHTS grants full access only to the file owner. Protecting the
    // DACL prevents inheriting any broader entries from the parent directory.
    let mut sddl = "D:P(A;;FA;;;OW)"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_mut_ptr(),
            u32::from(SDDL_REVISION_1),
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let descriptor = OwnedWindowsSecurityDescriptor(descriptor);
    if descriptor.0.is_null() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Windows returned no restrictive security descriptor",
        ));
    }
    let mut present: BOOL = 0;
    let mut acl: PACL = std::ptr::null_mut();
    let mut defaulted: BOOL = 0;
    if unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted) }
        == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if present == 0 || acl.is_null() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Windows returned no restrictive DACL",
        ));
    }
    let control = windows_dacl_control(descriptor.0)?;
    Ok((descriptor, WindowsDacl { control, acl }))
}

#[cfg(windows)]
pub(super) fn set_restrictive_dacl(file: &File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use winapi::um::{
        accctrl::SE_FILE_OBJECT,
        aclapi::SetSecurityInfo,
        winnt::{DACL_SECURITY_INFORMATION, HANDLE, PROTECTED_DACL_SECURITY_INFORMATION},
    };

    let (_descriptor, desired_dacl) = restrictive_creation_dacl()?;
    let status = unsafe {
        SetSecurityInfo(
            file.as_raw_handle() as HANDLE,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            desired_dacl.acl,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32));
    }

    let (_descriptor, actual_dacl) = read_windows_dacl_handle(file)?;
    if !windows_dacls_equal(desired_dacl, actual_dacl)? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Windows creation DACL did not match the restrictive DACL",
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn preserve_target_dacl(target: &Path, temporary: &File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use winapi::um::{
        accctrl::SE_FILE_OBJECT,
        aclapi::SetSecurityInfo,
        winnt::{
            DACL_SECURITY_INFORMATION, HANDLE, PROTECTED_DACL_SECURITY_INFORMATION,
            SE_DACL_PROTECTED, UNPROTECTED_DACL_SECURITY_INFORMATION,
        },
    };

    match fs::symlink_metadata(target) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return set_restrictive_dacl(temporary);
        }
        Err(error) => return Err(error),
    }

    let (_target_descriptor, target_dacl) = read_windows_dacl(target)?;
    let protection = if target_dacl.control & SE_DACL_PROTECTED != 0 {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    let status = unsafe {
        SetSecurityInfo(
            temporary.as_raw_handle() as HANDLE,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | protection,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            target_dacl.acl,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32));
    }

    let (_temporary_descriptor, temporary_dacl) = read_windows_dacl_handle(temporary)?;
    if !windows_dacls_equal(target_dacl, temporary_dacl)? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Windows replacement DACL did not match the existing target",
        ));
    }
    Ok(())
}

#[cfg(all(windows, test))]
fn snapshot_windows_dacl(dacl: WindowsDacl) -> std::io::Result<WindowsDaclSnapshot> {
    use winapi::um::winnt::ACL;

    let acl = if dacl.acl.is_null() {
        None
    } else {
        let length = unsafe { usize::from((*dacl.acl).AclSize) };
        if length < std::mem::size_of::<ACL>() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Windows returned an invalid ACL size",
            ));
        }
        Some(unsafe { std::slice::from_raw_parts(dacl.acl.cast::<u8>(), length) }.to_vec())
    };
    Ok(WindowsDaclSnapshot {
        control: dacl.control,
        acl,
    })
}

#[cfg(all(windows, test))]
pub(super) fn windows_dacl_snapshot(path: &Path) -> std::io::Result<WindowsDaclSnapshot> {
    let (_descriptor, dacl) = read_windows_dacl(path)?;
    snapshot_windows_dacl(dacl)
}
#[cfg(all(windows, test))]
pub(super) fn windows_dacl_snapshot_handle(file: &File) -> std::io::Result<WindowsDaclSnapshot> {
    let (_descriptor, dacl) = read_windows_dacl_handle(file)?;
    snapshot_windows_dacl(dacl)
}

#[cfg(all(windows, test))]
pub(super) fn set_permissive_windows_dacl(path: &Path) -> std::io::Result<()> {
    use winapi::{
        shared::{
            minwindef::BOOL,
            sddl::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        },
        um::{
            aclapi::SetNamedSecurityInfoW,
            securitybaseapi::GetSecurityDescriptorDacl,
            winnt::{
                DACL_SECURITY_INFORMATION, PACL, PSECURITY_DESCRIPTOR,
                PROTECTED_DACL_SECURITY_INFORMATION,
            },
        },
    };

    let mut sddl = "D:P(A;;GA;;;WD)"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_mut_ptr(),
            u32::from(SDDL_REVISION_1),
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let descriptor = OwnedWindowsSecurityDescriptor(descriptor);
    if descriptor.0.is_null() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Windows returned no permissive security descriptor",
        ));
    }
    let mut present: BOOL = 0;
    let mut acl: PACL = std::ptr::null_mut();
    let mut defaulted: BOOL = 0;
    if unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted) }
        == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if present == 0 || acl.is_null() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Windows returned no permissive DACL",
        ));
    }

    let mut path = windows_path(path)?;
    let status = unsafe {
        SetNamedSecurityInfoW(
            path.as_mut_ptr(),
            winapi::um::accctrl::SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            acl,
            std::ptr::null_mut(),
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status as i32))
    }
}

#[cfg(windows)]
pub(super) fn publish_new_file(replacement: &Path, target: &Path) -> Result<(), ReplaceFailure> {
    use winapi::um::winbase::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    let mut replacement_name = windows_path(replacement).map_err(|error| ReplaceFailure {
        error,
        replaced: false,
        indeterminate: false,
    })?;
    let mut target_name = windows_path(target).map_err(|error| ReplaceFailure {
        error,
        replaced: false,
        indeterminate: false,
    })?;

    // Without MOVEFILE_REPLACE_EXISTING, MoveFileExW atomically fails if the
    // destination already exists; no path check can turn a raced target into
    // a replacement.
    let status = unsafe {
        MoveFileExW(
            replacement_name.as_mut_ptr(),
            target_name.as_mut_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if status != 0 {
        Ok(())
    } else {
        Err(ReplaceFailure {
            error: std::io::Error::last_os_error(),
            replaced: false,
            indeterminate: false,
        })
    }
}

#[cfg(all(windows, test))]
pub(super) fn restrictive_windows_dacl_snapshot() -> std::io::Result<WindowsDaclSnapshot> {
    let (_descriptor, dacl) = restrictive_creation_dacl()?;
    snapshot_windows_dacl(dacl)
}

#[cfg(all(windows, test))]
pub(super) fn protect_windows_dacl(path: &Path) -> std::io::Result<()> {
    set_windows_dacl_protection(path, true)
}

#[cfg(all(windows, test))]
pub(super) fn unprotect_windows_dacl(path: &Path) -> std::io::Result<()> {
    set_windows_dacl_protection(path, false)
}

#[cfg(all(windows, test))]
fn set_windows_dacl_protection(path: &Path, protected: bool) -> std::io::Result<()> {
    use winapi::um::{
        accctrl::SE_FILE_OBJECT,
        aclapi::SetNamedSecurityInfoW,
        winnt::{
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
            UNPROTECTED_DACL_SECURITY_INFORMATION,
        },
    };

    let (descriptor, dacl) = read_windows_dacl(path)?;
    let mut path = windows_path(path)?;
    let protection = if protected {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    let status = unsafe {
        SetNamedSecurityInfoW(
            path.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | protection,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            dacl.acl,
            std::ptr::null_mut(),
        )
    };
    drop(descriptor);
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status as i32))
    }
}

#[cfg(windows)]
pub(super) fn replace_file(
    replacement: &Path,
    replaced: &Path,
    backup: &Path,
) -> Result<(), ReplaceFailure> {
    use winapi::um::winbase::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    match fs::symlink_metadata(backup) {
        Ok(_) => {
            return Err(ReplaceFailure {
                error: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "Windows replacement backup path already exists",
                ),
                replaced: false,
                indeterminate: false,
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(ReplaceFailure {
                error,
                replaced: false,
                indeterminate: false,
            });
        }
    }

    let mut replacement_name = windows_path(replacement).map_err(|error| ReplaceFailure {
        error,
        replaced: false,
        indeterminate: false,
    })?;
    let mut replaced_name = windows_path(replaced).map_err(|error| ReplaceFailure {
        error,
        replaced: false,
        indeterminate: false,
    })?;
    let mut backup_name = windows_path(backup).map_err(|error| ReplaceFailure {
        error,
        replaced: false,
        indeterminate: false,
    })?;

    match fs::symlink_metadata(replaced) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let status = unsafe {
                MoveFileExW(
                    replacement_name.as_mut_ptr(),
                    replaced_name.as_mut_ptr(),
                    MOVEFILE_WRITE_THROUGH,
                )
            };
            if status != 0 {
                Ok(())
            } else {
                Err(ReplaceFailure {
                    error: std::io::Error::last_os_error(),
                    replaced: false,
                    indeterminate: false,
                })
            }
        }
        Err(error) => Err(ReplaceFailure {
            error,
            replaced: false,
            indeterminate: false,
        }),
        Ok(_) => {
            // ReplaceFileW atomically transfers the replaced file's DACL to the
            // replacement, avoiding a path-based ACL snapshot race.
            let status = unsafe {
                replace_file_w(
                    replaced_name.as_mut_ptr(),
                    replacement_name.as_mut_ptr(),
                    backup_name.as_mut_ptr(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status != 0 {
                return match fs::remove_file(backup) {
                    Ok(()) => Ok(()),
                    Err(error) => Err(ReplaceFailure {
                        error,
                        replaced: true,
                        indeterminate: false,
                    }),
                };
            }

            let error = std::io::Error::last_os_error();
            Err(recover_replace_failure(error))
        }
    }
}

