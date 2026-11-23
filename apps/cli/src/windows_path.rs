//! Handle-relative local path access for Windows offline conversions.
//!
//! The path is walked one component at a time from a pinned volume-root handle. Every child is
//! opened with `FILE_OPEN_REPARSE_POINT`, checked on that handle, and compared with the root volume
//! identity before it can be read or used as a destination directory.

use std::ffi::{OsStr, OsString, c_void};
use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::path::{Component, Path, PathBuf, Prefix};
use std::ptr;

use ascii_core::CancellationToken;

use crate::arguments::{ConvertArgs, OutputPathState};
use crate::font::FontLoadError;
use crate::output::PublishError;
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TestHookPoint {
    RootResolved,
    Validated,
    Staged,
}
#[cfg(test)]
type TestHookCallback = dyn FnMut(TestHookPoint);

#[cfg(test)]
thread_local! {
    static TEST_HOOK: std::cell::RefCell<Option<Box<TestHookCallback>>> =
        std::cell::RefCell::new(None);
}
#[cfg(test)]
thread_local! {
    static TEST_LAST_PUBLISH_RENAME_ERROR: std::cell::Cell<Option<i32>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) struct TestHookGuard;

#[cfg(test)]
pub(crate) fn install_test_hook(hook: impl FnMut(TestHookPoint) + 'static) -> TestHookGuard {
    TEST_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    TestHookGuard
}

#[cfg(test)]
pub(crate) fn test_hook(point: TestHookPoint) {
    TEST_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().as_mut() {
            hook(point);
        }
    });
}
#[cfg(test)]
pub(crate) fn test_last_publish_rename_error() -> Option<i32> {
    TEST_LAST_PUBLISH_RENAME_ERROR.with(std::cell::Cell::get)
}

#[cfg(test)]
impl Drop for TestHookGuard {
    fn drop(&mut self) {
        TEST_HOOK.with(|slot| *slot.borrow_mut() = None);
    }
}

const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0000_0010;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const FILE_SHARE_DELETE: u32 = 0x0000_0004;
const FILE_LIST_DIRECTORY: u32 = 0x0000_0001;
const FILE_TRAVERSE: u32 = 0x0000_0020;
const FILE_READ_DATA: u32 = 0x0000_0001;
const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
const FILE_WRITE_DATA: u32 = 0x0000_0002;
const FILE_ADD_SUBDIRECTORY: u32 = 0x0000_0004;
const DELETE: u32 = 0x0001_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const OBJ_CASE_INSENSITIVE: u32 = 0x0000_0040;
const FILE_OPEN: u32 = 0x0000_0001;
const FILE_CREATE: u32 = 0x0000_0002;
const FILE_DIRECTORY_FILE: u32 = 0x0000_0001;
const FILE_NON_DIRECTORY_FILE: u32 = 0x0000_0040;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x0000_0020;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const OPEN_EXISTING: u32 = 3;
const FILE_ID_INFO_CLASS: i32 = 18;
const FILE_TYPE_DISK: u32 = 1;
const FILE_FS_DEVICE_INFORMATION_CLASS: i32 = 4;
const FILE_DEVICE_CD_ROM: u32 = 0x0000_0002;
const FILE_DEVICE_DISK: u32 = 0x0000_0007;
#[cfg(test)]
const FILE_DEVICE_NETWORK_FILE_SYSTEM: u32 = 0x0000_0014;
const FILE_REMOTE_DEVICE: u32 = 0x0000_0010;
const FILE_CHARACTERISTIC_TS_DEVICE: u32 = 0x0000_1000;
const FILE_CHARACTERISTIC_WEBDAV_DEVICE: u32 = 0x0000_2000;
// NtSetInformationFile uses native class 10; Win32 SetFileInformationByHandle uses class 3.
const FILE_RENAME_INFORMATION_CLASS: i32 = 10;
const FILE_DISPOSITION_INFO_CLASS: i32 = 4;
#[cfg(test)]
const ERROR_INVALID_PARAMETER: i32 = 87;
const ERROR_FILE_EXISTS: i32 = 80;
const ERROR_ALREADY_EXISTS: i32 = 183;
#[cfg(test)]
pub(crate) const ERROR_SHARING_VIOLATION: i32 = 32;
const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
const SHARE_READ_WRITE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;

#[allow(dead_code)]
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[allow(dead_code)]
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: *mut c_void,
    object_name: *mut UnicodeString,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_quality_of_service: *mut c_void,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Default)]
struct IoStatusBlock {
    status: i32,
    information: usize,
}
#[repr(C)]
#[derive(Default)]
struct FileFsDeviceInformation {
    device_type: u32,
    characteristics: u32,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Default)]
struct ByHandleFileInformation {
    attributes: u32,
    creation_time: FileTime,
    last_access_time: FileTime,
    last_write_time: FileTime,
    volume_serial: u32,
    size_high: u32,
    size_low: u32,
    number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}
#[repr(C)]
#[derive(Default)]
struct FileIdInfo {
    volume_serial: u64,
    file_id: [u8; 16],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VolumeIdentity(u64);

#[allow(dead_code)]
#[repr(C)]
struct RenameInfo {
    flags: u32,
    root_directory: *mut c_void,
    file_name_length: u32,
    file_name: [u16; 1],
}

#[derive(Debug)]
enum ChainResult {
    Ready(File),
    Missing(io::Error),
}
struct DirectoryChain {
    directory: File,
    writable: bool,
    write_error: Option<io::Error>,
}

enum DirectoryChainResult {
    Ready(DirectoryChain),
    Missing(io::Error),
}

#[derive(Clone, Copy)]
enum InputKind {
    Image,
    Font,
}

pub(crate) struct PreparedInput {
    file: Option<File>,
    error: Option<io::Error>,
}

impl PreparedInput {
    pub(crate) fn into_file(mut self) -> Result<File, FontLoadError> {
        if let Some(error) = self.error.take() {
            return Err(FontLoadError::Io(error));
        }
        self.file.take().ok_or(FontLoadError::NotRegularFile)
    }
}

pub(crate) struct PreparedOutput {
    parent: Option<File>,
    leaf: Option<Vec<u16>>,
    parent_error: Option<io::Error>,
    root_volume_identity: VolumeIdentity,
    pub(crate) state: OutputPathState,
}

impl PreparedOutput {
    pub(crate) fn publish(
        &self,
        overwrite: bool,
        cancellation: &CancellationToken,
        write_artifact: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> Result<(), PublishError> {
        if cancellation.is_cancelled() {
            return Err(PublishError::Cancelled);
        }
        if let Some(error) = &self.parent_error {
            return Err(io_or_cancel(
                io::Error::new(error.kind(), error.to_string()),
                cancellation,
            ));
        }
        let (Some(parent), Some(leaf)) = (&self.parent, &self.leaf) else {
            return Err(PublishError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "destination is not a file path",
            )));
        };
        if cancellation.is_cancelled() {
            return Err(PublishError::Cancelled);
        }
        publish_relative(
            parent,
            leaf,
            self.root_volume_identity,
            overwrite,
            cancellation,
            write_artifact,
        )
    }
}

pub(crate) struct PreparedConvertIo {
    pub(crate) image: Option<PreparedInput>,
    pub(crate) font: Option<PreparedInput>,
    pub(crate) output: Option<PreparedOutput>,
}

enum RootPath {
    Drive(u8),
    Native(Vec<u16>),
    VolumeGuid(Vec<u16>),
}

struct PathSpec {
    root_path: RootPath,
    components: Vec<Vec<u16>>,
}

struct RootPin {
    file: File,
    volume_identity: VolumeIdentity,
    writable: bool,
    write_error: Option<io::Error>,
}

#[derive(Debug)]
pub(crate) struct OfflinePathError {
    stage: &'static str,
    source: Option<io::Error>,
}

#[derive(Debug)]
struct UnsafeOfflinePath(&'static str);

impl fmt::Display for UnsafeOfflinePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for UnsafeOfflinePath {}

fn unsafe_offline_path_error(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, UnsafeOfflinePath(message))
}

impl OfflinePathError {
    fn rejected(stage: &'static str) -> Self {
        Self {
            stage,
            source: None,
        }
    }

    fn with_source(stage: &'static str, source: io::Error) -> Self {
        Self {
            stage,
            source: Some(source),
        }
    }
    pub(crate) fn is_rejected(&self) -> bool {
        self.source.is_none()
            || self.source.as_ref().is_some_and(|source| {
                source
                    .get_ref()
                    .is_some_and(|source| source.is::<UnsafeOfflinePath>())
            })
    }

    pub(crate) fn is_permission_denied(&self) -> bool {
        self.source
            .as_ref()
            .is_some_and(|source| source.kind() == io::ErrorKind::PermissionDenied)
    }
}

impl fmt::Display for OfflinePathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Windows offline path check failed during {}",
            self.stage
        )?;
        if let Some(source) = &self.source {
            write!(formatter, ": {source}")?;
        }
        Ok(())
    }
}

impl std::error::Error for OfflinePathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

impl From<()> for OfflinePathError {
    fn from(_: ()) -> Self {
        Self::rejected("path resolution or handle validation")
    }
}

pub(crate) fn prepare_offline_paths(
    request: &ConvertArgs,
    config: Option<&Path>,
) -> Result<PreparedConvertIo, OfflinePathError> {
    let cwd = std::env::current_dir()
        .map_err(|source| OfflinePathError::with_source("current-directory resolution", source))?;
    let image_path = resolve_path_spec(path_spec(&request.image, &cwd)?)?;
    let font_path = request
        .font
        .as_deref()
        .map(|path| path_spec(path, &cwd).and_then(resolve_path_spec))
        .transpose()?;
    let output_path = (request.output.as_os_str() != OsStr::new("-"))
        .then(|| path_spec(&request.output, &cwd).and_then(resolve_path_spec))
        .transpose()?;
    let config_path = config
        .map(|path| path_spec(path, &cwd).and_then(resolve_path_spec))
        .transpose()?;

    let output_parent_at_root = output_path.as_ref().is_some_and(output_parent_is_root);
    let specs = [
        ("image", Some(&image_path), false),
        ("font", font_path.as_ref(), false),
        ("output", output_path.as_ref(), output_parent_at_root),
        ("config", config_path.as_ref(), false),
    ];
    // Resolve aliases first, then reject only unresolved or unsupported root namespaces.
    // Locality is classified from the opened root handle below, never from a drive pathname.
    for (_, spec, _) in specs {
        if spec.is_some_and(|spec| matches!(&spec.root_path, RootPath::Drive(_))) {
            return Err(OfflinePathError::rejected("unresolved DOS drive root"));
        }
    }
    #[cfg(test)]
    test_hook(TestHookPoint::RootResolved);

    let mut roots = Vec::with_capacity(specs.len());
    for (_role, spec, writable_root) in specs {
        let Some(spec) = spec else {
            continue;
        };
        roots.push(open_root(spec, writable_root)?);
    }
    let mut roots = roots.into_iter();
    let image_root = roots.next().ok_or(())?;
    let font_root = font_path
        .as_ref()
        .map(|_| roots.next().ok_or(()))
        .transpose()?;
    let output_root = output_path
        .as_ref()
        .map(|_| roots.next().ok_or(()))
        .transpose()?;
    let config_root = config_path
        .as_ref()
        .map(|_| roots.next().ok_or(()))
        .transpose()?;

    let image = open_input(&image_root, &image_path, InputKind::Image)?;
    let font = match (font_root.as_ref(), font_path.as_ref()) {
        (Some(root), Some(path)) => Some(open_input(root, path, InputKind::Font)?),
        _ => None,
    };
    let output = match (output_root.as_ref(), output_path.as_ref()) {
        (Some(root), Some(path)) => Some(open_output(root, path)?),
        _ => None,
    };
    if let (Some(root), Some(path)) = (config_root.as_ref(), config_path.as_ref()) {
        check_existing_path(root, path)?;
    }

    Ok(PreparedConvertIo {
        image: Some(image),
        font,
        output,
    })
}
const PARENT_COMPONENT: [u16; 2] = [b'.' as u16, b'.' as u16];

fn drive_current_directory(drive: u8) -> Result<PathBuf, ()> {
    const MAX_PATH_UNITS: u32 = 32_768;
    let drive_relative_current = [drive as u16, b':' as u16, 0];
    let required = unsafe {
        GetFullPathNameW(
            drive_relative_current.as_ptr(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if required == 0 || required > MAX_PATH_UNITS {
        return Err(());
    }
    let capacity = usize::try_from(required)
        .map_err(|_| ())?
        .checked_add(1)
        .ok_or(())?;
    let capacity_u32 = u32::try_from(capacity).map_err(|_| ())?;
    let mut units = Vec::new();
    units.try_reserve_exact(capacity).map_err(|_| ())?;
    units.resize(capacity, 0);
    let written = unsafe {
        GetFullPathNameW(
            drive_relative_current.as_ptr(),
            capacity_u32,
            units.as_mut_ptr(),
            ptr::null_mut(),
        )
    };
    if written == 0 || written as usize >= capacity {
        return Err(());
    }
    let directory = PathBuf::from(OsString::from_wide(&units[..written as usize]));
    if directory.is_absolute() {
        Ok(directory)
    } else {
        Err(())
    }
}

fn path_spec(path: &Path, cwd: &Path) -> Result<PathSpec, ()> {
    let explicitly_verbatim = matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::VerbatimDisk(_) | Prefix::Verbatim(_))
    );
    if !explicitly_verbatim {
        for component in path.components() {
            if let Component::Normal(value) = component
                && matches!(
                    value.encode_wide().last(),
                    Some(unit) if unit == b'.' as u16 || unit == b' ' as u16
                )
            {
                return Err(());
            }
        }
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(Component::Prefix(prefix)) = path.components().next() {
        let Prefix::Disk(drive) = prefix.kind() else {
            return Err(());
        };
        let mut absolute = drive_current_directory(drive)?;
        // Resolve only the per-drive current directory; keep the input components for handle walking.
        for component in path.components().skip(1) {
            match component {
                Component::Prefix(_) | Component::RootDir => return Err(()),
                Component::CurDir => {}
                Component::ParentDir => absolute.push(".."),
                Component::Normal(value) => absolute.push(value),
            }
        }
        absolute
    } else {
        let mut absolute = cwd.to_path_buf();
        absolute.push(path);
        absolute
    };
    if !absolute.is_absolute() {
        return Err(());
    }

    let mut prefix = None;
    let mut rooted = false;
    let mut components = Vec::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(value) => prefix = Some(value),
            Component::RootDir => rooted = true,
            Component::CurDir => {}
            // Keep prior components visible so `..` cannot lexically hide a reparse point.
            Component::ParentDir => components.push(PARENT_COMPONENT.to_vec()),
            Component::Normal(value) => {
                let units: Vec<u16> = value.encode_wide().collect();
                if units.contains(&0) {
                    return Err(());
                }
                components.push(units);
            }
        }
    }
    if !rooted {
        return Err(());
    }

    let root_path = match prefix.ok_or(())?.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => RootPath::Drive(drive),
        Prefix::Verbatim(value) if is_volume_guid(value) => {
            let mut root = OsString::from(r"\\?\");
            root.push(value);
            root.push(r"\");
            let mut units: Vec<u16> = root.encode_wide().collect();
            units.push(0);
            RootPath::VolumeGuid(units)
        }
        Prefix::UNC(_, _)
        | Prefix::VerbatimUNC(_, _)
        | Prefix::DeviceNS(_)
        | Prefix::Verbatim(_) => return Err(()),
    };

    Ok(PathSpec {
        root_path,
        components,
    })
}
fn resolve_path_spec(mut spec: PathSpec) -> Result<PathSpec, ()> {
    let drive = match &spec.root_path {
        RootPath::Drive(drive) => Some(*drive),
        RootPath::Native(_) | RootPath::VolumeGuid(_) => None,
    };
    if let Some(drive) = drive {
        let (root_path, mut prefix_components) = resolve_dos_drive(drive, 0)?;
        prefix_components.extend(spec.components);
        spec.root_path = root_path;
        spec.components = prefix_components;
    }
    Ok(spec)
}

fn resolve_dos_drive(drive: u8, seen: u32) -> Result<(RootPath, Vec<Vec<u16>>), ()> {
    let drive = drive.to_ascii_uppercase();
    if !drive.is_ascii_uppercase() {
        return Err(());
    }
    let bit = 1_u32 << (drive - b'A');
    if seen & bit != 0 {
        return Err(());
    }

    let device_name = [drive as u16, b':' as u16, 0];
    let mut target = Vec::new();
    target.try_reserve_exact(32_768).map_err(|_| ())?;
    target.resize(32_768, 0);
    let written = unsafe {
        QueryDosDeviceW(
            device_name.as_ptr(),
            target.as_mut_ptr(),
            target.len() as u32,
        )
    };
    if written == 0 || written as usize >= target.len() {
        return Err(());
    }
    let target_length = target.iter().position(|unit| *unit == 0).ok_or(())?;
    let target = &target[..target_length];
    if target.is_empty() {
        return Err(());
    }

    if let Some(root_length) = harddisk_volume_root_length(target) {
        let mut root = target[..root_length].to_vec();
        root.push(b'\\' as u16);
        let components = parse_native_components(&target[root_length..])?;
        return Ok((RootPath::Native(root), components));
    }

    if !starts_with_ascii_case_insensitive(target, r"\??\") {
        return Err(());
    }
    let alias = &target[4..];
    if alias.len() >= 6
        && alias[0] <= u8::MAX as u16
        && (alias[0] as u8).is_ascii_alphabetic()
        && alias[1] == b':' as u16
        && is_path_separator(alias[2])
    {
        let (root, mut components) = resolve_dos_drive(alias[0] as u8, seen | bit)?;
        components.extend(parse_native_components(&alias[2..])?);
        return Ok((root, components));
    }

    if alias.len() >= 44
        && is_volume_guid_units(&alias[..44])
        && (alias.len() == 44 || is_path_separator(alias[44]))
    {
        let volume_name = OsString::from_wide(&alias[..44]);
        let mut root = OsString::from(r"\\?\");
        root.push(volume_name);
        root.push(r"\");
        let mut root_path: Vec<u16> = root.encode_wide().collect();
        root_path.push(0);
        let components = parse_native_components(&alias[44..])?;
        return Ok((RootPath::VolumeGuid(root_path), components));
    }

    Err(())
}

fn harddisk_volume_root_length(target: &[u16]) -> Option<usize> {
    const PREFIX: &str = r"\Device\HarddiskVolume";
    if !starts_with_ascii_case_insensitive(target, PREFIX) {
        return None;
    }
    let mut end = PREFIX.encode_utf16().count();
    let first_digit = end;
    while target.get(end).is_some_and(|unit| matches!(unit, 48..=57)) {
        end += 1;
    }
    (end > first_digit && (end == target.len() || is_path_separator(target[end]))).then_some(end)
}

fn verify_local_volume_provider_handle(file: &File) -> io::Result<bool> {
    let mut information = FileFsDeviceInformation::default();
    let mut io_status = IoStatusBlock::default();
    let status = unsafe {
        NtQueryVolumeInformationFile(
            file.as_raw_handle(),
            &mut io_status,
            &mut information as *mut FileFsDeviceInformation as *mut c_void,
            size_of::<FileFsDeviceInformation>() as u32,
            FILE_FS_DEVICE_INFORMATION_CLASS,
        )
    };
    if status < 0 {
        return Err(nt_status_error(status));
    }
    if io_status.status < 0 {
        return Err(nt_status_error(io_status.status));
    }
    if io_status.information != size_of::<FileFsDeviceInformation>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "filesystem device query returned an unexpected size",
        ));
    }
    Ok(is_local_volume_device(&information))
}

fn is_local_volume_device(information: &FileFsDeviceInformation) -> bool {
    let remote_characteristics =
        FILE_REMOTE_DEVICE | FILE_CHARACTERISTIC_TS_DEVICE | FILE_CHARACTERISTIC_WEBDAV_DEVICE;
    matches!(
        information.device_type,
        FILE_DEVICE_DISK | FILE_DEVICE_CD_ROM
    ) && information.characteristics & remote_characteristics == 0
}

fn parse_native_components(path: &[u16]) -> Result<Vec<Vec<u16>>, ()> {
    let mut components = Vec::new();
    let mut index = 0;
    while index < path.len() {
        while path.get(index).is_some_and(|unit| is_path_separator(*unit)) {
            index += 1;
        }
        let start = index;
        while path
            .get(index)
            .is_some_and(|unit| !is_path_separator(*unit))
        {
            index += 1;
        }
        if start == index {
            continue;
        }
        let component = &path[start..index];
        if component == [b'.' as u16] {
            continue;
        }
        if component == PARENT_COMPONENT {
            components.push(PARENT_COMPONENT.to_vec());
            continue;
        }
        if component
            .iter()
            .any(|unit| matches!(*unit, 0 | 34 | 42 | 58 | 60 | 62 | 63 | 124))
            || matches!(component.last(), Some(32 | 46))
        {
            return Err(());
        }
        components.push(component.to_vec());
    }
    Ok(components)
}

fn is_path_separator(unit: u16) -> bool {
    matches!(unit, 47 | 92)
}

fn starts_with_ascii_case_insensitive(units: &[u16], prefix: &str) -> bool {
    let prefix_length = prefix.encode_utf16().count();
    units.len() >= prefix_length
        && units
            .iter()
            .copied()
            .zip(prefix.encode_utf16())
            .take(prefix_length)
            .all(|(actual, expected)| ascii_lower(actual) == ascii_lower(expected))
}

fn ascii_lower(unit: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&unit) {
        unit + (b'a' - b'A') as u16
    } else {
        unit
    }
}

fn is_volume_guid(prefix: &OsStr) -> bool {
    let units: Vec<u16> = prefix.encode_wide().collect();
    is_volume_guid_units(&units)
}

fn is_volume_guid_units(units: &[u16]) -> bool {
    if units.len() != 44
        || !units[..7]
            .iter()
            .zip("Volume{".encode_utf16())
            .all(|(actual, expected)| ascii_lower(*actual) == ascii_lower(expected))
        || units[43] != b'}' as u16
    {
        return false;
    }
    for (index, unit) in units[7..43].iter().copied().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if unit != b'-' as u16 {
                return false;
            }
        } else if !matches!(unit, 48..=57 | 65..=70 | 97..=102) {
            return false;
        }
    }
    true
}

fn volume_identity(file: &File) -> io::Result<VolumeIdentity> {
    let mut info = FileIdInfo::default();
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FILE_ID_INFO_CLASS,
            (&mut info as *mut FileIdInfo).cast(),
            size_of::<FileIdInfo>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.volume_serial == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pinned root did not provide a stable volume identity",
        ));
    }
    Ok(VolumeIdentity(info.volume_serial))
}

fn volume_file_system_name(file: &File) -> io::Result<[u16; 64]> {
    let mut name = [0_u16; 64];
    let succeeded = unsafe {
        GetVolumeInformationByHandleW(
            file.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(name)
}

fn is_known_local_file_system(file: &File) -> io::Result<bool> {
    let name = volume_file_system_name(file)?;
    let end = name.iter().position(|unit| *unit == 0).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "unterminated filesystem name")
    })?;
    Ok(["NTFS", "ReFS", "FAT", "FAT32", "exFAT"]
        .iter()
        .any(|known| ascii_units_equal_ignore_case(&name[..end], known)))
}

fn ascii_units_equal_ignore_case(units: &[u16], text: &str) -> bool {
    units.len() == text.len()
        && units
            .iter()
            .copied()
            .zip(text.bytes())
            .all(|(unit, byte)| ascii_lower(unit) == u16::from(byte.to_ascii_lowercase()))
}

fn volume_identity_matches(actual: VolumeIdentity, expected: VolumeIdentity) -> bool {
    actual == expected
}

fn verify_same_volume(file: &File, expected: VolumeIdentity) -> io::Result<()> {
    if !volume_identity_matches(volume_identity(file)?, expected) {
        return Err(unsafe_offline_path_error(
            "opened path does not match the pinned local volume identity",
        ));
    }
    Ok(())
}

fn open_root_handle(root_path: &RootPath, access: u32) -> io::Result<File> {
    match root_path {
        RootPath::Native(path) => open_native_root(path, access),
        RootPath::VolumeGuid(path) => {
            let handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    access,
                    SHARE_ALL,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                Err(io::Error::last_os_error())
            } else {
                Ok(unsafe { File::from_raw_handle(handle as RawHandle) })
            }
        }
        RootPath::Drive(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unresolved DOS drive root",
        )),
    }
}

fn open_native_root(path: &[u16], access: u32) -> io::Result<File> {
    let byte_length = path
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "native root path is too long")
        })?;
    if byte_length == 0 || path.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid native root path",
        ));
    }
    let mut name = UnicodeString {
        length: byte_length,
        maximum_length: byte_length,
        buffer: path.as_ptr() as *mut u16,
    };
    let mut attributes = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root_directory: ptr::null_mut(),
        object_name: &mut name,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut io_status = IoStatusBlock::default();
    let mut handle = ptr::null_mut();
    // FILE_DIRECTORY_FILE is omitted because NtCreateFile documents it as incompatible with
    // FILE_OPEN_REPARSE_POINT; open_root validates directory type on this returned handle.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &mut attributes,
            &mut io_status,
            ptr::null_mut(),
            FILE_ATTRIBUTE_NORMAL,
            SHARE_ALL,
            FILE_OPEN,
            FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            ptr::null_mut(),
            0,
        )
    };
    if status < 0 {
        return Err(nt_status_error(status));
    }
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native root open returned an invalid handle",
        ));
    }
    Ok(unsafe { File::from_raw_handle(handle as RawHandle) })
}

fn open_root(spec: &PathSpec, writable: bool) -> Result<RootPin, OfflinePathError> {
    // A writable destination root needs FILE_ADD_SUBDIRECTORY for its private staging directory.
    let mut access = FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
    if writable {
        access |= FILE_WRITE_DATA | FILE_ADD_SUBDIRECTORY;
    }
    let file = match open_root_handle(&spec.root_path, access) {
        Ok(file) => file,
        Err(error) if writable => {
            // Keep destination-state inspection available; publication reports this write failure.
            let mut root = open_root(spec, false)?;
            root.write_error = Some(error);
            return Ok(root);
        }
        Err(error) => return Err(OfflinePathError::with_source("volume-root open", error)),
    };
    let volume_identity = volume_identity(&file)
        .map_err(|source| OfflinePathError::with_source("volume identity", source))?;
    // The documented handle-bound filesystem device information identifies remote providers;
    // a device-path spelling alone does not establish local backing.
    let local_provider = verify_local_volume_provider_handle(&file)
        .map_err(|source| OfflinePathError::with_source("provider classification", source))?;
    if !local_provider {
        return Err(OfflinePathError::rejected(
            "remote or unsupported filesystem provider",
        ));
    }
    if !is_known_local_file_system(&file)
        .map_err(|source| OfflinePathError::with_source("filesystem classification", source))?
    {
        return Err(OfflinePathError::rejected("unsupported filesystem"));
    }
    let info = file_information(&file)
        .map_err(|source| OfflinePathError::with_source("volume-root inspection", source))?;
    if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK
    {
        return Err(OfflinePathError::rejected("volume-root safety validation"));
    }
    verify_same_volume(&file, volume_identity)
        .map_err(|source| OfflinePathError::with_source("volume identity recheck", source))?;
    Ok(RootPin {
        file,
        volume_identity,
        writable,
        write_error: None,
    })
}

fn open_input(
    root: &RootPin,
    spec: &PathSpec,
    kind: InputKind,
) -> Result<PreparedInput, OfflinePathError> {
    let Some((leaf, parents)) = spec.components.split_last() else {
        return Ok(PreparedInput {
            file: None,
            error: None,
        });
    };
    if leaf.as_slice() == PARENT_COMPONENT.as_slice() {
        let error = match open_directories(root, &spec.components)? {
            ChainResult::Ready(_) => None,
            ChainResult::Missing(error) => Some(error),
        };
        return Ok(PreparedInput { file: None, error });
    }
    let parent = match open_directories(root, parents)? {
        ChainResult::Ready(parent) => parent,
        ChainResult::Missing(error) => {
            return Ok(PreparedInput {
                file: None,
                error: Some(error),
            });
        }
    };
    let file = match open_relative_with_share(
        &parent,
        leaf,
        FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        FILE_OPEN,
        SHARE_READ_WRITE,
    ) {
        Ok(file) => file,
        Err(error) if is_missing(&error) => {
            return Ok(PreparedInput {
                file: None,
                error: Some(error),
            });
        }
        Err(error) => {
            return Err(OfflinePathError::with_source("input leaf open", error));
        }
    };
    let info = checked_information(&file, root.volume_identity)
        .map_err(|source| OfflinePathError::with_source("input handle validation", source))?;
    if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(OfflinePathError::rejected("input file type validation"));
    }
    if info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return match kind {
            InputKind::Image => Err(OfflinePathError::rejected("input file type validation")),
            InputKind::Font => Ok(PreparedInput {
                file: None,
                error: None,
            }),
        };
    }
    Ok(PreparedInput {
        file: Some(file),
        error: None,
    })
}

fn open_output(root: &RootPin, spec: &PathSpec) -> Result<PreparedOutput, OfflinePathError> {
    let Some((leaf, parents)) = spec.components.split_last() else {
        return Ok(PreparedOutput {
            parent: None,
            leaf: None,
            parent_error: None,
            root_volume_identity: root.volume_identity,
            state: OutputPathState::Directory,
        });
    };
    if leaf.as_slice() == PARENT_COMPONENT.as_slice() {
        return Ok(match open_directories(root, &spec.components)? {
            ChainResult::Ready(_) => PreparedOutput {
                parent: None,
                leaf: None,
                parent_error: None,
                root_volume_identity: root.volume_identity,
                state: OutputPathState::Directory,
            },
            ChainResult::Missing(error) => PreparedOutput {
                parent: None,
                leaf: None,
                parent_error: Some(error),
                root_volume_identity: root.volume_identity,
                state: OutputPathState::Missing,
            },
        });
    }
    let DirectoryChain {
        directory,
        writable,
        write_error,
    } = match open_output_directories(root, parents)? {
        DirectoryChainResult::Ready(parent) => parent,
        DirectoryChainResult::Missing(error) => {
            return Ok(PreparedOutput {
                parent: None,
                leaf: Some(leaf.clone()),
                parent_error: Some(error),
                root_volume_identity: root.volume_identity,
                state: OutputPathState::Missing,
            });
        }
    };
    let state = match open_relative(
        &directory,
        leaf,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        FILE_OPEN,
    ) {
        Ok(file) => {
            let info = checked_information(&file, root.volume_identity).map_err(|source| {
                OfflinePathError::with_source("output handle validation", source)
            })?;
            if info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                OutputPathState::Directory
            } else {
                OutputPathState::File
            }
        }
        Err(error) if is_missing(&error) => OutputPathState::Missing,
        Err(error) => {
            return Err(OfflinePathError::with_source("output leaf open", error));
        }
    };
    Ok(PreparedOutput {
        parent: writable.then_some(directory),
        leaf: Some(leaf.clone()),
        parent_error: write_error,
        root_volume_identity: root.volume_identity,
        state,
    })
}

fn check_existing_path(root: &RootPin, spec: &PathSpec) -> Result<(), OfflinePathError> {
    let Some((leaf, parents)) = spec.components.split_last() else {
        return Ok(());
    };
    if leaf.as_slice() == PARENT_COMPONENT.as_slice() {
        return match open_directories(root, &spec.components)? {
            ChainResult::Ready(_) | ChainResult::Missing(_) => Ok(()),
        };
    }
    let parent = match open_directories(root, parents)? {
        ChainResult::Ready(parent) => parent,
        ChainResult::Missing(_) => return Ok(()),
    };
    match open_relative(
        &parent,
        leaf,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        FILE_OPEN,
    ) {
        Ok(file) => {
            checked_information(&file, root.volume_identity).map_err(|source| {
                OfflinePathError::with_source("existing path validation", source)
            })?;
            Ok(())
        }
        Err(error) if is_missing(&error) => Ok(()),
        Err(error) => Err(OfflinePathError::with_source("existing path open", error)),
    }
}

fn output_parent_is_root(spec: &PathSpec) -> bool {
    let Some((leaf, parents)) = spec.components.split_last() else {
        return false;
    };
    leaf.as_slice() != PARENT_COMPONENT.as_slice() && final_directory_component(parents).is_none()
}

fn final_directory_component(components: &[Vec<u16>]) -> Option<usize> {
    let mut parents_to_skip = 0usize;
    for (index, name) in components.iter().enumerate().rev() {
        if name.as_slice() == PARENT_COMPONENT.as_slice() {
            parents_to_skip = parents_to_skip.saturating_add(1);
        } else if parents_to_skip != 0 {
            parents_to_skip -= 1;
        } else {
            return Some(index);
        }
    }
    None
}

fn clone_io_error(error: &io::Error) -> io::Error {
    io::Error::new(error.kind(), error.to_string())
}

fn open_output_directories(
    root: &RootPin,
    components: &[Vec<u16>],
) -> Result<DirectoryChainResult, OfflinePathError> {
    open_directories_inner(root, components, final_directory_component(components))
}

fn open_directories(
    root: &RootPin,
    components: &[Vec<u16>],
) -> Result<ChainResult, OfflinePathError> {
    let mut stack = vec![root.file.try_clone().map_err(|_| ())?];
    for name in components {
        if name.as_slice() == PARENT_COMPONENT.as_slice() {
            if stack.len() > 1 {
                stack.truncate(stack.len() - 1);
            }
            continue;
        }
        let parent = stack.last().ok_or(())?;
        // NtCreateFile does not document FILE_DIRECTORY_FILE with FILE_OPEN_REPARSE_POINT.
        // Open without a type constraint, then validate this same no-follow handle below.
        let child = match open_relative(
            parent,
            name,
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            FILE_OPEN,
        ) {
            Ok(child) => child,
            Err(error) if is_missing(&error) => return Ok(ChainResult::Missing(error)),
            Err(error) => {
                return Err(OfflinePathError::with_source(
                    "directory component open",
                    error,
                ));
            }
        };
        let info = checked_information(&child, root.volume_identity).map_err(|source| {
            OfflinePathError::with_source("directory handle validation", source)
        })?;
        if info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
            return Ok(ChainResult::Missing(io::Error::new(
                io::ErrorKind::NotADirectory,
                "path component is not a directory",
            )));
        }
        stack.push(child);
    }
    Ok(ChainResult::Ready(stack.pop().ok_or(())?))
}

fn open_directories_inner(
    root: &RootPin,
    components: &[Vec<u16>],
    writable_component: Option<usize>,
) -> Result<DirectoryChainResult, OfflinePathError> {
    let mut stack = vec![root.file.try_clone().map_err(|_| ())?];
    let mut writable = writable_component.is_none() && root.writable;
    let mut write_error = if writable_component.is_none() {
        root.write_error.as_ref().map(clone_io_error)
    } else {
        None
    };
    for (index, name) in components.iter().enumerate() {
        if name.as_slice() == PARENT_COMPONENT.as_slice() {
            if stack.len() > 1 {
                stack.truncate(stack.len() - 1);
            }
            continue;
        }
        let parent = stack.last().ok_or(())?;
        let access = FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
        // Directory write data adds files; only the output parent also needs subdirectory access.
        let child = if writable_component == Some(index) {
            match open_relative(
                parent,
                name,
                access | FILE_WRITE_DATA | FILE_ADD_SUBDIRECTORY,
                FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
                FILE_OPEN,
            ) {
                Ok(child) => {
                    writable = true;
                    child
                }
                // Read-only inspection preserves existing-destination validation precedence.
                Err(request_error) => {
                    let child = match open_relative(
                        parent,
                        name,
                        access,
                        FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
                        FILE_OPEN,
                    ) {
                        Ok(child) => child,
                        Err(error) if is_missing(&error) => {
                            return Ok(DirectoryChainResult::Missing(error));
                        }
                        Err(error) => {
                            return Err(OfflinePathError::with_source(
                                "output directory read-only open",
                                error,
                            ));
                        }
                    };
                    write_error = Some(request_error);
                    child
                }
            }
        } else {
            match open_relative(
                parent,
                name,
                access,
                FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
                FILE_OPEN,
            ) {
                Ok(child) => child,
                Err(error) if is_missing(&error) => {
                    return Ok(DirectoryChainResult::Missing(error));
                }
                Err(error) => {
                    return Err(OfflinePathError::with_source(
                        "output directory open",
                        error,
                    ));
                }
            }
        };
        let info = checked_information(&child, root.volume_identity).map_err(|source| {
            OfflinePathError::with_source("output directory validation", source)
        })?;
        if info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
            return Ok(DirectoryChainResult::Missing(io::Error::new(
                io::ErrorKind::NotADirectory,
                "path component is not a directory",
            )));
        }
        stack.push(child);
    }
    Ok(DirectoryChainResult::Ready(DirectoryChain {
        directory: stack.pop().ok_or(())?,
        writable,
        write_error,
    }))
}

fn checked_information(
    file: &File,
    expected_volume_identity: VolumeIdentity,
) -> io::Result<ByHandleFileInformation> {
    verify_same_volume(file, expected_volume_identity)?;
    let info = file_information(file)?;
    if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK
    {
        return Err(unsafe_offline_path_error(
            "path component is a reparse point or non-disk handle",
        ));
    }
    Ok(info)
}

fn file_information(file: &File) -> io::Result<ByHandleFileInformation> {
    let mut info = ByHandleFileInformation::default();
    let succeeded = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(info)
    }
}

fn windows_error_code(error: &io::Error) -> Option<i32> {
    error.raw_os_error().or_else(|| {
        error
            .get_ref()?
            .downcast_ref::<NtStatusFailure>()
            .map(|failure| failure.win32_error as i32)
    })
}

fn is_missing(error: &io::Error) -> bool {
    matches!(windows_error_code(error), Some(2 | 3 | 267))
        || matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
        )
}

fn open_relative(
    parent: &File,
    name: &[u16],
    desired_access: u32,
    create_options: u32,
    disposition: u32,
) -> io::Result<File> {
    open_relative_with_share(
        parent,
        name,
        desired_access,
        create_options,
        disposition,
        SHARE_ALL,
    )
}

fn open_relative_with_share(
    parent: &File,
    name: &[u16],
    desired_access: u32,
    create_options: u32,
    disposition: u32,
    share_access: u32,
) -> io::Result<File> {
    open_relative_with_share_and_attributes(
        parent,
        name,
        desired_access,
        create_options,
        disposition,
        share_access,
        FILE_ATTRIBUTE_NORMAL,
    )
}

fn open_relative_with_share_and_attributes(
    parent: &File,
    name: &[u16],
    desired_access: u32,
    create_options: u32,
    disposition: u32,
    share_access: u32,
    file_attributes: u32,
) -> io::Result<File> {
    let byte_length = name
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path component is too long"))?;
    let mut name = UnicodeString {
        length: byte_length,
        maximum_length: byte_length,
        buffer: name.as_ptr() as *mut u16,
    };
    let mut attributes = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root_directory: parent.as_raw_handle(),
        object_name: &mut name,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut io_status = IoStatusBlock::default();
    let mut handle = ptr::null_mut();
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            desired_access,
            &mut attributes,
            &mut io_status,
            ptr::null_mut(),
            file_attributes,
            share_access,
            disposition,
            create_options,
            ptr::null_mut(),
            0,
        )
    };
    if status < 0 {
        return Err(nt_status_error(status));
    }
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::other(
            "relative file open returned an invalid handle",
        ));
    }
    Ok(unsafe { File::from_raw_handle(handle as RawHandle) })
}
fn destination_exists_relative(parent: &File, destination: &[u16]) -> io::Result<bool> {
    match open_relative(
        parent,
        destination,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        FILE_OPEN,
    ) {
        Ok(_) => Ok(true),
        Err(error) if is_missing(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

fn publish_relative(
    parent: &File,
    destination: &[u16],
    root_volume_identity: VolumeIdentity,
    overwrite: bool,
    cancellation: &CancellationToken,
    write_artifact: impl FnOnce(&mut File) -> io::Result<()>,
) -> Result<(), PublishError> {
    #[cfg(test)]
    TEST_LAST_PUBLISH_RENAME_ERROR.with(|error| error.set(None));

    if cancellation.is_cancelled() {
        return Err(PublishError::Cancelled);
    }
    let parent_info = checked_information(parent, root_volume_identity)
        .map_err(|error| io_or_cancel(error, cancellation))?;
    if parent_info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(io_or_cancel(
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "pinned destination directory failed identity verification",
            ),
            cancellation,
        ));
    }

    // Handle-relative rename uses RootDirectory only for a different target directory.
    // A private same-volume child keeps the source separate while binding the target to parent.
    let mut staging = create_staging(parent, root_volume_identity)
        .map_err(|error| io_or_cancel(error, cancellation))?;
    if cancellation.is_cancelled() {
        return Err(PublishError::Cancelled);
    }
    let staging_info = checked_information(staging.file(), root_volume_identity)
        .map_err(|error| io_or_cancel(error, cancellation))?;
    if staging_info.attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        return Err(io_or_cancel(
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "staging file failed local-volume verification",
            ),
            cancellation,
        ));
    }
    if cancellation.is_cancelled() {
        return Err(PublishError::Cancelled);
    }
    write_artifact(staging.file_mut()).map_err(|error| io_or_cancel(error, cancellation))?;
    staging
        .file_mut()
        .flush()
        .map_err(|error| io_or_cancel(error, cancellation))?;
    staging
        .file()
        .sync_all()
        .map_err(|error| io_or_cancel(error, cancellation))?;
    #[cfg(test)]
    test_hook(TestHookPoint::Staged);
    if cancellation.is_cancelled() {
        return Err(PublishError::Cancelled);
    }
    if let Err(error) = rename_relative(staging.file(), parent, destination, overwrite) {
        #[cfg(test)]
        TEST_LAST_PUBLISH_RENAME_ERROR
            .with(|last_error| last_error.set(windows_error_code(&error)));
        if !overwrite {
            // Providers can report an existing target with a non-collision status. Confirm the
            // target relative to the pinned parent, without following reparse points.
            if matches!(
                windows_error_code(&error),
                Some(ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS)
            ) || matches!(destination_exists_relative(parent, destination), Ok(true))
            {
                return Err(PublishError::DestinationExists);
            }
        }
        return Err(io_or_cancel(error, cancellation));
    }
    staging.published = true;
    Ok(())
}

struct StagingDirectory {
    file: File,
}

impl StagingDirectory {
    fn file(&self) -> &File {
        &self.file
    }
}

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        mark_for_deletion(&self.file);
    }
}

struct StagingFile {
    file: Option<File>,
    _directory: StagingDirectory,
    published: bool,
}

impl StagingFile {
    fn file(&self) -> &File {
        self.file.as_ref().expect("staging file remains open")
    }

    fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("staging file remains open")
    }
}

impl Drop for StagingFile {
    fn drop(&mut self) {
        if !self.published {
            mark_for_deletion(self.file());
        }
        drop(self.file.take());
    }
}

fn mark_for_deletion(file: &File) {
    let delete = 1u8;
    unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FILE_DISPOSITION_INFO_CLASS,
            (&delete as *const u8).cast(),
            size_of::<u8>() as u32,
        );
    }
}

fn random_temp_name(name: &mut [u16], prefix: &str, suffix: &str) -> io::Result<usize> {
    let prefix_length = prefix.encode_utf16().count();
    let suffix_length = suffix.encode_utf16().count();
    let length = prefix_length
        .checked_add(32)
        .and_then(|length| length.checked_add(suffix_length))
        .filter(|length| *length <= name.len())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "temporary name is too long"))?;
    let mut cursor = 0;
    for unit in prefix.encode_utf16() {
        name[cursor] = unit;
        cursor += 1;
    }

    let mut random = [0_u8; 16];
    let status =
        unsafe { BCryptGenRandom(ptr::null_mut(), random.as_mut_ptr(), random.len() as u32, 2) };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    const HEX: &[u16; 16] = &[
        b'0' as u16,
        b'1' as u16,
        b'2' as u16,
        b'3' as u16,
        b'4' as u16,
        b'5' as u16,
        b'6' as u16,
        b'7' as u16,
        b'8' as u16,
        b'9' as u16,
        b'a' as u16,
        b'b' as u16,
        b'c' as u16,
        b'd' as u16,
        b'e' as u16,
        b'f' as u16,
    ];
    for (index, byte) in random.iter().copied().enumerate() {
        name[cursor + index * 2] = HEX[(byte >> 4) as usize];
        name[cursor + index * 2 + 1] = HEX[(byte & 0x0f) as usize];
    }
    cursor += 32;
    for unit in suffix.encode_utf16() {
        name[cursor] = unit;
        cursor += 1;
    }
    Ok(length)
}

fn create_staging_directory(
    parent: &File,
    root_volume_identity: VolumeIdentity,
) -> io::Result<StagingDirectory> {
    const PREFIX: &str = ".asciiwall-stage-";
    const ATTEMPTS: usize = 32;
    let mut name = [0_u16; 64];
    for _ in 0..ATTEMPTS {
        let length = random_temp_name(&mut name, PREFIX, "")?;
        match open_relative_with_share_and_attributes(
            parent,
            &name[..length],
            FILE_LIST_DIRECTORY
                | FILE_TRAVERSE
                | FILE_READ_ATTRIBUTES
                | FILE_WRITE_DATA
                | DELETE
                | SYNCHRONIZE,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
            FILE_CREATE,
            SHARE_ALL,
            FILE_ATTRIBUTE_DIRECTORY,
        ) {
            Ok(file) => {
                let directory = StagingDirectory { file };
                let info = checked_information(directory.file(), root_volume_identity)?;
                if info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "staging directory failed local-volume verification",
                    ));
                }
                return Ok(directory);
            }
            Err(error)
                if matches!(
                    windows_error_code(&error),
                    Some(ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS)
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot allocate a unique output staging directory",
    ))
}

fn create_staging(parent: &File, root_volume_identity: VolumeIdentity) -> io::Result<StagingFile> {
    const PREFIX: &str = ".asciiwall-";
    const SUFFIX: &str = ".tmp";
    const ATTEMPTS: usize = 32;
    let directory = create_staging_directory(parent, root_volume_identity)?;
    let mut name = [0_u16; 47];
    for _ in 0..ATTEMPTS {
        let length = random_temp_name(&mut name, PREFIX, SUFFIX)?;
        match open_relative_with_share(
            directory.file(),
            &name[..length],
            GENERIC_READ | GENERIC_WRITE | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE,
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            FILE_CREATE,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
        ) {
            Ok(file) => {
                return Ok(StagingFile {
                    file: Some(file),
                    _directory: directory,
                    published: false,
                });
            }
            Err(error)
                if matches!(
                    windows_error_code(&error),
                    Some(ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS)
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot allocate a unique output staging name",
    ))
}

fn rename_relative(
    source: &File,
    parent: &File,
    destination: &[u16],
    overwrite: bool,
) -> io::Result<()> {
    let name = destination;
    if name.is_empty() || name.iter().any(|unit| matches!(*unit, 0 | 47 | 92)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination must be one path component",
        ));
    }
    let byte_length = name
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "destination name is too long")
        })?;
    let name_offset = offset_of!(RenameInfo, file_name);
    let name_end = name_offset
        .checked_add(byte_length as usize)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "destination name is too long")
        })?;
    let documented_buffer_length = size_of::<RenameInfo>()
        .checked_add(byte_length as usize)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "destination name is too long")
        })?;
    let total_length = name_end.max(documented_buffer_length);
    let word_size = size_of::<usize>();
    let word_count = total_length
        .checked_add(word_size - 1)
        .map(|length| length / word_size)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "destination name is too long")
        })?;
    let mut storage = Vec::<usize>::new();
    storage
        .try_reserve_exact(word_count)
        .map_err(|_| io::Error::other("cannot allocate rename information"))?;
    storage.resize(word_count, 0);
    let bytes = storage.as_mut_ptr().cast::<u8>();
    unsafe {
        ptr::write_unaligned(bytes.cast::<u32>(), if overwrite { 1 } else { 0 });
        ptr::write_unaligned(
            bytes
                .add(offset_of!(RenameInfo, root_directory))
                .cast::<*mut c_void>(),
            parent.as_raw_handle(),
        );
        ptr::write_unaligned(
            bytes
                .add(offset_of!(RenameInfo, file_name_length))
                .cast::<u32>(),
            byte_length,
        );
        ptr::copy_nonoverlapping(
            name.as_ptr().cast::<u8>(),
            bytes.add(name_offset),
            byte_length as usize,
        );
    }
    let length = u32::try_from(total_length).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "rename information is too large",
        )
    })?;
    let mut io_status = IoStatusBlock::default();
    let status = unsafe {
        NtSetInformationFile(
            source.as_raw_handle(),
            &mut io_status,
            bytes.cast(),
            length,
            FILE_RENAME_INFORMATION_CLASS,
        )
    };
    if status < 0 {
        Err(nt_status_error(status))
    } else {
        Ok(())
    }
}

fn io_or_cancel(error: io::Error, cancellation: &CancellationToken) -> PublishError {
    if cancellation.is_cancelled() {
        PublishError::Cancelled
    } else {
        PublishError::Io(error)
    }
}

#[derive(Debug)]
struct NtStatusFailure {
    nt_status: i32,
    win32_error: u32,
}

impl fmt::Display for NtStatusFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Windows native file operation failed with NTSTATUS 0x{:08X} (Win32 error {})",
            self.nt_status as u32, self.win32_error
        )
    }
}

impl std::error::Error for NtStatusFailure {}

fn nt_status_error(status: i32) -> io::Error {
    let win32_error = unsafe { RtlNtStatusToDosError(status) };
    let kind = io::Error::from_raw_os_error(win32_error as i32).kind();
    io::Error::new(
        kind,
        NtStatusFailure {
            nt_status: status,
            win32_error,
        },
    )
}

const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: *mut c_void,
    ) -> *mut c_void;
    fn QueryDosDeviceW(device_name: *const u16, target_path: *mut u16, max: u32) -> u32;
    fn GetFullPathNameW(
        file_name: *const u16,
        buffer_length: u32,
        buffer: *mut u16,
        file_part: *mut *mut u16,
    ) -> u32;
    fn GetFileType(file: *mut c_void) -> u32;
    fn GetFileInformationByHandle(
        file: *mut c_void,
        information: *mut ByHandleFileInformation,
    ) -> i32;
    fn GetFileInformationByHandleEx(
        file: *mut c_void,
        information_class: i32,
        information: *mut c_void,
        buffer_size: u32,
    ) -> i32;
    fn GetVolumeInformationByHandleW(
        file: *mut c_void,
        volume_name: *mut u16,
        volume_name_size: u32,
        volume_serial: *mut u32,
        maximum_component_length: *mut u32,
        file_system_flags: *mut u32,
        file_system_name: *mut u16,
        file_system_name_size: u32,
    ) -> i32;
    fn SetFileInformationByHandle(
        file: *mut c_void,
        information_class: i32,
        information: *const c_void,
        buffer_size: u32,
    ) -> i32;
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        file_handle: *mut *mut c_void,
        desired_access: u32,
        object_attributes: *mut ObjectAttributes,
        io_status_block: *mut IoStatusBlock,
        allocation_size: *mut i64,
        file_attributes: u32,
        share_access: u32,
        create_disposition: u32,
        create_options: u32,
        ea_buffer: *mut c_void,
        ea_length: u32,
    ) -> i32;
    fn NtSetInformationFile(
        file_handle: *mut c_void,
        io_status_block: *mut IoStatusBlock,
        file_information: *mut c_void,
        length: u32,
        file_information_class: i32,
    ) -> i32;
    fn NtQueryVolumeInformationFile(
        file_handle: *mut c_void,
        io_status_block: *mut IoStatusBlock,
        fs_information: *mut c_void,
        length: u32,
        fs_information_class: i32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(
        algorithm: *mut c_void,
        buffer: *mut u8,
        buffer_length: u32,
        flags: u32,
    ) -> i32;
}

#[cfg(test)]
fn verify_offline_test_path(path: &Path, cwd: &Path) {
    let spec = resolve_path_spec(path_spec(path, cwd).expect("test fixture path parses"))
        .expect("test fixture path resolves to a supported local volume");
    let root =
        open_root(&spec, false).expect("Windows offline tests require a direct local NTFS root");
    let file_system = volume_file_system_name(&root.file).expect("volume filesystem is available");
    let file_system_end = file_system
        .iter()
        .position(|unit| *unit == 0)
        .expect("volume filesystem name is terminated");
    assert!(
        ascii_units_equal_ignore_case(&file_system[..file_system_end], "NTFS"),
        "Windows offline test fixture must be on a local NTFS volume"
    );
    assert!(
        matches!(
            open_directories(&root, &spec.components)
                .expect("fixture ancestors open without following reparses"),
            ChainResult::Ready(_)
        ),
        "Windows offline test fixture and each ancestor must be ordinary directories"
    );
}

#[cfg(test)]
pub(crate) fn offline_test_tempdir() -> tempfile::TempDir {
    let cwd = std::env::current_dir().expect("Windows offline tests require a current directory");
    // Verify the worktree path before creating anything through it. This avoids %TEMP% and clearly
    // rejects test checkouts on remote or reparse-backed paths.
    verify_offline_test_path(&cwd, &cwd);
    let temp = tempfile::Builder::new()
        .prefix(".asciiwall-offline-test-")
        .tempdir_in(&cwd)
        .expect("Windows offline tests require a writable worktree-local fixture root");
    verify_offline_test_path(temp.path(), &cwd);
    temp
}
#[cfg(test)]
pub(crate) fn offline_test_tempdir_for_directory_rename() -> tempfile::TempDir {
    let cwd = std::env::current_dir().expect("Windows offline tests require a current directory");
    let base = std::env::temp_dir();
    // A checkout ACL can allow file replacement but deny directory renames. Keep this fixture
    // independent of the checkout ACL and prove the temp base is still direct local NTFS.
    verify_offline_test_path(&base, &cwd);
    let temp = tempfile::Builder::new()
        .prefix(".asciiwall-offline-swap-")
        .tempdir_in(&base)
        .expect("Windows offline swap tests require a writable local NTFS temp root");
    verify_offline_test_path(temp.path(), &cwd);
    temp
}
#[cfg(test)]
mod tests {
    use super::{
        ERROR_INVALID_PARAMETER, FILE_CHARACTERISTIC_TS_DEVICE, FILE_CHARACTERISTIC_WEBDAV_DEVICE,
        FILE_DEVICE_DISK, FILE_DEVICE_NETWORK_FILE_SYSTEM, FILE_REMOTE_DEVICE,
        FileFsDeviceInformation, NtStatusFailure, OfflinePathError, VolumeIdentity,
        ascii_units_equal_ignore_case, is_local_volume_device, offline_test_tempdir, open_root,
        path_spec, resolve_path_spec, verify_local_volume_provider_handle, verify_same_volume,
        volume_file_system_name, volume_identity, volume_identity_matches, windows_error_code,
    };

    #[test]
    fn offline_accepts_a_direct_local_ntfs_root_and_rejects_volume_mismatch() {
        let _fixture = offline_test_tempdir();
        let cwd = std::env::current_dir().unwrap();
        let root_spec = resolve_path_spec(path_spec(&cwd, &cwd).expect("current directory parses"))
            .expect("current directory resolves to a supported root volume");
        let root = open_root(&root_spec, false).expect("ordinary local NTFS root opens");
        assert!(
            verify_local_volume_provider_handle(&root.file)
                .expect("opened NTFS root has provider information"),
            "opened NTFS root must be a local disk provider"
        );
        let file_system =
            volume_file_system_name(&root.file).expect("volume filesystem is available");
        let file_system_end = file_system
            .iter()
            .position(|unit| *unit == 0)
            .expect("volume filesystem name is terminated");
        assert!(
            ascii_units_equal_ignore_case(&file_system[..file_system_end], "NTFS"),
            "handle-bound provider regression requires a local NTFS handle"
        );
        let actual = volume_identity(&root.file).expect("root volume identity is available");
        verify_same_volume(&root.file, actual).expect("matching volume identity is accepted");

        let other = VolumeIdentity(if actual.0 == u64::MAX {
            actual.0 - 1
        } else {
            actual.0 + 1
        });
        assert!(!volume_identity_matches(actual, other));
        assert!(verify_same_volume(&root.file, other).is_err());
    }
    #[test]
    fn provider_classification_rejects_remote_and_unknown_devices() {
        let local = FileFsDeviceInformation {
            device_type: FILE_DEVICE_DISK,
            characteristics: 0,
        };
        assert!(is_local_volume_device(&local));

        for remote_characteristics in [
            FILE_REMOTE_DEVICE,
            FILE_CHARACTERISTIC_TS_DEVICE,
            FILE_CHARACTERISTIC_WEBDAV_DEVICE,
        ] {
            assert!(!is_local_volume_device(&FileFsDeviceInformation {
                device_type: FILE_DEVICE_DISK,
                characteristics: remote_characteristics,
            }));
        }
        assert!(!is_local_volume_device(&FileFsDeviceInformation {
            device_type: FILE_DEVICE_NETWORK_FILE_SYSTEM,
            characteristics: 0,
        }));
    }

    #[test]
    fn ntcreatefile_failure_keeps_exact_status_and_win32_mapping() {
        let error = std::io::Error::other(NtStatusFailure {
            nt_status: 0xC000_000D_u32 as i32,
            win32_error: ERROR_INVALID_PARAMETER as u32,
        });
        let stage = OfflinePathError::with_source("test open", error);
        assert!(stage.to_string().contains("NTSTATUS 0xC000000D"));
        assert_eq!(
            windows_error_code(stage.source.as_ref().unwrap()),
            Some(ERROR_INVALID_PARAMETER)
        );
    }

    #[test]
    fn offline_rejects_volume_identity_mismatch_after_drive_remap() {
        let identity_from_resolved_drive = VolumeIdentity(0x1122_3344_5566_7788);
        let identity_from_open_root = VolumeIdentity(0x8877_6655_4433_2211);
        assert!(!volume_identity_matches(
            identity_from_open_root,
            identity_from_resolved_drive,
        ));
    }
}
