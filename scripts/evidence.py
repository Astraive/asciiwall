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
import stat
import sys
from typing import Any, Callable

if os.name == "nt":
    if __package__:
        from . import evidence_windows
    else:
        import evidence_windows


ROOT = Path(os.path.abspath(__file__)).parents[1] if os.name == "nt" else Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "tests" / "evidence" / "evidence-record.schema.json"
GOLDEN_PATH = ROOT / "tests" / "evidence" / "goldens" / "cpu-grid-v1.json"
MAX_JSON_BYTES = 1_048_576
MAX_LOG_BYTES = 262_144
MAX_GRID_CELLS = 4096
MAX_NATIVE_ARTIFACT_BYTES = 16_777_216
MAX_NATIVE_TOTAL_BYTES = 67_108_864
MAX_NATIVE_ARTIFACTS = 16
MAX_ARCHIVE_KEEP = 1000
MAX_ARCHIVE_ENTRIES = 8192
MAX_PRUNE_VERIFICATION_BYTES = 268_435_456
_SAFE_DIRECTORY_ACCESS = (
    os.name == "posix"
    and all(function in os.supports_dir_fd for function in (os.open, os.stat, os.unlink))
    and os.scandir in os.supports_fd
    and os.stat in os.supports_follow_symlinks
    and all(hasattr(os, name) for name in ("O_DIRECTORY", "O_NOFOLLOW", "O_NONBLOCK"))
)
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
_PRIVATE_LABEL_RE = re.compile(r"(?i)(?:window|input|media|pointer)")
_PRIVATE_PATH_RE = re.compile(r"(?i)(?:\bfile:|\b[a-z]:|[\\/])")


def _skip_control_string(value: str, index: int, *, allow_bel: bool) -> int:
    while index < len(value):
        character = value[index]
        if character == "\x9c" or (allow_bel and character == "\x07"):
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
                index = _skip_control_string(value, index + 1, allow_bel=introducer == "]")
                continue
            if 0x20 <= ord(introducer) <= 0x2F:
                index += 1
                while index < len(value) and 0x20 <= ord(value[index]) <= 0x2F:
                    index += 1
                if index < len(value) and 0x30 <= ord(value[index]) <= 0x7E:
                    index += 1
            else:
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
            index = _skip_control_string(value, index + 1, allow_bel=codepoint == 0x9D)
            continue
        if codepoint == 0x9C:
            index += 1
            continue
        output.append(character)
        index += 1
    return "".join(output)


def _is_bidi_control(character: str) -> bool:
    codepoint = ord(character)
    return (
        codepoint == 0x061C
        or 0x200E <= codepoint <= 0x200F
        or 0x202A <= codepoint <= 0x202E
        or 0x2066 <= codepoint <= 0x2069
    )


def _escape_bidi_controls(value: str) -> str:
    output: list[str] | None = None
    copied = 0
    for index, character in enumerate(value):
        if _is_bidi_control(character):
            if output is None:
                output = []
            output.append(value[copied:index])
            output.append(f"[U+{ord(character):04X}]")
            copied = index + 1
    if output is None:
        return value
    output.append(value[copied:])
    return "".join(output)


def _is_log_character(character: str) -> bool:
    codepoint = ord(character)
    return character in "\n\t" or not (
        codepoint < 32 or 0x7F <= codepoint <= 0x9F or _is_bidi_control(character)
    )


class EvidenceError(ValueError):
    """Expected, user-correctable evidence-tool failure."""


class _PruneBudgetExceeded(EvidenceError):
    """Archive verification exceeded its aggregate byte budget."""


class _PruneByteBudget:
    def __init__(self, limit: int) -> None:
        self.limit = limit
        self.used = 0

    def charge(self, amount: int) -> None:
        if amount < 0 or amount > self.limit - self.used:
            raise _PruneBudgetExceeded(
                "archive prune exceeds its aggregate verification-byte budget "
                f"({self.limit} bytes); no artifacts were deleted"
            )
        self.used += amount

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


def _decode_json(raw: bytes, invalid_message: str) -> Any:
    if len(raw) > MAX_JSON_BYTES:
        raise EvidenceError("JSON input exceeds the 1 MiB limit")
    try:
        return json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object, parse_constant=reject_constant)
    except EvidenceError:
        raise
    except (UnicodeError, ValueError, RecursionError) as exc:
        raise EvidenceError(invalid_message) from exc


def read_json(path: Path) -> Any:
    try:
        with path.open("rb") as stream:
            raw = stream.read(MAX_JSON_BYTES + 1)
    except OSError as exc:
        raise EvidenceError("cannot read JSON input") from exc
    return _decode_json(raw, "input is not valid UTF-8 JSON")

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


def _private_value_end(value: str, index: int) -> int:
    if index >= len(value):
        return index
    if value[index] in "\"'":
        quote = value[index]
        index += 1
        while index < len(value):
            if value[index] == "\\":
                index += 2
            elif value[index] == quote:
                return index + 1
            else:
                index += 1
        return len(value)
    if value[index] in "[{":
        closers = ["]" if value[index] == "[" else "}"]
        index += 1
        while index < len(value):
            character = value[index]
            if character in "\"'":
                index = _private_value_end(value, index)
                continue
            if character in "[{":
                closers.append("]" if character == "[" else "}")
            elif character in "]}":
                if character != closers.pop():
                    return len(value)
                if not closers:
                    return index + 1
            index += 1
        return len(value)
    while index < len(value) and value[index] not in "\r\n":
        index += 1
    return index


def sanitize_text(value: str) -> str:
    pieces: list[str] = []
    copied = search = 0
    while match := _PRIVATE_LABEL_RE.search(value, search):
        cursor = match.end()
        while cursor < len(value) and (
            value[cursor] in " ._-\t" or value[cursor].isalnum()
        ):
            cursor += 1
        if cursor < len(value) and value[cursor] in "\"'":
            cursor += 1
        while cursor < len(value) and value[cursor] in " \t\r\n":
            cursor += 1
        if cursor >= len(value) or value[cursor] not in ":=":
            search = max(cursor, match.end())
            continue
        cursor += 1
        while cursor < len(value) and value[cursor] in " \t\r\n":
            cursor += 1
        pieces.append(value[copied:match.start()])
        pieces.append("[REDACTED-FIELD]")
        copied = search = _private_value_end(value, cursor)
    pieces.append(value[copied:])
    value = "".join(pieces)
    # Free-text paths have no reliable delimiter: spaces and punctuation can be
    # part of a component. Drop the entire line rather than retain a fragment.
    sanitized = "\n".join(
        "[REDACTED-PATH]" if _PRIVATE_PATH_RE.search(line) else line
        for line in value.split("\n")
    )
    return _escape_bidi_controls(sanitized)


def has_private_text(value: str) -> bool:
    return sanitize_text(value) != value


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


def _require_safe_directory_access() -> None:
    if os.name != "nt" and not _SAFE_DIRECTORY_ACCESS:
        raise EvidenceError("safe directory-relative access is unsupported on this platform")


def _open_directory(path: Path) -> int:
    _require_safe_directory_access()
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | getattr(os, "O_CLOEXEC", 0)
    descriptor = os.open(path.anchor if path.is_absolute() else ".", flags)
    try:
        parts = path.parts[1:] if path.is_absolute() else path.parts
        for part in parts:
            next_descriptor = os.open(part, flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = next_descriptor
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def _same_file_identity(left: os.stat_result, right: os.stat_result) -> bool:
    return (left.st_dev, left.st_ino) == (right.st_dev, right.st_ino)


def _same_file_snapshot(left: os.stat_result, right: os.stat_result) -> bool:
    return (
        left.st_dev, left.st_ino, left.st_mode, left.st_size, left.st_mtime_ns, left.st_ctime_ns
    ) == (
        right.st_dev, right.st_ino, right.st_mode, right.st_size, right.st_mtime_ns, right.st_ctime_ns
    )


def _verify_regular_entry_at(
    directory_fd: int,
    name: str,
    opened: os.stat_result,
    message: str,
) -> None:
    entry = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
    if not stat.S_ISREG(entry.st_mode) or not _same_file_identity(opened, entry):
        raise EvidenceError(message)



def _read_regular_at(
    directory_fd: int,
    name: str,
    *,
    budget: _PruneByteBudget | None = None,
) -> tuple[bytes, os.stat_result]:
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | getattr(os, "O_CLOEXEC", 0)
    descriptor = os.open(name, flags, dir_fd=directory_fd)
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise EvidenceError("evidence artifact must be a regular file")
        read_limit = MAX_JSON_BYTES + 1
        if budget is not None:
            read_limit = min(read_limit, before.st_size)
            budget.charge(read_limit)
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            raw = stream.read(read_limit)
        after = os.fstat(descriptor)
        if not _same_file_snapshot(before, after):
            raise EvidenceError("evidence artifact changed while being read")
        _verify_regular_entry_at(
            directory_fd,
            name,
            before,
            "evidence artifact path changed while being read",
        )
        if len(raw) > MAX_JSON_BYTES:
            raise EvidenceError("evidence artifact exceeds the 1 MiB limit")
        return raw, after
    finally:
        os.close(descriptor)


def _read_windows_regular(
    artifact: evidence_windows.File,
    *,
    budget: _PruneByteBudget | None = None,
    expected_snapshot: evidence_windows.Snapshot | None = None,
) -> tuple[bytes, evidence_windows.Snapshot]:
    """Read at most the scan-time reservation, when one is supplied."""
    before = artifact.snapshot()
    if expected_snapshot is not None and before != expected_snapshot:
        raise EvidenceError("evidence artifact changed before bounded reread")
    if before.st_size > MAX_JSON_BYTES:
        raise EvidenceError("evidence artifact exceeds the 1 MiB limit")
    reservation = (
        expected_snapshot.st_size
        if expected_snapshot is not None
        else before.st_size
    )
    if budget is not None:
        budget.charge(reservation)
    raw = bytearray()
    if budget is not None or expected_snapshot is not None:
        read_limit = reservation
    else:
        read_limit = MAX_JSON_BYTES + 1
    while len(raw) < read_limit:
        chunk = artifact.read(min(65_536, read_limit - len(raw)))
        if not chunk:
            break
        raw.extend(chunk)
        if len(raw) > MAX_JSON_BYTES:
            raise EvidenceError("evidence artifact exceeds the 1 MiB limit")
    after = artifact.snapshot()
    if before != after or after.st_size != len(raw):
        raise EvidenceError("evidence artifact changed while being read")
    return bytes(raw), after


def _contains_synthetic_marker(value: Any) -> bool:
    pending = [value]
    while pending:
        item = pending.pop()
        if isinstance(item, dict):
            if item.get("claims_product_behavior") is False:
                return True
            pending.extend(item.values())
        elif isinstance(item, list):
            pending.extend(item)
        elif isinstance(item, str) and any(marker in item.lower() for marker in (
            "synthetic-control-report", "synthetic-control-only", "cpu-grid-diff"
        )):
            return True
    return False


class _NativeJsonPrefixScanner:
    """Recognize JSON containers and complete primitive scalars while streaming."""

    _whitespace = frozenset((0x20, 0x09, 0x0A, 0x0D))

    def __init__(self) -> None:
        self._probe = bytearray()
        self._encoding: str | None = None
        self._state = "start"
        self._keyword: bytes | None = None
        self._keyword_index = 0
        self._invalid = False
        self._container = False
        self._pending_byte: int | None = None
        self._pending_utf32_bytes = b""
        self._pending_high_surrogate = False
        self._malformed = False

    def update(self, raw: bytes) -> None:
        if self._encoding is None:
            needed = 4 - len(self._probe)
            take = min(needed, len(raw))
            self._probe.extend(raw[:take])
            raw = raw[take:]
            if not self._select_encoding(final=False):
                return
        self._process(raw)
    @property
    def encoding(self) -> str | None:
        return self._encoding

    def _select_encoding(self, *, final: bool) -> bool:
        if len(self._probe) < 4 and not final:
            return False
        probe = bytes(self._probe)
        self._probe.clear()
        if probe.startswith(b"\x00\x00\xfe\xff"):
            self._encoding = "utf-32-be"
            probe = probe[4:]
        elif probe.startswith(b"\xff\xfe\x00\x00"):
            self._encoding = "utf-32-le"
            probe = probe[4:]
        elif probe.startswith(b"\xff\xfe"):
            self._encoding = "utf-16-le"
            probe = probe[2:]
        elif probe.startswith(b"\xfe\xff"):
            self._encoding = "utf-16-be"
            probe = probe[2:]
        elif probe.startswith(b"\xef\xbb\xbf"):
            self._encoding = "utf-8"
            probe = probe[3:]
        elif (
            len(probe) >= 4
            and probe[0] != 0 and probe[0] < 0x80
            and probe[1] == 0 and probe[2] == 0 and probe[3] == 0
        ):
            self._encoding = "utf-32-le"
        elif (
            len(probe) >= 4
            and probe[0] == 0 and probe[1] == 0 and probe[2] == 0
            and probe[3] != 0 and probe[3] < 0x80
        ):
            self._encoding = "utf-32-be"
        elif (
            len(probe) >= 4
            and probe[0] < 0x80 and probe[0] != 0 and probe[1] == 0
            and probe[2] < 0x80 and probe[2] != 0 and probe[3] == 0
        ):
            self._encoding = "utf-16-le"
        # UTF-16BE strings allow any non-NUL first content code unit.
        elif (
            len(probe) >= 4
            and probe[0] == 0 and 0 < probe[1] < 0x80
            and (
                (probe[2] == 0 and probe[3] != 0)
                or (
                    probe[1] == ord('"')
                    and (probe[2] != 0 or probe[3] != 0)
                )
            )
        ):
            self._encoding = "utf-16-be"
        else:
            self._encoding = "utf-8"
        self._process(probe)
        return True

    def _process(self, raw: bytes) -> None:
        if self._invalid or self._container or self._malformed:
            return
        if self._encoding in ("utf-32-le", "utf-32-be"):
            self._process_utf32(raw)
            return

        if self._encoding == "utf-8":
            for byte in raw:
                self._consume(byte)
                if self._invalid or self._container:
                    return
            return
        if self._pending_byte is not None:
            if not raw:
                return
            first = self._pending_byte
            second = raw[0]
            self._pending_byte = None
            self._consume_utf16_unit(
                first | (second << 8)
                if self._encoding == "utf-16-le"
                else (first << 8) | second
            )
            raw = raw[1:]
        even_length = len(raw) & ~1
        for index in range(0, even_length, 2):
            first = raw[index]
            second = raw[index + 1]
            self._consume_utf16_unit(
                first | (second << 8)
                if self._encoding == "utf-16-le"
                else (first << 8) | second
            )
            if self._invalid or self._container or self._malformed:
                return
        if even_length < len(raw):
            self._pending_byte = raw[-1]


    def _process_utf32(self, raw: bytes) -> None:
        little_endian = self._encoding == "utf-32-le"
        if self._pending_utf32_bytes:
            needed = 4 - len(self._pending_utf32_bytes)
            if len(raw) < needed:
                self._pending_utf32_bytes += raw
                return
            unit_bytes = self._pending_utf32_bytes + raw[:needed]
            self._pending_utf32_bytes = b""
            self._consume_utf32_unit(
                int.from_bytes(
                    unit_bytes, "little" if little_endian else "big"
                )
            )
            if self._invalid or self._container or self._malformed:
                return
            raw = raw[needed:]
        complete_length = len(raw) & ~3
        for index in range(0, complete_length, 4):
            if little_endian:
                unit = (
                    raw[index]
                    | (raw[index + 1] << 8)
                    | (raw[index + 2] << 16)
                    | (raw[index + 3] << 24)
                )
            else:
                unit = (
                    (raw[index] << 24)
                    | (raw[index + 1] << 16)
                    | (raw[index + 2] << 8)
                    | raw[index + 3]
                )
            self._consume_utf32_unit(unit)
            if self._invalid or self._container or self._malformed:
                return
        if complete_length < len(raw):
            self._pending_utf32_bytes = raw[complete_length:]

    def _consume_utf32_unit(self, unit: int) -> None:
        if unit > 0x10FFFF or 0xD800 <= unit <= 0xDFFF:
            self._malformed = True
        else:
            self._consume(unit)

    def _consume_utf16_unit(self, unit: int) -> None:
        if self._pending_high_surrogate:
            if 0xDC00 <= unit <= 0xDFFF:
                self._pending_high_surrogate = False
                self._consume(0x10000)
            else:
                self._malformed = True
            return
        if 0xD800 <= unit <= 0xDBFF:
            self._pending_high_surrogate = True
        elif 0xDC00 <= unit <= 0xDFFF:
            self._malformed = True
        else:
            self._consume(unit)

    def _consume(self, character: int) -> None:
        if self._invalid or self._container or self._malformed:
            return
        state = self._state
        if state == "start":
            if character in self._whitespace:
                return
            if character in (ord("{"), ord("["), ord('"')):
                self._container = True
            elif character in (ord("t"), ord("f"), ord("n")):
                self._keyword = {
                    ord("t"): b"true",
                    ord("f"): b"false",
                    ord("n"): b"null",
                }[character]
                self._keyword_index = 1
                self._state = "keyword"
            elif character == ord("-"):
                self._state = "number-minus"
            elif character == ord("0"):
                self._state = "number-zero"
            elif ord("1") <= character <= ord("9"):
                self._state = "number-integer"
            else:
                self._invalid = True
        elif state == "keyword":
            if character != self._keyword[self._keyword_index]:
                self._invalid = True
                return
            self._keyword_index += 1
            if self._keyword_index == len(self._keyword):
                self._state = "keyword-complete"
        elif state == "keyword-complete":
            if character in self._whitespace:
                self._state = "trailing"
            else:
                self._invalid = True
        elif state == "number-minus":
            if character == ord("0"):
                self._state = "number-zero"
            elif ord("1") <= character <= ord("9"):
                self._state = "number-integer"
            else:
                self._invalid = True
        elif state == "number-zero":
            if character == ord("."):
                self._state = "number-fraction-start"
            elif character in (ord("e"), ord("E")):
                self._state = "number-exponent-start"
            elif character in self._whitespace:
                self._state = "trailing"
            else:
                self._invalid = True
        elif state == "number-integer":
            if ord("0") <= character <= ord("9"):
                return
            if character == ord("."):
                self._state = "number-fraction-start"
            elif character in (ord("e"), ord("E")):
                self._state = "number-exponent-start"
            elif character in self._whitespace:
                self._state = "trailing"
            else:
                self._invalid = True
        elif state == "number-fraction-start":
            if ord("0") <= character <= ord("9"):
                self._state = "number-fraction"
            else:
                self._invalid = True
        elif state == "number-fraction":
            if ord("0") <= character <= ord("9"):
                return
            if character in (ord("e"), ord("E")):
                self._state = "number-exponent-start"
            elif character in self._whitespace:
                self._state = "trailing"
            else:
                self._invalid = True
        elif state == "number-exponent-start":
            if character in (ord("+"), ord("-")):
                self._state = "number-exponent-sign"
            elif ord("0") <= character <= ord("9"):
                self._state = "number-exponent"
            else:
                self._invalid = True
        elif state == "number-exponent-sign":
            if ord("0") <= character <= ord("9"):
                self._state = "number-exponent"
            else:
                self._invalid = True
        elif state == "number-exponent":
            if ord("0") <= character <= ord("9"):
                return
            if character in self._whitespace:
                self._state = "trailing"
            else:
                self._invalid = True
        elif state == "trailing" and character not in self._whitespace:
            self._invalid = True

    def looks_like_json(self) -> bool:
        if self._encoding is None:
            self._select_encoding(final=True)
        if self._pending_byte is not None or self._pending_utf32_bytes or self._pending_high_surrogate:
            self._malformed = True
        return (
            self._container
            or self._malformed
            or (
                not self._invalid
                and self._state in {
                    "keyword-complete",
                    "number-zero",
                    "number-integer",
                    "number-fraction",
                    "number-exponent",
                    "trailing",
                }
            )
        )


_SYNTHETIC_MARKERS = (
    b"synthetic-control-report",
    b"synthetic-control-only",
    b"cpu-grid-diff",
)


class _SyntheticMarkerStream:
    """Match synthetic marker text in one bounded byte or code-unit view."""

    _markers = _SYNTHETIC_MARKERS
    _window_limit = max(map(len, _markers))
    _hex_digits = b"0123456789abcdefABCDEF"

    def __init__(self, encoding: str, alignment: int = 0) -> None:
        if encoding == "utf-8":
            self.width = 1
        elif encoding.startswith("utf-16"):
            self.width = 2
        else:
            self.width = 4
        self.little_endian = encoding.endswith("-le")
        self.alignment = alignment
        self.backslash_run = 0
        self.unicode_remaining = 0
        self.unicode_value = 0
        self.window = bytearray()
        self.found = False
        self.pending = bytearray()

    def _append(self, value: int) -> None:
        if value > 0x7f:
            self.window.clear()
            return
        if 0x41 <= value <= 0x5a:
            value += 0x20
        self.window.append(value)
        if len(self.window) > self._window_limit:
            del self.window[0]
        if any(marker in self.window for marker in self._markers):
            self.found = True

    def _consume(self, byte: int) -> None:
        if self.found:
            return
        if self.unicode_remaining:
            if byte in self._hex_digits:
                if ord("0") <= byte <= ord("9"):
                    digit = byte - ord("0")
                else:
                    digit = (byte | 0x20) - ord("a") + 10
                self.unicode_value = (self.unicode_value << 4) | digit
                self.unicode_remaining -= 1
                if not self.unicode_remaining:
                    self._append(self.unicode_value)
                    self.unicode_value = 0
                return
            self.unicode_remaining = 0
            self.unicode_value = 0
            self._append(0)
        if byte == ord("\\"):
            self.backslash_run += 1
            return
        if byte == ord("u") and self.backslash_run % 2:
            if self.backslash_run > 1:
                self._append(0)
            self.backslash_run = 0
            self.unicode_remaining = 4
            return
        if self.backslash_run:
            self._append(0)
            self.backslash_run = 0
        self._append(byte)

    def _unit(self, raw: bytes, index: int) -> int:
        if self.width == 2:
            first = raw[index]
            second = raw[index + 1]
            if self.little_endian:
                return first | (second << 8)
            return (first << 8) | second
        first = raw[index]
        second = raw[index + 1]
        third = raw[index + 2]
        fourth = raw[index + 3]
        if self.little_endian:
            return first | (second << 8) | (third << 16) | (fourth << 24)
        return (first << 24) | (second << 16) | (third << 8) | fourth

    def _consume_unit(self, unit: int) -> None:
        self._consume(unit if unit <= 0x7f else 0)

    def update(self, raw: bytes) -> None:
        if self.found:
            return
        if self.alignment:
            skipped = min(self.alignment, len(raw))
            self.alignment -= skipped
            raw = raw[skipped:]
            if not raw:
                return
        if self.width == 1:
            for byte in raw:
                self._consume(byte)
                if self.found:
                    return
            return
        if self.pending:
            needed = self.width - len(self.pending)
            if len(raw) < needed:
                self.pending.extend(raw)
                return
            unit_bytes = self.pending + raw[:needed]
            self.pending.clear()
            self._consume_unit(
                int.from_bytes(unit_bytes, "little" if self.little_endian else "big")
            )
            if self.found:
                return
            raw = raw[needed:]
        complete_length = len(raw) - (len(raw) % self.width)
        for index in range(0, complete_length, self.width):
            self._consume_unit(self._unit(raw, index))
            if self.found:
                return
        if complete_length < len(raw):
            self.pending.extend(raw[complete_length:])


class _SyntheticMarkerScanner:
    """Find fixed markers in UTF-8/16/32 views without classifying JSON."""

    _encodings = (
        ("utf-8", 1),
        ("utf-16-le", 2),
        ("utf-16-be", 2),
        ("utf-32-le", 4),
        ("utf-32-be", 4),
    )

    def __init__(self) -> None:
        self._streams = tuple(
            _SyntheticMarkerStream(encoding, alignment)
            for encoding, width in self._encodings
            for alignment in range(width)
        )
        self.found = False

    def update(self, raw: bytes) -> None:
        if self.found:
            return
        for stream in self._streams:
            stream.update(raw)
            if stream.found:
                self.found = True
                return


def _contains_synthetic_bytes(raw: bytes) -> bool:
    scanner = _SyntheticMarkerScanner()
    scanner.update(raw)
    return scanner.found


def _hash_native_stream(
    read: Callable[[int], bytes],
    limit: int,
    *,
    expected_size: int | None = None,
) -> tuple[str, int]:
    digest = hashlib.sha256()
    prefix = bytearray()
    synthetic_marker_scanner = _SyntheticMarkerScanner()
    json_prefix = _NativeJsonPrefixScanner()
    size = 0
    while True:
        if expected_size is None:
            read_size = min(65_536, limit - size + 1)
        else:
            remaining = expected_size - size
            if remaining <= 0:
                break
            read_size = min(65_536, limit - size + 1, remaining)
        chunk = read(read_size)
        if not chunk:
            break
        size += len(chunk)
        if size > limit:
            raise EvidenceError("native evidence artifacts exceed their size limit")
        digest.update(chunk)
        json_prefix.update(chunk)
        synthetic_marker_scanner.update(chunk)
        remaining = MAX_JSON_BYTES + 1 - len(prefix)
        if remaining > 0:
            prefix.extend(chunk[:remaining])
    if expected_size is not None and size != expected_size:
        raise EvidenceError("native evidence artifact changed while being read")
    if not size:
        raise EvidenceError("native evidence artifact must not be empty")
    # Oversized JSON keeps its size-limit error; markers still reject non-JSON evidence.
    if size > MAX_JSON_BYTES and json_prefix.looks_like_json():
        raise EvidenceError("native evidence JSON exceeds the 1 MiB inspection limit")
    if synthetic_marker_scanner.found:
        raise EvidenceError("synthetic artifacts cannot support a native-manual pass")
    if size <= MAX_JSON_BYTES:
        try:
            content = json.loads(bytes(prefix), object_pairs_hook=unique_object, parse_constant=reject_constant)
        except (ValueError, UnicodeError, RecursionError) as exc:
            if json_prefix.looks_like_json():
                raise EvidenceError("native evidence JSON must be inspectable") from exc
        else:
            if _contains_synthetic_marker(content):
                raise EvidenceError("synthetic artifacts cannot support a native-manual pass")
    return digest.hexdigest(), size


def _hash_native_artifact(
    relative_path: str,
    limit: int,
    *,
    budget: _PruneByteBudget | None = None,
) -> tuple[str, int]:
    if not _safe_relative_evidence_path(relative_path):
        raise EvidenceError("native evidence artifact path must be safe and repository-relative")
    _require_safe_directory_access()
    parts = PurePosixPath(relative_path).parts
    if os.name == "nt":
        try:
            with evidence_windows.open_directory(ROOT.joinpath(*parts[:-1])) as directory:
                with directory.open_file(parts[-1]) as artifact:
                    before = artifact.snapshot()
                    if before.st_size > limit:
                        raise EvidenceError("native evidence artifacts exceed their size limit")
                    if budget is not None:
                        budget.charge(before.st_size)
                    identity, size = _hash_native_stream(
                        artifact.read,
                        limit,
                        expected_size=before.st_size if budget is not None else None,
                    )
                    after = artifact.snapshot()
                    if before != after or after.st_size != size:
                        raise EvidenceError("native evidence artifact changed while being read")
                    return identity, size
        except OSError as exc:
            if isinstance(exc, evidence_windows.FileCloseError):
                raise
            raise EvidenceError(
                "native evidence artifact is inaccessible or unsafe; "
                "Windows requires local fixed NTFS without reparse points"
            ) from exc
    try:
        directory_fd = _open_directory(ROOT.joinpath(*parts[:-1]))
    except OSError as exc:
        raise EvidenceError("native evidence artifact is inaccessible or a symlink") from exc
    try:
        try:
            flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | getattr(os, "O_CLOEXEC", 0)
            descriptor = os.open(parts[-1], flags, dir_fd=directory_fd)
        except OSError as exc:
            raise EvidenceError("native evidence artifact is inaccessible or a symlink") from exc
        try:
            before = os.fstat(descriptor)
            if not stat.S_ISREG(before.st_mode):
                raise EvidenceError("native evidence artifact must be a regular file")
            if before.st_size > limit:
                raise EvidenceError("native evidence artifacts exceed their size limit")
            if budget is not None:
                budget.charge(before.st_size)
            with os.fdopen(descriptor, "rb", closefd=False) as stream:
                identity, size = _hash_native_stream(
                    stream.read,
                    limit,
                    expected_size=before.st_size if budget is not None else None,
                )
            after = os.fstat(descriptor)
            if not _same_file_snapshot(before, after) or after.st_size != size:
                raise EvidenceError("native evidence artifact changed while being read")
            _verify_regular_entry_at(
                directory_fd,
                parts[-1],
                before,
                "native evidence artifact path changed while being read",
            )
            return identity, size
        except OSError as exc:
            raise EvidenceError("cannot read native evidence artifact") from exc
        finally:
            os.close(descriptor)
    finally:
        os.close(directory_fd)


def _validate_native_manifest(
    content: dict[str, Any],
    record: dict[str, Any],
    *,
    budget: _PruneByteBudget | None = None,
) -> None:
    """Check manifest consistency and artifact hashes, not reviewer identity."""
    if (
        content.keys() != {
            "schema_version", "artifact_type", "execution", "procedure_id",
            "reviewer", "date", "status", "artifacts",
        }
        or content.get("schema_version") != 1
        or isinstance(content.get("schema_version"), bool)
        or content.get("artifact_type") != "native-manual-evidence"
        or content.get("execution") != "manual"
        or content.get("status") != "pass"
        or any(content.get(key) != record.get(key) for key in ("procedure_id", "reviewer", "date"))
    ):
        raise EvidenceError("native-manual evidence manifest metadata does not match the record")
    artifacts = content.get("artifacts")
    if not isinstance(artifacts, list) or not 1 <= len(artifacts) <= MAX_NATIVE_ARTIFACTS:
        raise EvidenceError("native-manual evidence manifest must contain 1 to 16 artifacts")
    seen: set[str] = set()
    total = 0
    for artifact in artifacts:
        if not isinstance(artifact, dict) or artifact.keys() != {"path", "sha256"}:
            raise EvidenceError("native-manual evidence artifact entry is invalid")
        path = artifact.get("path")
        expected = artifact.get("sha256")
        if not _safe_relative_evidence_path(path) or not isinstance(expected, str) or not HASH_RE.fullmatch(expected):
            raise EvidenceError("native-manual evidence artifact path or hash is invalid")
        if path in seen:
            raise EvidenceError("native-manual evidence artifact paths must be unique")
        seen.add(path)
        identity, size = _hash_native_artifact(
            path,
            min(MAX_NATIVE_ARTIFACT_BYTES, MAX_NATIVE_TOTAL_BYTES - total),
            budget=budget,
        )
        if identity != expected:
            raise EvidenceError("native-manual evidence artifact hash does not match")
        total += size


def _validate_native_pass_artifact(
    record: dict[str, Any],
    *,
    budget: _PruneByteBudget | None = None,
) -> None:
    _require_safe_directory_access()
    parts = PurePosixPath(record["evidence_path"]).parts
    try:
        if os.name == "nt":
            # Keep the manifest and its ancestor directories pinned while its
            # referenced artifacts are inspected.
            with evidence_windows.open_directory(ROOT.joinpath(*parts[:-1])) as directory:
                with directory.open_file(parts[-1]) as artifact:
                    raw, _ = _read_windows_regular(artifact, budget=budget)
                    _inspect_native_pass_content(raw, record, budget=budget)
            return
        directory_fd = _open_directory(ROOT.joinpath(*parts[:-1]))
        try:
            raw, _ = _read_regular_at(directory_fd, parts[-1], budget=budget)
        finally:
            os.close(directory_fd)
    except OSError as exc:
        if os.name == "nt":
            if isinstance(exc, evidence_windows.FileCloseError):
                raise
            raise EvidenceError(
                "native-manual pass requires accessible evidence on local fixed NTFS without reparse points"
            ) from exc
        raise EvidenceError("native-manual pass requires an accessible non-symlink evidence artifact") from exc
    _inspect_native_pass_content(raw, record, budget=budget)


def _inspect_native_pass_content(
    raw: bytes,
    record: dict[str, Any],
    *,
    budget: _PruneByteBudget | None = None,
) -> None:
    if not raw:
        raise EvidenceError("native-manual pass requires a non-empty evidence artifact")
    if _contains_synthetic_bytes(raw):
        raise EvidenceError("synthetic artifacts cannot support a native-manual pass")
    try:
        content = json.loads(raw, object_pairs_hook=unique_object, parse_constant=reject_constant)
    except (ValueError, UnicodeError, RecursionError) as exc:
        try:
            text = raw.decode("utf-8")
        except UnicodeError as decode_error:
            raise EvidenceError("native-manual evidence type cannot be inspected safely") from decode_error
        json_prefix = _NativeJsonPrefixScanner()
        json_prefix.update(raw)
        if json_prefix.looks_like_json():
            raise EvidenceError("native-manual JSON evidence must be inspectable") from exc
        if any(not _is_log_character(character) and character != "\r" for character in text):
            raise EvidenceError("native-manual evidence type cannot be inspected safely")
    else:
        if _contains_synthetic_marker(content):
            raise EvidenceError("synthetic artifacts cannot support a native-manual pass")
        if isinstance(content, dict) and content.get("artifact_type") == "native-manual-evidence":
            _validate_native_manifest(content, record, budget=budget)
        elif not validate_log_report(content):
            raise EvidenceError("native-manual JSON evidence must be a native manifest or sanitized log")


def validate_record(
    record: Any,
    *,
    check_identity: bool = True,
    _prune_budget: _PruneByteBudget | None = None,
) -> list[str]:
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
    evidence_path = record.get("evidence_path")
    if not _safe_relative_evidence_path(evidence_path):
        errors.append("evidence_path must be a safe repository-relative path")
    elif level == "native-manual" and status == "pass":
        try:
            _validate_native_pass_artifact(record, budget=_prune_budget)
        except _PruneBudgetExceeded:
            raise
        except EvidenceError as exc:
            errors.append(str(exc))
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
    if len(canonical_json(record)) > MAX_JSON_BYTES:
        raise EvidenceError("final record exceeds the 1 MiB limit")
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
    text = "".join(
        character
        for character in text
        if _is_log_character(character) or _is_bidi_control(character)
    )
    content = sanitize_text(text)
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


def _archive_names(directory_fd: Any) -> list[str]:
    if os.name == "nt":
        try:
            names = directory_fd.names(MAX_ARCHIVE_ENTRIES)
        except OSError as exc:
            raise EvidenceError("cannot enumerate archive safely or archive contains too many entries") from exc
        if len(set(names)) != len(names):
            raise EvidenceError("archive changed during enumeration")
        return names
    names: list[str] = []
    with os.scandir(directory_fd) as entries:
        for entry in entries:
            if len(names) >= MAX_ARCHIVE_ENTRIES:
                raise EvidenceError("archive contains too many entries to prune safely")
            names.append(entry.name)
    return names


def prune_archive(directory: Path, keep: int) -> dict[str, Any]:
    if keep < 1 or keep > MAX_ARCHIVE_KEEP:
        raise EvidenceError(f"keep must be between 1 and {MAX_ARCHIVE_KEEP}")
    _require_safe_directory_access()
    try:
        directory_fd = evidence_windows.open_directory(directory) if os.name == "nt" else _open_directory(directory)
    except OSError as exc:
        if os.name == "nt":
            raise EvidenceError(
                "archive must be accessible on local fixed NTFS without reparse points"
            ) from exc
        raise EvidenceError("archive must be an existing non-symlink directory") from exc
    operation_error: BaseException | None = None
    try:
        patterns = (
            ("evidence", ARCHIVE_RECORD_RE),
            ("control", ARCHIVE_CONTROL_RE),
            ("grid-diff", ARCHIVE_DIFF_RE),
            ("log", ARCHIVE_LOG_RE),
        )
        budget = _PruneByteBudget(MAX_PRUNE_VERIFICATION_BYTES)
        validators = {
            "control": validate_control_report,
            "grid-diff": validate_grid_diff_report,
            "log": validate_log_report,
        }
        valid: dict[str, list[tuple[tuple[int, str | int], str, str, Any, str]]] = {
            name: [] for name, _ in patterns
        }
        for name in _archive_names(directory_fd):
            for kind, pattern in patterns:
                match = pattern.fullmatch(name)
                if match is None:
                    continue
                try:
                    if os.name == "nt":
                        with directory_fd.open_file(name) as artifact:
                            raw, snapshot = _read_windows_regular(artifact, budget=budget)
                    else:
                        raw, snapshot = _read_regular_at(directory_fd, name, budget=budget)
                    content = _decode_json(raw, "input is not valid UTF-8 JSON")
                except _PruneBudgetExceeded:
                    raise
                except (EvidenceError, OSError) as exc:
                    if os.name == "nt" and isinstance(exc, evidence_windows.FileCloseError):
                        # Close failure remains fatal when chained with a scan/type/read error.
                        raise EvidenceError("cannot close an archive evidence artifact handle") from exc
                    break
                identity = match.group(1)
                if kind == "evidence":
                    try:
                        accepted = (
                            isinstance(content, dict)
                            and content.get("record_id") == identity
                            and not validate_record(content, _prune_budget=budget)
                        )
                    except OSError as exc:
                        if os.name == "nt" and isinstance(exc, evidence_windows.FileCloseError):
                            raise EvidenceError(
                                "cannot close a native evidence artifact handle during archive scan"
                            ) from exc
                        raise
                    sort_date = content.get("date", "") if accepted else ""
                else:
                    accepted = (
                        isinstance(content, dict)
                        and content.get("artifact_id") == identity
                        and validators[kind](content)
                    )
                    sort_date = ""
                if accepted:
                    # Records have explicit dates; generated artifacts use write time.
                    order = (0, sort_date) if sort_date else (1, snapshot.st_mtime_ns)
                    digest = sha256_bytes(raw) if os.name == "nt" else ""
                    valid[kind].append((order, identity, name, snapshot, digest))
                break
        if os.name == "nt":
            # Precharge each candidate's scan-time size for its single reread
            # before deletion; expected_snapshot bounds that reread to the charge.
            for candidates in valid.values():
                for _, _, _, snapshot, _ in candidates:
                    budget.charge(snapshot.st_size)
            # Check every candidate again before the first mutation; per-entry
            # rereads below close the race after this snapshot preflight.
            for candidates in valid.values():
                for _, _, name, snapshot, _ in candidates:
                    try:
                        with directory_fd.open_file(name) as artifact:
                            if artifact.snapshot() != snapshot:
                                raise EvidenceError(
                                    f"archive changed before pruning of candidate {name}"
                                )
                    except FileNotFoundError as exc:
                        raise EvidenceError(
                            f"archive candidate disappeared before pruning: {name}"
                        ) from exc
                    except OSError as exc:
                        if isinstance(exc, evidence_windows.FileCloseError):
                            raise EvidenceError(
                                "cannot close an archive evidence artifact handle"
                            ) from exc
                        raise EvidenceError(
                            "cannot verify archive candidates before pruning"
                        ) from exc
        removed: list[str] = []
        kept: dict[str, int] = {}
        try:
            for kind, _ in patterns:
                candidates = sorted(valid[kind], key=lambda item: (item[0], item[1]), reverse=True)
                kept[kind] = min(len(candidates), keep)
                for _, _, name, snapshot, digest in candidates[keep:]:
                    try:
                        if os.name == "nt":
                            # Reopen without write/delete sharing, then re-read through
                            # the very handle used for deletion. Both the object
                            # snapshot and the bytes must match the validated scan.
                            with directory_fd.open_file(name, for_delete=True) as artifact:
                                try:
                                    raw, current = _read_windows_regular(
                                        artifact, expected_snapshot=snapshot
                                    )
                                except EvidenceError as exc:
                                    raise EvidenceError(
                                        f"archive changed during pruning of expired artifact {name}"
                                    ) from exc
                                if current != snapshot or sha256_bytes(raw) != digest:
                                    raise EvidenceError(
                                        f"archive changed during pruning of expired artifact {name}"
                                    )
                                if not artifact.delete(snapshot):
                                    raise EvidenceError(
                                        f"archive changed during pruning of expired artifact {name}"
                                    )
                                removed.append(name)
                        else:
                            current = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                            if not _same_file_snapshot(snapshot, current):
                                raise EvidenceError(
                                    f"archive changed during pruning of expired artifact {name}"
                                )
                            # Recheck before reporting the POSIX limitation so replacements
                            # installed after the initial snapshot check produce an explicit
                            # incomplete-prune error.
                            current = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                            if not _same_file_snapshot(snapshot, current):
                                raise EvidenceError(
                                    f"archive changed during pruning of expired artifact {name}"
                                )
                            # POSIX cannot unlink an entry conditional on its inode.
                            # Refuse the deletion rather than risk removing a same-name
                            # replacement installed after the snapshot check.
                            raise EvidenceError(
                                "archive prune incomplete: cannot safely prune expired artifacts "
                                "on POSIX: atomic compare-and-unlink is unavailable"
                            )
                    except FileNotFoundError as exc:
                        raise EvidenceError(
                            f"expired artifact disappeared during pruning: {name}"
                        ) from exc
                    except OSError as exc:
                        raise EvidenceError("cannot remove an expired evidence artifact") from exc
            for kind, _ in patterns:
                candidates = sorted(valid[kind], key=lambda item: (item[0], item[1]), reverse=True)
                for _, _, name, snapshot, digest in candidates[:keep]:
                    try:
                        if os.name == "nt":
                            # Do not report scan-time retention counts for a
                            # retained entry that changed during this prune.
                            with directory_fd.open_file(name) as artifact:
                                try:
                                    raw, current = _read_windows_regular(
                                        artifact, expected_snapshot=snapshot
                                    )
                                except EvidenceError as exc:
                                    raise EvidenceError(
                                        f"archive changed during pruning of retained artifact {name}"
                                    ) from exc
                                if current != snapshot or sha256_bytes(raw) != digest:
                                    raise EvidenceError(
                                        f"archive changed during pruning of retained artifact {name}"
                                    )
                        else:
                            current = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                            if not _same_file_snapshot(snapshot, current):
                                raise EvidenceError(
                                    f"archive changed during pruning of retained artifact {name}"
                                )
                    except FileNotFoundError as exc:
                        raise EvidenceError(
                            f"retained artifact disappeared during pruning: {name}"
                        ) from exc
                    except OSError as exc:
                        raise EvidenceError("cannot verify a retained evidence artifact") from exc
        except EvidenceError as exc:
            if removed:
                removed_names = ", ".join(sorted(removed))
                raise EvidenceError(
                    f"{exc}; already removed {len(removed)} expired archive artifact(s): {removed_names}"
                ) from exc
            raise
        return {"kept_by_type": kept, "removed": len(removed), "removed_names": sorted(removed)}
    except BaseException as exc:
        operation_error = exc
        raise
    finally:
        if os.name == "nt":
            # Reuse Directory's exception-aware cleanup with only this operation's error.
            if operation_error is None:
                directory_fd.__exit__(None, None, None)
            else:
                directory_fd.__exit__(
                    type(operation_error), operation_error, operation_error.__traceback__
                )
        else:
            os.close(directory_fd)


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
        return _decode_json(raw, "standard input is not valid UTF-8 JSON")
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
