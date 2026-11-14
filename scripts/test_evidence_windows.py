"""Focused tests for the Windows evidence handle boundary."""
from __future__ import annotations

import ctypes
import mmap
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

if os.name == "nt":
    import evidence_windows


@unittest.skipUnless(os.name == "nt", "Windows handle semantics are required")
class WindowsEvidenceHandleTests(unittest.TestCase):
    def test_handle_enumeration_bounded_reads_and_snapshots(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "archive"
            directory.mkdir()
            payload = bytes(range(256)) * 300
            (directory / "artifact.bin").write_bytes(payload)
            (directory / "another").write_bytes(b"x")
            with evidence_windows.open_directory(directory) as opened:
                self.assertEqual(set(opened.names(2)), {"artifact.bin", "another"})
                with self.assertRaises(OSError):
                    opened.names(1)
                with opened.open_file("artifact.bin") as artifact:
                    before = artifact.snapshot()
                    chunks = []
                    while chunk := artifact.read(16_384):
                        chunks.append(chunk)
                    self.assertEqual(b"".join(chunks), payload)
                    self.assertEqual(before, artifact.snapshot())
                    self.assertEqual(before.st_size, len(payload))
                    with self.assertRaises(OSError):
                        artifact.read(1_048_578)

    def test_rejects_duplicate_names_during_enumeration(self):
        encoded = "same-name".encode("utf-16-le")
        record = ctypes.create_string_buffer(104 + len(encoded))
        ctypes.c_uint32.from_buffer(record, 60).value = len(encoded)
        ctypes.memmove(ctypes.addressof(record) + 104, encoded, len(encoded))
        calls = 0

        def query_directory(handle, info_class, buffer, size):
            nonlocal calls
            calls += 1
            ctypes.memset(buffer, 0, size)
            ctypes.memmove(buffer, record, len(record))
            return True

        with patch.object(evidence_windows, "_GetFileInformationByHandleEx",
                          side_effect=query_directory):
            with self.assertRaises(OSError):
                evidence_windows.Directory([1]).names(10)
        self.assertEqual(calls, 2)

    def test_relative_open_cleanup_does_not_mask_type_failure(self):
        failure = RuntimeError("type check failure")
        closed = []

        def create_file(handle_pointer, *args):
            ctypes.cast(handle_pointer, ctypes.POINTER(evidence_windows.wintypes.HANDLE)).contents.value = 37
            return 0

        def close_with_failure(handle):
            closed.append(handle)
            raise OSError("close failure")

        with patch.object(evidence_windows, "_NtCreateFile", side_effect=create_file):
            with patch.object(evidence_windows, "_check_type", side_effect=failure):
                with patch.object(evidence_windows, "_close", side_effect=close_with_failure):
                    with self.assertRaises(RuntimeError) as raised:
                        evidence_windows._open_relative(10, "entry", directory=False)
        self.assertIs(raised.exception, failure)
        self.assertEqual(closed, [37])

    def test_relative_open_closes_handle_when_python_value_conversion_fails(self):
        class HandleValueAllocationError(ctypes.c_void_p):
            @property
            def value(self):
                raise MemoryError("handle value allocation failure")

        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "handle-conversion.txt"
            path.write_bytes(b"native cleanup probe")
            native_handle = evidence_windows._CreateFileW(
                str(path),
                evidence_windows._FILE_READ_ATTRIBUTES | evidence_windows._SYNCHRONIZE,
                evidence_windows._FILE_SHARE_READ | evidence_windows._FILE_SHARE_WRITE,
                None,
                evidence_windows._OPEN_EXISTING,
                evidence_windows._FILE_FLAG_OPEN_REPARSE_POINT,
                None,
            )
            self.assertTrue(native_handle and native_handle != evidence_windows._INVALID_HANDLE_VALUE)

            def create_file(handle_pointer, *args):
                ctypes.cast(handle_pointer, ctypes.POINTER(ctypes.c_void_p)).contents.value = native_handle
                return 0

            try:
                with patch.object(evidence_windows.wintypes, "HANDLE", HandleValueAllocationError):
                    with patch.object(evidence_windows, "_NtCreateFile", side_effect=create_file):
                        with patch.object(evidence_windows, "_check_type") as check_type:
                            with self.assertRaisesRegex(MemoryError, "handle value allocation failure"):
                                evidence_windows._open_relative(10, "entry", directory=False)
                self.assertEqual(evidence_windows._GetFileType(native_handle), 0)
                check_type.assert_not_called()
            finally:
                if evidence_windows._GetFileType(native_handle) != 0:
                    evidence_windows._close(native_handle)

    def test_file_wrapper_failure_closes_open_handle(self):
        failure = RuntimeError("wrapper construction failure")
        closed = []

        def close_with_failure(handle):
            closed.append(handle)
            raise OSError("close failure")

        with patch.object(evidence_windows, "_open_relative", return_value=41):
            with patch.object(evidence_windows, "File", side_effect=failure):
                with patch.object(evidence_windows, "_close", side_effect=close_with_failure):
                    with self.assertRaises(RuntimeError) as raised:
                        evidence_windows.Directory([10]).open_file("entry")
        self.assertIs(raised.exception, failure)
        self.assertEqual(closed, [41])

    def test_fixed_volume_check_uses_opened_volume_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            checked_paths = []

            def drive_type(path):
                checked_paths.append(path)
                if evidence_windows._VOLUME_ROOT.fullmatch(path):
                    return evidence_windows._DRIVE_REMOVABLE
                return evidence_windows._DRIVE_FIXED

            close_handle = evidence_windows._close

            def close_then_fail(handle):
                close_handle(handle)
                raise OSError("simulated close failure")

            with patch.object(evidence_windows, "_GetDriveTypeW", side_effect=drive_type):
                with patch.object(evidence_windows, "_close", side_effect=close_then_fail):
                    with self.assertRaises(OSError) as raised:
                        evidence_windows.open_directory(Path(temporary))
            self.assertEqual(raised.exception.errno, evidence_windows.errno.EINVAL)
            self.assertTrue(checked_paths)
            self.assertTrue(all(evidence_windows._VOLUME_ROOT.fullmatch(path) for path in checked_paths))

    def test_existing_writable_mapping_blocks_helper_open(self):
        with tempfile.TemporaryDirectory() as temporary:
            entry = Path(temporary) / "mapped.bin"
            entry.write_bytes(b"original")
            with entry.open("r+b") as source:
                mapping = mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_WRITE)
            try:
                with evidence_windows.open_directory(entry.parent) as opened:
                    with self.assertRaises(OSError):
                        with opened.open_file(entry.name):
                            self.fail("helper opened a file with a live writable mapping")
                mapping[0:1] = b"M"
                mapping.flush()
            finally:
                mapping.close()
            self.assertEqual(entry.read_bytes(), b"Mriginal")

    def test_cleanup_attempts_all_ancestors_without_masking_open_failure(self):
        failure = RuntimeError("open failure")
        closed = []

        def open_relative(parent, name, *, directory, for_delete=False):
            if name == "broken":
                raise failure
            return parent + 1

        def close_with_failure(handle):
            closed.append(handle)
            raise OSError("close failure")

        with patch.object(evidence_windows, "_absolute_parts",
                          return_value=("C:\\", ["ancestor", "broken"])):
            with patch.object(evidence_windows, "_open_root", return_value=10):
                with patch.object(evidence_windows, "_open_relative", side_effect=open_relative):
                    with patch.object(evidence_windows, "_close", side_effect=close_with_failure):
                        with self.assertRaises(RuntimeError) as raised:
                            evidence_windows.open_directory("unused")
        self.assertIs(raised.exception, failure)
        self.assertEqual(closed, [11, 10])

    def test_handle_slot_allocation_failure_precedes_native_open(self):
        failure = MemoryError("handle-slot allocation failure")
        with patch.object(evidence_windows, "_absolute_parts",
                          return_value=("C:\\", ["child"])):
            with patch.object(evidence_windows, "_handle_slots", side_effect=failure):
                with patch.object(evidence_windows, "_open_root") as open_root:
                    with self.assertRaises(MemoryError) as raised:
                        evidence_windows.open_directory("unused")
        self.assertIs(raised.exception, failure)
        open_root.assert_not_called()

    def test_directory_allocation_failure_closes_all_owned_handles(self):
        failure = MemoryError("directory wrapper allocation failure")
        closed = []

        def close_with_failure(handle):
            closed.append(handle)
            raise OSError("close failure")

        with patch.object(evidence_windows, "_absolute_parts",
                          return_value=("C:\\", ["first", "second"])):
            with patch.object(evidence_windows, "_open_root", return_value=10):
                with patch.object(evidence_windows, "_open_relative", side_effect=[11, 12]):
                    with patch.object(evidence_windows, "Directory", side_effect=failure):
                        with patch.object(evidence_windows, "_close", side_effect=close_with_failure):
                            with self.assertRaises(MemoryError) as raised:
                                evidence_windows.open_directory("unused")
        self.assertIs(raised.exception, failure)
        self.assertEqual(closed, [12, 11, 10])


    def test_context_cleanup_errors_do_not_mask_body_exception(self):
        primary = RuntimeError("body failure")
        owners = (
            (evidence_windows.File(21, False), [21]),
            (evidence_windows.Directory([31, 32]), [32, 31]),
        )
        for owner, expected_closed in owners:
            closed = []

            def close_with_failure(handle):
                closed.append(handle)
                raise OSError("close failure")

            with patch.object(evidence_windows, "_close", side_effect=close_with_failure):
                with self.assertRaises(RuntimeError) as raised:
                    with owner:
                        raise primary
            self.assertIs(raised.exception, primary)
            self.assertEqual(closed, expected_closed)

    def test_parent_and_open_file_cannot_be_replaced(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            parent = root / "parent"
            archive = parent / "archive"
            archive.mkdir(parents=True)
            entry = archive / "entry"
            entry.write_bytes(b"original")
            replacement = root / "replacement"
            replacement.write_bytes(b"outside")
            with evidence_windows.open_directory(archive) as opened:
                with self.assertRaises(OSError):
                    parent.rename(root / "moved-parent")
                with opened.open_file("entry") as artifact:
                    before = artifact.snapshot()
                    with self.assertRaises(OSError):
                        os.replace(replacement, entry)
                    with self.assertRaises(OSError):
                        entry.write_bytes(b"modified")
                    self.assertEqual(artifact.read(32), b"original")
                    self.assertEqual(artifact.snapshot(), before)
            self.assertEqual(entry.read_bytes(), b"original")
            self.assertEqual(replacement.read_bytes(), b"outside")

    def test_delete_uses_open_entry_and_preserves_other_hard_link(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "archive"
            archive.mkdir()
            outside = root / "outside"
            outside.write_bytes(b"shared contents")
            entry = archive / "entry"
            os.link(outside, entry)
            with evidence_windows.open_directory(archive) as opened:
                with opened.open_file("entry", for_delete=True) as artifact:
                    before = artifact.snapshot()
                    self.assertEqual(artifact.read(64), b"shared contents")
                    self.assertEqual(artifact.snapshot(), before)
                    self.assertTrue(artifact.delete(before))
            self.assertFalse(entry.exists())
            self.assertEqual(outside.read_bytes(), b"shared contents")

    def test_replaced_entry_is_not_deleted_using_stale_snapshot(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "archive"
            archive.mkdir()
            entry = archive / "entry"
            entry.write_bytes(b"validated")
            replacement = root / "replacement"
            replacement.write_bytes(b"replacement")
            with evidence_windows.open_directory(archive) as opened:
                with opened.open_file("entry") as artifact:
                    original = artifact.snapshot()
                    self.assertEqual(artifact.read(64), b"validated")
                os.replace(replacement, entry)
                with opened.open_file("entry", for_delete=True) as artifact:
                    self.assertNotEqual(artifact.snapshot(), original)
                    self.assertFalse(artifact.delete(original))
            self.assertEqual(entry.read_bytes(), b"replacement")

    def test_rejects_non_components_and_directories_as_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / "archive"
            archive.mkdir()
            (archive / "nested").mkdir()
            with evidence_windows.open_directory(archive) as opened:
                for name in ("", ".", "..", "nested\\child", "nested/child", "entry:stream", "x\x00y"):
                    with self.subTest(name=name), self.assertRaises(OSError):
                        opened.open_file(name)
                with self.assertRaises(OSError):
                    opened.open_file("nested")
            with self.assertRaises(OSError):
                evidence_windows.open_directory(archive / ".." / "archive")

    def test_rejects_junction_ancestors(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / "target"
            target.mkdir()
            (target / "secret").write_bytes(b"outside")
            junction = root / "junction"
            result = subprocess.run(["cmd", "/c", "mklink", "/J", str(junction), str(target)],
                                    capture_output=True, check=False)
            if result.returncode:
                self.skipTest("junction creation is unavailable")
            with self.assertRaises(OSError):
                evidence_windows.open_directory(junction)
            with self.assertRaises(OSError):
                evidence_windows.open_directory(junction / "missing")
            self.assertEqual((target / "secret").read_bytes(), b"outside")

    def test_rejects_file_and_directory_symlinks(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / "target"
            target.mkdir()
            (target / "secret").write_bytes(b"outside")
            archive = root / "archive"
            archive.mkdir()
            try:
                (archive / "file-link").symlink_to(target / "secret")
                (archive / "directory-link").symlink_to(target, target_is_directory=True)
            except OSError:
                self.skipTest("symlink creation is unavailable")
            with evidence_windows.open_directory(archive) as opened:
                with self.assertRaises(OSError):
                    opened.open_file("file-link")
            with self.assertRaises(OSError):
                evidence_windows.open_directory(archive / "directory-link")
            self.assertEqual((target / "secret").read_bytes(), b"outside")


if __name__ == "__main__":
    unittest.main()
