#!/usr/bin/env python3
"""Validate a task's recorded changed paths against its active stage.

This command does not grant filesystem permission, but it fails completion
when a recorded path belongs to another stage's artifact group. Design Scope
Paths remain traceability metadata and are not enforced here.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import re
import subprocess
import sys
from pathlib import Path, PurePosixPath

from task_manifest import TaskManifestError, parse_task_manifest, stage_is_automatic


STAGES = {"proposal", "design", "testing", "implementation", "acceptance"}
MODULE_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")
TASK_NAME_RE = re.compile(r"^\d{3,}-[a-z0-9][a-z0-9_.-]*$")
MODULE_DOCS = {
    "proposal.md", "design.md", "risk-profile.yaml", "testing.md", "testplan.yaml",
    "acceptance-report.md",
}
TABLE_SEPARATOR_RE = re.compile(r"^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?\s*$")
MANIFEST_KEYS = ("changed_paths", "touched_paths", "paths")
RUST_CFG_TEST_RE = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")


def fail(message: str) -> None:
    print(f"stage-scope-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def git(args: list[str], root: Path) -> str:
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=root,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=False,
        )
    except FileNotFoundError:
        fail("git executable not found")
    except subprocess.CalledProcessError as error:
        stderr = error.stderr.decode("utf-8", errors="replace").strip()
        fail(stderr or f"git {' '.join(args)} failed")
    return result.stdout.decode("utf-8", errors="replace")


def normalize(path: str) -> str:
    normalized = path.replace("\\", "/").lstrip("\ufeff")
    if normalized.startswith("./"):
        normalized = normalized[2:]
    candidate = PurePosixPath(normalized)
    if candidate.is_absolute() or ".." in candidate.parts:
        fail(f"path must stay inside the repository and must not contain '..': {path}")
    return candidate.as_posix()


def canonical_repo_path(root: Path, path: str) -> str:
    """Resolve a path against the repository and reject root/symlink escapes."""
    normalized = normalize(path)
    root_resolved = root.resolve()
    candidate = (root_resolved / normalized).resolve(strict=False)
    try:
        return candidate.relative_to(root_resolved).as_posix()
    except ValueError:
        fail(f"path resolves outside the repository: {path}")


def baseline_sources_from_manifest(root: Path, raw_manifest: str) -> dict[str, str]:
    """Load and verify a project-local task-start baseline manifest."""
    root_resolved = root.resolve()
    manifest = Path(raw_manifest)
    if not manifest.is_absolute():
        manifest = root_resolved / manifest
    manifest = manifest.resolve(strict=False)
    try:
        manifest_relative = manifest.relative_to(root_resolved).as_posix()
    except ValueError:
        fail(f"baseline manifest resolves outside the repository: {raw_manifest}")
    if not manifest_relative.startswith(".harness/baselines/"):
        fail("baseline manifest must live under .harness/baselines/")
    if not manifest.is_file():
        fail(f"baseline manifest does not exist: {manifest_relative}")

    try:
        payload = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"invalid baseline manifest {manifest_relative}: {error}")
    if not isinstance(payload, dict) or payload.get("schema") not in {1, 3, 4, 5}:
        fail(f"baseline manifest {manifest_relative} must use schema 1, 3, 4, or 5")
    records = payload.get("files")
    if not isinstance(records, list) or (payload.get("schema") == 1 and not records):
        fail(f"baseline manifest {manifest_relative} must contain a valid files list")

    sources: dict[str, str] = {}
    manifest_dir = manifest.parent.resolve()
    for record in records:
        if not isinstance(record, dict):
            fail(f"baseline manifest {manifest_relative} contains a malformed file record")
        raw_path = record.get("path")
        raw_snapshot = record.get("snapshot")
        if not all(isinstance(value, str) and value for value in (raw_path, raw_snapshot)):
            fail(f"baseline manifest {manifest_relative} contains an incomplete file record")
        path = canonical_repo_path(root_resolved, raw_path)
        snapshot_relative = normalize(raw_snapshot)
        snapshot = (manifest_dir / snapshot_relative).resolve(strict=False)
        try:
            snapshot.relative_to(manifest_dir)
        except ValueError:
            fail(f"baseline snapshot resolves outside its task directory: {raw_snapshot}")
        if not snapshot.is_file():
            fail(f"baseline snapshot does not exist: {raw_snapshot}")
        data = snapshot.read_bytes()
        try:
            sources[path] = data.decode("utf-8")
        except UnicodeDecodeError as error:
            fail(f"baseline snapshot is not utf-8 for {path}: {error}")
    return sources


def parse_status_z(output: str) -> list[str]:
    changed: list[str] = []
    records = [record for record in output.split("\0") if record]
    index = 0
    while index < len(records):
        record = records[index]
        if len(record) < 4:
            fail(f"unexpected git status record: {record!r}")
        status = record[:2]
        path = record[3:]
        changed.append(normalize(path))
        index += 1
        if "R" in status or "C" in status:
            if index >= len(records):
                fail(f"rename/copy status missing old path for: {path}")
            changed.append(normalize(records[index]))
            index += 1
    return changed


def parse_diff_name_status_z(output: str) -> list[str]:
    changed: list[str] = []
    records = [record for record in output.split("\0") if record]
    index = 0
    while index < len(records):
        status = records[index]
        index += 1
        if not status:
            continue
        if status[0] in {"R", "C"}:
            if index + 1 >= len(records):
                fail(f"rename/copy diff status missing path data: {status}")
            old_path = records[index]
            new_path = records[index + 1]
            changed.extend([normalize(old_path), normalize(new_path)])
            index += 2
        else:
            if index >= len(records):
                fail(f"diff status missing path data: {status}")
            changed.append(normalize(records[index]))
            index += 1
    return changed


def changed_paths_from_git(root: Path, base: str | None, include_untracked: bool) -> list[str]:
    if base:
        output = git(["diff", "--name-status", "-z", f"{base}...HEAD"], root)
        return sorted(set(parse_diff_name_status_z(output)))

    args = ["status", "--porcelain=v1", "-z"]
    if include_untracked:
        args.append("--untracked-files=all")
    else:
        args.append("--untracked-files=no")
    output = git(args, root)
    return sorted(set(parse_status_z(output)))


def path_from_status_like_line(line: str) -> list[str]:
    """Accept either plain paths or simple name-status style lines."""
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        return []
    if stripped.startswith("- "):
        stripped = stripped[2:].strip()
    tab_parts = [part.strip() for part in stripped.split("\t") if part.strip()]
    if len(tab_parts) >= 2 and re.fullmatch(r"[A-Z?][A-Z0-9?]*", tab_parts[0]):
        return [normalize(part) for part in tab_parts[1:]]
    space_status = re.match(r"^[A-Z?][A-Z0-9?]*\s+(.+)$", stripped)
    if space_status:
        return [normalize(space_status.group(1))]
    return [normalize(stripped)]


def changed_paths_from_file(path: Path) -> list[str]:
    if not path.exists():
        fail(f"changed paths file does not exist: {path}")
    text = path.read_text(encoding="utf-8")
    stripped = text.lstrip()
    paths: list[str] = []
    if stripped.startswith("[") or stripped.startswith("{"):
        try:
            data = json.loads(text)
        except json.JSONDecodeError as error:
            fail(f"invalid JSON changed paths file {path}: {error}")
        if isinstance(data, list):
            values = data
        elif isinstance(data, dict):
            values = None
            for key in MANIFEST_KEYS:
                if key in data:
                    values = data[key]
                    break
            if values is None:
                fail(f"JSON changed paths file {path} must contain one of: {', '.join(MANIFEST_KEYS)}")
        else:
            fail(f"JSON changed paths file {path} must be an array or object")
        if not isinstance(values, list) or not all(isinstance(item, str) for item in values):
            fail(f"JSON changed paths in {path} must be a list of strings")
        paths = [normalize(item) for item in values]
    else:
        for line in text.splitlines():
            paths.extend(path_from_status_like_line(line))
    return sorted({path for path in paths if path})


def normalized_optional_path(root: Path, value: object) -> str | None:
    if value in {None, ""}:
        return None
    path = Path(str(value))
    if not path.is_absolute():
        path = root / path
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError:
        fail(f"stage-scope metadata path resolves outside repository: {value}")


def validate_manifest_sidecar(args: argparse.Namespace, root: Path, raw_file: str) -> None:
    manifest = Path(raw_file)
    if not manifest.is_absolute():
        manifest = root / manifest
    sidecar = manifest.with_name(manifest.name + ".meta.json")
    if not sidecar.is_file():
        fail(f"changed paths manifest requires sidecar: {sidecar}")
    try:
        data = json.loads(sidecar.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"invalid changed paths sidecar {sidecar}: {error}")
    if not isinstance(data, dict) or data.get("schema") != 1:
        fail(f"{sidecar} schema must be 1")
    target_module = args.target_module or args.module
    expected = {
        "stage": args.stage,
        "version": args.version,
        "module": args.module,
        "submodule": args.submodule,
        "target_module": target_module,
        "change_ids": sorted(args.change_ids or []),
    }
    for field, value in expected.items():
        actual = data.get(field)
        if field == "change_ids":
            if not isinstance(actual, list) or not all(isinstance(item, str) for item in actual):
                fail(f"{sidecar} change_ids must be a list of strings")
            actual = sorted(actual)
        if actual != value:
            fail(f"{sidecar} {field} mismatch: expected {value!r}, got {actual!r}")
    expected_baseline = normalized_optional_path(root, args.baseline_manifest)
    actual_baseline = normalized_optional_path(root, data.get("baseline_manifest"))
    if actual_baseline != expected_baseline:
        fail(
            f"{sidecar} baseline_manifest mismatch: expected "
            f"{expected_baseline!r}, got {actual_baseline!r}"
        )


def explicit_changed_paths(args: argparse.Namespace, root: Path) -> list[str]:
    paths: list[str] = []
    for raw in args.changed_paths or []:
        paths.append(normalize(raw))
    for raw_file in args.changed_paths_files or []:
        file_path = Path(raw_file)
        if not file_path.is_absolute():
            file_path = root / file_path
        try:
            manifest_rel = file_path.resolve().relative_to(root.resolve()).as_posix()
        except ValueError:
            fail(f"changed paths file resolves outside the repository: {raw_file}")
        expected_prefix = (
            f".harness/evidence/{args.version}/stage-scope/"
            if args.version
            else ".harness/evidence/"
        )
        if not manifest_rel.startswith(expected_prefix):
            fail(
                "changed paths file must live under "
                f"{expected_prefix}<task-id>.paths"
            )
        paths.extend(changed_paths_from_file(file_path))
    if args.from_git:
        paths.extend(
            changed_paths_from_git(
                root,
                args.git_diff_base,
                include_untracked=not args.ignore_untracked,
            )
        )
    if not paths:
        fail(
            "no task changed paths supplied; pass --changed-paths-file "
            ".harness/evidence/<version>/stage-scope/<task-id>.paths or repeated --changed-path values"
        )
    return sorted({canonical_repo_path(root, path) for path in paths if path})


def packet_parts(path: str) -> tuple[str, str, str] | None:
    parts = path.split("/")
    if len(parts) < 6:
        return None
    if parts[0] != "docs" or parts[1] != "versions" or parts[3] != "modules":
        return None
    version = parts[2]
    module = parts[4]
    relative = "/".join(parts[5:])
    return version, module, relative


def active_packet(path: str, version: str | None, module: str | None, submodule: str | None = None) -> tuple[str, str, str] | None:
    packet = packet_parts(path)
    if packet is None:
        return None
    packet_version, packet_module, relative = packet
    if version and packet_version != version:
        return None
    if module and packet_module != module:
        return None
    if submodule:
        if relative == submodule:
            relative = ""
        elif relative.startswith(f"{submodule}/"):
            relative = relative[len(submodule) + 1 :]
        else:
            return None
    return packet_version, packet_module, relative


def is_architecture_doc(path: str) -> bool:
    return path.startswith("docs/architecture/") and (path.endswith(".md") or path.endswith(".yaml") or path.endswith(".yml"))


def is_module_boundary_sync(path: str, module: str | None) -> bool:
    if not path.startswith("docs/modules/") or not path.endswith(".md"):
        return False
    if module is None:
        return True
    return path == f"docs/modules/{module}.md"


def is_review_report(path: str) -> bool:
    packet = packet_parts(path)
    if packet is None:
        return False
    leaf = packet[2].rsplit("/", 1)[-1]
    return leaf == "acceptance-report.md" or leaf.endswith("-acceptance-report.md")


def is_legacy_review_area(path: str) -> bool:
    if path.startswith("docs/reviews/") and path.endswith(".md"):
        return True
    parts = path.split("/")
    return (
        len(parts) >= 5
        and parts[0] == "docs"
        and parts[1] == "versions"
        and parts[3] == "reviews"
        and path.endswith(".md")
    )


def pipeline_artifact_path(
    path: str, leaf: str, version: str | None, module: str | None, submodule: str | None
) -> bool:
    if not version or not module or not submodule:
        return False
    return path == f"docs/versions/{version}/modules/{module}/{submodule}/pipeline/{leaf}"


def is_any_pipeline_artifact(path: str) -> bool:
    return bool(
        re.fullmatch(
            r"docs/versions/[^/]+/modules/[^/]+/.+/pipeline/(?:plan\.md|state\.json)",
            path,
        )
    )


def has_value(value: str) -> bool:
    return value.strip().strip('"').strip("'").lower() not in {"", "-", "n/a", "na", "none", "tbd", "todo", "pending"}


def pipeline_trigger_value(text: str, label: str) -> str | None:
    match = re.search(rf"(?mi)^\s*-\s*{re.escape(label)}:\s*(.+)$", text)
    return match.group(1).strip() if match else None


def pipeline_uses_plan_design(
    root: Path, version: str, module: str, task_name: str | None
) -> bool:
    if not task_name:
        return False
    plan = root / "docs" / "versions" / version / "modules" / module / task_name / "pipeline" / "plan.md"
    if not plan.exists():
        return False
    manifest = plan.parent.parent / "task.yaml"
    if not manifest.is_file():
        return False
    try:
        task = parse_task_manifest(manifest)
    except TaskManifestError as error:
        fail(str(error))
    policy = {
        "stage": str(task["stage"]) if task.get("stage") is not None else None,
        "mode": str(task["mode"]) if task.get("mode") is not None else None,
        "start": (
            str(task["auto_pipeline_start_stage"])
            if task.get("auto_pipeline_start_stage") is not None
            else None
        ),
    }
    return stage_is_automatic(policy, "design")


def is_unified_test_entrypoint(path: str) -> bool:
    return path == "harness/scripts/test-run.py"


def is_stage_doc_path(path: str) -> bool:
    packet = packet_parts(path)
    if packet is None:
        return False
    relative = packet[2]
    relative_parts = relative.split("/")
    leaf = relative.rsplit("/", 1)[-1]
    return (
        leaf in MODULE_DOCS
        or "design" in relative_parts
        or "testing" in relative_parts
    )


def is_test_artifact(path: str) -> bool:
    parts = path.split("/")
    leaf = parts[-1].lower()
    return (
        "tests" in parts
        or "test" in parts
        or "__tests__" in parts
        or leaf.startswith("test_")
        or leaf.endswith("_test.py")
        or ".test." in leaf
        or ".spec." in leaf
        or leaf.endswith("_test.rs")
        or leaf.endswith("_tests.rs")
        or leaf in {"test.rs", "tests.rs"}
    )


def rust_lexical_mask(source: str) -> str:
    """Mask Rust comments and literals while preserving character positions and braces."""
    chars = list(source)
    masked = list(source)

    def blank(index: int) -> None:
        if masked[index] not in {"\n", "\r"}:
            masked[index] = " "

    index = 0
    block_depth = 0
    while index < len(chars):
        if block_depth:
            if source.startswith("/*", index):
                blank(index)
                blank(index + 1)
                block_depth += 1
                index += 2
            elif source.startswith("*/", index):
                blank(index)
                blank(index + 1)
                block_depth -= 1
                index += 2
            else:
                blank(index)
                index += 1
            continue

        if source.startswith("//", index):
            while index < len(chars) and chars[index] not in {"\n", "\r"}:
                blank(index)
                index += 1
            continue
        if source.startswith("/*", index):
            blank(index)
            blank(index + 1)
            block_depth = 1
            index += 2
            continue

        raw = re.match(r'(?:br|cr|r)(?P<delimiters>#{0,255})"', source[index:])
        if raw:
            terminator = '"' + raw.group("delimiters")
            end = source.find(terminator, index + raw.end())
            end = len(chars) if end < 0 else end + len(terminator)
            while index < end:
                blank(index)
                index += 1
            continue

        prefix_length = (
            2
            if source.startswith(('b"', 'c"'), index)
            else 1
            if chars[index] == '"'
            else 0
        )
        if prefix_length:
            end = index + prefix_length
            escaped = False
            while end < len(chars):
                char = chars[end]
                end += 1
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == '"':
                    break
            while index < end:
                blank(index)
                index += 1
            continue

        # Rust character/byte literals. Lifetimes such as 'a are deliberately
        # left visible because they cannot contain structural braces.
        char_match = re.match(r"(?:b)?'(?:\\.|[^'\\\r\n])'", source[index:])
        if char_match:
            end = index + char_match.end()
            while index < end:
                blank(index)
                index += 1
            continue
        index += 1

    return "".join(masked)


def cfg_test_item_spans(source: str) -> list[tuple[int, int]]:
    """Return outermost exact #[cfg(test)] item spans in Rust source."""
    masked = rust_lexical_mask(source)
    spans: list[tuple[int, int]] = []
    for match in RUST_CFG_TEST_RE.finditer(masked):
        if any(start <= match.start() < end for start, end in spans):
            continue
        cursor = match.end()
        opening = -1
        while cursor < len(masked):
            if masked[cursor] == "{":
                opening = cursor
                break
            if masked[cursor] == ";":
                opening = cursor
                break
            cursor += 1
        if opening < 0:
            continue
        if masked[opening] == ";":
            spans.append((match.start(), opening + 1))
            continue
        depth = 1
        cursor = opening + 1
        while cursor < len(masked) and depth:
            if masked[cursor] == "{":
                depth += 1
            elif masked[cursor] == "}":
                depth -= 1
            cursor += 1
        if depth == 0:
            spans.append((match.start(), cursor))
    return spans


def rust_production_projection(source: str) -> tuple[str, int]:
    """Replace each cfg(test) item with a stable marker for content comparison."""
    spans = cfg_test_item_spans(source)
    pieces: list[str] = []
    cursor = 0
    for start, end in spans:
        pieces.append(source[cursor:start])
        pieces.append("#[cfg(test)]<existing-test-item>")
        cursor = end
    pieces.append(source[cursor:])
    return "".join(pieces), len(spans)


def rust_inline_test_only_change(root: Path, path: str, baselines: dict[str, str]) -> bool:
    """Prove that a mixed Rust file changed only inside pre-existing cfg(test) items."""
    if not path.endswith(".rs"):
        return False
    current_path = root / path
    if not current_path.is_file():
        return False
    baseline = baselines.get(path)
    if baseline is None:
        return False
    current = current_path.read_text(encoding="utf-8")
    if current == baseline:
        # A manifest claims this path changed, so equality cannot prove that the
        # task changed only an existing inline test item.
        return False
    before_projection, before_count = rust_production_projection(baseline)
    after_projection, after_count = rust_production_projection(current)
    return before_count > 0 and before_count == after_count and before_projection == after_projection


def normalize_column(value: str) -> str:
    return re.sub(r"[^a-z0-9]+", "_", value.strip().lower()).strip("_")


def split_table_row(line: str) -> list[str]:
    parts = [part.strip() for part in line.strip().split("|")]
    if parts and parts[0] == "":
        parts = parts[1:]
    if parts and parts[-1] == "":
        parts = parts[:-1]
    return parts


def parse_scope_paths(cell: str) -> list[str]:
    entries = [match.group(1) for match in re.finditer(r"`([^`]+)`", cell)]
    if not entries:
        entries = cell.split(",")
    normalized: list[str] = []
    for entry in entries:
        cleaned = normalize(entry.strip().strip("`"))
        if cleaned and cleaned not in normalized:
            normalized.append(cleaned)
    return normalized


def design_scope_paths(
    root: Path,
    version: str,
    module: str,
    submodule: str | None,
    target_module: str,
    change_ids: list[str],
) -> list[str]:
    if pipeline_uses_plan_design(root, version, module, submodule):
        design = root / "docs" / "versions" / version / "modules" / module / str(submodule) / "pipeline" / "plan.md"
        if not design.exists():
            fail(f"missing task-local pipeline plan for auto-pipeline scope binding: {design}")
        text = design.read_text(encoding="utf-8")
        section = "Implementation Scope Bindings"
    else:
        design = root / "docs" / "versions" / version / "modules" / module
        if submodule:
            design = design / submodule
        design = design / "design.md"
        if not design.exists():
            fail(f"missing design document for scope binding: {design}")
        text = design.read_text(encoding="utf-8")
        section = "Directly Mapped Change Items"

    heading = re.search(rf"(?m)^##\s+{re.escape(section)}\s*$", text)
    if not heading:
        fail(f"{design} missing required section: ## {section}")
    lines = text[heading.end() :].splitlines()
    table_start = None
    for index, line in enumerate(lines):
        if re.match(r"^##\s+", line):
            break
        if "|" in line and index + 1 < len(lines) and TABLE_SEPARATOR_RE.match(lines[index + 1]):
            table_start = index
            break
    if table_start is None:
        fail(f"{design} ## {section} missing required table")

    headers = [normalize_column(cell) for cell in split_table_row(lines[table_start])]
    rows: list[dict[str, str]] = []
    for line in lines[table_start + 2 :]:
        if not line.strip() or not line.lstrip().startswith("|"):
            break
        values = split_table_row(line)
        rows.append({header: values[pos].strip() if pos < len(values) else "" for pos, header in enumerate(headers)})

    scope_paths: list[str] = []
    for change_id in change_ids:
        matches = [
            row
            for row in rows
            if row.get("change_id") == change_id
            and row.get("target_module") == target_module
        ]
        if not matches:
            fail(
                f"change_id {change_id} for target_module {target_module} "
                f"missing from {design} ## {section}"
            )
        if len(matches) > 1:
            fail(
                f"change_id {change_id} for target_module {target_module} "
                f"appears multiple times in {design} ## {section}"
            )
        entries = parse_scope_paths(matches[0].get("scope_paths", ""))
        if not entries:
            fail(f"change_id {change_id} has no parsable Scope Paths entries in {design} ## {section}")
        for entry in entries:
            if any(token in entry for token in ("*", "?", "[", "]")):
                prefix = re.split(r"[*?\[]", entry, maxsplit=1)[0].rstrip("/")
                if not prefix:
                    fail(f"Scope Path glob must have a concrete repository prefix: {entry}")
                canonical_repo_path(root, prefix)
            else:
                entry = canonical_repo_path(root, entry)
            if entry not in scope_paths:
                scope_paths.append(entry)
    return scope_paths


def in_scope_paths(path: str, scope_paths: list[str]) -> bool:
    for entry in scope_paths:
        if any(token in entry for token in ("*", "?", "[", "]")) and fnmatch.fnmatchcase(path, entry):
            return True
        if path == entry or path.startswith(entry + "/"):
            return True
    return False


def harness_tooling_implementation_binding(
    root: Path,
    version: str | None,
    module: str | None,
    submodule: str | None,
    target_module: str | None,
    change_ids: list[str] | None,
) -> bool:
    """Verify that a globals task manifest owns the requested Harness tool changes."""
    if not (
        version
        and module == "globals"
        and submodule
        and TASK_NAME_RE.fullmatch(submodule)
        and target_module
        and target_module != "globals"
        and MODULE_NAME_RE.fullmatch(target_module)
        and change_ids
    ):
        return False
    if not (root / "docs" / "modules" / f"{target_module}.md").is_file():
        return False

    manifest = (
        root
        / "docs"
        / "versions"
        / version
        / "modules"
        / "globals"
        / submodule
        / "task.yaml"
    )
    try:
        task = parse_task_manifest(manifest)
    except (TaskManifestError, OSError, UnicodeError):
        return False
    if any(
        task.get(field) != expected
        for field, expected in {
            "version": version,
            "packet_module": "globals",
            "task_name": submodule,
            "workflow_tier": "high-risk",
        }.items()
    ):
        return False

    changes = task.get("changes")
    if not isinstance(changes, list):
        return False
    for change_id in change_ids:
        matches = [
            change
            for change in changes
            if isinstance(change, dict) and change.get("id") == change_id
        ]
        if len(matches) != 1 or matches[0].get("target_module") != target_module:
            return False
    return True


def allowed_for_stage(
    path: str,
    stage: str,
    version: str | None,
    module: str | None,
    submodule: str | None = None,
    harness_tooling_bound: bool = False,
) -> bool:
    # `.harness/` contains generated runtime state. It is git-ignored and never
    # belongs in the task changed-path manifest being validated.
    if path == ".harness" or path.startswith(".harness/"):
        return False

    packet = active_packet(path, version, module, submodule)
    relative = packet[2] if packet is not None else ""
    leaf = relative.rsplit("/", 1)[-1]

    # task.yaml is the canonical control artifact for document/testing/review
    # stages. The manifest is evidence only and never controls file access.
    if packet is not None and leaf == "task.yaml" and stage != "implementation":
        return True

    if stage == "proposal":
        return packet is not None and leaf in {"proposal.md", "risk-profile.yaml"}

    if stage == "design":
        if packet is not None and (leaf in {"design.md", "risk-profile.yaml"} or relative.startswith("design/")):
            return True
        return (
            is_module_boundary_sync(path, module)
            or is_architecture_doc(path)
            or pipeline_artifact_path(path, "plan.md", version, module, submodule)
        )

    if stage == "testing":
        if packet is not None and (
            leaf == "testing.md"
            or leaf == "testplan.yaml"
            or relative.startswith("testing/")
        ):
            return True
        return (
            is_test_artifact(path)
            or is_unified_test_entrypoint(path)
        )

    if stage == "acceptance":
        if packet is not None and leaf == "acceptance-report.md":
            return True
        return is_review_report(path)

    if stage == "implementation":
        if is_any_pipeline_artifact(path):
            return False
        if packet_parts(path) is not None and path.rsplit("/", 1)[-1] == "task.yaml":
            return False
        if is_stage_doc_path(path) or is_review_report(path) or is_legacy_review_area(path) or is_module_boundary_sync(path, module) or is_architecture_doc(path):
            return False
        if is_test_artifact(path):
            return False
        if path == "AGENTS.md":
            return False
        # A concrete globals sibling task may deliver non-rule Harness tools.
        # All other Harness governance surfaces remain outside implementation.
        if path.startswith("harness/scripts/"):
            return harness_tooling_bound
        if path.startswith("harness/"):
            return False
        return True

    fail(f"unknown stage: {stage}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--stage", required=True, choices=sorted(STAGES))
    parser.add_argument("--version")
    parser.add_argument("--module")
    parser.add_argument("--submodule")
    parser.add_argument("--target-module")
    parser.add_argument(
        "--changed-path",
        action="append",
        dest="changed_paths",
        help="repo-relative path changed by this task; repeat for multiple paths",
    )
    parser.add_argument(
        "--changed-paths-file",
        action="append",
        dest="changed_paths_files",
        help=(
            "per-task changed path manifest; line-based paths or JSON with "
            "changed_paths/touched_paths/paths"
        ),
    )
    parser.add_argument(
        "--from-git",
        action="store_true",
        help="discover paths from git status/diff for diagnosis; not recommended as the task boundary",
    )
    parser.add_argument(
        "--change-id",
        action="append",
        dest="change_ids",
        help="optional change ids for traceability; Scope Paths are not enforced",
    )
    parser.add_argument(
        "--baseline-manifest",
        help=(
            "testing-stage direct-content baseline manifest with selected full-content "
            "copies under .harness/baselines/<version>/<task-id>-testing/manifest.json"
        ),
    )
    parser.add_argument(
        "--git-diff-base",
        help="with --from-git, discover committed paths from <git-diff-base>...HEAD",
    )
    parser.add_argument("--ignore-untracked", action="store_true")
    args = parser.parse_args()

    if args.ignore_untracked and not args.from_git:
        fail("--ignore-untracked applies only with --from-git")
    if args.git_diff_base and not args.from_git:
        fail("--git-diff-base applies only with --from-git")
    if args.baseline_manifest and args.stage != "testing":
        fail("--baseline-manifest applies only to the testing stage")
    if args.stage in {"proposal", "design", "testing", "acceptance"} and not (
        args.version and args.module
    ):
        fail(
            f"--version and --module are required for the {args.stage} stage; "
            "without them any task packet could pass the scope check"
        )
    if args.change_ids and args.stage != "implementation":
        fail("--change-id applies only to the implementation stage")
    if args.stage == "implementation" and not args.change_ids:
        fail("--change-id is required for the implementation stage")
    if args.stage == "implementation" and not (args.version and args.module):
        fail("--version and --module are required for the implementation stage")
    if args.stage == "implementation" and args.module == "globals":
        if not args.target_module or args.target_module == "globals":
            fail("--module globals requires a concrete --target-module")
    elif (
        args.stage == "implementation"
        and args.target_module
        and args.target_module != args.module
    ):
        fail("--target-module may differ from --module only when --module globals")
    target_module = args.target_module or args.module
    if args.stage == "implementation" and (
        not target_module or not MODULE_NAME_RE.fullmatch(target_module)
    ):
        fail(f"invalid --target-module: {target_module}")

    root = Path(args.root)
    harness_tooling_bound = (
        harness_tooling_implementation_binding(
            root,
            args.version,
            args.module,
            args.submodule,
            target_module,
            args.change_ids,
        )
        if args.stage == "implementation"
        else False
    )
    for manifest in args.changed_paths_files or []:
        validate_manifest_sidecar(args, root, manifest)
    baselines = (
        baseline_sources_from_manifest(root, args.baseline_manifest)
        if args.baseline_manifest
        else {}
    )
    paths = explicit_changed_paths(args, root)
    if not paths:
        print("stage-scope-check: no task changed paths")
        return 0

    violations: list[str] = []
    for path in paths:
        path_allowed = allowed_for_stage(
            path,
            args.stage,
            args.version,
            args.module,
            args.submodule,
            harness_tooling_bound,
        )
        if (
            not path_allowed
            and args.stage == "testing"
            and rust_inline_test_only_change(root, path, baselines)
        ):
            path_allowed = True
        if not path_allowed:
            violations.append(path)
    if violations:
        print(f"stage-scope-check: {args.stage} stage scope violation", file=sys.stderr)
        for path in violations:
            print(f"  - {path}", file=sys.stderr)
        return 1

    print(f"stage-scope-check: passed ({args.stage}, {len(paths)} task path(s))")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
