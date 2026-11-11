#!/usr/bin/env python3
"""Read-only standard-library checks for tracked repository content."""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import runpy
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
EXCLUDED_DIRS = {".git", ".issues", "__pycache__", "target"}
FORBIDDEN_FILES = {"publication-ledger.json", "publication-ledger.lock", "backlog-audit.json",
                   "backlog-creation-ledger.json", "backlog-dependency-graph.md",
                   "backlog-inventory.md", "backlog-requirement-traceability.md", "issue.json"}


def load_validator_primitives():
    # run_path executes definitions only; validate-backlog.py guards its CLI entry point.
    return runpy.run_path(str(ROOT / "scripts" / "validate-backlog.py"))


def repository_files() -> tuple[list[Path], list[str]]:
    files: list[Path] = []
    errors: list[str] = []
    for directory, dirs, names in os.walk(ROOT, followlinks=False):
        current = Path(directory)
        for name in dirs:
            path = current / name
            if name == ".issues":
                errors.append(f"{path.relative_to(ROOT).as_posix()}: external backlog directory must stay outside the repository")
            elif path.is_symlink() and name not in EXCLUDED_DIRS:
                errors.append(f"{path.relative_to(ROOT).as_posix()}: symlinked repository directories are not allowed")
        dirs[:] = sorted(name for name in dirs if name not in EXCLUDED_DIRS and not (current / name).is_symlink())
        for name in sorted(names):
            path = current / name
            relative = path.relative_to(ROOT).as_posix()
            if name in FORBIDDEN_FILES:
                errors.append(f"{relative}: forbidden external backlog artifact; keep it outside the repository")
            if path.is_symlink():
                errors.append(f"{relative}: symlinked repository files are not checked or allowed")
                continue
            if path.is_file():
                files.append(path)
    return files, errors


def without_inline_code(text: str) -> str:
    """Mask CommonMark backtick spans without changing link offsets or newlines."""
    runs = list(re.finditer(r"`+", text))
    masked = list(text)
    index = 0
    while index < len(runs):
        opening = runs[index]
        start, width = opening.start(), len(opening[0])
        previous = start - 1
        while previous >= 0 and text[previous] == "\\":
            previous -= 1
        if (start - previous - 1) % 2:
            start += 1
            width -= 1
        if width:
            closing = next(
                (candidate for candidate in range(index + 1, len(runs))
                 if len(runs[candidate][0]) == width),
                None,
            )
            if closing is not None:
                for position in range(start, runs[closing].end()):
                    if masked[position] != "\n":
                        masked[position] = " "
                index = closing + 1
                continue
        index += 1
    return "".join(masked)


def main() -> int:
    try:
        markdown = load_validator_primitives()
    except Exception as exc:
        print(f"ERROR scripts/validate-backlog.py: cannot load Markdown primitives: {exc}", file=sys.stderr)
        return 1
    files, errors = repository_files()
    markdown_files = {path for path in files if path.suffix.lower() == ".md"}
    json_files = [path for path in files if path.suffix.lower() == ".json"]

    for path in json_files:
        relative = path.relative_to(ROOT).as_posix()
        try:
            json.loads(path.read_text(encoding="utf-8-sig"))
        except (OSError, UnicodeError, json.JSONDecodeError) as exc:
            errors.append(f"{relative}: invalid UTF-8 JSON ({exc})")

    for path in sorted(markdown_files):
        relative = path.relative_to(ROOT).as_posix()
        try:
            text = path.read_text(encoding="utf-8-sig")
        except (OSError, UnicodeError) as exc:
            errors.append(f"{relative}: cannot read UTF-8 Markdown ({exc})")
            continue
        lines = text.splitlines()
        fence: tuple[str, int] | None = None
        for line in lines:
            match = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line)
            if fence is None:
                if match:
                    fence = (match[1][0], len(match[1]))
            elif match and match[1][0] == fence[0] and len(match[1]) >= fence[1] and not match[2].strip():
                fence = None
        if fence is not None:
            errors.append(f"{relative}:{len(lines)}: unclosed Markdown code fence")

        # Reuse the validator's link, fence, heading and GitHub-compatible anchor primitives.
        unfenced = without_inline_code(markdown["unfenced"](text))
        source_anchors = markdown["anchors"](text)
        for link in markdown["LINK"].finditer(unfenced):
            raw_target = link.group(1).strip().strip("<>")
            parsed = urlsplit(raw_target)
            if parsed.scheme or parsed.netloc or raw_target.startswith("//"):
                continue
            target_text = unquote(parsed.path)
            target = (path.parent / target_text).resolve() if target_text else path.resolve()
            try:
                target.relative_to(ROOT)
            except ValueError:
                errors.append(f"{relative}: local link escapes repository: {raw_target}")
                continue
            if not target.exists() or not target.is_file():
                errors.append(f"{relative}: local link target does not exist: {raw_target}")
                continue
            if parsed.fragment:
                if target.suffix.lower() == ".md":
                    try:
                        target_content = target.read_text(encoding="utf-8-sig")
                    except (OSError, UnicodeError) as exc:
                        errors.append(f"{target.relative_to(ROOT).as_posix()}: cannot read UTF-8 Markdown ({exc})")
                        continue
                    anchors = source_anchors if target == path.resolve() else markdown["anchors"](target_content)
                    if unquote(parsed.fragment) not in anchors:
                        errors.append(f"{relative}: fragment #{parsed.fragment} not found in {target.relative_to(ROOT).as_posix()}")
                else:
                    errors.append(f"{relative}: fragment links are supported only for Markdown targets: {raw_target}")

    if errors:
        for error in errors:
            print(f"ERROR {error}", file=sys.stderr)
        print(f"FAIL: repository checks found {len(errors)} error(s).", file=sys.stderr)
        return 1
    print(f"PASS: checked {len(markdown_files)} Markdown and {len(json_files)} JSON files; no external backlog artifacts.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
