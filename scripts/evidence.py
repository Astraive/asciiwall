#!/usr/bin/env python3
"""Offline, standard-library evidence recorder and deterministic QA controls.

Records describe evidence; they do not claim that an application or native host
was exercised. Native/manual records are explicitly distinct from automated runs.
"""
from __future__ import annotations

import argparse
from datetime import date
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "tests" / "evidence" / "evidence-record.schema.json"
GOLDEN_PATH = ROOT / "tests" / "evidence" / "goldens" / "cpu-grid-v1.json"
MAX_JSON_BYTES = 1_048_576
MAX_LOG_BYTES = 262_144
MAX_GRID_CELLS = 4096
MAX_ARCHIVE_KEEP = 1000
CONTROL_LIMIT = 2
LEVELS = {
    "document-qa", "unit-property", "integration", "fuzz", "benchmark",
    "native-manual", "release-acceptance",
}
HASH_RE = re.compile(r"^[0-9a-f]{64}$")
EC_RE = re.compile(r"^EC-[0-9]{3}$")
ISSUE_RE = re.compile(r"^[A-Z][A-Z0-9]+-[A-Z0-9-]+$")
REVIEWER_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$")
PROCEDURE_RE = re.compile(r"^[A-Z0-9][A-Z0-9._-]{0,79}$")
ARCHIVE_RECORD_RE = re.compile(r"^qa-evidence-([0-9a-f]{64})\.json$")
ARCHIVE_CONTROL_RE = re.compile(r"^qa-control-([0-9a-f]{64})\.json$")
ARCHIVE_DIFF_RE = re.compile(r"^qa-grid-diff-([0-9a-f]{64})\.json$")
ARCHIVE_LOG_RE = re.compile(r"^qa-log-([0-9a-f]{64})\.json$")
GRID_CHARS = frozenset(" .#*+|-/\\")

# These filters operate only on free-text metadata, never on the structured,
# repository-relative evidence_path. They intentionally redact rather than log
# the sensitive value or the path itself.
_PRIVATE_LABEL_RE = re.compile(
    r"(?i)\b(window(?:[ _-]*[a-z0-9]+)*|input(?:[ _-]*[a-z0-9]+)*|"
    r"media(?:[ _-]*[a-z0-9]+)*|pointer(?:[ _-]*[a-z0-9]+)*)"
    r"""["']?\s*[:=]\s*(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^,;{}\r\n]+)"""
)
_PRIVATE_PATH_RE = re.compile(
    r"(?<![A-Za-z0-9/])(?:[A-Za-z]:[\\/][^\s,;\"']*|\\\\[^\\/\s]+[\\/][^\s,;\"']*|"
    r"(?<![A-Za-z0-9:/])/(?!/)[^\s,;\"']+)"
)
def _skip_control_string(value: str, index: int) -> int:
    while index < len(value):
        character = value[index]
        if character in {"\x07", "\x9c"}:
            return index + 1
        if character == "\x1b" and index + 1 < len(value) and value[index + 1] == "\\":
            return index + 2
        index += 1
    return len(value)


def _strip_terminal_controls(value: str) -> str:
    output: list[str] = []
    index = 0
    while index < len(value):
        character = value[index]
        codepoint = ord(character)
        if character == "\x1b":
            index += 1
            if index >= len(value):
                break
            introducer = value[index]
            if introducer == "[":
                index += 1
                while index < len(value):
                    final = ord(value[index])
                    index += 1
                    if 0x40 <= final <= 0x7E:
                        break
                continue
            if introducer in "]P^_X":
                index = _skip_control_string(value, index + 1)
                continue
            index += 1
            while index < len(value) and 0x20 <= ord(value[index]) <= 0x2F:
                index += 1
            if index < len(value) and 0x30 <= ord(value[index]) <= 0x7E:
                index += 1
            continue
        if codepoint == 0x9B:
            index += 1
            while index < len(value):
                final = ord(value[index])
                index += 1
                if 0x40 <= final <= 0x7E:
                    break
            continue
        if codepoint in {0x90, 0x98, 0x9D, 0x9E, 0x9F}:
            index = _skip_control_string(value, index + 1)
            continue
        if codepoint == 0x9C:
            index += 1
            continue
        output.append(character)
        index += 1
    return "".join(output)


def _is_log_character(character: str) -> bool:
    codepoint = ord(character)
    return character in "\n\t" or not (codepoint < 32 or 0x7F <= codepoint <= 0x9F)


class EvidenceError(ValueError):
    """Expected, user-correctable evidence-tool failure."""


def canonical_json(value: Any) -> bytes:
    try:
        encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n"
        return encoded.encode("utf-8")
    except UnicodeEncodeError as exc:
        raise EvidenceError("JSON contains invalid Unicode text") from exc


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def record_identity(record: dict[str, Any]) -> str:
    material = dict(record)
    material.pop("record_id", None)
    return sha256_bytes(canonical_json(material))


def artifact_identity(report: dict[str, Any]) -> str:
    material = dict(report)
    material.pop("artifact_id", None)
    return sha256_bytes(canonical_json(material))


def _artifact_identity_matches(report: dict[str, Any]) -> bool:
    identity = report.get("artifact_id")
    if not isinstance(identity, str) or not HASH_RE.fullmatch(identity):
        return False
    try:
        return identity == artifact_identity(report)
    except EvidenceError:
        return False


def reject_constant(value: str) -> None:
    raise EvidenceError(f"non-finite JSON number is not allowed ({value})")


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise EvidenceError("duplicate JSON object key")
        result[key] = value
    return result


def read_json(path: Path) -> Any:
    try:
        with path.open("rb") as stream:
            raw = stream.read(MAX_JSON_BYTES + 1)
    except OSError as exc:
        raise EvidenceError("cannot read JSON input") from exc
    if len(raw) > MAX_JSON_BYTES:
        raise EvidenceError("JSON input exceeds the 1 MiB limit")
    try:
        return json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object, parse_constant=reject_constant)
    except (UnicodeError, json.JSONDecodeError, RecursionError) as exc:
        raise EvidenceError("input is not valid UTF-8 JSON") from exc

_RECORD_FIELDS = {
    "schema_version", "record_id", "level", "execution", "procedure_id", "ec_ids",
    "source_hash", "binary_hash", "fixture_hash", "environment", "permissions",
    "result", "evidence_path", "issue", "reviewer", "date",
}
_SCHEMA_CACHE: dict[str, Any] | None = None


def load_record_schema() -> dict[str, Any]:
    global _SCHEMA_CACHE
    if _SCHEMA_CACHE is not None:
        return _SCHEMA_CACHE
    schema = read_json(SCHEMA_PATH)
    if not isinstance(schema, dict):
        raise EvidenceError("evidence schema must be a JSON object")
    properties = schema.get("properties")
    required = schema.get("required")
    level_schema = properties.get("level") if isinstance(properties, dict) else None
    if (
        schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
        or schema.get("type") != "object"
        or schema.get("additionalProperties") is not False
        or not isinstance(properties, dict)
        or not isinstance(required, list)
        or any(not isinstance(item, str) for item in required)
        or set(required) != _RECORD_FIELDS
        or set(properties) != _RECORD_FIELDS
        or not isinstance(level_schema, dict)
        or not isinstance(level_schema.get("enum"), list)
        or any(not isinstance(item, str) for item in level_schema["enum"])
        or set(level_schema["enum"]) != LEVELS
    ):
        raise EvidenceError("evidence schema contract is incompatible with the recorder")
    _SCHEMA_CACHE = schema
    return schema


def sanitize_text(value: str) -> str:
    value = _PRIVATE_LABEL_RE.sub("[REDACTED-FIELD]", value)
    return _PRIVATE_PATH_RE.sub("[REDACTED-PATH]", value)


def has_private_text(value: str) -> bool:
    return bool(_PRIVATE_LABEL_RE.search(value) or _PRIVATE_PATH_RE.search(value))


def sanitize_record(record: Any) -> Any:
    if not isinstance(record, dict):
        return record
    cleaned = dict(record)
    environment = cleaned.get("environment")
    if isinstance(environment, dict):
        environment = dict(environment)
        for key in ("os", "session", "compositor", "gpu"):
            if isinstance(environment.get(key), str):
                environment[key] = sanitize_text(environment[key])
        cleaned["environment"] = environment
    result = cleaned.get("result")
    if isinstance(result, dict):
        result = dict(result)
        if isinstance(result.get("summary"), str):
            result["summary"] = sanitize_text(result["summary"])
        cleaned["result"] = result
    return cleaned


def valid_date(value: Any) -> bool:
    if not isinstance(value, str) or not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", value):
        return False
    try:
        date.fromisoformat(value)
    except ValueError:
        return False
    return True


def _nonempty_text(value: Any, maximum: int = 200) -> bool:
    return isinstance(value, str) and 0 < len(value) <= maximum and value.strip() == value and not any(
        ord(ch) < 32 or 0x7F <= ord(ch) <= 0x9F for ch in value
    )


def _safe_relative_evidence_path(value: Any) -> bool:
    if not isinstance(value, str) or not value or "\\" in value or value.startswith(("/", "~")) or ":" in value:
        return False
    path = PurePosixPath(value)
    return (
        not path.is_absolute()
        and all(part not in ("", ".", "..") for part in value.split("/"))
        and path.as_posix() == value
        and re.fullmatch(r"[A-Za-z0-9._/-]+", value) is not None
    )


def validate_record(record: Any, *, check_identity: bool = True) -> list[str]:
    errors: list[str] = []
    if not isinstance(record, dict):
        return ["record must be a JSON object"]
    try:
        load_record_schema()
    except EvidenceError:
        return ["evidence schema is missing or incompatible with the recorder"]
    expected = _RECORD_FIELDS
    if record.keys() != expected:
        missing = expected - record.keys()
        extra = record.keys() - expected
        if missing:
            errors.append("missing required fields: " + ", ".join(sorted(missing)))
        if extra:
            errors.append("unknown fields are not permitted")
    if record.get("schema_version") != 1 or isinstance(record.get("schema_version"), bool):
        errors.append("schema_version must be 1")
    level = record.get("level")
    if not isinstance(level, str) or level not in LEVELS:
        errors.append("level must name one of the seven evidence levels")
    execution = record.get("execution")
    if not isinstance(execution, str) or execution not in {"automated", "manual"}:
        errors.append("execution must be automated or manual")
    if level == "native-manual":
        if execution != "manual":
            errors.append("native-manual evidence must use manual execution")
        if not isinstance(record.get("procedure_id"), str) or not PROCEDURE_RE.fullmatch(record["procedure_id"]):
            errors.append("native-manual evidence requires a valid procedure_id")
    else:
        if execution != "automated":
            errors.append("automated evidence levels must use automated execution")
        if record.get("procedure_id") is not None:
            errors.append("procedure_id is only valid for native-manual evidence")
    ec_ids = record.get("ec_ids")
    if not isinstance(ec_ids, list) or any(not isinstance(item, str) or not EC_RE.fullmatch(item) for item in ec_ids):
        errors.append("ec_ids must be an array of EC-### identifiers (empty when not applicable)")
    elif len(set(ec_ids)) != len(ec_ids):
        errors.append("ec_ids must not contain duplicates")
    for key in ("source_hash", "binary_hash", "fixture_hash"):
        if not isinstance(record.get(key), str) or not HASH_RE.fullmatch(record[key]):
            errors.append(f"{key} must be a lowercase SHA-256 hex digest")
    environment = record.get("environment")
    env_keys = {"availability", "os", "session", "compositor", "gpu"}
    if not isinstance(environment, dict) or environment.keys() != env_keys:
        errors.append("environment must contain availability, os, session, compositor, and gpu only")
        environment = {}
    availability = environment.get("availability")
    if not isinstance(availability, str) or availability not in {"present", "absent"}:
        errors.append("environment.availability must be present or absent")
    for key in ("os", "session", "compositor", "gpu"):
        value = environment.get(key)
        if not _nonempty_text(value, 120):
            errors.append(f"environment.{key} must be a non-empty bounded string")
        elif has_private_text(value):
            errors.append(f"environment.{key} contains unredacted private text")
        if isinstance(value, str) and availability == "present" and value in {"", "not-available", "unknown", "n/a"}:
            errors.append(f"present environment requires an observed environment.{key} value")
    if environment.get("availability") == "absent" and any(
        environment.get(key) != "not-available" for key in ("os", "session", "compositor", "gpu")
    ):
        errors.append("absent environments must use not-available for all environment details")
    permissions = record.get("permissions")
    if not isinstance(permissions, dict) or permissions.keys() != {"state", "items"}:
        errors.append("permissions must contain state and items only")
        permissions = {}
    permission_state = permissions.get("state")
    if not isinstance(permission_state, str) or permission_state not in {"granted", "denied", "not-required", "unavailable", "unknown"}:
        errors.append("permissions.state is invalid")
    items = permissions.get("items")
    if not isinstance(items, list) or any(
        not isinstance(item, str) or not re.fullmatch(r"[a-z][a-z0-9-]{0,39}", item) for item in items
    ):
        errors.append("permissions.items must be safe lowercase permission identifiers")
    result = record.get("result")
    if not isinstance(result, dict) or result.keys() != {"status", "summary"}:
        errors.append("result must contain status and summary only")
        result = {}
    status = result.get("status")
    if not isinstance(status, str) or status not in {"pass", "fail", "not-run"}:
        errors.append("result.status must be pass, fail, or not-run")
    summary = result.get("summary")
    if not _nonempty_text(summary, 500):
        errors.append("result.summary must be a non-empty bounded string")
    elif has_private_text(summary):
        errors.append("result.summary contains unredacted private text")
    if environment.get("availability") == "absent" and status != "not-run":
        errors.append("absent environments must be recorded as not-run")
    if record.get("level") == "native-manual" and environment.get("availability") == "absent" and status != "not-run":
        errors.append("unavailable native/manual environments can never pass")
    if not _safe_relative_evidence_path(record.get("evidence_path")):
        errors.append("evidence_path must be a safe repository-relative path")
    if not isinstance(record.get("issue"), str) or not ISSUE_RE.fullmatch(record["issue"]):
        errors.append("issue must be a ticket identifier")
    if not isinstance(record.get("reviewer"), str) or not REVIEWER_RE.fullmatch(record["reviewer"]):
        errors.append("reviewer must be a non-empty reviewer identifier without personal path data")
    if not valid_date(record.get("date")):
        errors.append("date must be a valid ISO-8601 calendar date (YYYY-MM-DD)")
    record_id = record.get("record_id")
    if not isinstance(record_id, str) or not HASH_RE.fullmatch(record_id):
        errors.append("record_id must be a lowercase SHA-256 hex digest")
    elif check_identity:
        try:
            identity_matches = record_id == record_identity(record)
        except EvidenceError:
            identity_matches = False
        if not identity_matches:
            errors.append("record_id does not match the canonical record content")
    return errors


def finalize_record(value: Any) -> dict[str, Any]:
    record = sanitize_record(value)
    if not isinstance(record, dict):
        raise EvidenceError("record input must be a JSON object")
    has_supplied_id = "record_id" in record
    supplied_id = record.pop("record_id", None)
    generated_id = record_identity(record)
    if has_supplied_id and supplied_id != generated_id:
        raise EvidenceError("supplied record_id does not match canonical record content")
    record["record_id"] = generated_id
    errors = validate_record(record)
    if errors:
        raise EvidenceError("; ".join(errors))
    return record


def write_new(path: Path, content: bytes) -> None:
    if not path.parent.is_dir():
        raise EvidenceError("output directory does not exist")
    try:
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(content)
    except FileExistsError as exc:
        raise EvidenceError("refusing to overwrite an existing output") from exc
    except OSError as exc:
        raise EvidenceError("cannot write output") from exc


def emit(value: Any) -> None:
    sys.stdout.buffer.write(canonical_json(value))


def load_record(path: Path) -> dict[str, Any]:
    data = read_json(path)
    errors = validate_record(data)
    if errors:
        raise EvidenceError("; ".join(errors))
    return data


class FakeDecoder:
    """Tiny bounded timestamped queue used only by synthetic evidence controls."""

    def __init__(self, queue_limit: int = 2) -> None:
        self.queue_limit = queue_limit
        self.queue: list[tuple[str, int, int]] = []
        self.queue_peak = 0
        self.last_timestamp: int | None = None

    def submit(self, frame: str, timestamp: int, generation: int) -> bool:
        if self.last_timestamp is not None and timestamp <= self.last_timestamp:
            return False
        if len(self.queue) >= self.queue_limit:
            return False
        self.last_timestamp = timestamp
        self.queue.append((frame, timestamp, generation))
        self.queue_peak = max(self.queue_peak, len(self.queue))
        return True

    def drain(self, generation: int) -> tuple[list[str], int]:
        output: list[str] = []
        stale = 0
        while self.queue:
            frame, _, queued_generation = self.queue.pop(0)
            if queued_generation != generation:
                stale += 1
            else:
                output.append(frame)
        return output, stale


class FakeHost:
    """Fake surface owner with bounded resource and generation state."""

    def __init__(self, *, surface_available: bool = True, resource_limit: int = 2) -> None:
        self.surface_available = surface_available
        self.resource_limit = resource_limit
        self.resources = 0
        self.resource_peak = 0
        self.generation = 1
        self.generation_peak = 1

    def create_surface(self) -> bool:
        if not self.surface_available or self.resources >= self.resource_limit:
            return False
        self.resources += 1
        self.resource_peak = max(self.resource_peak, self.resources)
        return True

    def change_capabilities(self) -> None:
        self.generation += 1
        self.generation_peak = max(self.generation_peak, self.generation)

    def cancel(self) -> None:
        self.generation += 1
        self.generation_peak = max(self.generation_peak, self.generation)

    def release_surface(self) -> None:
        self.resources = max(0, self.resources - 1)


def fake_control(scenario: str) -> dict[str, Any]:
    """Run deterministic fake decoder/host fault controls; never exercises product code."""
    if not isinstance(scenario, str) or scenario not in {"timestamp-fault", "capability-change", "surface-failure", "cancellation"}:
        raise EvidenceError("unknown synthetic fault scenario")
    queue_limit = resource_limit = generation_limit = CONTROL_LIMIT
    decoder = FakeDecoder(queue_limit)
    host = FakeHost(resource_limit=resource_limit)
    stale_outputs = 0
    safely_rejected = False

    if scenario == "timestamp-fault":
        fault = "nonmonotonic-timestamp"
        host.create_surface()
        decoder.submit("frame-0", 0, host.generation)
        output, stale_outputs = decoder.drain(host.generation)
        safely_rejected = not decoder.submit("frame-bad-timestamp", -1, host.generation)
        final_output = output[-1] if output else "no-output"
        expected_output = "frame-0"
    elif scenario == "capability-change":
        fault = "host-capability-change"
        host.create_surface()
        decoder.submit("frame-generation-1", 0, host.generation)
        host.change_capabilities()
        _, stale_outputs = decoder.drain(host.generation)
        decoder.submit("frame-generation-2", 1, host.generation)
        output, additional_stale = decoder.drain(host.generation)
        stale_outputs += additional_stale
        safely_rejected = stale_outputs >= 1
        final_output = output[-1] if output else "no-output"
        expected_output = "frame-generation-2"
    elif scenario == "surface-failure":
        fault = "surface-creation-failure"
        host.surface_available = False
        safely_rejected = not host.create_surface()
        final_output = "cpu-grid-fallback"
        expected_output = "cpu-grid-fallback"
    else:
        fault = "generation-cancelled"
        host.create_surface()
        decoder.submit("frame-cancelled-generation", 0, host.generation)
        host.cancel()
        _, stale_outputs = decoder.drain(host.generation)
        safely_rejected = stale_outputs == 1
        final_output = "cancelled-no-output"
        expected_output = "cancelled-no-output"

    host.release_surface()
    checks = {
        "fault_safely_handled": safely_rejected and (
            scenario not in {"capability-change", "cancellation"} or stale_outputs >= 1
        ),
        "resources_bounded": host.resource_peak <= resource_limit,
        "queue_bounded": decoder.queue_peak <= queue_limit,
        "generation_bounded": host.generation_peak <= generation_limit,
        "final_output_matches": final_output == expected_output,
    }
    report: dict[str, Any] = {
        "schema_version": 1,
        "artifact_type": "synthetic-control-report",
        "scenario": scenario,
        "scope": "synthetic-control-only",
        "claims_product_behavior": False,
        "fault": fault,
        "resource_peak": host.resource_peak,
        "resource_limit": resource_limit,
        "queue_peak": decoder.queue_peak,
        "queue_limit": queue_limit,
        "generation_peak": host.generation_peak,
        "generation_limit": generation_limit,
        "generation": host.generation,
        "stale_outputs": stale_outputs,
        "safely_rejected": safely_rejected,
        "final_output": final_output,
        "expected_output": expected_output,
        "checks": checks,
        "status": "pass" if all(checks.values()) else "fail",
    }
    report["artifact_id"] = artifact_identity(report)
    return report


def validate_control_report(report: Any) -> bool:
    if not isinstance(report, dict) or report.get("artifact_type") != "synthetic-control-report":
        return False
    required = {
        "schema_version", "artifact_type", "scenario", "scope", "claims_product_behavior", "fault",
        "resource_peak", "resource_limit", "queue_peak", "queue_limit", "generation_peak",
        "generation_limit", "generation", "stale_outputs", "safely_rejected", "final_output",
        "expected_output", "checks", "status", "artifact_id",
    }
    if report.keys() != required or report.get("schema_version") != 1 or isinstance(report.get("schema_version"), bool):
        return False
    scenario = report.get("scenario")
    if not isinstance(scenario, str) or scenario not in {"timestamp-fault", "capability-change", "surface-failure", "cancellation"}:
        return False
    expected_fault, expected_output = {
        "timestamp-fault": ("nonmonotonic-timestamp", "frame-0"),
        "capability-change": ("host-capability-change", "frame-generation-2"),
        "surface-failure": ("surface-creation-failure", "cpu-grid-fallback"),
        "cancellation": ("generation-cancelled", "cancelled-no-output"),
    }[scenario]
    if report.get("fault") != expected_fault or report.get("expected_output") != expected_output:
        return False
    if any(report.get(key) != CONTROL_LIMIT for key in ("resource_limit", "queue_limit", "generation_limit")):
        return False
    final_output = report.get("final_output")
    if not isinstance(final_output, str) or not re.fullmatch(r"[a-z0-9-]{1,80}", final_output):
        return False
    if report.get("scope") != "synthetic-control-only" or report.get("claims_product_behavior") is not False:
        return False
    if not isinstance(report.get("safely_rejected"), bool):
        return False
    numeric = (
        "resource_peak", "resource_limit", "queue_peak", "queue_limit",
        "generation_peak", "generation_limit", "generation", "stale_outputs",
    )
    if any(not isinstance(report.get(key), int) or isinstance(report.get(key), bool) or report[key] < 0 for key in numeric):
        return False
    checks = report.get("checks")
    check_names = {
        "fault_safely_handled", "resources_bounded", "queue_bounded",
        "generation_bounded", "final_output_matches",
    }
    if not isinstance(checks, dict) or checks.keys() != check_names or any(not isinstance(value, bool) for value in checks.values()):
        return False
    expected_checks = {
        "fault_safely_handled": report["safely_rejected"] and (
            scenario not in {"capability-change", "cancellation"} or report["stale_outputs"] >= 1
        ),
        "resources_bounded": report["resource_peak"] <= report["resource_limit"],
        "queue_bounded": report["queue_peak"] <= report["queue_limit"],
        "generation_bounded": report["generation_peak"] <= report["generation_limit"],
        "final_output_matches": report.get("final_output") == report.get("expected_output"),
    }
    if checks != expected_checks or report["generation"] > report["generation_limit"]:
        return False
    if report.get("status") != ("pass" if all(checks.values()) else "fail"):
        return False
    return _artifact_identity_matches(report)


def _validate_grid(value: Any, label: str) -> list[str]:
    if isinstance(value, dict):
        value = value.get("grid")
    if not isinstance(value, list) or not value or len(value) > MAX_GRID_CELLS:
        raise EvidenceError(f"{label} must be a non-empty grid of at most {MAX_GRID_CELLS} cells")
    rows: list[str] = []
    width: int | None = None
    cells = 0
    for row in value:
        if not isinstance(row, str) or not row or any(char not in GRID_CHARS for char in row):
            raise EvidenceError(f"{label} rows must use the restricted deterministic grid alphabet")
        if width is None:
            width = len(row)
        if len(row) != width:
            raise EvidenceError(f"{label} must be rectangular")
        cells += len(row)
        if cells > MAX_GRID_CELLS:
            raise EvidenceError(f"{label} exceeds the {MAX_GRID_CELLS}-cell limit")
        rows.append(row)
    return rows


def grid_diff(actual_value: Any, golden_value: Any, reviewer: str | None = None, review_date: str | None = None) -> dict[str, Any]:
    if not isinstance(golden_value, dict) or not isinstance(golden_value.get("fixture_id"), str) or not re.fullmatch(
        r"[a-z0-9][a-z0-9._-]{0,79}", golden_value["fixture_id"]
    ):
        raise EvidenceError("golden fixture_id must be a safe lowercase identifier")
    expected = _validate_grid(golden_value.get("grid"), "golden")
    actual = _validate_grid(actual_value.get("grid") if isinstance(actual_value, dict) else actual_value, "actual grid")
    if (reviewer is None) != (review_date is None):
        raise EvidenceError("reviewer and review-date must be supplied together")
    if reviewer is not None and (
        not isinstance(reviewer, str) or not REVIEWER_RE.fullmatch(reviewer) or not valid_date(review_date)
    ):
        raise EvidenceError("review metadata must contain a safe reviewer identifier and valid date")
    changes: list[dict[str, Any]] = []
    height = max(len(expected), len(actual))
    width = max(max(map(len, expected)), max(map(len, actual)))
    if height * width > MAX_GRID_CELLS:
        raise EvidenceError("combined grid comparison exceeds the cell limit")
    for y in range(height):
        for x in range(width):
            before = expected[y][x] if y < len(expected) and x < len(expected[y]) else None
            after = actual[y][x] if y < len(actual) and x < len(actual[y]) else None
            if before != after:
                changes.append({"x": x, "y": y, "expected": before, "actual": after})
    report: dict[str, Any] = {
        "schema_version": 1,
        "artifact_type": "cpu-grid-diff",
        "fixture_id": golden_value["fixture_id"],
        "fixture_sha256": sha256_bytes(canonical_json(golden_value)),
        "expected_grid_sha256": sha256_bytes(canonical_json(expected)),
        "actual_grid_sha256": sha256_bytes(canonical_json(actual)),
        "matches": not changes,
        "review": {
            "status": "reviewed" if reviewer is not None and changes else "unreviewed",
            "reviewer": reviewer if reviewer is not None and changes else None,
            "date": review_date if reviewer is not None and changes else None,
        },
        "changes": changes,
    }
    report["artifact_id"] = artifact_identity(report)
    return report


def validate_grid_diff_report(report: Any) -> bool:
    if not isinstance(report, dict) or report.get("artifact_type") != "cpu-grid-diff":
        return False
    if report.get("schema_version") != 1 or isinstance(report.get("schema_version"), bool) or report.keys() != {
        "schema_version", "artifact_type", "fixture_id", "fixture_sha256", "expected_grid_sha256",
        "actual_grid_sha256", "matches", "review", "changes", "artifact_id",
    }:
        return False
    fixture_id = report.get("fixture_id")
    if not isinstance(fixture_id, str) or not re.fullmatch(r"[a-z0-9][a-z0-9._-]{0,79}", fixture_id):
        return False
    if any(not isinstance(report.get(key), str) or not HASH_RE.fullmatch(report[key]) for key in (
        "fixture_sha256", "expected_grid_sha256", "actual_grid_sha256"
    )):
        return False
    changes = report.get("changes")
    review = report.get("review")
    if not isinstance(changes, list) or not isinstance(review, dict) or review.keys() != {"status", "reviewer", "date"}:
        return False
    if not isinstance(report.get("matches"), bool) or report["matches"] is not (len(changes) == 0):
        return False
    for change in changes:
        if not isinstance(change, dict) or change.keys() != {"x", "y", "expected", "actual"}:
            return False
        if any(not isinstance(change.get(key), int) or isinstance(change[key], bool) or change[key] < 0 for key in ("x", "y")):
            return False
        for key in ("expected", "actual"):
            value = change.get(key)
            if value is not None and (not isinstance(value, str) or len(value) != 1 or value not in GRID_CHARS):
                return False
    if review.get("status") == "reviewed":
        if not changes or not isinstance(review.get("reviewer"), str) or not REVIEWER_RE.fullmatch(review["reviewer"]) or not valid_date(review.get("date")):
            return False
    elif review != {"status": "unreviewed", "reviewer": None, "date": None}:
        return False
    return _artifact_identity_matches(report)


def sanitize_log(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as stream:
            raw = stream.read(MAX_LOG_BYTES + 1)
    except OSError as exc:
        raise EvidenceError("cannot read log input") from exc
    if len(raw) > MAX_LOG_BYTES:
        raise EvidenceError("log input exceeds the 256 KiB limit")
    try:
        text = raw.decode("utf-8")
    except UnicodeError as exc:
        raise EvidenceError("log input is not valid UTF-8") from exc
    text = _strip_terminal_controls(text).replace("\r\n", "\n").replace("\r", "\n")
    text = "".join(character for character in text if _is_log_character(character))
    content = "\n".join(sanitize_text(line) for line in text.split("\n"))
    if len(content.encode("utf-8")) > MAX_LOG_BYTES:
        raise EvidenceError("sanitized log exceeds the 256 KiB limit")
    if has_private_text(content):
        raise EvidenceError("log contains private fields that could not be redacted safely")
    report: dict[str, Any] = {
        "schema_version": 1,
        "artifact_type": "sanitized-log",
        "content_sha256": sha256_bytes(content.encode("utf-8")),
        "content": content,
    }
    report["artifact_id"] = artifact_identity(report)
    return report


def validate_log_report(report: Any) -> bool:
    if not isinstance(report, dict) or report.get("artifact_type") != "sanitized-log":
        return False
    if report.keys() != {"schema_version", "artifact_type", "content_sha256", "content", "artifact_id"}:
        return False
    if report.get("schema_version") != 1 or isinstance(report.get("schema_version"), bool):
        return False
    content = report.get("content")
    if not isinstance(content, str):
        return False
    try:
        content_bytes = content.encode("utf-8")
    except UnicodeEncodeError:
        return False
    if len(content_bytes) > MAX_LOG_BYTES or has_private_text(content):
        return False
    if any(not _is_log_character(character) for character in content):
        return False
    if not isinstance(report.get("content_sha256"), str) or report["content_sha256"] != sha256_bytes(content_bytes):
        return False
    return _artifact_identity_matches(report)


def store_artifact(report: dict[str, Any], prefix: str, output: Path | None, archive: Path | None) -> None:
    if output is not None:
        write_new(output, canonical_json(report))
        return
    if archive is None or archive.is_symlink() or not archive.is_dir():
        raise EvidenceError("archive must be an existing non-symlink directory")
    write_new(archive / f"{prefix}-{report['artifact_id']}.json", canonical_json(report))


def prune_archive(directory: Path, keep: int) -> dict[str, Any]:
    if keep < 1 or keep > MAX_ARCHIVE_KEEP:
        raise EvidenceError(f"keep must be between 1 and {MAX_ARCHIVE_KEEP}")
    if directory.is_symlink() or not directory.is_dir():
        raise EvidenceError("archive must be an existing non-symlink directory")
    patterns = (
        ("evidence", ARCHIVE_RECORD_RE),
        ("control", ARCHIVE_CONTROL_RE),
        ("grid-diff", ARCHIVE_DIFF_RE),
        ("log", ARCHIVE_LOG_RE),
    )
    valid: dict[str, list[tuple[str, str, Path]]] = {name: [] for name, _ in patterns}
    for entry in directory.iterdir():
        if entry.is_symlink() or not entry.is_file():
            continue
        for kind, pattern in patterns:
            match = pattern.fullmatch(entry.name)
            if match is None:
                continue
            try:
                content = read_json(entry)
            except EvidenceError:
                break
            identity = match.group(1)
            if kind == "evidence":
                accepted = isinstance(content, dict) and content.get("record_id") == identity and not validate_record(content)
                sort_date = content.get("date", "") if accepted else ""
            else:
                validators = {
                    "control": validate_control_report,
                    "grid-diff": validate_grid_diff_report,
                    "log": validate_log_report,
                }
                accepted = (
                    isinstance(content, dict)
                    and content.get("artifact_id") == identity
                    and validators[kind](content)
                )
                sort_date = ""
            if accepted:
                try:
                    modified = entry.stat().st_mtime_ns
                except OSError:
                    break
                # Explicit record dates order evidence; other generated artifacts use
                # their archive write time because they have no run date.
                order = sort_date or f"{modified:020d}"
                valid[kind].append((order, identity, entry))
            break
    removed: list[str] = []
    kept: dict[str, int] = {}
    for kind, _ in patterns:
        candidates = sorted(valid[kind], key=lambda item: (item[0], item[1]), reverse=True)
        kept[kind] = min(len(candidates), keep)
        for _, _, path in candidates[keep:]:
            try:
                path.unlink()
            except OSError as exc:
                raise EvidenceError("cannot remove an expired evidence artifact") from exc
            removed.append(path.name)
    return {"kept_by_type": kept, "removed": len(removed), "removed_names": sorted(removed)}


def add_store_options(parser: argparse.ArgumentParser, *, required: bool = False) -> None:
    target = parser.add_mutually_exclusive_group(required=required)
    target.add_argument("--output", type=Path, help="write a new standalone JSON file")
    target.add_argument("--archive", type=Path, help="store the deterministic report in an existing archive")


def make_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Offline reproducible QA evidence recorder")
    subparsers = parser.add_subparsers(dest="command", required=True)
    record = subparsers.add_parser("record", help="sanitize, validate, and store an evidence record")
    record.add_argument("input", help="JSON record fields (without record_id), or - for stdin")
    add_store_options(record, required=True)
    validate = subparsers.add_parser("validate", help="validate an existing record without modifying it")
    validate.add_argument("record", type=Path)
    controls = subparsers.add_parser("fake-control", help="emit a synthetic decoder/host failure-control report")
    controls.add_argument("scenario", choices=("timestamp-fault", "capability-change", "surface-failure", "cancellation"))
    add_store_options(controls)
    diff = subparsers.add_parser("grid-diff", help="compare a restricted CPU character grid exactly")
    diff.add_argument("actual", type=Path, help="JSON object containing a grid array")
    diff.add_argument("--golden", type=Path, default=GOLDEN_PATH)
    diff.add_argument("--reviewer", help="explicit reviewer ID for a human-reviewed diff")
    diff.add_argument("--review-date", help="review date (YYYY-MM-DD); required with --reviewer")
    add_store_options(diff)
    logs = subparsers.add_parser("sanitize-log", help="redact and archive a bounded UTF-8 text log")
    logs.add_argument("input", type=Path)
    add_store_options(logs)
    prune = subparsers.add_parser("prune", help="retain bounded sets of valid tool-owned archive artifacts")
    prune.add_argument("archive", type=Path)
    prune.add_argument("--keep", type=int, default=100, help="maximum artifacts retained per artifact type (1..1000)")
    return parser


def _input_json(argument: str) -> Any:
    if argument == "-":
        raw = sys.stdin.buffer.read(MAX_JSON_BYTES + 1)
        if len(raw) > MAX_JSON_BYTES:
            raise EvidenceError("JSON input exceeds the 1 MiB limit")
        try:
            return json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object, parse_constant=reject_constant)
        except (UnicodeError, json.JSONDecodeError, RecursionError) as exc:
            raise EvidenceError("standard input is not valid UTF-8 JSON") from exc
    return read_json(Path(argument))


def main(argv: list[str] | None = None) -> int:
    args = make_parser().parse_args(argv)
    try:
        if args.command == "record":
            record = finalize_record(_input_json(args.input))
            if args.output is not None:
                write_new(args.output, canonical_json(record))
            else:
                if args.archive.is_symlink() or not args.archive.is_dir():
                    raise EvidenceError("archive must be an existing non-symlink directory")
                write_new(args.archive / f"qa-evidence-{record['record_id']}.json", canonical_json(record))
            emit({"status": "recorded", "schema_version": 1, "record_id": record["record_id"]})
            return 0
        if args.command == "validate":
            record = load_record(args.record)
            emit({"status": "valid", "schema_version": 1, "record_id": record["record_id"]})
            return 0
        if args.command == "fake-control":
            report = fake_control(args.scenario)
            if not validate_control_report(report):
                raise EvidenceError("internal synthetic control report validation failed")
            if args.output is not None or args.archive is not None:
                store_artifact(report, "qa-control", args.output, args.archive)
            emit(report)
            return 0 if report["status"] == "pass" else 1
        if args.command == "grid-diff":
            actual = read_json(args.actual)
            golden = read_json(args.golden)
            report = grid_diff(actual, golden, args.reviewer, args.review_date)
            if not validate_grid_diff_report(report):
                raise EvidenceError("internal grid diff report validation failed")
            if args.output is not None or args.archive is not None:
                store_artifact(report, "qa-grid-diff", args.output, args.archive)
            emit(report)
            return 0 if report["matches"] else 1
        if args.command == "sanitize-log":
            report = sanitize_log(args.input)
            if args.output is not None or args.archive is not None:
                store_artifact(report, "qa-log", args.output, args.archive)
            emit(report)
            return 0
        if args.command == "prune":
            emit(prune_archive(args.archive, args.keep))
            return 0
        raise EvidenceError("unknown command")
    except EvidenceError as exc:
        emit({"status": "error", "error": str(exc)})
        return 2
    except OSError:
        emit({"status": "error", "error": "filesystem operation failed"})
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
