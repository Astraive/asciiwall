#!/usr/bin/env python3
"""Record hosted CI receipts for QA-102 portable component tests only."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from datetime import date
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
CONVERT_SOURCE = ROOT / "crates/ascii-convert/tests/qa_102_input_conversion.rs"
MEDIA_SOURCE = ROOT / "crates/ascii-media/tests/image_decode_jpeg_controls.rs"
EC010_FIXTURE = ROOT / "crates/ascii-media/tests/fixtures/ec010-oversized-budget.jpg"
NOT_RUN_SOURCE = ROOT / "docs/quality/edge-case-matrix.md"
CONVERT_TESTS = {
    "EC-006": "ec_006_unsupported_hdr_and_profile_errors_are_actionable_through_public_conversion",
    "EC-007": "ec_007_black_white_and_flat_images_map_stably_by_measured_glyph_coverage",
    "EC-008": "ec_008_padded_odd_stride_preserves_visible_rows_and_ignores_padding; ec_008_decoder_stride_preserves_odd_width_rows_through_conversion",
}
EC010_TEST = "oversized_jpeg_fixture_is_rejected_by_public_decoder_with_raster_budget_error"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def sha256_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def test_binary(target_dir: Path, target_name: str) -> Path:
    debug = target_dir / "debug"
    deps = debug / "deps"
    directories = [deps] if deps.is_dir() else [debug / "build"]
    candidates = sorted(
        path
        for directory in directories
        for path in directory.rglob(f"{target_name}-*")
        if path.is_file() and path.suffix not in {".d", ".pdb", ".rlib", ".rmeta"}
    )
    if len(candidates) != 1:
        raise RuntimeError(f"expected one compiled {target_name} test executable, found {len(candidates)}")
    return candidates[0]

def require_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"required GitHub Actions environment variable {name} is missing")
    return value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--recorder", type=Path, required=True, help="QA-004 scripts/evidence.py checked out at its pinned commit")
    parser.add_argument("--output-dir", type=Path, required=True, help="new hosted evidence output directory")
    args = parser.parse_args()
    recorder = args.recorder.resolve()
    output_dir = args.output_dir.resolve()
    if not recorder.is_file():
        raise RuntimeError("the pinned QA-004 evidence recorder is missing")
    if output_dir.exists():
        raise RuntimeError("evidence output directory already exists")

    run_id = require_env("GITHUB_RUN_ID")
    attempt = require_env("GITHUB_RUN_ATTEMPT")
    repository = require_env("GITHUB_REPOSITORY")
    run_sha = require_env("GITHUB_SHA")
    pr_head_sha = require_env("QA102_HEAD_SHA")
    run_url = f"{require_env('GITHUB_SERVER_URL')}/{repository}/actions/runs/{run_id}"
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    convert_binary = test_binary(target_dir, "qa_102_input_conversion")
    media_binary = test_binary(target_dir, "image_decode_jpeg_controls")
    convert_source_hash = sha256(CONVERT_SOURCE)
    media_source_hash = sha256(MEDIA_SOURCE)
    ec010_fixture_hash = sha256(EC010_FIXTURE)
    ec009_source_hash = sha256(NOT_RUN_SOURCE)

    records: list[dict[str, Any]] = []
    for ec_id, test_names in CONVERT_TESTS.items():
        fixture_hash = sha256_text(f"qa-102-inline-fixture-v1\0{ec_id}\0{convert_source_hash}\0{test_names}")
        records.append(
            {
                "file": f"{ec_id.lower()}.json",
                "fields": {
                    "schema_version": 1,
                    "level": "integration",
                    "execution": "automated",
                    "procedure_id": None,
                    "ec_ids": [ec_id],
                    "source_hash": convert_source_hash,
                    "binary_hash": sha256(convert_binary),
                    "fixture_hash": fixture_hash,
                    "environment": {
                        "availability": "present",
                        "os": f"{os.environ.get('RUNNER_OS', platform.system())} hosted GitHub Actions runner",
                        "session": "headless CI; no interactive session",
                        "compositor": "not exercised by portable component tests",
                        "gpu": "not exercised by portable component tests",
                    },
                    "permissions": {"state": "not-required", "items": []},
                    "result": {
                        "status": "pass",
                        "summary": f"Automated component evidence only: {test_names} passed; not human, native, or product acceptance.",
                    },
                    "evidence_path": "run.json",
                    "issue": "QA-102",
                    "reviewer": "ci-runner",
                    "date": date.today().isoformat(),
                },
                "test_names": test_names,
                "source": CONVERT_SOURCE.relative_to(ROOT).as_posix(),
                "binary": convert_binary.relative_to(target_dir).as_posix(),
                "source_hash": convert_source_hash,
                "binary_hash": sha256(convert_binary),
                "fixture_hash": fixture_hash,
                "fixture_description": "Inline scenario fixture definitions in the hashed Rust test source; fixture digest binds the source hash and exact test name(s).",
            }
        )

    records.append(
        {
            "file": "ec-010.json",
            "fields": {
                "schema_version": 1,
                "level": "integration",
                "execution": "automated",
                "procedure_id": None,
                "ec_ids": ["EC-010"],
                "source_hash": media_source_hash,
                "binary_hash": sha256(media_binary),
                "fixture_hash": ec010_fixture_hash,
                "environment": {
                    "availability": "present",
                    "os": f"{os.environ.get('RUNNER_OS', platform.system())} hosted GitHub Actions runner",
                    "session": "headless CI; no interactive session",
                    "compositor": "not exercised by portable component tests",
                    "gpu": "not exercised by portable component tests",
                },
                "permissions": {"state": "not-required", "items": []},
                "result": {
                    "status": "pass",
                    "summary": f"Automated component evidence only: {EC010_TEST} passed; not human, native, or product acceptance.",
                },
                "evidence_path": "run.json",
                "issue": "QA-102",
                "reviewer": "ci-runner",
                "date": date.today().isoformat(),
            },
            "test_names": EC010_TEST,
            "source": MEDIA_SOURCE.relative_to(ROOT).as_posix(),
            "binary": media_binary.relative_to(target_dir).as_posix(),
            "fixture": EC010_FIXTURE.relative_to(ROOT).as_posix(),
            "source_hash": media_source_hash,
            "binary_hash": sha256(media_binary),
            "fixture_hash": ec010_fixture_hash,
            "fixture_description": "Persisted EC-010 JPEG fixture; fixture_hash is the SHA-256 of the exact checked-in bytes.",
        }
    )

    ec009_binary_note = "NOT RUN: no EC-009 restricted-worker/UI binary was built or executed"
    ec009_fixture_note = "NOT RUN: no malformed encoded-input/worker/UI fixture was executed"
    records.append(
        {
            "file": "ec-009-not-run.json",
            "fields": {
                "schema_version": 1,
                "level": "integration",
                "execution": "automated",
                "procedure_id": None,
                "ec_ids": ["EC-009"],
                "source_hash": ec009_source_hash,
                "binary_hash": sha256_text(ec009_binary_note),
                "fixture_hash": sha256_text(ec009_fixture_note),
                "environment": {
                    "availability": "absent",
                    "os": "not-available",
                    "session": "not-available",
                    "compositor": "not-available",
                    "gpu": "not-available",
                },
                "permissions": {"state": "unavailable", "items": []},
                "result": {
                    "status": "not-run",
                    "summary": "NOT RUN: this repository has no restricted media worker or UI safe-error surface; decoder-only tests do not satisfy EC-009.",
                },
                "evidence_path": "run.json",
                "issue": "QA-102",
                "reviewer": "ci-runner",
                "date": date.today().isoformat(),
            },
            "source": NOT_RUN_SOURCE.relative_to(ROOT).as_posix(),
            "source_hash": ec009_source_hash,
            "binary_hash": sha256_text(ec009_binary_note),
            "fixture_hash": sha256_text(ec009_fixture_note),
            "binary": ec009_binary_note,
            "fixture": ec009_fixture_note,
            "result": "not-run",
        }
    )

    payload = {
        "schema_version": 1,
        "scope": "automated portable component evidence only; no human sign-off, native acceptance, or product acceptance",
        "repository": repository,
        "pull_request": 341,
        "pr_head_sha": pr_head_sha,
        "workflow_run": {
            "id": run_id,
            "attempt": attempt,
            "url": run_url,
            "workflow_sha": run_sha,
            "event": require_env("GITHUB_EVENT_NAME"),
        },
        "qa004_recorder": {
            "repository": "Astraive/asciiwall",
            "commit": "fb20e84ec196c421b954b90985dc2217a35767c6",
            "dependency": "unmerged QA-004 PR #310; checked out separately from the QA-102 source tree",
        },
        "components": records,
        "ec_009": "NOT RUN; no restricted worker/UI path was exercised",
        "reviewer_field": "ci-runner is an automation identifier required by the schema; it is not a human reviewer or acceptance",
    }

    output_dir.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="qa-102-evidence-") as temporary:
        staging = Path(temporary) / "artifact"
        staging.mkdir()
        (staging / "run.json").write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        for index, record in enumerate(records):
            input_path = Path(temporary) / f"record-input-{index}.json"
            input_path.write_text(json.dumps(record["fields"], indent=2, sort_keys=True) + "\n", encoding="utf-8")
            subprocess.run(
                [sys.executable, str(recorder), "record", str(input_path), "--output", str(staging / record["file"])],
                cwd=ROOT,
                check=True,
            )
        shutil.copytree(staging, output_dir)

    # The copied artifact tree is now complete and remains quiescent during validation.
    for record in records:
        subprocess.run(
            [
                sys.executable,
                str(recorder),
                "validate",
                str(output_dir / record["file"]),
                "--evidence-root",
                str(output_dir),
            ],
            cwd=ROOT,
            check=True,
        )
    print(f"Hosted QA-102 component evidence recorded and validated: {output_dir}")
    print(f"Run: {run_url}; PR head: {pr_head_sha}; EC-009: NOT RUN")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
