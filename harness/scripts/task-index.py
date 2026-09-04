#!/usr/bin/env python3
"""Manage the version-local unfinished Harness task index.

The index is a machine-owned `.harness/tasks/<version>/tasks.json` file.
Agents and humans use this command instead of editing the JSON directly.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

from task_manifest import TaskManifestError, parse_task_manifest


TASK_NAME_RE = re.compile(r"^\d{3,}-[a-z0-9][a-z0-9_.-]*$")
MODULE_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")
WORKFLOW_TIERS = {"pending", "trivial", "standard", "high-risk"}
INDEX_FIELDS = {"schema_version", "version", "tasks"}
TASK_FIELDS = {"task_id", "task_manifest"}


def fail(message: str) -> None:
    print(f"task-index: {message}", file=sys.stderr)
    raise SystemExit(1)


def safe_relative(root: Path, value: str, label: str) -> Path:
    relative = Path(value)
    if relative.is_absolute() or ".." in relative.parts:
        fail(f"unsafe {label}: {value}")
    resolved = (root / relative).resolve()
    try:
        resolved.relative_to(root.resolve())
    except ValueError:
        fail(f"{label} resolves outside repository: {value}")
    return resolved


def index_path(root: Path, version: str) -> Path:
    return root / ".harness" / "tasks" / version / "tasks.json"


def task_parts(root: Path, task_path: Path) -> tuple[str, str, str, str]:
    try:
        relative = task_path.resolve().relative_to(root.resolve())
    except ValueError:
        fail(f"task manifest resolves outside repository: {task_path}")
    parts = relative.parts
    if (
        len(parts) != 7
        or parts[0] != "docs"
        or parts[1] != "versions"
        or parts[3] != "modules"
        or parts[6] != "task.yaml"
    ):
        fail(
            "task manifest must use "
            "docs/versions/<version>/modules/<packet-module>/<task-name>/task.yaml: "
            + relative.as_posix()
        )
    version, packet_module, task_name = parts[2], parts[4], parts[5]
    if not MODULE_RE.fullmatch(packet_module):
        fail(f"invalid packet module: {packet_module}")
    if not TASK_NAME_RE.fullmatch(task_name):
        fail(f"invalid task name: {task_name}")
    if not task_path.is_file():
        fail(f"missing task manifest: {task_path}")
    try:
        task = parse_task_manifest(task_path)
    except TaskManifestError as error:
        fail(str(error))
    tier = task.get("workflow_tier", "high-risk")
    if tier not in WORKFLOW_TIERS:
        fail(
            "workflow_tier must be one of: "
            + ", ".join(sorted(WORKFLOW_TIERS))
        )
    proposal = task_path.parent / "proposal.md"
    if not proposal.is_file():
        fail(f"common proposal packet is missing proposal.md: {proposal}")
    text = task_path.read_text(encoding="utf-8")
    expected = {
        "version": version,
        "packet_module": packet_module,
        "task_name": task_name,
    }
    for key, value in expected.items():
        if not re.search(rf"(?m)^{re.escape(key)}:\s*{re.escape(value)}\s*$", text):
            fail(f"{task_path} missing or mismatched {key}: {value}")
    return version, packet_module, task_name, relative.as_posix()


def load_index(root: Path, version: str) -> tuple[Path, dict[str, object]]:
    path = index_path(root, version)
    if not path.is_file():
        fail(f"missing unfinished-task index: {path.relative_to(root)}")
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"invalid unfinished-task index {path}: {error}")
    if not isinstance(data, dict) or set(data) != INDEX_FIELDS:
        fail(f"{path} must contain exactly: schema_version, version, tasks")
    if data.get("schema_version") != 1:
        fail(f"{path} schema_version must be 1")
    if data.get("version") != version:
        fail(f"{path} version must be {version}")
    tasks = data.get("tasks")
    if not isinstance(tasks, list):
        fail(f"{path} tasks must be a list")
    seen_ids: set[str] = set()
    seen_manifests: set[str] = set()
    for index, entry in enumerate(tasks):
        if not isinstance(entry, dict) or set(entry) != TASK_FIELDS:
            fail(f"{path} tasks[{index}] must contain exactly task_id and task_manifest")
        task_id = entry.get("task_id")
        manifest = entry.get("task_manifest")
        if not isinstance(task_id, str) or not TASK_NAME_RE.fullmatch(task_id):
            fail(f"{path} tasks[{index}] has invalid task_id")
        if not isinstance(manifest, str):
            fail(f"{path} tasks[{index}] has invalid task_manifest")
        task_path = safe_relative(root, manifest, "task_manifest")
        item_version, _, item_id, canonical = task_parts(root, task_path)
        if item_version != version or item_id != task_id or canonical != manifest:
            fail(f"{path} tasks[{index}] does not match its canonical task manifest")
        if task_id in seen_ids or manifest in seen_manifests:
            fail(f"{path} contains duplicate task id or manifest: {task_id}")
        seen_ids.add(task_id)
        seen_manifests.add(manifest)
    return path, data


def write_index(path: Path, data: dict[str, object]) -> None:
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    temporary.replace(path)


def resolve_task(root: Path, value: str) -> tuple[Path, str, str, str]:
    path = safe_relative(root, value, "task manifest")
    version, packet_module, task_name, relative = task_parts(root, path)
    return path, version, packet_module, task_name


def validate_task_completed(root: Path, task_path: Path) -> None:
    try:
        task = parse_task_manifest(task_path)
    except TaskManifestError as error:
        fail(str(error))
    tier = task.get("workflow_tier", "high-risk")
    if tier == "pending":
        fail("task workflow tier must be user-confirmed before removal")
    if tier not in {"trivial", "standard", "high-risk"}:
        fail(f"unsupported confirmed workflow tier: {tier}")

    proposal = task_path.parent / "proposal.md"
    if not proposal.is_file():
        fail(f"task cannot be removed without proposal: {proposal}")
    proposal_text = proposal.read_text(encoding="utf-8")
    if not re.search(r"(?m)^status:\s*approved\s*$", proposal_text):
        fail(f"task proposal must be approved before removal: {proposal}")
    if not re.search(
        rf"(?mi)^\s*-\s*Final tier:\s*{re.escape(str(tier))}\s*$",
        proposal_text,
    ):
        fail(f"task proposal final tier does not match task.yaml: {proposal}")

    if tier in {"trivial", "standard"}:
        report_value = task.get("completion_report") or "completion-report.md"
        if report_value != "completion-report.md":
            fail("lower-tier completion_report must use task-packet completion-report.md")
        report = task_path.parent / str(report_value)
        if not report.is_file():
            fail(f"task cannot be removed without lightweight acceptance report: {report}")
        report_text = report.read_text(encoding="utf-8")
        if not re.search(
            r"(?im)^\s*-\s*Accepted / rejected / needs changes:\s*accepted\s*$",
            report_text,
        ):
            fail(f"task lightweight acceptance report is not accepted: {report}")
        checker = Path(__file__).with_name("lower-tier-check.py")
        if not checker.is_file():
            checker = Path(__file__).with_name("lower-tier-check.template.py")
        if not checker.is_file():
            fail(f"missing lightweight acceptance checker: {checker}")
        completed = subprocess.run(
            [
                sys.executable, str(checker),
                "--root", str(root),
                "--task", str(task_path),
                "--profile", "completion",
            ],
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            detail = completed.stderr.strip() or completed.stdout.strip() or "unknown error"
            fail(f"task lower-tier completion validation failed: {detail}")
        return

    stage = task.get("stage")
    if task.get("mode") != "auto-pipeline" and stage != "acceptance":
        current = str(stage) if stage is not None else "missing"
        fail(
            f"manual task must be in acceptance stage before removal; current stage: {current}"
        )

    report = task_path.parent / "acceptance-report.md"
    if not report.is_file():
        fail(f"task cannot be removed without acceptance report: {report}")
    report_text = report.read_text(encoding="utf-8")
    if not re.search(
        r"(?im)^\s*-\s*Accepted / rejected / needs changes:\s*accepted\s*$",
        report_text,
    ):
        fail(f"task acceptance report is not accepted: {report}")

    checker = Path(__file__).with_name("acceptance-report-check.py")
    if not checker.is_file():
        checker = Path(__file__).with_name("acceptance-report-check.template.py")
    if not checker.is_file():
        fail(f"missing acceptance report checker: {checker}")
    completed = subprocess.run(
        [sys.executable, str(checker), str(report), "--root", str(root)],
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "unknown error"
        fail(f"task acceptance report validation failed: {detail}")

    lifecycle = Path(__file__).with_name("lifecycle-check.py")
    if not lifecycle.is_file():
        lifecycle = Path(__file__).with_name("lifecycle-check.template.py")
    if not lifecycle.is_file():
        fail(f"missing lifecycle checker: {lifecycle}")
    completed = subprocess.run(
        [
            sys.executable, str(lifecycle),
            "--root", str(root),
            "--task", str(task_path),
            "--require-complete",
        ],
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "unknown error"
        fail(f"task high-risk lifecycle validation failed: {detail}")


def command_validate(args: argparse.Namespace) -> int:
    load_index(Path(args.root).resolve(), args.version)
    print("task-index: passed")
    return 0


def command_init(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    version = args.version
    if not MODULE_RE.fullmatch(version):
        fail(f"invalid version: {version}")
    path = index_path(root, version)
    if path.exists():
        load_index(root, version)
        print(f"task-index: already initialized: {path.relative_to(root)}")
        return 0
    path.parent.mkdir(parents=True, exist_ok=True)
    write_index(path, {"schema_version": 1, "version": version, "tasks": []})
    print(f"task-index: initialized {path.relative_to(root)}")
    return 0


def command_add(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    task_path, version, _, task_name = resolve_task(root, args.task)
    path, data = load_index(root, version)
    relative = task_path.relative_to(root).as_posix()
    tasks = data["tasks"]
    assert isinstance(tasks, list)
    if any(entry["task_manifest"] == relative for entry in tasks):
        print(f"task-index: already registered: {relative}")
        return 0
    if any(entry["task_id"] == task_name for entry in tasks):
        fail(f"task id already registered to another manifest: {task_name}")
    tasks.append({"task_id": task_name, "task_manifest": relative})
    write_index(path, data)
    print(f"task-index: added {task_name}")
    return 0


def command_remove(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    task_path, version, _, task_name = resolve_task(root, args.task)
    validate_task_completed(root, task_path)
    path, data = load_index(root, version)
    relative = task_path.relative_to(root).as_posix()
    tasks = data["tasks"]
    assert isinstance(tasks, list)
    remaining = [entry for entry in tasks if entry["task_manifest"] != relative]
    if len(remaining) == len(tasks):
        fail(f"task is not registered as unfinished: {relative}")
    data["tasks"] = remaining
    write_index(path, data)
    print(f"task-index: removed completed task {task_name}")
    return 0


def selected_entries(root: Path, version: str, module: str | None) -> list[dict[str, str]]:
    _, data = load_index(root, version)
    tasks = data["tasks"]
    assert isinstance(tasks, list)
    selected: list[dict[str, str]] = []
    for entry in tasks:
        manifest = str(entry["task_manifest"])
        parts = Path(manifest).parts
        packet_module = parts[4]
        if module and packet_module != module:
            continue
        selected.append({
            "task_id": str(entry["task_id"]),
            "packet_module": packet_module,
            "task_manifest": manifest,
        })
    return selected


def command_list(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    entries = selected_entries(root, args.version, args.module)
    if args.json:
        print(json.dumps({"version": args.version, "tasks": entries}, indent=2))
    else:
        for entry in entries:
            print(entry["task_manifest"])
    return 0


def command_contains(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    task_path, version, _, _ = resolve_task(root, args.task)
    relative = task_path.relative_to(root).as_posix()
    entries = selected_entries(root, version, None)
    if not any(entry["task_manifest"] == relative for entry in entries):
        fail(f"unfinished-task index does not select canonical task manifest {relative}")
    print("task-index: selected")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    commands = parser.add_subparsers(dest="command", required=True)

    initialize = commands.add_parser("init")
    initialize.add_argument("--version", required=True)
    initialize.set_defaults(func=command_init)

    validate = commands.add_parser("validate")
    validate.add_argument("--version", required=True)
    validate.set_defaults(func=command_validate)

    add = commands.add_parser("add")
    add.add_argument("--task", required=True)
    add.set_defaults(func=command_add)

    remove = commands.add_parser("remove")
    remove.add_argument("--task", required=True)
    remove.set_defaults(func=command_remove)

    listing = commands.add_parser("list")
    listing.add_argument("--version", required=True)
    listing.add_argument("--module")
    listing.add_argument("--json", action="store_true")
    listing.set_defaults(func=command_list)

    contains = commands.add_parser("contains")
    contains.add_argument("--task", required=True)
    contains.set_defaults(func=command_contains)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
