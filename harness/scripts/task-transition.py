#!/usr/bin/env python3
"""Advance or return a high-risk task through legal lifecycle transitions."""

from __future__ import annotations

import argparse
import importlib.util
import re
import subprocess
import sys
from pathlib import Path

from task_manifest import stage_is_automatic


STAGES = ("proposal", "design", "implementation", "testing", "acceptance")


def fail(message: str) -> None:
    print(f"task-transition: {message}", file=sys.stderr)
    raise SystemExit(1)


def sibling_script(name: str) -> Path:
    installed = Path(__file__).with_name(f"{name}.py")
    return installed if installed.is_file() else Path(__file__).with_name(f"{name}.template.py")


def load_lifecycle():
    path = sibling_script("lifecycle-check")
    spec = importlib.util.spec_from_file_location("lifecycle_check", path)
    if spec is None or spec.loader is None:
        fail(f"cannot load lifecycle checker: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def task_policy(task: dict[str, object]) -> dict[str, str | None]:
    return {
        "stage": str(task.get("stage")) if task.get("stage") is not None else None,
        "mode": str(task.get("mode")) if task.get("mode") is not None else None,
        "start": (
            str(task.get("auto_pipeline_start_stage"))
            if task.get("auto_pipeline_start_stage") is not None
            else None
        ),
    }


def run_completion(root: Path, task_path: Path) -> None:
    command = [
        sys.executable,
        str(sibling_script("harness-check")),
        "--root", str(root),
        "--task", str(task_path),
        "--profile", "completion",
    ]
    completed = subprocess.run(command, capture_output=True, text=True)
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "unknown error"
        fail(f"stage completion failed: {detail}")


def require_accepted_report(task_path: Path, task: dict[str, object]) -> None:
    report_name = str(task.get("acceptance_report") or "acceptance-report.md")
    report = task_path.parent / report_name
    if not report.is_file():
        fail(f"accepted completion requires acceptance report: {report}")
    text = report.read_text(encoding="utf-8")
    if not re.search(
        r"(?im)^\s*-\s*Accepted / rejected / needs changes:\s*accepted\s*$",
        text,
    ):
        fail(
            "complete is valid only for an accepted report; use return --to "
            "<design|implementation|testing> for needs changes"
        )


def write_stage(task_path: Path, stage: str) -> None:
    text = task_path.read_text(encoding="utf-8")
    updated, count = re.subn(r"(?m)^stage:\s*.*$", f"stage: {stage}", text)
    if count != 1:
        fail(f"{task_path} must contain exactly one top-level stage field")
    temporary = task_path.with_name(task_path.name + ".tmp")
    temporary.write_text(updated, encoding="utf-8")
    temporary.replace(task_path)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--task", required=True)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("advance")
    commands.add_parser("complete")
    returned = commands.add_parser("return")
    returned.add_argument("--to", required=True, choices=("design", "implementation", "testing"))
    args = parser.parse_args()

    root = Path(args.root).resolve()
    task_path = Path(args.task)
    if not task_path.is_absolute():
        task_path = root / task_path
    task_path = task_path.resolve()
    lifecycle = load_lifecycle()
    task = lifecycle.validate_task(task_path)
    current = str(task["stage"])

    if args.command == "advance":
        if current == "acceptance":
            fail("acceptance has no next stage; use complete")
        if current in STAGES[1:] and stage_is_automatic(task_policy(task), current):
            fail(f"automatic stage {current} is owned by pipeline runtime state")
        run_completion(root, task_path)
        lifecycle.record_stage(root, task_path, task, current)
        next_stage = STAGES[STAGES.index(current) + 1]
        write_stage(task_path, next_stage)
        print(f"task-transition: advanced {current} -> {next_stage}")
        return 0

    if args.command == "complete":
        if current != "acceptance":
            fail(f"complete is valid only in acceptance; current stage: {current}")
        if stage_is_automatic(task_policy(task), current):
            fail("automatic acceptance completion is owned by pipeline runtime state")
        require_accepted_report(task_path, task)
        run_completion(root, task_path)
        lifecycle.record_stage(root, task_path, task, current)
        print("task-transition: completed acceptance")
        return 0

    if current != "acceptance" or task.get("mode") != "manual":
        fail("return is supported only from manual acceptance")
    lifecycle.clear_from(task_path, task, args.to)
    write_stage(task_path, args.to)
    print(f"task-transition: returned acceptance -> {args.to}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
