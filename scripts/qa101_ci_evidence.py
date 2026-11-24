#!/usr/bin/env python3
"""Run QA-101 EC-001–EC-005 and archive automated CI evidence with QA-004."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
from typing import Any

QA004_COMMIT = "27a81685878ca8b7fd47e13148384824bbfb7bea"
TEST_TARGET = "qa_101_input_conversion"
EXPECTED_EC_IDS = {"001", "002", "003", "004", "005"}
FIXTURE_LINE = re.compile(r"fixture=([A-Za-z0-9._-]+).*?sha256=([0-9a-f]{64})")
CASE_LINE = re.compile(r"^test (ec_(00[1-5])_[A-Za-z0-9_]+) \.\.\. (ok|FAILED|ignored)$")


def command(args: list[str], *, cwd: Path, input_bytes: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        args,
        cwd=cwd,
        input=input_bytes,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def git_text(args: list[str], *, cwd: Path) -> str:
    completed = command(["git", *args], cwd=cwd)
    if completed.returncode:
        raise RuntimeError(completed.stderr.decode("utf-8", errors="replace").strip() or "git command failed")
    return completed.stdout.decode("utf-8", errors="strict").strip()


def source_identity(repo_root: Path) -> tuple[str, str, str]:
    commit = git_text(["rev-parse", "HEAD"], cwd=repo_root)
    tree_oid = git_text(["rev-parse", f"{commit}^{{tree}}"], cwd=repo_root)
    archive = command(["git", "archive", "--format=tar", commit], cwd=repo_root)
    if archive.returncode:
        raise RuntimeError(archive.stderr.decode("utf-8", errors="replace").strip() or "git archive failed")
    return commit, tree_oid, hashlib.sha256(archive.stdout).hexdigest()


def output_executable(build_stdout: bytes) -> Path:
    executables: set[str] = set()
    for raw_line in build_stdout.splitlines():
        try:
            message = json.loads(raw_line)
        except (json.JSONDecodeError, UnicodeDecodeError):
            continue
        target = message.get("target") if isinstance(message, dict) else None
        executable = message.get("executable") if isinstance(message, dict) else None
        if (
            message.get("reason") == "compiler-artifact"
            and isinstance(target, dict)
            and target.get("name") == TEST_TARGET
            and "test" in target.get("kind", [])
            and isinstance(executable, str)
        ):
            executables.add(executable)
    if len(executables) != 1:
        raise RuntimeError(f"expected one Cargo executable for {TEST_TARGET}, found {len(executables)}")
    binary = Path(next(iter(executables)))
    if not binary.is_file():
        raise RuntimeError(f"Cargo-reported test executable does not exist: {binary.name}")
    return binary


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def fixture_records(output: str) -> list[dict[str, str]]:
    entries = {(name, digest) for name, digest in FIXTURE_LINE.findall(output)}
    return [{"name": name, "sha256": digest} for name, digest in sorted(entries)]


def fixture_set_hash(fixtures: list[dict[str, str]]) -> str:
    canonical = json.dumps(fixtures, ensure_ascii=True, separators=(",", ":")).encode("ascii")
    return hashlib.sha256(b"QA-101 fixture hash set v1\0" + canonical).hexdigest()


def observed_cases(output: str) -> dict[str, str]:
    cases: dict[str, str] = {}
    for line in output.splitlines():
        match = CASE_LINE.fullmatch(line.strip())
        if match:
            name, ec_id, result = match.groups()
            cases[name] = f"EC-{ec_id}: {result}"
    return cases


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n", encoding="utf-8")


def tool_version(args: list[str], *, cwd: Path) -> str:
    completed = command(args, cwd=cwd)
    if completed.returncode:
        return "unavailable"
    return completed.stdout.decode("utf-8", errors="replace").splitlines()[0]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qa004-root", type=Path, required=True, help="checkout of QA-004 tooling at its pinned commit")
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[1]
    qa004_root = args.qa004_root.resolve(strict=True)
    runner_os = os.environ.get("RUNNER_OS", platform.system())
    os_slug = {"Linux": "linux", "Windows": "windows", "macOS": "macos"}.get(
        runner_os, re.sub(r"[^a-z0-9-]+", "-", runner_os.lower()).strip("-") or "unknown"
    )
    output_dir = repo_root / "artifacts" / "qa-101-ec-001-005" / os_slug
    output_dir.mkdir(parents=True, exist_ok=True)
    date = datetime.now(timezone.utc).date().isoformat()
    evidence_path = f"artifacts/qa-101-ec-001-005/{os_slug}/test-output.log"
    manifest_path = output_dir / "run-manifest.json"
    manifest: dict[str, Any] = {
        "schema_version": 1,
        "automation": {
            "kind": "github-actions",
            "run_id": os.environ.get("GITHUB_RUN_ID"),
            "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
            "repository": os.environ.get("GITHUB_REPOSITORY", "Astraive/asciiwall"),
            "workflow": os.environ.get("GITHUB_WORKFLOW", "QA-101 hosted evidence"),
        },
        "issue": {"id": "QA-101", "github_number": 77},
        "date": date,
        "evidence_path": evidence_path,
        "acceptance_gaps": {
            "authenticated_acceptance_receipt": "not-issued",
            "independent_human_review": "not-performed",
            "native_product_acceptance": "not-performed",
        },
    }
    stage = "source-identity"
    try:
        commit, tree_oid, source_hash = source_identity(repo_root)
        qa004_commit = git_text(["-C", str(qa004_root), "rev-parse", "HEAD"], cwd=repo_root)
        if qa004_commit != QA004_COMMIT:
            raise RuntimeError(f"QA-004 checkout is {qa004_commit}, expected pinned {QA004_COMMIT}")
        environment = {
            "availability": "present",
            "os": f"{runner_os} {platform.release()} ({os.environ.get('ImageOS', 'hosted')})",
            "session": "headless-github-hosted-runner",
            "compositor": "not-exercised",
            "gpu": "not-exercised",
        }
        rustc_version = tool_version(["rustc", "--version"], cwd=repo_root)
        cargo_version = tool_version(["cargo", "--version"], cwd=repo_root)
        manifest.update({
            "source": {
                "commit": commit,
                "git_tree_oid": tree_oid,
                "sha256": source_hash,
                "sha256_method": "SHA-256 of `git archive --format=tar HEAD` bytes",
            },
            "qa004": {"commit": qa004_commit},
            "environment": {
                **environment,
                "image_version": os.environ.get("ImageVersion", "not-reported"),
                "rustc": rustc_version,
                "cargo": cargo_version,
                "python": platform.python_version(),
            },
            "permissions": {
                "workflow_token": {"contents": "read"},
                "artifact_upload": "GitHub Actions runtime; no additional GITHUB_TOKEN write permission",
            },
            "checkout": {
                "github_sha": os.environ.get("GITHUB_SHA"),
                "pull_request_head_sha": os.environ.get("QA101_PR_HEAD_SHA"),
            },
        })

        stage = "cargo-build"
        build = command(
            [
                "cargo", "test", "--locked", "--package", "ascii-convert", "--test", TEST_TARGET,
                "--no-run", "--message-format=json",
            ],
            cwd=repo_root,
        )
        (output_dir / "cargo-build.stdout.jsonl").write_bytes(build.stdout)
        (output_dir / "cargo-build.stderr.log").write_bytes(build.stderr)
        if build.returncode:
            manifest["result"] = {"status": "not-run", "stage": stage, "exit_code": build.returncode}
            write_json(manifest_path, manifest)
            return 1

        stage = "binary-identity"
        binary = output_executable(build.stdout)
        binary_hash = sha256_file(binary)
        manifest["test_binary"] = {"target": TEST_TARGET, "sha256": binary_hash}

        stage = "test-run"
        test_run = command([str(binary), "--nocapture", "--color", "never", "--test-threads=1"], cwd=repo_root)
        test_log = (
            f"test_target={TEST_TARGET}\n"
            "command=qa_101_input_conversion --nocapture --color never --test-threads=1\n"
            "--- stdout ---\n"
        ).encode("utf-8") + test_run.stdout + b"\n--- stderr ---\n" + test_run.stderr
        (output_dir / "test-output.log").write_bytes(test_log)
        test_output = (test_run.stdout + b"\n" + test_run.stderr).decode("utf-8", errors="replace")
        fixtures = fixture_records(test_output)
        cases = observed_cases(test_output)
        result_line = next((line.strip() for line in test_output.splitlines() if line.startswith("test result:")), "")
        exact_case_set = {value.split(":", 1)[0].removeprefix("EC-") for value in cases.values()} == EXPECTED_EC_IDS
        test_passed = (
            test_run.returncode == 0
            and len(cases) == 5
            and exact_case_set
            and all(value.endswith(": ok") for value in cases.values())
            and "5 passed; 0 failed; 0 ignored;" in result_line
        )
        fixture_hash = fixture_set_hash(fixtures)
        status = "pass" if test_passed else "fail"
        summary = (
            f"Hosted {runner_os} CI: EC-001–EC-005 {status} ({sum(value.endswith(': ok') for value in cases.values())}/5); "
            "automated only; no independent human review or native/product acceptance."
        )
        record = {
            "schema_version": 1,
            "level": "integration",
            "execution": "automated",
            "procedure_id": None,
            "ec_ids": [f"EC-{ec_id}" for ec_id in sorted(EXPECTED_EC_IDS)],
            "source_hash": source_hash,
            "binary_hash": binary_hash,
            "fixture_hash": fixture_hash,
            "environment": environment,
            "permissions": {"state": "granted", "items": ["contents-read"]},
            "result": {"status": status, "summary": summary},
            "evidence_path": evidence_path,
            "issue": "QA-101",
            "reviewer": "automated-ci",
            "date": date,
        }
        record_path = output_dir / "qa004-record.json"
        stage = "qa004-record"
        recorder = command(
            [sys.executable, str(qa004_root / "scripts" / "evidence.py"), "record", "-", "--output", str(record_path)],
            cwd=qa004_root,
            input_bytes=json.dumps(record, separators=(",", ":")).encode("utf-8"),
        )
        (output_dir / "qa004-record.stdout.json").write_bytes(recorder.stdout)
        (output_dir / "qa004-record.stderr.log").write_bytes(recorder.stderr)
        if recorder.returncode:
            manifest["result"] = {"status": "fail", "stage": stage, "exit_code": recorder.returncode}
            write_json(manifest_path, manifest)
            return 1

        stage = "qa004-validation"
        validation = command(
            [sys.executable, str(qa004_root / "scripts" / "evidence.py"), "validate", str(record_path)],
            cwd=qa004_root,
        )
        (output_dir / "qa004-validation.stdout.json").write_bytes(validation.stdout)
        (output_dir / "qa004-validation.stderr.log").write_bytes(validation.stderr)
        if validation.returncode:
            manifest["result"] = {"status": "fail", "stage": stage, "exit_code": validation.returncode}
            write_json(manifest_path, manifest)
            return 1

        record_response = json.loads(recorder.stdout)
        manifest.update({
            "fixture_hash": {"sha256": fixture_hash, "method": "SHA-256 of canonical sorted fixture name/hash pairs"},
            "fixtures": fixtures,
            "test": {
                "target": TEST_TARGET,
                "observed_cases": cases,
                "harness_summary": result_line,
                "exit_code": test_run.returncode,
                "status": status,
            },
            "result": {"status": status, "summary": summary},
            "qa004": {
                "commit": qa004_commit,
                "record_path": f"artifacts/qa-101-ec-001-005/{os_slug}/qa004-record.json",
                "record_id": record_response.get("record_id"),
                "validation": json.loads(validation.stdout),
            },
        })
        write_json(manifest_path, manifest)
        return 0 if test_passed else 1
    except Exception as exc:  # Keep partial build/test evidence uploadable on tool failures.
        (output_dir / "runner-error.log").write_text(f"{type(exc).__name__}: {exc}\n", encoding="utf-8")
        manifest["result"] = {"status": "not-run", "stage": stage, "error_type": type(exc).__name__}
        write_json(manifest_path, manifest)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
