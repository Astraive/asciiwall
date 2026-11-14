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

ROOT = Path(__file__).resolve().parents[1]
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


class EvidenceRecordTests(unittest.TestCase):
    def test_record_identity_and_canonical_encoding_are_deterministic(self):
        first = evidence.finalize_record(base_record())
        reversed_input = dict(reversed(list(base_record().items())))
        second = evidence.finalize_record(reversed_input)
        self.assertEqual(first, second)
        self.assertEqual(evidence.canonical_json(first), evidence.canonical_json(second))
        self.assertEqual(first["record_id"], evidence.record_identity(first))

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
            "summary": 'window_title="Tax return" input_value=secret media_path=C:\\Users\\Alice\\clip.mp4 from /home/alice/private.png',
        })
        clean = evidence.finalize_record(raw)
        summary = clean["result"]["summary"]
        self.assertNotIn("Tax return", summary)
        self.assertNotIn("secret", summary)
        self.assertNotIn("Alice", summary)
        self.assertNotIn("/home/alice", summary)
        self.assertFalse(evidence.has_private_text(summary))
        extra_private_field = base_record(window_title="private")
        with self.assertRaises(evidence.EvidenceError):
            evidence.finalize_record(extra_private_field)

    def test_record_rejects_path_traversal_and_invalid_calendar_date(self):
        with self.assertRaises(evidence.EvidenceError):
            evidence.finalize_record(base_record(evidence_path="../../private.json"))
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
        for level in levels:
            with self.subTest(level=level):
                native = level == "native-manual"
                record = evidence.finalize_record(base_record(
                    level=level,
                    execution="manual" if native else "automated",
                    procedure_id="NATIVE-001" if native else None,
                ))
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
                '{"window_title":"Tax return","input_value":"typed private text",'
                '"media_path":"C:\\\\Users\\\\Alice\\\\clip.mp4"}\n',
                encoding="utf-8",
            )
            report = evidence.sanitize_log(source)
            self.assertTrue(evidence.validate_log_report(report))
            content = report["content"]
            for private_value in ("Tax return", "typed private text", "Alice", "/home/alice", "\x1b"):
                self.assertNotIn(private_value, content)

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
    def test_prune_removes_only_old_valid_tool_records(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary)
            old = evidence.finalize_record(base_record(date="2026-11-12"))
            recent = evidence.finalize_record(base_record(date="2026-11-14"))
            for record in (old, recent):
                path = archive / f"qa-evidence-{record['record_id']}.json"
                path.write_bytes(evidence.canonical_json(record))
            unrelated = archive / "unrelated.json"
            unrelated.write_text("keep me", encoding="utf-8")
            malformed = archive / ("qa-evidence-" + "0" * 64 + ".json")
            malformed.write_text("not an evidence record", encoding="utf-8")
            result = evidence.prune_archive(archive, keep=1)
            self.assertEqual(result["removed"], 1)
            self.assertEqual(result["removed_names"], [f"qa-evidence-{old['record_id']}.json"])
            self.assertFalse((archive / f"qa-evidence-{old['record_id']}.json").exists())
            self.assertTrue((archive / f"qa-evidence-{recent['record_id']}.json").exists())
            self.assertEqual(unrelated.read_text(encoding="utf-8"), "keep me")
            self.assertTrue(malformed.exists())

    def test_grid_diff_retention_uses_modification_time_for_reviewed_and_unreviewed(self):
        with tempfile.TemporaryDirectory() as temporary:
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

            result = evidence.prune_archive(archive, keep=1)

            self.assertEqual(result["removed"], 1)
            self.assertFalse(reviewed_path.exists())
            self.assertTrue(unreviewed_path.exists())

    def test_retention_bounds_reports_diffs_and_sanitized_logs_separately(self):
        with tempfile.TemporaryDirectory() as temporary:
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

            result = evidence.prune_archive(archive, keep=1)

            self.assertEqual(result["removed"], 3)
            self.assertEqual(set(result["kept_by_type"]), {"evidence", "control", "grid-diff", "log"})
            self.assertTrue(unrelated.is_file())
            self.assertEqual(unrelated.read_bytes(), b"keep")
            for prefix in ("qa-control-", "qa-grid-diff-", "qa-log-"):
                self.assertEqual(len(list(archive.glob(f"{prefix}*.json"))), 1)

    def test_prune_limit_is_bounded_and_requires_existing_archive(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(evidence.EvidenceError):
                evidence.prune_archive(Path(temporary), 0)
            with self.assertRaises(evidence.EvidenceError):
                evidence.prune_archive(Path(temporary), evidence.MAX_ARCHIVE_KEEP + 1)
            with self.assertRaises(evidence.EvidenceError):
                evidence.prune_archive(Path(temporary) / "missing", 1)


if __name__ == "__main__":
    unittest.main()
