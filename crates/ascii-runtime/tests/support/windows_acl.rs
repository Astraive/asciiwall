use std::ffi::c_void;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
const PROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x8000_0000;
const SDDL_REVISION_1: u32 = 1;
const SE_DACL_PROTECTED: u16 = 0x1000;

#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        string_security_descriptor: *const u16,
        string_sd_revision: u32,
        security_descriptor: *mut *mut c_void,
        security_descriptor_size: *mut u32,
    ) -> i32;
    fn SetFileSecurityW(
        file_name: *const u16,
        security_information: u32,
        security_descriptor: *const c_void,
    ) -> i32;
    fn GetFileSecurityW(
        file_name: *const u16,
        requested_information: u32,
        security_descriptor: *mut c_void,
        length: u32,
        length_needed: *mut u32,
    ) -> i32;
    fn GetSecurityDescriptorDacl(
        security_descriptor: *const c_void,
        dacl_present: *mut i32,
        dacl: *mut *mut c_void,
        dacl_defaulted: *mut i32,
    ) -> i32;
    fn GetSecurityDescriptorControl(
        security_descriptor: *const c_void,
        control: *mut u16,
        revision: *mut u32,
    ) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub fn set_custom_dacl(path: &Path) -> io::Result<()> {
    let sddl: Vec<u16> = "D:P(A;;GA;;;OW)(A;;GR;;;WD)"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: SDDL is NUL-terminated UTF-16 and both output pointers are valid.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    let mut dacl_present = 0;
    let mut dacl = std::ptr::null_mut();
    let mut dacl_defaulted = 0;
    // SAFETY: descriptor is a valid self-relative descriptor from the SDDL conversion.
    let got_dacl = unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
    };
    if got_dacl == 0 || dacl_present == 0 || dacl.is_null() {
        // SAFETY: descriptor was allocated by the Windows security API.
        unsafe { LocalFree(descriptor) };
        return Err(io::Error::other("custom SDDL did not produce a DACL"));
    }

    let path_wide = wide_path(path);
    // SAFETY: path is NUL-terminated and DACL points into the still-live descriptor.
    let result = unsafe {
        SetFileSecurityW(
            path_wide.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    // SAFETY: descriptor was allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
    unsafe { LocalFree(descriptor) };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn dacl(path: &Path) -> io::Result<(Vec<u8>, bool)> {
    let path_wide = wide_path(path);
    let mut required = 0;
    // SAFETY: path is NUL-terminated; this sizing call intentionally supplies no buffer.
    let first = unsafe {
        GetFileSecurityW(
            path_wide.as_ptr(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut required,
        )
    };
    if first != 0 {
        return Err(io::Error::other(
            "GetFileSecurityW unexpectedly succeeded without a buffer",
        ));
    }
    let sizing_error = io::Error::last_os_error();
    if sizing_error.raw_os_error() != Some(122) {
        return Err(sizing_error);
    }
    let words = usize::try_from(required)
        .ok()
        .and_then(|bytes| bytes.checked_add(3))
        .map(|bytes| bytes / 4)
        .ok_or_else(|| io::Error::other("security descriptor size overflowed"))?;
    let mut descriptor = vec![0u32; words];
    // SAFETY: descriptor is writable, suitably aligned storage of the required size.
    if unsafe {
        GetFileSecurityW(
            path_wide.as_ptr(),
            DACL_SECURITY_INFORMATION,
            descriptor.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    let descriptor_ptr = descriptor.as_ptr().cast::<c_void>();
    let mut dacl_present = 0;
    let mut dacl = std::ptr::null_mut();
    let mut dacl_defaulted = 0;
    // SAFETY: descriptor contains the valid self-relative descriptor returned above.
    if unsafe {
        GetSecurityDescriptorDacl(
            descriptor_ptr,
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
    } == 0
        || dacl_present == 0
        || dacl.is_null()
    {
        return Err(io::Error::other("file security descriptor has no DACL"));
    }
    // ACL.AclSize is a 16-bit field at byte offset 2.
    // SAFETY: a valid ACL header has at least four bytes and dacl points to that header.
    let acl_size = usize::from(unsafe { dacl.cast::<u8>().add(2).cast::<u16>().read_unaligned() });
    // SAFETY: the API returned a valid ACL pointer whose AclSize bounds this slice.
    let acl = unsafe { std::slice::from_raw_parts(dacl.cast::<u8>(), acl_size) }.to_vec();

    let mut control = 0;
    let mut revision = 0;
    // SAFETY: descriptor_ptr points to the valid self-relative descriptor.
    if unsafe { GetSecurityDescriptorControl(descriptor_ptr, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((acl, control & SE_DACL_PROTECTED != 0))
}
