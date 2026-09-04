#!/usr/bin/env python3
"""Allocate version-local Harness task sequence names.

Task packet names use <task-seq>-<task-slug>, for example 001-login-flow.
The sequence is allocated per docs/versions/<version>/ across every project
module and globals. This tool scans both existing packet directories and the
machine-owned unfinished-task index so agents do not hand-pick sequence numbers.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import sys
from pathlib import Path


TASK_NAME_RE = re.compile(r"^(?P<seq>\d{3,})-(?P<slug>[a-z0-9][a-z0-9_.-]*)$")
SLUG_RE = re.compile(r"^[a-z0-9][a-z0-9_.-]*$")
EXCLUDED_MODULE_DIRS = {"_template"}


def fail(message: str) -> None:
    print(f"task-seq: {message}", file=sys.stderr)
    raise SystemExit(1)


def normalize_slug(slug: str) -> str:
    value = slug.strip().lower().replace("_", "-").replace(" ", "-")
    value = re.sub(r"[^a-z0-9.-]+", "-", value)
    value = re.sub(r"-+", "-", value).strip("-.")
    if not value or not SLUG_RE.fullmatch(value):
        fail(f"invalid task slug after normalization: {slug!r}")
    return value


def task_seq(task_name: str) -> int | None:
    match = TASK_NAME_RE.fullmatch(task_name)
    if not match:
        return None
    return int(match.group("seq"))


def load_task_index_module():
    path = Path(__file__).with_name("task-index.py")
    if not path.is_file():
        path = Path(__file__).with_name("task-index.template.py")
    spec = importlib.util.spec_from_file_location("task_index", path)
    if spec is None or spec.loader is None:
        fail(f"cannot load required sibling script: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def collect_from_task_index(root: Path, version: str) -> dict[str, str]:
    found: dict[str, str] = {}
    task_index = load_task_index_module()
    path = task_index.index_path(root, version)
    if not path.is_file():
        return found
    for entry in task_index.selected_entries(root.resolve(), version, None):
        found[entry["task_id"]] = entry["task_manifest"]
    return found


def collect_from_directories(modules_dir: Path) -> dict[str, str]:
    found: dict[str, str] = {}
    if not modules_dir.is_dir():
        return found

    for module_dir in sorted(path for path in modules_dir.iterdir() if path.is_dir()):
        if module_dir.name in EXCLUDED_MODULE_DIRS:
            continue
        if module_dir.name == "globals":
            search_roots = [module_dir]
        else:
            search_roots = [module_dir]
        for root in search_roots:
            for task_dir in sorted(path for path in root.iterdir() if path.is_dir()):
                if TASK_NAME_RE.fullmatch(task_dir.name):
                    found.setdefault(task_dir.name, task_dir.as_posix())
    return found


def collect_task_names(root: Path, version: str) -> dict[str, str]:
    modules_dir = root / "docs" / "versions" / version / "modules"
    found = collect_from_directories(modules_dir)
    for name, source in collect_from_task_index(root, version).items():
        found.setdefault(name, source)
    return found


def next_sequence(root: Path, version: str, width: int) -> tuple[int, dict[str, str]]:
    names = collect_task_names(root, version)
    sequences = [seq for name in names for seq in [task_seq(name)] if seq is not None]
    next_seq = max(sequences, default=0) + 1
    if next_seq >= 10**width:
        width = len(str(next_seq))
    return next_seq, names


def format_task_name(sequence: int, width: int, slug: str) -> str:
    return f"{sequence:0{width}d}-{slug}"


def command_next(args: argparse.Namespace) -> int:
    root = Path(args.root)
    slug = normalize_slug(args.slug) if args.slug else None
    sequence, names = next_sequence(root, args.version, args.width)
    task_name = format_task_name(sequence, args.width, slug) if slug else f"{sequence:0{args.width}d}"
    if args.json:
        print(json.dumps({"version": args.version, "sequence": sequence, "task_name": task_name, "existing": sorted(names)}, indent=2))
    else:
        print(task_name)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    subparsers = parser.add_subparsers(dest="command", required=True)

    next_parser = subparsers.add_parser("next", help="print the next sequence or sequence-prefixed task name")
    next_parser.add_argument("--version", required=True)
    next_parser.add_argument("--slug", help="task slug; when provided, output <seq>-<slug>")
    next_parser.add_argument("--width", type=int, default=3)
    next_parser.add_argument("--json", action="store_true")
    next_parser.set_defaults(func=command_next)

    args = parser.parse_args()
    if args.width < 1:
        fail("--width must be positive")
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
