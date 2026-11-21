"""Standard-library tests for the offline evidence recorder and synthetic controls."""
from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import evidence

ROOT = (
    Path(os.path.abspath(__file__)).parents[1]
    if os.name == "nt"
    else Path(__file__).resolve().parents[1]
)
SCRIPT = ROOT / "scripts" / "evidence.py"
SCHEMA = ROOT / "tests" / "evidence" / "evidence-record.schema.json"
GOLDEN = ROOT / "tests" / "evidence" / "goldens" / "cpu-grid-v1.json"


def base_record(**overrides):
    record = {
        "schema_version": 1,
        "level": "unit-property",
        "execution": "automated",
        "procedure_id": None,
        "ec_ids": ["EC-023"],
        "source_hash": hashlib.sha256(b"source").hexdigest(),
        "binary_hash": hashlib.sha256(b"binary").hexdigest(),
        "fixture_hash": hashlib.sha256(b"fixture").hexdigest(),
        "environment": {
            "availability": "present",
            "os": "synthetic-ci",
            "session": "headless",
            "compositor": "fake-host",
            "gpu": "none",
        },
        "permissions": {"state": "not-required", "items": []},
        "result": {"status": "pass", "summary": "Synthetic evidence-tool test."},
        "evidence_path": "tests/evidence/reports/example.json",
        "issue": "QA-004",
        "reviewer": "qa-reviewer",
        "date": "2026-11-14",
    }
    record.update(overrides)
    return record


def prefixed_unicode_escaped_report(report):
    text = evidence.canonical_json(report).decode("utf-8")
    for marker in ("synthetic-control-report", "synthetic-control-only", "cpu-grid-diff"):
        escaped = f"\\u{ord(marker[0]):04x}" + marker[1:]
        text = text.replace(marker, escaped)
    return b'note "' + text.encode("utf-8")


def _supports_safe_directory_access():
    if os.name == "nt":
        return True
    return (
        os.name == "posix"
        and all(
            operation in getattr(os, "supports_dir_fd", set())
            for operation in (os.open, os.stat, os.unlink)
        )
        and os.scandir in getattr(os, "supports_fd", set())
        and os.stat in getattr(os, "supports_follow_symlinks", set())
        and all(hasattr(os, flag) for flag in ("O_DIRECTORY", "O_NOFOLLOW", "O_NONBLOCK"))
    )



class EvidenceRecordTests(unittest.TestCase):
    def test_record_identity_and_canonical_encoding_are_deterministic(self):
        first = evidence.finalize_record(base_record())
        reversed_input = dict(reversed(list(base_record().items())))
        second = evidence.finalize_record(reversed_input)
        self.assertEqual(first, second)
        self.assertEqual(evidence.canonical_json(first), evidence.canonical_json(second))
        self.assertEqual(first["record_id"], evidence.record_identity(first))

    def test_record_cli_escapes_bidi_controls_in_canonical_json(self):
        raw_summary = (
            "safe \u202eRLO \u2066LRI \u2067RLI \u2068FSI \u2069PDI "
            "plain text stays readable"
        )
        expected_summary = (
            "safe [U+202E]RLO [U+2066]LRI [U+2067]RLI [U+2068]FSI "
            "[U+2069]PDI plain text stays readable"
        )
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary)
            payload = json.dumps(base_record(result={"status": "pass", "summary": raw_summary}))
            completed = subprocess.run(
                [sys.executable, str(SCRIPT), "record", "-", "--archive", str(archive)],
                cwd=ROOT, input=payload, capture_output=True, text=True, check=False,
            )
            self.assertEqual(completed.returncode, 0)
            result = json.loads(completed.stdout)
            record_path = archive / f"qa-evidence-{result['record_id']}.json"
            encoded = record_path.read_bytes()
            record = json.loads(encoded)
            self.assertEqual(record["result"]["summary"], expected_summary)
            self.assertEqual(encoded, evidence.canonical_json(record))
            for control in ("\u202e", "\u2066", "\u2067", "\u2068", "\u2069"):
                self.assertNotIn(control, encoded.decode("utf-8"))

    def test_canonical_json_rejects_lone_surrogates_as_evidence_errors(self):
        with self.assertRaisesRegex(evidence.EvidenceError, "invalid Unicode"):
            evidence.canonical_json({"invalid": "\ud800"})

    def test_record_metadata_rejects_c1_terminal_controls(self):
        for control in ("\x7f", "\x9b", "\x9d"):
            with self.subTest(control=repr(control)):
                record = base_record(result={"status": "pass", "summary": f"synthetic{control}summary"})
                with self.assertRaises(evidence.EvidenceError):
                    evidence.finalize_record(record)

    def test_required_hashes_fields_and_ec_ids_are_enforced(self):
        for field in base_record():
            with self.subTest(missing_field=field):
                missing = base_record()
                del missing[field]
                with self.assertRaises(evidence.EvidenceError):
                    evidence.finalize_record(missing)
        bad_hash = base_record(source_hash="not-a-hash")
        with self.assertRaises(evidence.EvidenceError):
            evidence.finalize_record(bad_hash)
        bad_ec = base_record(ec_ids=["not-an-ec"])
        with self.assertRaises(evidence.EvidenceError):
            evidence.finalize_record(bad_ec)
        missing_reviewer = base_record()
        missing_reviewer["reviewer"] = ""
        with self.assertRaises(evidence.EvidenceError):
            evidence.finalize_record(missing_reviewer)

    def test_native_manual_is_separate_and_absent_environment_is_not_run(self):
        native = base_record(
            level="native-manual",
            execution="manual",
            procedure_id="NATIVE-SHELL-001",
            ec_ids=[],
            environment={
                "availability": "absent",
                "os": "not-available",
                "session": "not-available",
                "compositor": "not-available",
                "gpu": "not-available",
            },
            permissions={"state": "unavailable", "items": ["screen-capture"]},
            result={"status": "not-run", "summary": "Required native host was absent."},
        )
        finalized = evidence.finalize_record(native)
        self.assertEqual(finalized["result"]["status"], "not-run")
        invalid_pass = dict(native)
        invalid_pass["result"] = {"status": "pass", "summary": "Must not pass."}
        with self.assertRaises(evidence.EvidenceError):
            evidence.finalize_record(invalid_pass)
        invalid_automation = dict(native)
        invalid_automation["execution"] = "automated"
        with self.assertRaises(evidence.EvidenceError):
            evidence.finalize_record(invalid_automation)

    def test_privacy_filter_redacts_paths_and_personal_fields(self):
        raw = base_record(result={
            "status": "pass",
            "summary": 'window_title="Tax return" input_value=secret',
        })
        clean = evidence.finalize_record(raw)
        summary = clean["result"]["summary"]
        for private_value in ("Tax return", "secret"):
            self.assertNotIn(private_value, summary)
        self.assertIn("[REDACTED-FIELD]", summary)
        self.assertFalse(evidence.has_private_text(summary))
        for private_path in (
            r"opened C:\Users\Alice Smith\Private Documents\Tax return.pdf",
            "opened file:///home/alice/Private Documents/Tax return.pdf",
            "opened file://server/share/Private Documents/Tax return.pdf",
            "opened C:MedicalReport.pdf",
            "from /home/alice/private.png",
        ):
            with self.subTest(path=private_path):
                self.assertEqual(evidence.sanitize_text(private_path), "[REDACTED-PATH]")
        extra_private_field = base_record(window_title="private")
        with self.assertRaises(evidence.EvidenceError):
            evidence.finalize_record(extra_private_field)

    def test_dotted_private_fields_and_drive_paths_are_redacted(self):
        private_values = (
            "NestedInputSecretQA004",
            "NestedWindowSecretQA004",
            "NestedArraySecretQA004",
            "DottedInputSecretQA004",
            "DottedWindowSecretQA004",
            "DottedArraySecretQA004",
            "hunter2",
            "Confidential",
            "EnvironmentInputSecretQA004",
            "EnvironmentWindowSecretQA004",
        )
        drive_relative_path = "C:MedicalReport.pdf"
        structured = json.dumps({
            "input": {"text": private_values[0], "details": [{"value": private_values[2]}]},
            "window": {"title": private_values[1]},
        }, separators=(",", ":"))
        dotted_structured = json.dumps({
            "metadata": {
                "input.value": {
                    "text": private_values[3],
                    "details": [{"value": private_values[5]}],
                },
                "window.title": private_values[4],
            },
        }, separators=(",", ":"))
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "nested-private.jsonl"
            source.write_text(
                f"input.value={private_values[6]}\n"
                f"window.title={private_values[7]}\n"
                f"opened {drive_relative_path}\n"
                + structured + "\n" + dotted_structured + "\n",
                encoding="utf-8",
            )
            report = evidence.sanitize_log(source)
            self.assertTrue(evidence.validate_log_report(report))
            for private_value in (*private_values, drive_relative_path, "MedicalReport.pdf"):
                self.assertNotIn(private_value, report["content"])

        summary = "\n".join((
            f"input.value={private_values[6]}",
            f"window.title={private_values[7]}",
            f"opened {drive_relative_path}",
            structured,
            dotted_structured,
        ))
        sanitized_summary = evidence.sanitize_text(summary)
        for private_value in (*private_values, drive_relative_path, "MedicalReport.pdf"):
            self.assertNotIn(private_value, sanitized_summary)
        self.assertFalse(evidence.has_private_text(sanitized_summary))

        environment = {
            "availability": "present",
            "os": f"input.value={private_values[8]}",
            "session": f"window.title={private_values[9]}",
            "compositor": drive_relative_path,
            "gpu": "none",
        }
        clean = evidence.finalize_record(base_record(
            result={"status": "pass", "summary": f"input.value={private_values[6]}"},
            environment=environment,
        ))
        clean_summary = clean["result"]["summary"]
        clean_environment = "\n".join(clean["environment"].values())
        for private_value in (*private_values, drive_relative_path, "MedicalReport.pdf"):
            self.assertNotIn(private_value, clean_summary)
            self.assertNotIn(private_value, clean_environment)
        self.assertFalse(evidence.has_private_text(clean_summary))
        self.assertFalse(evidence.has_private_text(clean_environment))

        truncated = '{\n  "input.value": {\n    "text": "TruncatedDottedSecretQA004"\n'
        self.assertNotIn("TruncatedDottedSecretQA004", evidence.sanitize_text(truncated))


    def test_native_manual_cannot_claim_a_synthetic_control_artifact(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            artifact = Path(temporary) / "synthetic-control.json"
            report = evidence.fake_control("timestamp-fault")
            artifact.write_bytes(evidence.canonical_json(report))
            self.assertEqual(report["artifact_type"], "synthetic-control-report")
            record = base_record(
                level="native-manual",
                execution="manual",
                procedure_id="NATIVE-TEST-001",
                ec_ids=[],
                evidence_path=artifact.relative_to(ROOT).as_posix(),
            )
            with self.assertRaises(evidence.EvidenceError):
                evidence.finalize_record(record)
            artifact.write_bytes(prefixed_unicode_escaped_report(report))
            with self.assertRaises(evidence.EvidenceError):
                evidence.finalize_record(record)


    def test_native_manual_manifest_rejects_oversized_utf16_synthetic_json(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            directory = Path(temporary)
            artifact = directory / "oversized-utf16-native.bin"
            manifest_path = directory / "native-evidence.json"
            report = evidence.fake_control("timestamp-fault")
            payload = (
                " " * (evidence.MAX_JSON_BYTES // 2 + 1)
                + json.dumps(report, separators=(",", ":"))
            ).encode("utf-16")
            self.assertGreater(len(payload), evidence.MAX_JSON_BYTES)
            artifact.write_bytes(payload)
            artifact_relative = artifact.relative_to(ROOT).as_posix()
            manifest_relative = manifest_path.relative_to(ROOT).as_posix()
            record = base_record(
                level="native-manual",
                execution="manual",
                procedure_id="NATIVE-UTF16-001",
                ec_ids=[],
                evidence_path=manifest_relative,
            )
            manifest = {
                "schema_version": 1,
                "artifact_type": "native-manual-evidence",
                "execution": "manual",
                "procedure_id": record["procedure_id"],
                "reviewer": record["reviewer"],
                "date": record["date"],
                "status": "pass",
                "artifacts": [{
                    "path": artifact_relative,
                    "sha256": hashlib.sha256(payload).hexdigest(),
                }],
            }
            manifest_path.write_bytes(evidence.canonical_json(manifest))
            with self.assertRaisesRegex(evidence.EvidenceError, "native evidence JSON exceeds the 1 MiB inspection limit"):
                evidence.finalize_record(record)

    def test_native_manual_manifest_rejects_oversized_utf16_primitive_json(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            directory = Path(temporary)
            manifest_path = directory / "native-evidence.json"
            manifest_relative = manifest_path.relative_to(ROOT).as_posix()
            record = base_record(
                level="native-manual",
                execution="manual",
                procedure_id="NATIVE-UTF16-SCALAR-001",
                ec_ids=[],
                evidence_path=manifest_relative,
            )
            encodings = (
                ("utf-16-le", b"\xff\xfe"),
                ("utf-16-be", b"\xfe\xff"),
            )
            scalars = ("true", "false", "null", "0", "-1.25e+2")
            case_index = 0
            for scalar in scalars:
                for encoding, bom in encodings:
                    with self.subTest(scalar=scalar, encoding=encoding):
                        artifact = directory / f"oversized-utf16-scalar-{case_index}.bin"
                        case_index += 1
                        leading_space_count = (
                            524_286 if scalar == "true"
                            else evidence.MAX_JSON_BYTES // 2 + 1
                        )
                        payload = bom + (
                            " " * leading_space_count + scalar
                        ).encode(encoding)
                        self.assertGreater(len(payload), evidence.MAX_JSON_BYTES)
                        self.assertEqual(
                            json.loads(payload.decode("utf-16")),
                            json.loads(scalar),
                        )
                        artifact.write_bytes(payload)
                        artifact_relative = artifact.relative_to(ROOT).as_posix()
                        manifest = {
                            "schema_version": 1,
                            "artifact_type": "native-manual-evidence",
                            "execution": "manual",
                            "procedure_id": record["procedure_id"],
                            "reviewer": record["reviewer"],
                            "date": record["date"],
                            "status": "pass",
                            "artifacts": [{
                                "path": artifact_relative,
                                "sha256": hashlib.sha256(payload).hexdigest(),
                            }],
                        }
                        manifest_path.write_bytes(evidence.canonical_json(manifest))
                        with self.assertRaisesRegex(
                            evidence.EvidenceError,
                            "native evidence JSON exceeds the 1 MiB inspection limit",
                        ):
                            evidence.finalize_record(record)

    def test_native_manual_manifest_classifies_oversized_numeric_streams(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            directory = Path(temporary)
            manifest_path = directory / "native-evidence.json"
            manifest_relative = manifest_path.relative_to(ROOT).as_posix()
            record = base_record(
                level="native-manual",
                execution="manual",
                procedure_id="NATIVE-SCALAR-002",
                ec_ids=[],
                evidence_path=manifest_relative,
            )
            cases = (
                (
                    "numeric-invalid-byte",
                    b"123" + b" " * 65_533 + b"\xff" + b"X" * evidence.MAX_JSON_BYTES,
                    False,
                ),
                (
                    "nul-padded-binary",
                    b"123" + b"\x00" * evidence.MAX_JSON_BYTES,
                    False,
                ),
                (
                    "long-valid-integer",
                    b"9" * (evidence.MAX_JSON_BYTES + 1),
                    True,
                ),
            )
            for name, payload, is_json in cases:
                with self.subTest(artifact=name):
                    artifact = directory / f"{name}.bin"
                    artifact.write_bytes(payload)
                    self.assertGreater(len(payload), evidence.MAX_JSON_BYTES)
                    artifact_relative = artifact.relative_to(ROOT).as_posix()
                    manifest = {
                        "schema_version": 1,
                        "artifact_type": "native-manual-evidence",
                        "execution": "manual",
                        "procedure_id": record["procedure_id"],
                        "reviewer": record["reviewer"],
                        "date": record["date"],
                        "status": "pass",
                        "artifacts": [{
                            "path": artifact_relative,
                            "sha256": hashlib.sha256(payload).hexdigest(),
                        }],
                    }
                    manifest_path.write_bytes(evidence.canonical_json(manifest))
                    if is_json:
                        with self.assertRaisesRegex(
                            evidence.EvidenceError,
                            "native evidence JSON exceeds the 1 MiB inspection limit",
                        ):
                            evidence.finalize_record(record)
                    else:
                        finalized = evidence.finalize_record(record)
                        self.assertEqual(finalized["result"]["status"], "pass")


    def test_native_manual_manifest_binds_pass_to_binary_artifact(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            directory = Path(temporary)
            manifest_path = directory / "native-evidence.json"
            binary_path = directory / "synthetic-screenshot.bin"
            binary = b"123\xffPNG\r\n\x1a\n" + bytes(range(256)) * (
                evidence.MAX_JSON_BYTES // 256 + 1
            )
            self.assertGreater(len(binary), evidence.MAX_JSON_BYTES)
            binary_path.write_bytes(binary)
            binary_relative = binary_path.relative_to(ROOT).as_posix()
            manifest_relative = manifest_path.relative_to(ROOT).as_posix()
            self.assertIn("/", binary_relative)
            self.assertNotIn("\\", binary_relative)
            procedure_id = "NATIVE-TEST-002"
            record = base_record(
                level="native-manual",
                execution="manual",
                procedure_id=procedure_id,
                ec_ids=[],
                evidence_path=manifest_relative,
            )

            def write_manifest(
                *,
                artifact_path=binary_relative,
                artifact_hash=hashlib.sha256(binary).hexdigest(),
                manifest_procedure=procedure_id,
            ):
                manifest = {
                    "schema_version": 1,
                    "artifact_type": "native-manual-evidence",
                    "execution": "manual",
                    "procedure_id": manifest_procedure,
                    "reviewer": record["reviewer"],
                    "date": record["date"],
                    "status": "pass",
                    "artifacts": [{"path": artifact_path, "sha256": artifact_hash}],
                }
                manifest_path.write_bytes(evidence.canonical_json(manifest))

            write_manifest()
            # This verifies artifact/metadata consistency, not reviewer identity or execution.
            accepted = evidence.finalize_record(record)
            self.assertEqual(accepted["result"]["status"], "pass")

            synthetic_path = directory / "renamed-control-output.bin"
            synthetic_report = evidence.fake_control("timestamp-fault")
            self.assertEqual(synthetic_report["artifact_type"], "synthetic-control-report")
            synthetic_bytes = evidence.canonical_json(synthetic_report)
            synthetic_path.write_bytes(synthetic_bytes)
            write_manifest(
                artifact_path=synthetic_path.relative_to(ROOT).as_posix(),
                artifact_hash=hashlib.sha256(synthetic_bytes).hexdigest(),
            )
            with self.assertRaises(evidence.EvidenceError):
                evidence.finalize_record(record)
            synthetic_bytes = prefixed_unicode_escaped_report(synthetic_report)
            synthetic_path.write_bytes(synthetic_bytes)
            write_manifest(
                artifact_path=synthetic_path.relative_to(ROOT).as_posix(),
                artifact_hash=hashlib.sha256(synthetic_bytes).hexdigest(),
            )
            with self.assertRaises(evidence.EvidenceError):
                evidence.finalize_record(record)

            golden = json.loads(GOLDEN.read_text(encoding="utf-8"))
            grid_report = evidence.grid_diff({"grid": golden["grid"]}, golden)
            self.assertEqual(grid_report["artifact_type"], "cpu-grid-diff")
            grid_bytes = evidence.canonical_json(grid_report)
            grid_path = directory / "renamed-grid-diff-output.dat"
            grid_path.write_bytes(grid_bytes)
            write_manifest(
                artifact_path=grid_path.relative_to(ROOT).as_posix(),
                artifact_hash=hashlib.sha256(grid_bytes).hexdigest(),
            )
            with self.assertRaises(evidence.EvidenceError):
                evidence.finalize_record(record)

            escaped_marker = (
                r"\u0073\u0079\u006e\u0074\u0068\u0065\u0074\u0069\u0063\u002d"
                r"\u0063\u006f\u006e\u0074\u0072\u006f\u006c\u002d\u006f\u006e\u006c\u0079"
            )
            oversized_json_header = (
                b' \n{"artifact_type":"' + escaped_marker.encode("ascii") + b'"}'
            )
            oversized_json = oversized_json_header + b" " * (
                evidence.MAX_JSON_BYTES + 1 - len(oversized_json_header)
            )
            self.assertEqual(len(oversized_json), evidence.MAX_JSON_BYTES + 1)
            self.assertNotIn(b"synthetic-control-only", oversized_json_header)
            escaped_json_path = directory / "unicode-escaped-control.bin"
            escaped_json_path.write_bytes(oversized_json)
            write_manifest(
                artifact_path=escaped_json_path.relative_to(ROOT).as_posix(),
                artifact_hash=hashlib.sha256(oversized_json).hexdigest(),
            )
            with self.assertRaises(evidence.EvidenceError):
                evidence.finalize_record(record)

            write_manifest(artifact_hash="f" * 64)
            with self.assertRaises(evidence.EvidenceError):
                evidence.finalize_record(record)

            write_manifest(manifest_procedure="NATIVE-OTHER-002")
            with self.assertRaises(evidence.EvidenceError):
                evidence.finalize_record(record)

            windows_path = binary_relative.replace("/", "\\")
            write_manifest(artifact_path=windows_path)
            with self.assertRaises(evidence.EvidenceError):
                evidence.finalize_record(record)

    def test_native_manual_accepts_a_direct_utf8_evidence_artifact(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            artifact = Path(temporary) / "native-observation.txt"
            artifact.write_text("Native shell displayed the expected window.\n", encoding="utf-8")
            relative_path = artifact.relative_to(ROOT).as_posix()
            self.assertNotIn("\\", relative_path)
            record = base_record(
                level="native-manual",
                execution="manual",
                procedure_id="NATIVE-TEST-003",
                ec_ids=[],
                evidence_path=relative_path,
            )

            accepted = evidence.finalize_record(record)

            self.assertEqual(accepted["result"]["status"], "pass")
            self.assertEqual(accepted["evidence_path"], relative_path)

    def test_native_manual_manifest_rejects_a_symlinked_binary_artifact(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            directory = Path(temporary)
            target = directory / "artifact.bin"
            target.write_bytes(b"\x89PNG\r\n\x1a\nbinary evidence")
            linked = directory / "linked-artifact.bin"
            try:
                os.symlink(target, linked)
            except OSError:
                self.skipTest("creating a file symlink is unavailable")
            manifest_path = directory / "native-evidence.json"
            manifest = {
                "schema_version": 1,
                "artifact_type": "native-manual-evidence",
                "execution": "manual",
                "procedure_id": "NATIVE-TEST-004",
                "reviewer": "qa-reviewer",
                "date": "2026-11-14",
                "status": "pass",
                "artifacts": [{
                    "path": linked.relative_to(ROOT).as_posix(),
                    "sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
                }],
            }
            manifest_path.write_bytes(evidence.canonical_json(manifest))
            record = base_record(
                level="native-manual",
                execution="manual",
                procedure_id="NATIVE-TEST-004",
                ec_ids=[],
                evidence_path=manifest_path.relative_to(ROOT).as_posix(),
            )

            with self.assertRaises(evidence.EvidenceError):
                evidence.finalize_record(record)

    def test_finalized_record_size_is_readable_after_persistence(self):
        empty = base_record(permissions={"state": "not-required", "items": []})
        one_item = base_record(permissions={
            "state": "not-required",
            "items": ["screen-capture"],
        })
        two_items = base_record(permissions={
            "state": "not-required",
            "items": ["screen-capture", "screen-capture"],
        })
        empty_size = len(evidence.canonical_json(empty))
        first_item_growth = len(evidence.canonical_json(one_item)) - empty_size
        further_item_growth = (
            len(evidence.canonical_json(two_items)) - len(evidence.canonical_json(one_item))
        )
        empty_with_id = dict(empty)
        empty_with_id["record_id"] = evidence.record_identity(empty)
        identifier_growth = len(evidence.canonical_json(empty_with_id)) - empty_size
        over_count = (
            evidence.MAX_JSON_BYTES - empty_size - first_item_growth - identifier_growth
        ) // further_item_growth + 2
        over_limit_input = base_record(permissions={
            "state": "not-required",
            "items": ["screen-capture"] * over_count,
        })
        self.assertLessEqual(len(evidence.canonical_json(over_limit_input)), evidence.MAX_JSON_BYTES)
        with_id = dict(over_limit_input)
        with_id["record_id"] = evidence.record_identity(over_limit_input)
        self.assertGreater(len(evidence.canonical_json(with_id)), evidence.MAX_JSON_BYTES)
        with self.assertRaises(evidence.EvidenceError):
            evidence.finalize_record(over_limit_input)
        payload = json.dumps(over_limit_input, separators=(",", ":"))
        self.assertLessEqual(len(payload.encode("utf-8")), evidence.MAX_JSON_BYTES)
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary)
            rejected = subprocess.run(
                [sys.executable, str(SCRIPT), "record", "-", "--archive", str(archive)],
                cwd=ROOT, input=payload, capture_output=True, text=True, check=False,
            )
            self.assertEqual(rejected.returncode, 2)
            self.assertEqual(json.loads(rejected.stdout)["status"], "error")
            self.assertEqual(list(archive.iterdir()), [])

        readable_input = base_record(permissions={
            "state": "not-required",
            "items": ["screen-capture"] * (over_count - 10),
        })
        readable = evidence.finalize_record(readable_input)
        encoded = evidence.canonical_json(readable)
        self.assertLessEqual(len(encoded), evidence.MAX_JSON_BYTES)
        with tempfile.TemporaryDirectory() as temporary:
            persisted = Path(temporary) / "bounded-record.json"
            persisted.write_bytes(encoded)
            self.assertEqual(evidence.load_record(persisted), readable)

    def test_record_rejects_path_traversal_and_invalid_calendar_date(self):
        for path in ("../../private.json", r"tests\evidence\run.json", "C:/private/evidence.json"):
            with self.subTest(path=path), self.assertRaises(evidence.EvidenceError):
                evidence.finalize_record(base_record(evidence_path=path))
        with self.assertRaises(evidence.EvidenceError):
            evidence.finalize_record(base_record(date="2026-02-31"))


    def test_record_schema_declares_required_levels_and_closed_fields(self):
        schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
        self.assertEqual(schema["$schema"], "https://json-schema.org/draft/2020-12/schema")
        self.assertFalse(schema["additionalProperties"])
        self.assertEqual(set(schema["required"]), set(base_record()) | {"record_id"})
        self.assertEqual(set(schema["properties"]["level"]["enum"]), evidence.LEVELS)
        self.assertIn("date", schema["required"])
        path_pattern = re.compile(schema["properties"]["evidence_path"]["pattern"])
        self.assertIsNotNone(path_pattern.fullmatch("tests/evidence/run.json"))
        for invalid_path in ("tests//evidence/run.json", "tests/./evidence/run.json", "tests/evidence/"):
            self.assertIsNone(path_pattern.fullmatch(invalid_path))

    def test_all_seven_evidence_levels_have_distinct_valid_records(self):
        levels = (
            "document-qa", "unit-property", "integration", "fuzz",
            "benchmark", "native-manual", "release-acceptance",
        )
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            native_artifact = Path(temporary) / "native-level-evidence.txt"
            native_artifact.write_text("Native procedure observation.\n", encoding="utf-8")
            native_evidence_path = native_artifact.relative_to(ROOT).as_posix()
            for level in levels:
                with self.subTest(level=level):
                    native = level == "native-manual"
                    record_fields = {
                        "level": level,
                        "execution": "manual" if native else "automated",
                        "procedure_id": "NATIVE-001" if native else None,
                    }
                    if native:
                        record_fields["evidence_path"] = native_evidence_path
                    record = evidence.finalize_record(base_record(**record_fields))
                    self.assertEqual(record["level"], level)


    def test_validate_cli_accepts_written_record_and_rejects_bad_record(self):
        record = evidence.finalize_record(base_record())
        with tempfile.TemporaryDirectory() as temporary:
            record_path = Path(temporary) / "record.json"
            record_path.write_bytes(evidence.canonical_json(record))
            valid = subprocess.run(
                [sys.executable, str(SCRIPT), "validate", str(record_path)],
                cwd=ROOT, capture_output=True, text=True, check=False,
            )
            self.assertEqual(valid.returncode, 0)
            self.assertEqual(json.loads(valid.stdout)["status"], "valid")
            broken = dict(record)
            broken["binary_hash"] = "missing"
            broken_path = Path(temporary) / "broken.json"
            broken_path.write_bytes(evidence.canonical_json(broken))
            invalid = subprocess.run(
                [sys.executable, str(SCRIPT), "validate", str(broken_path)],
                cwd=ROOT, capture_output=True, text=True, check=False,
            )
            self.assertEqual(invalid.returncode, 2)
            self.assertEqual(json.loads(invalid.stdout)["status"], "error")

    def test_record_cli_writes_exclusive_deterministic_archive_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary)
            payload = json.dumps(base_record())
            stored = subprocess.run(
                [sys.executable, str(SCRIPT), "record", "-", "--archive", str(archive)],
                cwd=ROOT, input=payload, capture_output=True, text=True, check=False,
            )
            self.assertEqual(stored.returncode, 0)
            result = json.loads(stored.stdout)
            record_path = archive / f"qa-evidence-{result['record_id']}.json"
            self.assertTrue(record_path.is_file())
            record = json.loads(record_path.read_text(encoding="utf-8"))
            self.assertEqual(record["record_id"], result["record_id"])
            repeated = subprocess.run(
                [sys.executable, str(SCRIPT), "record", "-", "--archive", str(archive)],
                cwd=ROOT, input=payload, capture_output=True, text=True, check=False,
            )
            self.assertEqual(repeated.returncode, 2)
            self.assertEqual(len(list(archive.iterdir())), 1)


class SyntheticControlsTests(unittest.TestCase):
    def test_all_fake_decoder_and_host_fault_controls_are_bounded(self):
        scenarios = ("timestamp-fault", "capability-change", "surface-failure", "cancellation")
        for scenario in scenarios:
            with self.subTest(scenario=scenario):
                report = evidence.fake_control(scenario)
                self.assertTrue(evidence.validate_control_report(report))
                self.assertEqual(report["scope"], "synthetic-control-only")
                self.assertFalse(report["claims_product_behavior"])
                self.assertLessEqual(report["resource_peak"], report["resource_limit"])
                self.assertLessEqual(report["queue_peak"], report["queue_limit"])
                self.assertLessEqual(report["generation_peak"], report["generation_limit"])
                self.assertLessEqual(report["generation"], report["generation_limit"])
                self.assertEqual(report["final_output"], report["expected_output"])
                self.assertEqual(report["status"], "pass")
                self.assertTrue(report["checks"]["fault_safely_handled"])
                if scenario in {"capability-change", "cancellation"}:
                    self.assertGreaterEqual(report["stale_outputs"], 1)
                    self.assertEqual(report["generation"], 2)
                if scenario in {"timestamp-fault", "surface-failure", "cancellation"}:
                    self.assertTrue(report["safely_rejected"])

    def test_control_report_cannot_claim_pass_without_safe_fault_handling(self):
        report = evidence.fake_control("timestamp-fault")
        report["safely_rejected"] = False
        report["checks"]["fault_safely_handled"] = False
        report["status"] = "pass"
        report["artifact_id"] = evidence.artifact_identity(report)
        self.assertFalse(evidence.validate_control_report(report))

    def test_control_report_rejects_unbounded_or_misattributed_claims(self):
        for field, value in (
            ("resource_limit", evidence.CONTROL_LIMIT + 1),
            ("fault", "different-fault"),
            ("expected_output", "unexpected-frame"),
        ):
            with self.subTest(field=field):
                report = evidence.fake_control("timestamp-fault")
                report[field] = value
                report["artifact_id"] = evidence.artifact_identity(report)
                self.assertFalse(evidence.validate_control_report(report))

    def test_fake_control_cli_is_machine_readable_and_offline(self):
        completed = subprocess.run(
            [sys.executable, str(SCRIPT), "fake-control", "timestamp-fault"],
            cwd=ROOT, capture_output=True, text=True, check=False,
        )
        self.assertEqual(completed.returncode, 0)
        report = json.loads(completed.stdout)
        self.assertEqual(report["fault"], "nonmonotonic-timestamp")
        self.assertFalse(report["claims_product_behavior"])

    def test_grid_differences_are_explicit_and_require_review_metadata(self):
        golden = json.loads(GOLDEN.read_text(encoding="utf-8"))
        changed = list(golden["grid"])
        changed[1] = "|#.|"
        unreviewed = evidence.grid_diff({"grid": changed}, golden)
        self.assertFalse(unreviewed["matches"])
        self.assertEqual(unreviewed["review"]["status"], "unreviewed")
        self.assertEqual(unreviewed["changes"], [{"x": 2, "y": 1, "expected": "#", "actual": "."}])
        reviewed = evidence.grid_diff({"grid": changed}, golden, "qa-reviewer", "2026-11-14")
        self.assertEqual(reviewed["review"]["status"], "reviewed")
        self.assertTrue(evidence.validate_grid_diff_report(reviewed))
        with self.assertRaises(evidence.EvidenceError):
            evidence.grid_diff({"grid": changed}, golden, "qa-reviewer", None)

    def test_grid_alphabet_blocks_unreviewed_arbitrary_text(self):
        golden = json.loads(GOLDEN.read_text(encoding="utf-8"))
        with self.assertRaises(evidence.EvidenceError):
            evidence.grid_diff({"grid": ["alice@example.invalid"]}, golden)

    def test_sanitized_log_removes_personal_paths_window_input_media_and_ansi(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "private.log"
            source.write_text(
                '\x1b[31mwindow title: Tax return\x1b[0m\n'
                "input_value=typed private text\n"
                "media_path=C:\\\\Users\\\\Alice\\\\clip.mp4\n"
                'opened /home/alice/personal.png\n'
                r"opened C:\Users\Alice Smith\Private Documents\Tax return.pdf" "\n"
                "opened file:///home/alice/Private Documents/Tax return.pdf\n"
                "opened file://server/share/Private Documents/Tax return.pdf\n"
                '{"window_title":"Tax return","input_value":"typed private text",'
                '"media_path":"C:\\\\Users\\\\Alice\\\\clip.mp4"}\n',
                encoding="utf-8",
            )
            report = evidence.sanitize_log(source)
            self.assertTrue(evidence.validate_log_report(report))
            content = report["content"]
            for private_value in (
                "Tax return", "typed private text", "Alice", "/home/alice",
                "Alice Smith", "Private Documents", "file:///", "file://server", "\x1b",
            ):
                self.assertNotIn(private_value, content)

    def test_sanitized_log_preserves_character_after_escape_7(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "escape-7.log"
            source.write_text("before\x1b" + "7" + "AFTER-ESC-7", encoding="utf-8")
            report = evidence.sanitize_log(source)

            self.assertEqual(report["content"], "beforeAFTER-ESC-7")

    def test_sanitized_log_requires_st_to_terminate_dcs_but_osc_accepts_bel(self):
        cases = (
            (
                "visible\x1b]title=osc-secret\x07after-osc-bell",
                "visibleafter-osc-bell",
                ("osc-secret",),
            ),
            (
                "visible\x1bP1;2;dcs-secret\x07still-private\x1b\\after-dcs-st",
                "visibleafter-dcs-st",
                ("dcs-secret", "still-private"),
            ),
            (
                "visible\x1bP1;2;dcs-secret\x1b\\after-dcs-st",
                "visibleafter-dcs-st",
                ("dcs-secret",),
            ),
        )
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "terminated-controls.log"
            for raw, expected, private_values in cases:
                with self.subTest(raw=raw):
                    source.write_text(raw, encoding="utf-8")
                    report = evidence.sanitize_log(source)
                    self.assertEqual(report["content"], expected)
                    for private_value in private_values:
                        self.assertNotIn(private_value, report["content"])


    def test_log_report_validator_rejects_c1_controls_and_invalid_unicode(self):
        base_report = {
            "schema_version": 1,
            "artifact_type": "sanitized-log",
            "content_sha256": "0" * 64,
            "artifact_id": "0" * 64,
        }
        for content in ("\x9b", "\ud800"):
            with self.subTest(content=repr(content)):
                report = dict(base_report, content=content)
                self.assertFalse(evidence.validate_log_report(report))

    def test_log_report_validator_rejects_invalid_artifact_identity(self):
        content = "safe diagnostic"
        report = {
            "schema_version": 1,
            "artifact_type": "sanitized-log",
            "content_sha256": hashlib.sha256(content.encode("utf-8")).hexdigest(),
            "content": content,
            "artifact_id": "\ud800",
        }
        self.assertFalse(evidence.validate_log_report(report))

    def test_sanitize_log_cli_escapes_bidi_controls_in_canonical_json(self):
        raw_content = (
            "safe \u202eRLO \u2066LRI \u2067RLI \u2068FSI \u2069PDI "
            "plain text stays readable"
        )
        expected_content = (
            "safe [U+202E]RLO [U+2066]LRI [U+2067]RLI [U+2068]FSI "
            "[U+2069]PDI plain text stays readable"
        )
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "bidi-controls.log"
            source.write_text(raw_content, encoding="utf-8")
            completed = subprocess.run(
                [sys.executable, str(SCRIPT), "sanitize-log", str(source)],
                cwd=ROOT, capture_output=True, text=True, check=False,
            )
            self.assertEqual(completed.returncode, 0)
            report = json.loads(completed.stdout)
            self.assertEqual(report["content"], expected_content)
            self.assertTrue(evidence.validate_log_report(report))
            self.assertEqual(completed.stdout.encode("utf-8"), evidence.canonical_json(report))
            for control in ("\u202e", "\u2066", "\u2067", "\u2068", "\u2069"):
                self.assertNotIn(control, completed.stdout)

    def test_sanitized_log_discards_truncated_terminal_string_payloads(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "truncated-control.log"
            for raw in (
                "visible\x1b]52;c;clipboard-secret",
                "visible\x1bP1;2;private-dcs-payload",
                "visible\x9d52;c;clipboard-secret",
                "visible\x90private-dcs-payload",
            ):
                with self.subTest(prefix=raw[:12]):
                    source.write_text(raw, encoding="utf-8")
                    report = evidence.sanitize_log(source)
                    self.assertEqual(report["content"], "visible")
                    self.assertTrue(evidence.validate_log_report(report))
    def test_sanitized_log_reads_only_one_byte_past_limit(self):
        class BoundedReader:
            size_requested = None

            def __enter__(self):
                return self

            def __exit__(self, _exception_type, _exception, _traceback):
                return None

            def read(self, size=-1):
                self.size_requested = size
                return b"x" * (evidence.MAX_LOG_BYTES + 1)

        reader = BoundedReader()
        with patch.object(Path, "open", return_value=reader):
            with self.assertRaises(evidence.EvidenceError):
                evidence.sanitize_log(Path("unused.log"))
        self.assertEqual(reader.size_requested, evidence.MAX_LOG_BYTES + 1)

    def test_sanitized_log_rejects_redaction_expansion_over_output_limit(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "expanding.log"
            record = b"window=x\n"
            source.write_bytes(record * (evidence.MAX_LOG_BYTES // len(record)))
            self.assertLessEqual(source.stat().st_size, evidence.MAX_LOG_BYTES)
            with self.assertRaisesRegex(evidence.EvidenceError, "sanitized log exceeds"):
                evidence.sanitize_log(source)


class RetentionTests(unittest.TestCase):
    def test_prune_removes_only_old_valid_tool_records_and_skips_malformed_inputs(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            archive = Path(temporary)
            old = evidence.finalize_record(base_record(date="2026-11-12"))
            old_path = archive / f"qa-evidence-{old['record_id']}.json"
            recent = evidence.finalize_record(base_record(date="2026-11-14"))
            for record in (old, recent):
                path = archive / f"qa-evidence-{record['record_id']}.json"
                path.write_bytes(evidence.canonical_json(record))
            unrelated = archive / "unrelated.json"
            unrelated.write_text("keep me", encoding="utf-8")
            malformed = archive / ("qa-evidence-" + "0" * 64 + ".json")
            malformed.write_text("not an evidence record", encoding="utf-8")
            huge_integer = archive / ("qa-evidence-" + "1" * 64 + ".json")
            huge_integer_payload = '{"value":' + "9" * 5000 + "}"
            huge_integer.write_text(huge_integer_payload, encoding="utf-8")

            int_limit_getter = getattr(sys, "get_int_max_str_digits", None)
            int_limit_setter = getattr(sys, "set_int_max_str_digits", None)
            old_int_limit = (
                int_limit_getter()
                if int_limit_getter is not None and int_limit_setter is not None
                else None
            )
            try:
                if old_int_limit is not None:
                    int_limit_setter(4300)
                if os.name == "posix":
                    with self.assertRaisesRegex(evidence.EvidenceError, "atomic compare-and-unlink"):
                        evidence.prune_archive(archive, keep=1)
                    self.assertEqual(old_path.read_bytes(), evidence.canonical_json(old))
                else:
                    result = evidence.prune_archive(archive, keep=1)
                    self.assertEqual(result["removed"], 1)
                    self.assertEqual(result["removed_names"], [f"qa-evidence-{old['record_id']}.json"])
                    self.assertFalse(old_path.exists())
                self.assertTrue((archive / f"qa-evidence-{recent['record_id']}.json").exists())
                self.assertEqual(unrelated.read_text(encoding="utf-8"), "keep me")
                self.assertTrue(malformed.exists())
                self.assertEqual(huge_integer.read_text(encoding="utf-8"), huge_integer_payload)
            finally:
                if old_int_limit is not None:
                    int_limit_setter(old_int_limit)


    def test_prune_rejects_a_reparse_archive_directory_without_deleting_entries(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            root = Path(temporary)
            archive = root / "archive"
            archive.mkdir()
            old = evidence.finalize_record(base_record(date="2026-11-12"))
            old_path = archive / f"qa-evidence-{old['record_id']}.json"
            old_path.write_bytes(evidence.canonical_json(old))
            reparse_path = root / "archive-link"
            try:
                os.symlink(archive, reparse_path, target_is_directory=True)
            except OSError:
                self.skipTest("creating a directory symlink is unavailable")

            with self.assertRaises(evidence.EvidenceError):
                evidence.prune_archive(reparse_path, keep=1)

            self.assertEqual(old_path.read_bytes(), evidence.canonical_json(old))

    def test_prune_entry_limit_fails_before_any_candidate_is_deleted(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            archive = Path(temporary)
            old = evidence.finalize_record(base_record(date="2026-11-12"))
            recent = evidence.finalize_record(base_record(date="2026-11-14"))
            old_path = archive / f"qa-evidence-{old['record_id']}.json"
            recent_path = archive / f"qa-evidence-{recent['record_id']}.json"
            old_bytes = evidence.canonical_json(old)
            recent_bytes = evidence.canonical_json(recent)
            old_path.write_bytes(old_bytes)
            recent_path.write_bytes(recent_bytes)
            unrelated = archive / "unrelated.bin"
            unrelated.write_bytes(b"must remain untouched")

            with patch.object(evidence, "MAX_ARCHIVE_ENTRIES", 2):
                with self.assertRaisesRegex(evidence.EvidenceError, "too many entries"):
                    evidence.prune_archive(archive, keep=1)

            self.assertEqual(old_path.read_bytes(), old_bytes)
            self.assertEqual(recent_path.read_bytes(), recent_bytes)
            self.assertEqual(unrelated.read_bytes(), b"must remain untouched")

    @unittest.skipUnless(
        os.name == "posix" and _supports_safe_directory_access(),
        "same-directory replacement injection requires POSIX directory-relative access",
    )
    def test_prune_fails_closed_for_same_directory_replacement_at_delete_boundary(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            archive = Path(temporary)
            old = evidence.finalize_record(base_record(date="2026-11-12"))
            recent = evidence.finalize_record(base_record(date="2026-11-14"))
            old_name = f"qa-evidence-{old['record_id']}.json"
            recent_name = f"qa-evidence-{recent['record_id']}.json"
            old_path = archive / old_name
            old_bytes = evidence.canonical_json(old)
            old_path.write_bytes(old_bytes)
            (archive / recent_name).write_bytes(evidence.canonical_json(recent))
            replacement_path = archive / "replacement.bin"
            replacement_bytes = b"same-directory replacement must be preserved"
            replacement_path.write_bytes(replacement_bytes)
            displaced_path = archive / "displaced-original.json"
            original_compare = evidence._same_file_snapshot
            original_snapshot = old_path.stat()
            matching_checks = 0

            def replace_after_snapshot_check(left, right):
                nonlocal matching_checks
                matches = original_compare(left, right)
                if (
                    matches
                    and (left.st_dev, left.st_ino) == (original_snapshot.st_dev, original_snapshot.st_ino)
                ):
                    matching_checks += 1
                    if matching_checks == 2:
                        old_path.rename(displaced_path)
                        replacement_path.rename(old_path)
                return matches

            with patch.object(evidence, "_same_file_snapshot", side_effect=replace_after_snapshot_check):
                with self.assertRaisesRegex(evidence.EvidenceError, "atomic compare-and-unlink"):
                    evidence.prune_archive(archive, keep=1)

            self.assertEqual(matching_checks, 2)
            self.assertEqual(old_path.read_bytes(), replacement_bytes)
            self.assertEqual(displaced_path.read_bytes(), old_bytes)
            self.assertTrue((archive / recent_name).is_file())
    @unittest.skipUnless(
        os.name == "posix" and _supports_safe_directory_access(),
        "same-directory replacement injection requires POSIX directory-relative access",
    )
    def test_prune_skips_same_directory_replacement_before_snapshot_recheck(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            archive = Path(temporary)
            old = evidence.finalize_record(base_record(date="2026-11-12"))
            recent = evidence.finalize_record(base_record(date="2026-11-14"))
            old_name = f"qa-evidence-{old['record_id']}.json"
            recent_name = f"qa-evidence-{recent['record_id']}.json"
            old_path = archive / old_name
            recent_path = archive / recent_name
            old_bytes = evidence.canonical_json(old)
            recent_bytes = evidence.canonical_json(recent)
            old_path.write_bytes(old_bytes)
            recent_path.write_bytes(recent_bytes)
            replacement_path = archive / "replacement.bin"
            replacement_bytes = b"unvalidated same-directory replacement"
            replacement_path.write_bytes(replacement_bytes)
            displaced_path = archive / "displaced-original.json"
            original_stat = os.stat
            replaced = False

            def replace_before_snapshot_recheck(name, *args, **kwargs):
                nonlocal replaced
                if (
                    not replaced
                    and name == old_name
                    and kwargs.get("dir_fd") is not None
                    and kwargs.get("follow_symlinks") is False
                ):
                    # Inject before lstat; this does not test the later stat/unlink window.
                    old_path.rename(displaced_path)
                    replacement_path.rename(old_path)
                    replaced = True
                return original_stat(name, *args, **kwargs)

            with patch.object(evidence.os, "stat", side_effect=replace_before_snapshot_recheck):
                result = evidence.prune_archive(archive, keep=1)

            self.assertTrue(replaced)
            self.assertEqual(result["removed"], 0)
            self.assertEqual(old_path.read_bytes(), replacement_bytes)
            self.assertEqual(displaced_path.read_bytes(), old_bytes)
            self.assertEqual(recent_path.read_bytes(), recent_bytes)


    @unittest.skipUnless(os.name == "nt", "uses the NTFS anchored-directory integration")
    def test_prune_revalidates_ntfs_entry_after_candidate_replacement(self):
        import evidence_windows

        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            root = Path(temporary)
            archive = root / "archive"
            archive.mkdir()
            old = evidence.finalize_record(base_record(date="2026-11-12"))
            recent = evidence.finalize_record(base_record(date="2026-11-14"))
            old_name = f"qa-evidence-{old['record_id']}.json"
            recent_name = f"qa-evidence-{recent['record_id']}.json"
            old_path = archive / old_name
            recent_path = archive / recent_name
            old_path.write_bytes(evidence.canonical_json(old))
            recent_bytes = evidence.canonical_json(recent)
            recent_path.write_bytes(recent_bytes)
            unrelated = archive / "unrelated.bin"
            unrelated.write_bytes(b"keep")
            replacement_path = root / "replacement.bin"
            replacement_bytes = b"unvalidated replacement entry"
            replacement_path.write_bytes(replacement_bytes)
            moved_path = root / "moved-original.json"
            original_open_file = evidence_windows.Directory.open_file
            attempted = False
            mutation_denied = False
            replaced = False

            def replace_before_delete(directory, name, *, for_delete=False):
                nonlocal attempted, mutation_denied, replaced
                if for_delete and name == old_name and not attempted:
                    attempted = True
                    try:
                        old_path.rename(moved_path)
                        replacement_path.rename(old_path)
                    except OSError:
                        mutation_denied = True
                        if moved_path.exists() and not old_path.exists():
                            moved_path.rename(old_path)
                    else:
                        replaced = True
                return original_open_file(directory, name, for_delete=for_delete)

            with patch.object(evidence_windows.Directory, "open_file", new=replace_before_delete):
                result = evidence.prune_archive(archive, keep=1)

            self.assertTrue(attempted)
            self.assertEqual(recent_path.read_bytes(), recent_bytes)
            self.assertEqual(unrelated.read_bytes(), b"keep")
            if replaced:
                self.assertEqual(old_path.read_bytes(), replacement_bytes)
                self.assertTrue(moved_path.is_file())
                self.assertEqual(result["removed"], 0)
            else:
                self.assertTrue(mutation_denied)
                self.assertTrue(replacement_path.is_file())

    @unittest.skipUnless(os.name == "nt", "uses the NTFS anchored-directory integration")
    def test_prune_rejects_duplicate_windows_names_before_deletion(self):
        import evidence_windows

        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            archive = Path(temporary)
            old = evidence.finalize_record(base_record(date="2026-11-12"))
            recent = evidence.finalize_record(base_record(date="2026-11-14"))
            old_path = archive / f"qa-evidence-{old['record_id']}.json"
            recent_path = archive / f"qa-evidence-{recent['record_id']}.json"
            old_bytes = evidence.canonical_json(old)
            recent_bytes = evidence.canonical_json(recent)
            old_path.write_bytes(old_bytes)
            recent_path.write_bytes(recent_bytes)
            original_names = evidence_windows.Directory.names

            def duplicate_names(directory, limit):
                names = original_names(directory, limit)
                return names + names[:1]

            with patch.object(evidence_windows.Directory, "names", new=duplicate_names):
                with self.assertRaisesRegex(evidence.EvidenceError, "changed during enumeration"):
                    evidence.prune_archive(archive, keep=1)

            self.assertEqual(old_path.read_bytes(), old_bytes)
            self.assertEqual(recent_path.read_bytes(), recent_bytes)

    def test_grid_diff_retention_uses_modification_time_for_reviewed_and_unreviewed(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            archive = Path(temporary)
            golden = json.loads(GOLDEN.read_text(encoding="utf-8"))
            reviewed = evidence.grid_diff(
                {"grid": ["....", "|##|", "|..|", "+--+"]},
                golden,
                "qa-reviewer",
                "2026-11-14",
            )
            unreviewed = evidence.grid_diff(
                {"grid": ["+--+", "|#.|", "|..|", "+--+"]},
                golden,
            )
            reviewed_path = archive / f"qa-grid-diff-{reviewed['artifact_id']}.json"
            unreviewed_path = archive / f"qa-grid-diff-{unreviewed['artifact_id']}.json"
            reviewed_path.write_bytes(evidence.canonical_json(reviewed))
            unreviewed_path.write_bytes(evidence.canonical_json(unreviewed))
            os.utime(reviewed_path, ns=(1_700_000_000_000_000_000, 1_700_000_000_000_000_000))
            os.utime(unreviewed_path, ns=(1_800_000_000_000_000_000, 1_800_000_000_000_000_000))

            if os.name == "posix":
                with self.assertRaisesRegex(evidence.EvidenceError, "atomic compare-and-unlink"):
                    evidence.prune_archive(archive, keep=1)
                self.assertTrue(reviewed_path.is_file())
            else:
                result = evidence.prune_archive(archive, keep=1)
                self.assertEqual(result["removed"], 1)
                self.assertFalse(reviewed_path.exists())
            self.assertTrue(unreviewed_path.exists())

    def test_retention_bounds_reports_diffs_and_sanitized_logs_separately(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            archive = Path(temporary)
            controls = [evidence.fake_control(name) for name in ("timestamp-fault", "surface-failure")]
            for report in controls:
                (archive / f"qa-control-{report['artifact_id']}.json").write_bytes(evidence.canonical_json(report))
            golden = json.loads(GOLDEN.read_text(encoding="utf-8"))
            diffs = [
                evidence.grid_diff({"grid": golden["grid"]}, golden),
                evidence.grid_diff({"grid": ["....", "|##|", "|..|", "+--+"]}, golden),
            ]
            for report in diffs:
                (archive / f"qa-grid-diff-{report['artifact_id']}.json").write_bytes(evidence.canonical_json(report))
            logs = []
            for index in range(2):
                source = archive / f"source-{index}.log"
                source.write_text(f"safe diagnostic {index}", encoding="utf-8")
                logs.append(evidence.sanitize_log(source))
            for report in logs:
                (archive / f"qa-log-{report['artifact_id']}.json").write_bytes(evidence.canonical_json(report))
            unrelated = archive / "unrelated.bin"
            unrelated.write_bytes(b"keep")

            if os.name == "posix":
                with self.assertRaisesRegex(evidence.EvidenceError, "atomic compare-and-unlink"):
                    evidence.prune_archive(archive, keep=1)
                for prefix in ("qa-control-", "qa-grid-diff-", "qa-log-"):
                    self.assertEqual(len(list(archive.glob(f"{prefix}*.json"))), 2)
            else:
                result = evidence.prune_archive(archive, keep=1)
                self.assertEqual(result["removed"], 3)
                self.assertEqual(set(result["kept_by_type"]), {"evidence", "control", "grid-diff", "log"})
                for prefix in ("qa-control-", "qa-grid-diff-", "qa-log-"):
                    self.assertEqual(len(list(archive.glob(f"{prefix}*.json"))), 1)
            self.assertTrue(unrelated.is_file())
            self.assertEqual(unrelated.read_bytes(), b"keep")

    def test_prune_limit_is_bounded_and_requires_existing_archive(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as temporary:
            with self.assertRaises(evidence.EvidenceError):
                evidence.prune_archive(Path(temporary), 0)
            with self.assertRaises(evidence.EvidenceError):
                evidence.prune_archive(Path(temporary), evidence.MAX_ARCHIVE_KEEP + 1)
            with self.assertRaises(evidence.EvidenceError):
                evidence.prune_archive(Path(temporary) / "missing", 1)


if __name__ == "__main__":
    unittest.main()
