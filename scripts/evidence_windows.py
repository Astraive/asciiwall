"""Windows NTFS handle primitives for evidence reads and archive retention.

Each path component is opened relative to an already opened directory, with
reparse processing disabled. Directory handles remain open without delete
sharing until the operation ends. Files are opened without write or delete
sharing; an expired archive entry is deleted through the very handle used to
revalidate it, never by looking its name up a second time.

This backend deliberately accepts only drive-rooted local NTFS volumes. Network
shares, substituted drives and filesystems whose handle/identity semantics have
not been established are rejected rather than weakening these guarantees.
"""
from __future__ import annotations

import ctypes
from ctypes import wintypes
from dataclasses import dataclass
import errno
import ntpath
import os
import re
import struct

if os.name != "nt":
    raise ImportError("Windows evidence handles require Windows")


# NtCreateFile's RootDirectory and OBJ_DONT_REPARSE are documented in:
# https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile
# https://learn.microsoft.com/en-us/windows/win32/api/ntdef/ns-ntdef-_object_attributes
_FILE_LIST_DIRECTORY = 0x0001
_FILE_READ_DATA = 0x0001
_FILE_READ_ATTRIBUTES = 0x0080
_DELETE = 0x00010000
_SYNCHRONIZE = 0x00100000
_FILE_SHARE_READ = 0x00000001
_FILE_SHARE_WRITE = 0x00000002
_FILE_OPEN = 1
_FILE_DIRECTORY_FILE = 0x00000001
_FILE_SYNCHRONOUS_IO_NONALERT = 0x00000020
_FILE_NON_DIRECTORY_FILE = 0x00000040
_OBJ_CASE_INSENSITIVE = 0x00000040
_OBJ_DONT_REPARSE = 0x00001000
_FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000
_FILE_FLAG_BACKUP_SEMANTICS = 0x02000000
_OPEN_EXISTING = 3
_FILE_ATTRIBUTE_DIRECTORY = 0x00000010
_FILE_ATTRIBUTE_REPARSE_POINT = 0x00000400
_FILE_TYPE_DISK = 1
_DRIVE_FIXED = 3
_DRIVE_REMOVABLE = 2
_FILE_BASIC_INFO = 0
_FILE_STANDARD_INFO = 1
_FILE_DISPOSITION_INFO = 4
_FILE_ATTRIBUTE_TAG_INFO = 9
_FILE_ID_BOTH_DIRECTORY_INFO = 10
_FILE_ID_BOTH_DIRECTORY_RESTART_INFO = 11
_FILE_ID_INFO = 18
_ERROR_NO_MORE_FILES = 18
_INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value
_MAX_READ_CHUNK = 1_048_577
_DIRECTORY_BUFFER_SIZE = 65_536
_FILETIME_UNIX_EPOCH = 116_444_736_000_000_000
_VOLUME_ROOT = re.compile(r"\\\\\?\\Volume\{[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\}\\\Z", re.IGNORECASE)


class _UnicodeString(ctypes.Structure):
    _fields_ = [("Length", wintypes.USHORT), ("MaximumLength", wintypes.USHORT), ("Buffer", ctypes.c_void_p)]


class _ObjectAttributes(ctypes.Structure):
    _fields_ = [
        ("Length", wintypes.ULONG), ("RootDirectory", wintypes.HANDLE),
        ("ObjectName", ctypes.POINTER(_UnicodeString)), ("Attributes", wintypes.ULONG),
        ("SecurityDescriptor", ctypes.c_void_p), ("SecurityQualityOfService", ctypes.c_void_p),
    ]


class _IoStatusBlock(ctypes.Structure):
    # The first field is the pointer-sized union of NTSTATUS and PVOID.
    _fields_ = [("StatusOrPointer", ctypes.c_void_p), ("Information", ctypes.c_size_t)]


class _AttributeTagInfo(ctypes.Structure):
    _fields_ = [("FileAttributes", wintypes.DWORD), ("ReparseTag", wintypes.DWORD)]


class _BasicInfo(ctypes.Structure):
    _fields_ = [
        ("CreationTime", ctypes.c_int64), ("LastAccessTime", ctypes.c_int64),
        ("LastWriteTime", ctypes.c_int64), ("ChangeTime", ctypes.c_int64),
        ("FileAttributes", wintypes.DWORD),
    ]


class _StandardInfo(ctypes.Structure):
    _fields_ = [
        ("AllocationSize", ctypes.c_int64), ("EndOfFile", ctypes.c_int64),
        ("NumberOfLinks", wintypes.DWORD), ("DeletePending", ctypes.c_ubyte),
        ("Directory", ctypes.c_ubyte),
    ]


class _IdInfo(ctypes.Structure):
    _fields_ = [("VolumeSerialNumber", ctypes.c_uint64), ("FileId", ctypes.c_ubyte * 16)]


_kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
_ntdll = ctypes.WinDLL("ntdll", use_last_error=True)
_CreateFileW = _kernel32.CreateFileW
_CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p,
                         wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
_CreateFileW.restype = wintypes.HANDLE
_CloseHandle = _kernel32.CloseHandle
_CloseHandle.argtypes = [wintypes.HANDLE]
_CloseHandle.restype = wintypes.BOOL
_GetFileType = _kernel32.GetFileType
_GetFileType.argtypes = [wintypes.HANDLE]
_GetFileType.restype = wintypes.DWORD
_GetDriveTypeW = _kernel32.GetDriveTypeW
_GetDriveTypeW.argtypes = [wintypes.LPCWSTR]
_GetDriveTypeW.restype = wintypes.UINT
_GetFinalPathNameByHandleW = _kernel32.GetFinalPathNameByHandleW
_GetFinalPathNameByHandleW.argtypes = [wintypes.HANDLE, wintypes.LPWSTR, wintypes.DWORD, wintypes.DWORD]
_GetFinalPathNameByHandleW.restype = wintypes.DWORD
_GetVolumeInformationByHandleW = _kernel32.GetVolumeInformationByHandleW
_GetVolumeInformationByHandleW.argtypes = [wintypes.HANDLE, wintypes.LPWSTR, wintypes.DWORD,
                                           ctypes.POINTER(wintypes.DWORD), ctypes.POINTER(wintypes.DWORD),
                                           ctypes.POINTER(wintypes.DWORD), wintypes.LPWSTR, wintypes.DWORD]
_GetVolumeInformationByHandleW.restype = wintypes.BOOL
_GetFileInformationByHandleEx = _kernel32.GetFileInformationByHandleEx
_GetFileInformationByHandleEx.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
_GetFileInformationByHandleEx.restype = wintypes.BOOL
_SetFileInformationByHandle = _kernel32.SetFileInformationByHandle
_SetFileInformationByHandle.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
_SetFileInformationByHandle.restype = wintypes.BOOL
_ReadFile = _kernel32.ReadFile
_ReadFile.argtypes = [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
                      ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p]
_ReadFile.restype = wintypes.BOOL
_NtCreateFile = _ntdll.NtCreateFile
_NtCreateFile.argtypes = [ctypes.POINTER(wintypes.HANDLE), wintypes.DWORD, ctypes.POINTER(_ObjectAttributes),
                          ctypes.POINTER(_IoStatusBlock), ctypes.c_void_p, wintypes.ULONG, wintypes.ULONG,
                          wintypes.ULONG, wintypes.ULONG, ctypes.c_void_p, wintypes.ULONG]
_NtCreateFile.restype = ctypes.c_int32
_RtlNtStatusToDosError = _ntdll.RtlNtStatusToDosError
_RtlNtStatusToDosError.argtypes = [ctypes.c_int32]
_RtlNtStatusToDosError.restype = wintypes.ULONG


def _error(code: int | None = None) -> OSError:
    # Deliberately do not include file names, paths or system-provided messages.
    return ctypes.WinError(ctypes.get_last_error() if code is None else code,
                           "filesystem object is inaccessible or unsafe")


def _unsafe(message: str = "filesystem object is inaccessible or unsafe") -> OSError:
    return OSError(errno.EINVAL, message)


def _close(handle: int | wintypes.HANDLE) -> None:
    if not _CloseHandle(handle):
        raise _error()


def _note_cleanup_error(primary: BaseException, cleanup: BaseException) -> None:
    add_note = getattr(primary, "add_note", None)
    if callable(add_note):
        try:
            add_note(f"Handle cleanup also failed ({type(cleanup).__name__}).")
        except BaseException:
            pass


def _close_preserving(handle: int | wintypes.HANDLE, primary: BaseException) -> None:
    try:
        _close(handle)
    except BaseException as cleanup:
        _note_cleanup_error(primary, cleanup)


def _handle_slots(count: int) -> list[int | None]:
    """Allocate handle ownership storage before opening native handles."""
    return [None] * count


def _close_many(handles: list[int | None]) -> BaseException | None:
    first_error = None
    while handles:
        handle = handles.pop()
        if handle is None:
            continue
        try:
            _close(handle)
        except BaseException as error:
            if first_error is None:
                first_error = error
    return first_error


def _info(handle: int, kind: int, value: ctypes.Structure) -> None:
    if not _GetFileInformationByHandleEx(handle, kind, ctypes.byref(value), ctypes.sizeof(value)):
        raise _error()


def _check_type(handle: int, *, directory: bool) -> None:
    if _GetFileType(handle) != _FILE_TYPE_DISK:
        raise _unsafe()
    tag = _AttributeTagInfo()
    _info(handle, _FILE_ATTRIBUTE_TAG_INFO, tag)
    if tag.FileAttributes & _FILE_ATTRIBUTE_REPARSE_POINT:
        raise _unsafe()
    if bool(tag.FileAttributes & _FILE_ATTRIBUTE_DIRECTORY) != directory:
        raise _unsafe()


def _component(name: str) -> bytes:
    if (not isinstance(name, str) or not name or name in (".", "..")
            or any(char in name for char in "\\/:\x00")):
        raise _unsafe()
    encoded = name.encode("utf-16-le", "surrogatepass")
    if len(encoded) > 65_532:
        raise _unsafe()
    return encoded


def _open_relative(parent: int, name: str, *, directory: bool, for_delete: bool = False) -> int:
    encoded = _component(name)
    buffer = ctypes.create_string_buffer(encoded, len(encoded) + 2)
    string = _UnicodeString(len(encoded), len(encoded) + 2, ctypes.addressof(buffer))
    attributes = _ObjectAttributes(ctypes.sizeof(_ObjectAttributes), parent, ctypes.pointer(string),
                                   _OBJ_CASE_INSENSITIVE | _OBJ_DONT_REPARSE, None, None)
    handle = wintypes.HANDLE()
    status_block = _IoStatusBlock()
    access = _FILE_READ_ATTRIBUTES | _SYNCHRONIZE
    access |= _FILE_LIST_DIRECTORY if directory else _FILE_READ_DATA
    if for_delete:
        access |= _DELETE
    # Keep directory mutation sharing compatible with child operations;
    # names() is explicitly an observed multi-call enumeration, not a snapshot.
    share = _FILE_SHARE_READ | (_FILE_SHARE_WRITE if directory else 0)
    options = _FILE_SYNCHRONOUS_IO_NONALERT | (_FILE_DIRECTORY_FILE if directory else _FILE_NON_DIRECTORY_FILE)
    status = _NtCreateFile(ctypes.byref(handle), access, ctypes.byref(attributes), ctypes.byref(status_block),
                           None, 0, share, _FILE_OPEN, options, None, 0)
    if status < 0:
        raise _error(_RtlNtStatusToDosError(status))
    try:
        native_handle = handle.value
    except BaseException as primary:
        # Keep the ctypes HANDLE available if allocating its Python int fails.
        _close_preserving(handle, primary)
        raise
    if not native_handle or native_handle == _INVALID_HANDLE_VALUE:
        raise _unsafe()
    try:
        _check_type(native_handle, directory=directory)
    except BaseException as primary:
        _close_preserving(native_handle, primary)
        raise
    return native_handle


def _absolute_parts(path: os.PathLike[str] | str) -> tuple[str, list[str]]:
    raw = os.fspath(path)
    if not isinstance(raw, str) or not raw or "\x00" in raw:
        raise _unsafe()
    raw = raw.replace("/", "\\")
    drive, suffix = ntpath.splitdrive(raw)
    if raw.startswith("\\\\") or (drive and not re.fullmatch(r"[A-Za-z]:", drive)):
        raise _unsafe()
    if any(part == ".." for part in suffix.split("\\")):
        raise _unsafe()
    if drive:
        if not suffix.startswith("\\"):
            raise _unsafe()  # Drive-relative paths depend on hidden per-drive state.
        absolute = ntpath.normpath(raw)
    else:
        if raw.startswith("\\"):
            raise _unsafe()
        absolute = ntpath.normpath(ntpath.join(os.getcwd(), raw))
    drive, suffix = ntpath.splitdrive(absolute)
    if not re.fullmatch(r"[A-Za-z]:", drive) or not suffix.startswith("\\"):
        raise _unsafe()
    parts = [part for part in suffix.split("\\") if part]
    for part in parts:
        _component(part)
    return drive + "\\", parts


def _open_root(root: str) -> int:
    handle = _CreateFileW(root, _FILE_LIST_DIRECTORY | _FILE_READ_ATTRIBUTES | _SYNCHRONIZE,
                          _FILE_SHARE_READ | _FILE_SHARE_WRITE, None, _OPEN_EXISTING,
                          _FILE_FLAG_BACKUP_SEMANTICS | _FILE_FLAG_OPEN_REPARSE_POINT, None)
    if handle == _INVALID_HANDLE_VALUE or not handle:
        raise _error()
    try:
        _check_type(handle, directory=True)
        # Classify the volume resolved by this opened handle, not the drive
        # letter mapping that could change between a type check and open.
        path = ctypes.create_unicode_buffer(128)
        length = _GetFinalPathNameByHandleW(handle, path, len(path), 1)  # VOLUME_NAME_GUID
        if not length:
            raise _error()
        if length >= len(path) or not _VOLUME_ROOT.fullmatch(path.value):
            raise _unsafe()
        if _GetDriveTypeW(path.value) != _DRIVE_FIXED:
            raise _unsafe()
        filesystem = ctypes.create_unicode_buffer(261)
        if not _GetVolumeInformationByHandleW(handle, None, 0, None, None, None,
                                               filesystem, len(filesystem)):
            raise _error()
        if filesystem.value.upper() != "NTFS":
            raise _unsafe()
        return handle
    except BaseException as primary:
        _close_preserving(handle, primary)
        raise


@dataclass(frozen=True)
class Snapshot:
    """Identity and change indicators, not a digest of file contents.

    Callers must validate bytes read through this same handle; metadata equality
    alone is not proof that contents remained unchanged.
    """
    volume: int
    file_id: bytes
    attributes: int
    creation_time: int
    change_time: int
    link_count: int
    st_size: int
    st_mtime_ns: int


def _snapshot(handle: int) -> Snapshot:
    _check_type(handle, directory=False)
    basic = _BasicInfo()
    standard = _StandardInfo()
    identity = _IdInfo()
    _info(handle, _FILE_BASIC_INFO, basic)
    _info(handle, _FILE_STANDARD_INFO, standard)
    _info(handle, _FILE_ID_INFO, identity)
    file_id = bytes(identity.FileId)
    if (basic.FileAttributes & (_FILE_ATTRIBUTE_REPARSE_POINT | _FILE_ATTRIBUTE_DIRECTORY)
            or standard.Directory or standard.DeletePending or standard.EndOfFile < 0 or not any(file_id)):
        raise _unsafe()
    return Snapshot(identity.VolumeSerialNumber, file_id, basic.FileAttributes, basic.CreationTime,
                    basic.ChangeTime, standard.NumberOfLinks, standard.EndOfFile,
                    (basic.LastWriteTime - _FILETIME_UNIX_EPOCH) * 100)


class FileCloseError(OSError):
    """A native evidence file handle failed to close."""


class File:
    """An opened, non-reparse regular file with write/delete sharing denied."""

    def __init__(self, handle: int, for_delete: bool):
        self._handle = handle
        self._for_delete = for_delete

    def __enter__(self) -> File:
        self._get_handle()
        return self

    def __exit__(self, exc_type, exc, traceback) -> None:
        if exc is None:
            self.close()
            return
        try:
            self.close()
        except BaseException as cleanup:
            _note_cleanup_error(exc, cleanup)

    def _get_handle(self) -> int:
        if self._handle is None:
            raise _unsafe()
        return self._handle

    def close(self) -> None:
        if self._handle is not None:
            handle, self._handle = self._handle, None
            try:
                _close(handle)
            except OSError as exc:
                raise FileCloseError("native evidence file handle could not be closed") from exc

    def snapshot(self) -> Snapshot:
        return _snapshot(self._get_handle())

    def read(self, size: int) -> bytes:
        if not isinstance(size, int) or not 0 <= size <= _MAX_READ_CHUNK:
            raise _unsafe("invalid bounded read size")
        if not size:
            return b""
        buffer = ctypes.create_string_buffer(size)
        count = wintypes.DWORD()
        if not _ReadFile(self._get_handle(), buffer, size, ctypes.byref(count), None):
            raise _error()
        if count.value > size:
            raise _unsafe()
        return buffer.raw[:count.value]

    def delete(self, expected: Snapshot) -> bool:
        """Delete this open entry only if its current snapshot still matches.

        The caller must validate its contents through this same handle before
        invoking this method. False means the entry changed and was not deleted.
        """
        if not self._for_delete:
            raise _unsafe()
        handle = self._get_handle()
        if self.snapshot() != expected:
            return False
        disposition = ctypes.c_ubyte(1)  # FILE_DISPOSITION_INFO.DeleteFile
        if not _SetFileInformationByHandle(handle, _FILE_DISPOSITION_INFO,
                                            ctypes.byref(disposition), ctypes.sizeof(disposition)):
            raise _error()
        return True


class Directory:
    """A directory and every ancestor pinned for the lifetime of this object."""

    def __init__(self, handles: list[int | None]):
        self._handles = handles

    def __enter__(self) -> Directory:
        return self

    def __exit__(self, exc_type, exc, traceback) -> None:
        if exc is None:
            self.close()
            return
        try:
            self.close()
        except BaseException as cleanup:
            _note_cleanup_error(exc, cleanup)

    def _get_handle(self) -> int:
        if not self._handles:
            raise _unsafe()
        handle = self._handles[-1]
        if handle is None:
            raise _unsafe()
        return handle

    def close(self) -> None:
        error = _close_many(self._handles)
        if error is not None:
            raise error

    def open_file(self, name: str, *, for_delete: bool = False) -> File:
        handle = _open_relative(self._get_handle(), name, directory=False, for_delete=for_delete)
        try:
            return File(handle, for_delete)
        except BaseException as primary:
            _close_preserving(handle, primary)
            raise

    def names(self, limit: int) -> list[str]:
        """Enumerate bounded observed names through the pinned directory handle.

        This multi-call enumeration is not an atomic snapshot. Concurrent
        creation, removal or rename may alter, omit or duplicate observed names;
        duplicates fail closed. Returned names are candidates to open and
        validate through this same directory handle. Retention ordering needs
        separate stable-membership and stable-ranking guarantees; enumeration
        alone cannot establish either.
        """
        if not isinstance(limit, int) or limit < 0:
            raise _unsafe()
        handle = self._get_handle()
        buffer = ctypes.create_string_buffer(_DIRECTORY_BUFFER_SIZE)
        names: list[str] = []
        seen: set[str] = set()
        entries_seen = 0
        info_class = _FILE_ID_BOTH_DIRECTORY_RESTART_INFO
        while True:
            if not _GetFileInformationByHandleEx(handle, info_class, buffer, len(buffer)):
                error = ctypes.get_last_error()
                if error == _ERROR_NO_MORE_FILES:
                    return names
                raise _error(error)
            info_class = _FILE_ID_BOTH_DIRECTORY_INFO
            offset = 0
            while True:
                # FILE_ID_BOTH_DIR_INFO's FileName starts at byte 104 on Win32
                # and Win64; each subsequent record is eight-byte aligned.
                if offset + 104 > len(buffer):
                    raise _unsafe()
                next_offset = struct.unpack_from("<I", buffer, offset)[0]
                name_size = struct.unpack_from("<I", buffer, offset + 60)[0]
                end = offset + next_offset if next_offset else len(buffer)
                if (end > len(buffer) or end < offset + 104 or name_size == 0 or name_size % 2
                        or offset + 104 + name_size > end or (next_offset and next_offset % 8)):
                    raise _unsafe()
                raw = buffer[offset + 104:offset + 104 + name_size]
                name = raw.decode("utf-16-le", "surrogatepass")
                entries_seen += 1
                if entries_seen > limit + 2:
                    raise _unsafe("archive contains too many entries to prune safely")
                if name not in (".", ".."):
                    if name in seen:
                        raise _unsafe("directory changed during enumeration")
                    seen.add(name)
                    if len(names) >= limit:
                        raise _unsafe("archive contains too many entries to prune safely")
                    names.append(name)
                if not next_offset:
                    break
                offset = end


def open_directory(path: os.PathLike[str] | str) -> Directory:
    """Open an absolute or current-directory-relative path without traversal.

    The returned context manager owns every ancestor handle and must be closed.
    A path containing a junction, symlink or other reparse point is rejected.
    """
    root, parts = _absolute_parts(path)
    handles = _handle_slots(len(parts) + 1)
    try:
        handles[0] = _open_root(root)
        for index, part in enumerate(parts, start=1):
            parent = handles[index - 1]
            if parent is None:
                raise _unsafe()
            handles[index] = _open_relative(parent, part, directory=True)
        return Directory(handles)
    except BaseException as primary:
        cleanup = _close_many(handles)
        if cleanup is not None:
            _note_cleanup_error(primary, cleanup)
        raise
