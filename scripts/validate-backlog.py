#!/usr/bin/env python3
"""Validate external local backlog and optionally write derived, unpublished reports.

Python 3.12+, standard library only. No network, subprocesses, or ticket mutation.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict, deque
import html
import json
import math
import os
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BACKLOG_ROOT = ROOT.parent / ".issues"
REPOSITORIES = ("asciiwall", "asciiwall-library")
_LOAD_FAILED = object()

def _safe_resolve(path: Path) -> tuple[Path, OSError | None]:
    try:
        return path.resolve(), None
    except OSError as exc:
        return path, exc


def is_in_repo(path: Path, source_root: Path) -> bool:
    if path.is_relative_to(source_root) or path.is_relative_to(ROOT):
        return True
    if ".git" in path.parts:
        return True
    for base in {source_root.parent, ROOT.parent}:
        for repo in REPOSITORIES:
            candidate = base / repo
            if path.is_relative_to(candidate):
                return True
            if candidate.exists():
                resolved_candidate, err = _safe_resolve(candidate)
                if err is None and path.is_relative_to(resolved_candidate):
                    return True
    for ancestor in (path, *path.parents):
        if source_root.is_relative_to(ancestor) or ROOT.is_relative_to(ancestor):
            continue
        if (ancestor / ".git").exists():
            return True
    return False
# Frozen original documentation inventory; generated and future planning docs are excluded.
ORIGINAL_GROUPS = {
    "": "start-here readme documentation-index",
    "ui": "library-and-import-flows information-architecture editor-and-preview accessibility-and-localization",
    "tech-stack": "stack-decisions dependency-selection-and-version-policy build-environment-and-toolchain",
    "settings": "validation-and-migrations settings-reference settings-precedence-and-presets",
    "security": "untrusted-media-and-packages threat-model supply-chain-and-updates privacy-and-network-policy plugin-and-shader-policy",
    "research": "source-register research-summary prior-art-and-differentiation feasibility-matrix assumptions-and-open-questions",
    "reference": "glossary",
    "quality": "test-strategy edge-case-matrix documentation-verification diagnostics-and-troubleshooting benchmark-plan acceptance-criteria",
    "product": "vision-and-scope requirements-traceability mode-comparison feature-catalog",
    "platform": "windows support-policy power-lock-and-session permissions-and-privacy multi-monitor-and-scaling macos linux-x11 linux-wayland linux-kde-plasma linux-gnome",
    "plans": "packages-implementation-plan hardening-implementation-plan engine-implementation-plan desktop-implementation-plan",
    "package": "scene-schema manifest-schema import-and-validation format-specification export-and-reproducibility examples-and-test-vectors compatibility-and-migrations",
    "library": "starter-wallpaper-catalog repository-structure release-pipeline-and-distribution licensing-and-attribution contribution-and-review catalog-and-index-schema",
    "interaction": "window-aware-behavior transitions-and-motion refresh-and-live-editing pointer-and-click-reactions automation-and-local-signals",
    "engine": "video-timing-and-decoding quality-evaluation procedural-scenes pre-rendering-and-export performance-and-power gpu-rendering glyph-matching-and-fonts effects-and-scene-graph conversion-pipeline color-and-image-processing ascii-clip-format",
    "delivery": "risks-and-spikes packaging-and-signing milestones-and-dependencies implementation-backlog contributor-workflow ci-and-release",
    "decisions": "0004-offline-core-and-static-library 0003-declarative-portable-packages 0002-capability-aware-native-hosts 0001-native-rust-engine",
    "cli": "exit-codes-and-errors command-reference automation-and-ipc",
    "architecture": "system-overview storage-and-cache runtime-state-machine resource-ownership-and-shutdown repository-structure process-model-and-ipc event-model-and-coordinate-spaces data-model-and-contracts crate-boundaries capabilities-and-fallbacks",
}
ORIGINAL_DOCS = frozenset("docs/" + (directory + "/" if directory else "") + name + ".md"
                          for directory, names in ORIGINAL_GROUPS.items() for name in names.split())
MANDATORY = ("Metadata", "Summary", "Context", "Why It Matters", "Scope", "In Scope", "Out of Scope",
             "Where to Look", "Assets and Dependencies", "Requirements", "Technical Notes",
             "Acceptance Criteria", "Test Plan", "Negative Cases and Edge Cases", "References",
             "Dependencies", "Depends On", "Blocks / Unblocks", "Related Issues", "Tracking")
WORK_UNITS = {f"{prefix}{n:02}" for prefix, count in (("E", 12), ("D", 12), ("P", 8), ("H", 8))
              for n in range(1, count + 1)}
LINK = re.compile(r"(?<!!)\[[^\]\n]*(?:\][^\]\n]*)?\]\(\s*(<[^>]+>|[^\s)]+)(?:\s+[\"'][^\n]*?[\"'])?\s*\)")
TICKET_LINK = re.compile(
    r"(?<!!)\[([A-Z][A-Z0-9]*(?:-[A-Z0-9]+)+)\]\(\s*(?:<((?:\.\./|\.\./\.\./(?:asciiwall|asciiwall-library)/)\1/CONTENT\.md(?:#[^>\s]*)?)>|((?:\.\./|\.\./\.\./(?:asciiwall|asciiwall-library)/)\1/CONTENT\.md(?:#[^)\s]*)?))(?:\s+[\"'][^\n]*?[\"'])?\s*\)"
)


def plain(value: str) -> str:
    value = html.unescape(value)
    value = re.sub(r"\\([\\`*_{}\[\]()#+.!|<>-])", r"\1", value)
    value = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", value)
    return " ".join(re.findall(r"[\w]+", value.casefold()))


def unfenced(text: str) -> str:
    lines = []
    fence = None
    for line in text.splitlines():
        match = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line)
        if fence:
            if match and match[1][0] == fence[0] and len(match[1]) >= len(fence) and not match[2].strip():
                fence = None
            lines.append("")
        elif match:
            fence = match[1]
            lines.append("")
        else:
            lines.append(line)
    return "\n".join(lines)


def headings(text: str) -> list[tuple[int, str, int]]:
    lines = unfenced(text).splitlines()
    result = []
    for i, line in enumerate(lines):
        match = re.match(r"^ {0,3}(#{1,6})\s+(.+?)\s*#*\s*$", line)
        if match:
            result.append((len(match[1]), match[2], i))
        elif i and re.fullmatch(r" {0,3}(?:=+|-+)\s*", line) and lines[i - 1].strip():
            result.append((1 if "=" in line else 2, lines[i - 1].strip(), i - 1))
    return result


def sections(text: str) -> dict[str, str]:
    lines = text.splitlines()
    hs = headings(text)
    result = {}
    for index, (level, name, line) in enumerate(hs):
        end = next((h[2] for h in hs[index + 1:] if h[0] <= level), len(lines))
        result[plain(name)] = "\n".join(lines[line + 1:end])
    return result


def anchors(text: str) -> set[str]:
    counts = Counter()
    found = set()
    for _, heading, _ in headings(text):
        heading = html.unescape(re.sub(r"<[^>]+>", "", heading)).lower().replace(" ", "-")
        base = re.sub(r"[^\w\-]", "", heading)
        suffix = "" if not counts[base] else f"-{counts[base]}"
        found.add(base + suffix)
        counts[base] += 1
    return found


class Audit:
    def __init__(self, backlog_root: Path | None = None, source_root: Path | None = None) -> None:
        self.raw_backlog_root = DEFAULT_BACKLOG_ROOT if backlog_root is None else Path(backlog_root)
        self.raw_source_root = ROOT if source_root is None else Path(source_root)
        self.backlog_root, self._backlog_resolve_error = _safe_resolve(self.raw_backlog_root)
        self.source_root, self._source_resolve_error = _safe_resolve(self.raw_source_root)
        self.backlog_label = self.backlog_root.name or ".issues"
        self._backlog_root_writable = False
        self.errors: list[dict[str, str]] = []
        self.tickets: dict[str, dict] = {}
        self.bodies: dict[str, str] = {}
        self.sources: dict[str, str] = {}
        self.coverage: list[dict] = []
        self.mappings: list[dict] = []
        self.order: list[str] = []
        self.predecessors: dict[str, list[str]] = {}
        self.children: dict[str, list[str]] = defaultdict(list)

    def error(self, location: str, message: str) -> None:
        self.errors.append({"location": location, "message": message})

    def format_error(self, exc: BaseException) -> str:
        if isinstance(exc, OSError) and exc.strerror:
            if isinstance(exc.filename, (str, Path)):
                return f"{exc.strerror}: {self.label(Path(exc.filename))}"
            return exc.strerror
        return str(exc)

    def read(self, path: Path) -> str:
        try:
            return path.read_text(encoding="utf-8-sig")
        except (OSError, UnicodeError) as exc:
            self.error(self.label(path), f"Cannot read: {self.format_error(exc)}")
            return ""

    def label(self, path: Path) -> str:
        for base, prefix in ((self.backlog_root, self.backlog_label), (self.source_root, "")):
            try:
                rel = path.relative_to(base).as_posix()
            except ValueError:
                continue
            if not prefix:
                return rel or "."
            return f"{prefix}/{rel}" if rel != "." else prefix
        return path.as_posix()

    def can_write_reports(self) -> bool:
        return self._backlog_root_writable

    def validate_source_root(self) -> bool:
        if self._source_resolve_error is not None:
            self.error(self.label(self.raw_source_root), f"Cannot resolve source root: {self.format_error(self._source_resolve_error)}")
            return False
        if not self.source_root.is_dir():
            self.error(self.label(self.source_root), "Missing or invalid source root directory")
            return False
        docs_dir = self.source_root / "docs"
        if docs_dir.is_symlink():
            self.error("docs", "Refusing symlinked source docs directory")
            return False
        if not docs_dir.is_dir():
            self.error("docs", "Missing docs directory in source root")
            return False
        resolved_docs, err = _safe_resolve(docs_dir)
        if err is not None or not resolved_docs.is_relative_to(self.source_root):
            self.error("docs", "Source docs directory escapes source root")
            return False
        return True

    def validate_backlog_root(self) -> bool:
        ok = True
        if self.raw_backlog_root.is_symlink():
            self.error(self.label(self.raw_backlog_root), "Refusing symlinked backlog root")
            ok = False
        if self._backlog_resolve_error is not None:
            self.error(self.label(self.raw_backlog_root), f"Cannot resolve backlog root: {self.format_error(self._backlog_resolve_error)}")
            self._backlog_root_writable = False
            return False
        raw_abs = Path(os.path.abspath(self.raw_backlog_root))
        if is_in_repo(self.backlog_root, self.source_root) or is_in_repo(raw_abs, self.source_root):
            self.error(self.label(self.backlog_root), "Backlog root must live outside git repositories")
            ok = False
        elif self.source_root.is_relative_to(self.backlog_root) or ROOT.is_relative_to(self.backlog_root):
            self.error(self.label(self.backlog_root), "Backlog root cannot contain source repository")
            ok = False
        if not self.raw_backlog_root.is_symlink():
            if not self.backlog_root.exists():
                self.error(self.label(self.backlog_root), "Missing backlog root directory")
                ok = False
            elif not self.backlog_root.is_dir():
                self.error(self.label(self.backlog_root), "Backlog root is not a directory")
                ok = False
        self._backlog_root_writable = ok
        return ok

    def load(self, path: Path):
        if path.is_symlink():
            self.error(self.label(path), "Refusing symlinked JSON file")
            return _LOAD_FAILED
        resolved, err = _safe_resolve(path)
        if err is not None:
            self.error(self.label(path), f"Cannot resolve JSON path: {self.format_error(err)}")
            return _LOAD_FAILED
        if not resolved.is_relative_to(self.backlog_root):
            self.error(self.label(path), "JSON path escapes backlog root")
            return _LOAD_FAILED
        try:
            text = path.read_text(encoding="utf-8-sig")
        except (OSError, UnicodeError) as exc:
            self.error(self.label(path), f"Cannot read: {self.format_error(exc)}")
            return _LOAD_FAILED
        if not text.strip():
            self.error(self.label(path), "Empty JSON file")
            return _LOAD_FAILED
        def pairs(values):
            obj = {}
            for key, value in values:
                if key in obj:
                    raise ValueError(f"Duplicate JSON property {key!r}")
                obj[key] = value
            return obj
        try:
            return json.loads(text, object_pairs_hook=pairs,
                              parse_constant=lambda value: (_ for _ in ()).throw(ValueError(f"Nonfinite JSON number {value}")))
        except (ValueError, RecursionError) as exc:
            self.error(self.label(path), f"Invalid JSON: {exc}")
            return _LOAD_FAILED

    def validate_project_views(self) -> None:
        path = self.backlog_root / "project.json"
        location = self.label(path)
        project = self.load(path)
        if not isinstance(project, dict):
            self.error(location, "Missing or invalid project registry object")
            return
        fields = project.get("fields")
        views = project.get("views")
        if not isinstance(fields, list) or not isinstance(views, list):
            self.error(location, "Project registry must contain fields and views arrays")
            return
        field_names = {field.get("name") for field in fields if isinstance(field, dict) and isinstance(field.get("name"), str)}
        if len(field_names) != len(fields):
            self.error(location, "Project field registry contains invalid or duplicate field names")
        if len(views) != 18:
            self.error(location, f"Project registry must contain 18 views, found {len(views)}")
        counts = Counter(view.get("layout") for view in views
                         if isinstance(view, dict) and isinstance(view.get("layout"), str))
        if counts != Counter({"TABLE_LAYOUT": 17, "BOARD_LAYOUT": 1}):
            self.error(location, f"Expected 17 table views and 1 board view, found {dict(counts)}")
        names = [view.get("name") for view in views if isinstance(view, dict) and isinstance(view.get("name"), str)]
        if len(set(names)) != len(names):
            self.error(location, "Project view registry contains duplicate names")
        current_group_keys = {"group_by", "secondary_group_by", "slice_by", "sort_by"}
        for index, view in enumerate(views):
            item_location = f"{location}.views[{index}]"
            if not isinstance(view, dict):
                self.error(item_location, "View must be an object")
                continue
            if not current_group_keys.issubset(view):
                self.error(item_location, "View must declare group_by, secondary_group_by, slice_by, and sort_by")
            legacy_aliases = {"secondary_grouping", "secondary_grouping_by", "secondary_group_by_legacy",
                              "grouping_by", "grouping", "slice", "slice_field", "slice_by_field",
                              "group_by_secondary", "group_by_fields", "sort_fields"}
            legacy = sorted(key for key in view if key in legacy_aliases)
            if legacy:
                self.error(item_location, f"Unsupported legacy grouping/sorting aliases: {', '.join(legacy)}")
            layout = view.get("layout")
            primary = view.get("group_by")
            secondary = view.get("secondary_group_by")
            slice_by = view.get("slice_by")
            sort_by = view.get("sort_by")
            if not isinstance(primary, str) or primary not in field_names:
                self.error(item_location, f"Unknown primary grouping field {primary!r}")
            if not isinstance(sort_by, list) or len(sort_by) != 2 or any(not isinstance(field, str) for field in sort_by) or len(set(sort_by)) != 2:
                self.error(item_location, "sort_by must contain exactly two distinct fields in explicit order")
            elif any(field not in field_names for field in sort_by):
                self.error(item_location, "sort_by references a field absent from the field registry")
            if layout == "TABLE_LAYOUT":
                if secondary is not None:
                    self.error(item_location, "TABLE_LAYOUT requires secondary_group_by: null; its secondary field belongs in slice_by")
                if not isinstance(slice_by, str) or slice_by not in field_names:
                    self.error(item_location, f"Unknown table slice_by field {slice_by!r}")
            elif layout == "BOARD_LAYOUT":
                if not isinstance(secondary, str) or secondary not in field_names:
                    self.error(item_location, f"Unknown board secondary grouping field {secondary!r}")
                if slice_by is not None:
                    self.error(item_location, "BOARD_LAYOUT requires slice_by: null")
            else:
                self.error(item_location, f"Unsupported layout {layout!r}")
            visible = view.get("visible_fields")
            if not isinstance(visible, list) or any(not isinstance(field, str) or field not in field_names for field in visible):
                self.error(item_location, "visible_fields must reference only registered fields")

    def schema(self, value, rule: dict, location: str, root: dict, emit: bool = True) -> bool:
        """Interpret exactly the Draft 2020-12 vocabulary used by schema.json.

        Unsupported keywords fail closed; this is not a general JSON Schema library.
        """
        failures = []
        known = {"$schema", "$id", "$defs", "title", "description", "$ref", "type", "const", "enum", "anyOf",
                 "allOf", "if", "then", "else", "required", "properties", "additionalProperties", "items",
                 "minItems", "uniqueItems", "minLength", "pattern", "exclusiveMinimum", "maximum"}
        for key in rule.keys() - known:
            failures.append(f"Unsupported schema keyword {key!r}")
        if "$ref" in rule:
            ref = rule["$ref"]
            target = root
            try:
                if not ref.startswith("#/"):
                    raise KeyError(ref)
                for part in ref[2:].split("/"):
                    target = target[part.replace("~1", "/").replace("~0", "~")]
                if not self.schema(value, target, location, root, emit):
                    failures.append(f"Does not match {ref}")
            except (KeyError, TypeError):
                failures.append(f"Unresolvable local schema reference {ref}")
        types = {"object": isinstance(value, dict), "array": isinstance(value, list), "string": isinstance(value, str),
                 "integer": isinstance(value, int) and not isinstance(value, bool),
                 "number": (isinstance(value, int) and not isinstance(value, bool)) or (isinstance(value, float) and math.isfinite(value)),
                 "null": value is None, "boolean": isinstance(value, bool)}
        if "type" in rule and not types.get(rule["type"], False):
            failures.append(f"Expected {rule['type']}, got {type(value).__name__}")
        if "const" in rule and (value != rule["const"] or type(value) is not type(rule["const"])):
            failures.append(f"Expected constant {rule['const']!r}")
        if "enum" in rule and value not in rule["enum"]:
            failures.append(f"Expected one of {rule['enum']!r}")
        if "anyOf" in rule and not any(self.schema(value, item, location, root, False) for item in rule["anyOf"]):
            failures.append("Does not match any allowed schema alternative")
        for item in rule.get("allOf", []):
            if not self.schema(value, item, location, root, emit):
                failures.append("Does not match allOf constraint")
        if "if" in rule:
            branch = "then" if self.schema(value, rule["if"], location, root, False) else "else"
            if branch in rule and not self.schema(value, rule[branch], location, root, emit):
                failures.append(f"Does not match conditional {branch} constraint")
        if isinstance(value, dict):
            for key in rule.get("required", []):
                if key not in value:
                    failures.append(f"Missing required field {key}")
            properties = rule.get("properties", {})
            if rule.get("additionalProperties") is False:
                for key in value.keys() - properties.keys():
                    failures.append(f"Unexpected field {key}")
            for key in value.keys() & properties.keys():
                if not self.schema(value[key], properties[key], f"{location}.{key}", root, emit):
                    failures.append(f"Invalid field {key}")
        if isinstance(value, list):
            if len(value) < rule.get("minItems", 0):
                failures.append(f"Requires at least {rule['minItems']} items")
            if rule.get("uniqueItems") and len({json.dumps(v, sort_keys=True) for v in value}) != len(value):
                failures.append("Duplicate array entries")
            for index, item in enumerate(value):
                if "items" in rule and not self.schema(item, rule["items"], f"{location}[{index}]", root, emit):
                    failures.append(f"Invalid array item {index}")
        if isinstance(value, str):
            if len(value) < rule.get("minLength", 0):
                failures.append("String too short")
            if "pattern" in rule and not re.search(rule["pattern"], value):
                failures.append(f"Does not match {rule['pattern']}")
        if types["number"]:
            if "maximum" in rule and value > rule["maximum"]:
                failures.append(f"Exceeds maximum {rule['maximum']}")
            if "exclusiveMinimum" in rule and value <= rule["exclusiveMinimum"]:
                failures.append(f"Must exceed {rule['exclusiveMinimum']}")
        if emit:
            for message in failures:
                self.error(location, message)
        return not failures

    def source(self, value, location: str) -> bool:
        if not isinstance(value, dict) or set(value) != {"path", "section"} or not all(isinstance(v, str) for v in value.values()):
            self.error(location, "Source must contain exactly string path and section")
            return False
        path, heading = value["path"], value["section"]
        if path not in ORIGINAL_DOCS:
            self.error(location, f"Not an original source path: {path}")
            return False
        if heading not in {h[1] for h in headings(self.sources.get(path, ""))}:
            self.error(location, f"Exact source heading not found: {path} :: {heading}")
            return False
        return True

    def tickets_load(self, schema: dict) -> None:
        seen_titles = {}
        for repo in REPOSITORIES:
            directory = self.backlog_root / repo
            if directory.is_symlink():
                self.error(self.label(directory), "Refusing symlinked repository ticket directory")
                continue
            if not directory.is_dir():
                self.error(self.label(directory), "Missing repository ticket directory")
                continue
            resolved_dir, err = _safe_resolve(directory)
            if err is not None or not resolved_dir.is_relative_to(self.backlog_root):
                self.error(self.label(directory), "Repository ticket directory escapes backlog root")
                continue
            for folder in sorted(directory.iterdir()):
                if folder.is_symlink():
                    self.error(self.label(folder), "Refusing symlinked ticket directory")
                    continue
                if not folder.is_dir():
                    self.error(self.label(folder), "Expected one directory per issue")
                    continue
                resolved_folder, err = _safe_resolve(folder)
                if err is not None or not resolved_folder.is_relative_to(self.backlog_root):
                    self.error(self.label(folder), "Ticket directory escapes backlog root")
                    continue
                location = self.label(folder / "issue.json")
                issue = self.load(folder / "issue.json")
                content_path = folder / "CONTENT.md"
                if content_path.is_symlink():
                    self.error(self.label(content_path), "Refusing symlinked CONTENT.md")
                    body = ""
                else:
                    resolved_content, err = _safe_resolve(content_path)
                    if err is not None or not resolved_content.is_relative_to(self.backlog_root):
                        self.error(self.label(content_path), "CONTENT.md escapes backlog root")
                        body = ""
                    else:
                        body = self.read(content_path)
                if issue is _LOAD_FAILED or not self.schema(issue, schema, location, schema):
                    continue
                ident = issue["id"]
                if folder.name != ident or issue["repository"] != repo:
                    self.error(location, "ID/repository do not match directory path")
                if ident in self.tickets:
                    self.error(location, f"Duplicate global issue ID {ident}")
                    continue
                self.tickets[ident], self.bodies[ident] = issue, body
                if not issue["title"].startswith(f"[{ident}] "):
                    self.error(location, "Title must begin with its own [ID]")
                title = plain(re.sub(r"^\[[^]]+\]\s*", "", issue["title"]))
                if title in seen_titles:
                    self.error(location, f"Duplicate descriptive title with {seen_titles[title]}")
                seen_titles[title] = ident
                for source in issue["sources"]:
                    self.source(source, ident)
                self.body(issue, body, folder)
                for extra in sorted(folder.iterdir()):
                    if extra.name not in {"CONTENT.md", "issue.json"}:
                        self.error(self.label(extra), "Unexpected file in ticket directory")

    def body(self, issue: dict, body: str, folder: Path) -> None:
        ident = issue["id"]
        parts = sections(body)
        for heading in MANDATORY:
            if not parts.get(plain(heading), "").strip():
                self.error(ident, f"Missing or empty CONTENT heading: {heading}")
        if not any(level == 1 and plain(name) == plain(issue["title"]) for level, name, _ in headings(body)):
            self.error(ident, "CONTENT H1 title disagrees with JSON title")
        metadata = plain(parts.get("metadata", ""))
        for field in ("id", "repository", "milestone", "phase", "priority", "size", "type", "area", "status"):
            if not re.search(r"(?<!\w)" + re.escape(plain(str(issue[field]))) + r"(?!\w)", metadata):
                self.error(ident, f"CONTENT Metadata omits/disagrees with {field}={issue[field]!r}")
        for platform in issue["platforms"]:
            if plain(platform) not in metadata.split():
                self.error(ident, f"CONTENT Metadata omits platform {platform}")
        effort = str(issue["effort_days"]).removesuffix(".0")
        if not re.search(r"(?<![\d.])" + re.escape(effort) + r"(?:\.0)?(?![\d.])", parts.get("metadata", "")):
            self.error(ident, "CONTENT Metadata effort disagrees with effort_days")
        for field in ("parent", "epic"):
            if issue[field] and plain(issue[field]) not in plain(parts.get("metadata", "") + parts.get("tracking", "")):
                self.error(ident, f"CONTENT omits {field} {issue[field]}")
        for field, heading in (("requirements", "requirements"), ("acceptance_criteria", "acceptance criteria")):
            content = plain(parts.get(heading, ""))
            for item in issue[field]:
                if plain(item) not in content:
                    self.error(ident, f"CONTENT {heading} disagrees with JSON: {item}")
        if not re.search(r"(?m)^\s*\d+[.)]\s+", parts.get("requirements", "")):
            self.error(ident, "Requirements must contain numbered items")
        if len(re.findall(r"(?m)^\s*[-*]\s+\[[ xX]\]\s+", parts.get("acceptance criteria", ""))) < len(issue["acceptance_criteria"]):
            self.error(ident, "Acceptance Criteria must contain a checkbox per JSON criterion")
        for test in issue["test_plan"]:
            for field in ("scenario", "command", "expected"):
                if plain(test[field]) not in plain(parts.get("test plan", "")):
                    self.error(ident, f"CONTENT Test Plan omits {field}: {test[field]}")
        for field, heading in (("depends_on", "depends on"), ("related", "related issues")):
            listed = {match[1] for match in TICKET_LINK.finditer(unfenced(parts.get(heading, "")))}
            if listed != set(issue[field]):
                self.error(ident, f"CONTENT {heading} IDs {sorted(listed)} != JSON {sorted(issue[field])}")
        if "local only" not in plain(parts.get("tracking", "")):
            self.error(ident, "Tracking must explicitly state local-only publication")
        if "https://github.com/orgs/Astraive/projects/4" not in body:
            self.error(ident, "Tracking must name the intended project URL (without publishing)")
        if "evidence" not in plain(parts.get("tracking", "")):
            self.error(ident, "Tracking must name a verification evidence location")
        master = "ROADMAP-001" if issue["repository"] == "asciiwall" else "LIB-ROADMAP-001"
        if master not in parts.get("tracking", ""):
            self.error(ident, f"Tracking must name master {master}")
        for path in issue["proposed_paths"]:
            if path not in body:
                self.error(ident, f"CONTENT omits proposed path {path}")
        linked_sources = set()
        for match in LINK.finditer(unfenced(body)):
            href = html.unescape(match[1].strip("<>"))
            parsed = urlsplit(href)
            if parsed.scheme or parsed.netloc:
                if re.search(r"github\.com/[^/]+/[^/]+/(?:issues|pull)/\d+", href):
                    self.error(ident, f"Remote issue link forbidden for local backlog: {href}")
                continue
            raw_target = (folder / unquote(parsed.path)) if parsed.path else (folder / "CONTENT.md")
            if raw_target.is_symlink():
                self.error(ident, f"Refusing symlinked local link target: {href}")
                continue
            target, err = _safe_resolve(raw_target)
            if err is not None:
                self.error(ident, f"Cannot resolve local link target {href}: {self.format_error(err)}")
                continue
            in_backlog = target.is_relative_to(self.backlog_root)
            in_source = target.is_relative_to(self.source_root)
            if not (in_backlog or in_source):
                self.error(ident, f"Local link escapes backlog and source roots: {href}")
                continue
            if not target.is_file():
                self.error(ident, f"Local link target missing: {href}")
                continue
            text = None
            if in_source:
                relative = target.relative_to(self.source_root).as_posix()
                if relative in ORIGINAL_DOCS:
                    linked_sources.add(relative)
                if parsed.fragment and target.suffix.lower() == ".md":
                    text = self.sources.get(relative)
            if parsed.fragment and target.suffix.lower() == ".md":
                if text is None:
                    text = self.read(target)
                if unquote(parsed.fragment) not in anchors(text):
                    self.error(ident, f"Local link heading anchor missing: {href}")
        for source in issue["sources"]:
            if source["path"] not in linked_sources:
                self.error(ident, f"References need a valid relative Markdown link to {source['path']}")

    def graph(self) -> None:
        for ident, issue in self.tickets.items():
            parent = issue["parent"]
            if issue["kind"] == "roadmap":
                expected = "ROADMAP-001" if issue["repository"] == "asciiwall" else "LIB-ROADMAP-001"
                if ident != expected:
                    self.error(ident, f"Unexpected roadmap ID; expected {expected}")
            elif parent not in self.tickets:
                self.error(ident, f"Undefined parent {parent}")
            else:
                self.children[parent].append(ident)
                if self.tickets[parent]["repository"] != issue["repository"]:
                    self.error(ident, "Parent tree crosses repository boundary")
                if self.tickets[parent]["kind"] == "gate":
                    self.error(ident, "A gate cannot contain children")
                if issue["kind"] == "epic" and self.tickets[parent]["kind"] != "roadmap":
                    self.error(ident, "Epic must be directly parented to its roadmap")
            ancestry, cursor = set(), ident
            while cursor in self.tickets:
                if cursor in ancestry:
                    self.error(ident, f"Parent cycle through {cursor}")
                    break
                ancestry.add(cursor)
                cursor = self.tickets[cursor]["parent"]
            if issue["kind"] in {"task", "gate"}:
                epic = issue["epic"]
                if epic not in ancestry or epic not in self.tickets or self.tickets[epic]["kind"] != "epic":
                    self.error(ident, "epic must identify an actual ancestor epic")
            for field in ("depends_on", "related"):
                for ref in issue[field]:
                    if ref not in self.tickets:
                        self.error(ident, f"Undefined {field} reference {ref}")
                    elif ref == ident:
                        self.error(ident, f"Self reference in {field}")
                    elif field == "depends_on" and self.tickets[ref]["kind"] in {"epic", "roadmap"}:
                        self.error(ident, f"Aggregate {ref} cannot be a hard blocker")
            overlap = set(issue["depends_on"]) & set(issue["related"])
            if overlap:
                self.error(ident, f"Hard blockers redundantly listed as related: {sorted(overlap)}")
        for ident, issue in self.tickets.items():
            if issue["kind"] in {"roadmap", "epic"} and not self.children[ident]:
                self.error(ident, "Container has no mapped child issues")
        for repo, prefix, count, master in (("asciiwall", "M", 11, "ROADMAP-001"),
                                            ("asciiwall-library", "L", 5, "LIB-ROADMAP-001")):
            if master not in self.tickets:
                self.error(repo, f"Missing master {master}")
            gates = Counter(i["milestone"] for i in self.tickets.values() if i["repository"] == repo and i["kind"] == "gate")
            for n in range(count):
                if gates[f"{prefix}{n}"] != 1:
                    self.error(repo, f"Requires exactly one {prefix}{n} gate, found {gates[f'{prefix}{n}']}")
        self.predecessors = {ident: [ref for ref in issue["depends_on"] if ref in self.tickets]
                             for ident, issue in self.tickets.items()}
        indegree = {ident: len(refs) for ident, refs in self.predecessors.items()}
        outgoing = defaultdict(list)
        for ident, refs in self.predecessors.items():
            for ref in refs:
                outgoing[ref].append(ident)
        ready = deque(sorted(ident for ident, degree in indegree.items() if not degree))
        while ready:
            ident = ready.popleft()
            self.order.append(ident)
            for child in sorted(outgoing[ident]):
                indegree[child] -= 1
                if not indegree[child]:
                    ready.append(child)
        if len(self.order) != len(self.tickets):
            self.error("dependencies", f"Blocker cycle or cycle-dependent nodes: {sorted(set(self.tickets) - set(self.order))}")
            return
        ancestors = {}
        for ident in self.order:
            ancestors[ident] = set(self.predecessors[ident])
            for ref in self.predecessors[ident]:
                ancestors[ident].update(ancestors[ref])
            for ref in self.predecessors[ident]:
                via = [other for other in self.predecessors[ident] if other != ref and ref in ancestors[other]]
                if via:
                    self.error(ident, f"Redundant blocker {ref}; already reachable through {', '.join(sorted(via))}")
        for ident, body in self.bodies.items():
            declared = {match[1] for match in TICKET_LINK.finditer(unfenced(sections(body).get("blocks unblocks", "")))}
            actual = set(outgoing[ident])
            if declared != actual:
                self.error(ident, f"CONTENT Blocks / Unblocks IDs {sorted(declared)} != actual {sorted(actual)}")

    def coverage_load(self) -> None:
        coverage_dir = self.backlog_root / "coverage"
        if coverage_dir.is_symlink():
            self.error(self.label(coverage_dir), "Refusing symlinked coverage directory")
            return
        if not coverage_dir.is_dir():
            self.error(self.label(coverage_dir), "Missing coverage directory")
            paths = []
        else:
            resolved_coverage, err = _safe_resolve(coverage_dir)
            if err is not None or not resolved_coverage.is_relative_to(self.backlog_root):
                self.error(self.label(coverage_dir), "Coverage directory escapes backlog root")
                return
            paths = sorted(coverage_dir.glob("*.json"))
        if not paths:
            self.error("coverage", "No coverage records found")
        read_docs, mapped_docs, slices, requirement_ids = set(), set(), set(), set()
        for path in paths:
            record = self.load(path)
            location = self.label(path)
            if record is _LOAD_FAILED:
                continue
            if not isinstance(record, dict):
                self.error(location, "Coverage file must be an object")
                continue
            if not isinstance(record.get("slice"), str) or not record["slice"].strip():
                self.error(location, "Coverage slice must be a nonempty string")
            elif record["slice"] in slices:
                self.error(location, "Duplicate coverage slice")
            else:
                slices.add(record["slice"])
            if not isinstance(record.get("notes"), list) or not all(isinstance(v, str) for v in record.get("notes", [])):
                self.error(location, "Coverage notes must be an array of strings")
            documents = record.get("documents")
            if not isinstance(documents, list) or not all(isinstance(v, str) for v in documents):
                self.error(location, "documents must list fully read original source paths")
                documents = []
            if len(set(documents)) != len(documents):
                self.error(location, "Duplicate document read records")
            for doc in documents:
                if doc not in ORIGINAL_DOCS:
                    self.error(location, f"Unknown original document: {doc}")
                else:
                    read_docs.add(doc)
            requirements = record.get("requirements")
            if not isinstance(requirements, list) or not requirements:
                self.error(location, "requirements must be a nonempty array")
                requirements = []
            for index, row in enumerate(requirements):
                where = f"{location}.requirements[{index}]"
                if not isinstance(row, dict):
                    self.error(where, "Requirement mapping must be an object")
                    continue
                for field in ("id", "requirement", "acceptance_test"):
                    if not isinstance(row.get(field), str) or not row[field].strip():
                        self.error(where, f"Missing nonempty string {field}")
                ident = row.get("id")
                if isinstance(ident, str):
                    if ident in requirement_ids:
                        self.error(where, f"Duplicate requirement ID {ident}")
                    requirement_ids.add(ident)
                source_ok = self.source(row.get("source"), where)
                issues = row.get("issues")
                if not isinstance(issues, list) or not issues or not all(isinstance(v, str) for v in issues):
                    self.error(where, "issues must contain actionable issue IDs")
                    continue
                if len(set(issues)) != len(issues):
                    self.error(where, "Duplicate mapped issue IDs")
                for ref in issues:
                    issue = self.tickets.get(ref)
                    if issue is None:
                        self.error(where, f"Undefined mapped issue {ref}")
                    elif issue["kind"] in {"epic", "roadmap"}:
                        self.error(where, f"Requirement mapped only to aggregate {ref}; use actionable tasks/gates")
                    elif source_ok and row["source"] not in issue["sources"]:
                        self.error(where, f"Mapped issue {ref} does not cite mapping's exact source section")
                if source_ok:
                    mapped_docs.add(row["source"]["path"])
                    if row["source"]["path"] not in documents:
                        self.error(where, "Mapped source absent from this slice's fully-read documents")
                if all(isinstance(row.get(f), str) for f in ("id", "requirement", "acceptance_test")) and source_ok:
                    self.mappings.append(row)
            self.coverage.append({**record, "documents": documents})
        for label, docs in (("fully read", read_docs), ("mapped", mapped_docs)):
            for missing in sorted(ORIGINAL_DOCS - docs):
                self.error("coverage", f"Original source not {label}: {missing}")
        settings = set(re.findall(r"^\|\s*`([a-z][a-z0-9-]*\.[a-z0-9.-]+)`\s*\|", self.sources.get("docs/settings/settings-reference.md", ""), re.M))
        scenarios = set(re.findall(r"^\|\s*(EC-\d{3})\s*\|", self.sources.get("docs/quality/edge-case-matrix.md", ""), re.M))
        recipes = set(re.findall(r"^\|\s*([a-z][a-z0-9]*(?:-[a-z0-9]+)+)\s*\|", self.sources.get("docs/library/starter-wallpaper-catalog.md", ""), re.M))
        for name, values, count in (("settings", settings, 172), ("edge cases", scenarios, 120), ("recipes", recipes, 36)):
            if len(values) != count:
                self.error("source registry", f"Expected {count} original {name}, extracted {len(values)}")
        for field, required in (("settings_keys", settings), ("scenario_ids", scenarios), ("work_units", WORK_UNITS)):
            owners = defaultdict(list)
            for ident, issue in self.tickets.items():
                for value in issue[field]:
                    if issue["kind"] in {"roadmap", "epic"}:
                        self.error(ident, f"{field} ownership must be on actionable tickets, not containers")
                    owners[value].append(ident)
            for missing in sorted(required - owners.keys()):
                self.error(field, f"Unmapped required identifier {missing}")
            for unknown in sorted(owners.keys() - required):
                self.error(field, f"Unknown identifier {unknown}")
        # Recipe slugs can live in extensible metadata, but must have an actionable
        # requirement row naming the slug, its source section, and owning issues.
        mapped_recipes = set()
        for row in self.mappings:
            if row["source"]["path"] == "docs/library/starter-wallpaper-catalog.md":
                encoded = json.dumps(row, ensure_ascii=False)
                for slug in recipes:
                    if re.search(r"(?<![\w-])" + re.escape(slug) + r"(?![\w-])", encoded):
                        mapped_recipes.add(slug)
        for missing in sorted(recipes - mapped_recipes):
            self.error("recipes", f"No actionable requirement mapping for recipe brief {missing}")
        mapped_issues = {ref for row in self.mappings for ref in row["issues"]}
        for ident, issue in self.tickets.items():
            if issue["kind"] in {"task", "gate"} and ident not in mapped_issues:
                self.error(ident, "Actionable issue has no coverage requirement mapping")

    def run(self) -> None:
        source_ok = self.validate_source_root()
        backlog_ok = self.validate_backlog_root()
        if not (source_ok and backlog_ok):
            return
        self.validate_project_views()
        for path in sorted(ORIGINAL_DOCS):
            doc_path = self.source_root / path
            if doc_path.is_symlink():
                self.error(path, f"Refusing symlinked original source document: {path}")
                self.sources[path] = ""
            else:
                resolved_doc, err = _safe_resolve(doc_path)
                if err is not None or not resolved_doc.is_relative_to(self.source_root):
                    self.error(path, f"Original source document escapes source root: {path}")
                    self.sources[path] = ""
                else:
                    self.sources[path] = self.read(doc_path)
        if len(ORIGINAL_DOCS) != 100:
            self.error("inventory", f"Frozen original source manifest has {len(ORIGINAL_DOCS)} paths, expected 100")
        schema_path = self.backlog_root / "schema.json"
        schema_label = self.label(schema_path)
        schema = self.load(schema_path)
        if not isinstance(schema, dict):
            self.error(schema_label, "Missing or invalid schema object")
            return
        if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
            self.error(schema_label, "Schema must declare Draft 2020-12")
        self.tickets_load(schema)
        self.graph()
        self.coverage_load()

    def machine(self) -> dict:
        return {"schema_version": 1, "valid": not self.errors, "publication_status": "local-only",
                "counts": {"local_objects": len(self.tickets), "remote_objects": 0,
                           "original_source_documents": len(ORIGINAL_DOCS), "coverage_records": len(self.coverage),
                           "requirement_mappings": len(self.mappings), "errors": len(self.errors),
                           "by_repository": dict(sorted(Counter(i["repository"] for i in self.tickets.values()).items())),
                           "by_kind": dict(sorted(Counter(i["kind"] for i in self.tickets.values()).items()))},
                "errors": sorted(self.errors, key=lambda error: (error["location"], error["message"]))}

    def reports(self) -> dict[str, str]:
        machine = self.machine()
        ledger = {"schema_version": 1, "publication_status": "local-only", "remote_object_count": 0,
                  "local_object_count": len(self.tickets), "objects": [
                      {"id": ident, "title": i["title"], "repository": i["repository"], "kind": i["kind"],
                       "path": self.label(self.backlog_root / i["repository"] / ident), "github_number": None,
                       "github_url": None, "project_item_id": None, "publication_status": "local-only"}
                      for ident, i in sorted(self.tickets.items())]}
        banner = "Generated from local backlog data; no GitHub issues or project items were created.\n\n"
        if self.errors:
            banner += f"**INVALID AUDIT: {len(self.errors)} failures. Reports do not certify completeness.**\n\n"
        trace = ["# Requirement traceability\n\n", banner,
                 "| Requirement | Exact source section | Local issues | Acceptance evidence to obtain |\n|---|---|---|---|\n"]
        for row in sorted(self.mappings, key=lambda row: (row["id"], row["source"]["path"], row["source"]["section"])):
            trace.append(table_row(row["id"] + ": " + row["requirement"], row["source"]["path"] + " :: " + row["source"]["section"],
                                   ", ".join(row["issues"]), row["acceptance_test"]))
        inventory = ["# Local backlog inventory\n\n", banner,
                     f"Local objects: {len(self.tickets)}. Remote objects: **0**. Original source documents: **100**.\n\n",
                     "| ID | Repository | Kind | Milestone | Priority | Size | Own effort (days) | Parent | Title |\n|---|---|---|---|---|---|---|---|---|\n"]
        for ident, i in sorted(self.tickets.items()):
            inventory.append(table_row(ident, i["repository"], i["kind"], i["milestone"], i["priority"], i["size"],
                                       i["effort_days"], i["parent"] or "—", i["title"]))
        inventory.extend(["\n## Original source inventory and read attestations\n\n", "| Source | Read by slices | Mapped requirements |\n|---|---|---|\n"])
        for path in sorted(ORIGINAL_DOCS):
            readers = sorted(str(r.get("slice") if isinstance(r.get("slice"), str) and r.get("slice").strip() else "?")
                             for r in self.coverage if isinstance(r.get("documents"), list) and path in r["documents"])
            count = sum(row["source"]["path"] == path for row in self.mappings)
            inventory.append(table_row(path, ", ".join(readers) or "MISSING", count))
        graph = ["# Dependencies and effort-weighted critical path\n\n", banner,
                 "Hard edges only; parent containment and related links do not impose execution order. Edges below run blocker → dependent. Cross-repository edges are included.\n\n",
                 "Effort estimates are proposed person-days, not measured duration. The critical path assumes unlimited parallel workers, zero handoff/resource contention, stable scope, and sequential own effort per ticket. It is a lower-bound planning model, not a delivery date. Container estimates represent only coordination, are not child totals, and are excluded from execution-path calculations. No execution/test evidence is implied.\n\n"]
        atomic = {ident for ident, i in self.tickets.items() if i["kind"] not in {"roadmap", "epic"}}
        total_effort = sum(self.tickets[ident]["effort_days"] for ident in atomic)
        coordination = sum(i["effort_days"] for i in self.tickets.values() if i["kind"] in {"roadmap", "epic"})
        graph.append(f"Known atomic effort: **{total_effort:g} person-days**; separate container coordination: **{coordination:g} person-days**.\n\n")
        downstream = defaultdict(set)
        if len(self.order) == len(self.tickets):
            distance, chain = {}, {}
            for ident in self.order:
                if ident not in atomic:
                    continue
                refs = [r for r in self.predecessors[ident] if r in distance]
                best = max(refs, key=lambda r: (distance[r], r), default=None)
                distance[ident] = self.tickets[ident]["effort_days"] + (distance[best] if best else 0)
                chain[ident] = (chain[best] if best else []) + [ident]
            end = max(distance, key=lambda ident: (distance[ident], ident), default=None)
            if end:
                graph.append(f"Longest estimated hard-blocker chain: **{distance[end]:g} person-days**.\n\n" + " → ".join(chain[end]) + "\n\n")
            for ident in reversed(self.order):
                for ref in self.predecessors[ident]:
                    downstream[ref].add(ident)
                    downstream[ref].update(downstream[ident])
        else:
            graph.append("Critical path unavailable: blocker graph is cyclic. Resolve audit errors first.\n\n")
        graph.extend(["## Most blocking local objects\n\n", "| ID | Direct dependents | All reachable dependents |\n|---|---|---|\n"])
        direct = Counter(ref for refs in self.predecessors.values() for ref in refs)
        for ident in sorted(atomic, key=lambda ident: (-len(downstream[ident]), -direct[ident], ident))[:25]:
            graph.append(table_row(ident, direct[ident], len(downstream[ident])))
        graph.extend(["\n## All hard edges\n\n", "| Blocker | Dependent | Cross-repository |\n|---|---|---|\n"])
        for ident, refs in sorted(self.predecessors.items()):
            for ref in sorted(refs):
                graph.append(table_row(ref, ident, "yes" if self.tickets[ref]["repository"] != self.tickets[ident]["repository"] else "no"))
        return {"backlog-audit.json": json.dumps(machine, ensure_ascii=False, sort_keys=True, indent=2) + "\n",
                "backlog-creation-ledger.json": json.dumps(ledger, ensure_ascii=False, sort_keys=True, indent=2) + "\n",
                "backlog-requirement-traceability.md": "".join(trace), "backlog-dependency-graph.md": "".join(graph),
                "backlog-inventory.md": "".join(inventory)}


def table_row(*cells) -> str:
    return "| " + " | ".join(str(cell).replace("|", "\\|").replace("\r\n", " ").replace("\r", " ").replace("\n", " ") for cell in cells) + " |\n"


def _oserror_text(exc: OSError) -> str:
    if exc.strerror:
        if isinstance(exc.filename, (str, Path)):
            return f"{exc.strerror}: {Path(exc.filename).as_posix()}"
        return exc.strerror
    return str(exc)


def write_reports(reports: dict[str, str], backlog_root: Path | None = None,
                  source_root: Path | None = None, raw_backlog_root: Path | None = None) -> Path:
    raw_target_backlog = DEFAULT_BACKLOG_ROOT if backlog_root is None else Path(backlog_root)
    raw_backlog = raw_target_backlog if raw_backlog_root is None else Path(raw_backlog_root)
    resolved_backlog, err = _safe_resolve(raw_target_backlog)
    if err is not None:
        raise OSError(f"Cannot resolve backlog root: {_oserror_text(err)}")
    resolved_source, src_err = _safe_resolve(ROOT if source_root is None else Path(source_root))
    if src_err is not None:
        raise OSError(f"Cannot resolve source root: {_oserror_text(src_err)}")
    destination = resolved_backlog / "reports"
    # Never write reports inside any repository or follow a user-controlled symlink, and
    # never accept caller-selected output paths or modify source/backlog objects.
    if raw_backlog.is_symlink() or destination.is_symlink():
        raise OSError("Refusing symlinked backlog or report directory")
    if not resolved_backlog.is_dir():
        raise OSError("Backlog root directory does not exist")
    if is_in_repo(resolved_backlog, resolved_source) or is_in_repo(Path(os.path.abspath(raw_backlog)), resolved_source):
        raise OSError("Refusing to write reports for in-repo backlog root")
    resolved_dest, dest_err = _safe_resolve(destination)
    if dest_err is not None:
        raise OSError(f"Cannot resolve report directory: {_oserror_text(dest_err)}")
    if not resolved_dest.is_relative_to(resolved_backlog):
        raise OSError("Report directory escapes backlog root")
    if is_in_repo(resolved_dest, resolved_source):
        raise OSError("Refusing to write reports inside repository")
    destination.mkdir(parents=True, exist_ok=True)
    for name in sorted(reports):
        path = destination / name
        temporary = destination / ("." + name + ".tmp")
        if path.is_symlink() or temporary.exists() or temporary.is_symlink():
            raise OSError(f"Refusing symlinked report or preexisting temporary file: {path.name}")
    for name, text in sorted(reports.items()):
        path = destination / name
        temporary = destination / ("." + name + ".tmp")
        if path.is_symlink() or temporary.exists() or temporary.is_symlink():
            raise OSError(f"Refusing symlinked report or preexisting temporary file: {path.name}")
        created = False
        try:
            with temporary.open("x", encoding="utf-8", newline="\n") as stream:
                created = True
                stream.write(text)
            temporary.replace(path)
        finally:
            if created and temporary.exists():
                temporary.unlink()
    return destination


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backlog-root", type=Path, default=DEFAULT_BACKLOG_ROOT,
                        help="external local backlog directory (default: %(default)s)")
    parser.add_argument("--source-root", type=Path, default=ROOT,
                        help="main asciiwall repository root containing frozen docs/ (default: %(default)s)")
    parser.add_argument("--write-reports", action="store_true",
                        help="write five derived reports under <backlog-root>/reports/; never modify tickets")
    parser.add_argument("--json", action="store_true",
                        help="emit machine JSON audit to stdout instead of human summary")
    args = parser.parse_args()
    audit = Audit(backlog_root=args.backlog_root, source_root=args.source_root)
    audit.run()
    if args.write_reports:
        if not audit.can_write_reports():
            audit.error(audit.label(audit.backlog_root / "reports"),
                        "Cannot write reports: backlog root is missing, symlinked, or inside a repository")
        else:
            try:
                write_reports(audit.reports(), audit.backlog_root, audit.source_root, audit.raw_backlog_root)
            except OSError as exc:
                audit.error(audit.label(audit.backlog_root / "reports"), f"Cannot safely write reports: {audit.format_error(exc)}")
    if args.json:
        print(json.dumps(audit.machine(), ensure_ascii=False, sort_keys=True, indent=2))
    else:
        for error in audit.machine()["errors"]:
            print(f"ERROR {error['location']}: {error['message']}", file=sys.stderr)
        print(f"{'FAIL' if audit.errors else 'PASS'}: {len(audit.tickets)} local objects, 0 remote objects; "
              f"{len(audit.mappings)} requirement mappings; {len(audit.errors)} errors.")
    return 1 if audit.errors else 0

if __name__ == "__main__":
    raise SystemExit(main())
