#!/usr/bin/env python3
"""Validate proportional trivial/standard delivery evidence.

Lower-tier work captures the same task-start working-tree baseline used by
high-risk stages before project edits: copies of already-dirty tracked files and
existing eligible untracked files only. It materializes the resulting changed-
path manifest at completion and reviews the delivery against the proposal.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

from task_manifest import TaskManifestError, parse_task_manifest


PROFILES = ("pre-edit", "completion")


def fail(message: str) -> None:
    print(f"lower-tier-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def sibling_script(name: str, root: Path) -> Path:
    generated = root / "harness" / "scripts" / f"{name}.py"
    if generated.is_file():
        return generated
    installed = Path(__file__).with_name(f"{name}.py")
    return installed if installed.is_file() else Path(__file__).with_name(f"{name}.template.py")


def load_task(root: Path, raw_task: str) -> tuple[Path, dict[str, object]]:
    path = Path(raw_task)
    if not path.is_absolute():
        path = root / path
    path = path.resolve()
    try:
        path.relative_to(root)
    except ValueError:
        fail(f"task manifest resolves outside repository: {path}")
    try:
        task = parse_task_manifest(path)
    except TaskManifestError as error:
        fail(str(error))
    tier = task.get("workflow_tier")
    if tier not in {"trivial", "standard"}:
        fail("lower-tier-check applies only to confirmed trivial or standard tasks")
    proposal = path.parent / "proposal.md"
    if not proposal.is_file():
        fail(f"missing approved proposal: {proposal}")
    text = proposal.read_text(encoding="utf-8")
    if not re.search(r"(?m)^status:\s*approved\s*$", text):
        fail(f"proposal must be approved before lower-tier delivery: {proposal}")
    if not re.search(rf"(?mi)^\s*-\s*Final tier:\s*{re.escape(str(tier))}\s*$", text):
        fail(f"proposal final tier does not match task.yaml: {proposal}")
    return path, task


def required_evidence_paths(
    root: Path, task: dict[str, object]
) -> tuple[str, Path, str, Path]:
    version = str(task.get("version") or "")
    task_name = str(task.get("task_name") or "")
    expected_baseline = (
        Path(".harness") / "baselines" / version
        / f"{task_name}-delivery" / "manifest.json"
    ).as_posix()
    expected_changed = (
        Path(".harness") / "evidence" / version
        / "stage-scope" / f"{task_name}.paths"
    ).as_posix()
    values = {
        "baseline_manifest": (task.get("baseline_manifest"), expected_baseline),
        "changed_paths_file": (task.get("changed_paths_file"), expected_changed),
    }
    resolved: dict[str, tuple[str, Path]] = {}
    for field, (raw, expected) in values.items():
        if not isinstance(raw, str) or not raw:
            fail(f"confirmed lower-tier task requires {field}: {expected}")
        normalized = Path(raw).as_posix()
        if normalized != expected:
            fail(f"{field} must use the canonical lower-tier path: {expected}")
        path = (root / normalized).resolve(strict=False)
        try:
            path.relative_to(root)
        except ValueError:
            fail(f"{field} resolves outside repository: {raw}")
        resolved[field] = (normalized, path)
    baseline_relative, baseline = resolved["baseline_manifest"]
    changed_relative, changed = resolved["changed_paths_file"]
    return baseline_relative, baseline, changed_relative, changed


def run_checked(command: list[str]) -> None:
    completed = subprocess.run(command, capture_output=True, text=True)
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "unknown error"
        fail(detail)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--task", required=True)
    parser.add_argument("--profile", required=True, choices=PROFILES)
    args = parser.parse_args()

    root = Path(args.root).resolve()
    task_path, task = load_task(root, args.task)
    baseline_relative, baseline, changed_relative, _ = required_evidence_paths(root, task)
    task_relative = task_path.relative_to(root).as_posix()
    snapshot = sibling_script("baseline-snapshot", root)
    if args.profile == "pre-edit":
        if baseline.is_file():
            command = [
                sys.executable, str(snapshot), "verify", "--root", str(root),
                "--manifest", baseline_relative, "--task", task_relative,
            ]
        else:
            command = [
                sys.executable, str(snapshot), "capture", "--root", str(root),
                "--task-id", f"{task['version']}/{task['task_name']}-delivery",
                "--task", task_relative,
            ]
        run_checked(command)
        print("lower-tier-check: passed (pre-edit)")
        return 0

    if not baseline.is_file():
        fail("completion requires the lower-tier task-start baseline; run --profile pre-edit before project edits")
    run_checked([
        sys.executable, str(snapshot), "diff", "--root", str(root),
        "--manifest", baseline_relative, "--task", task_relative,
        "--output", changed_relative,
    ])

    report = task_path.parent / str(task.get("completion_report") or "completion-report.md")
    run_checked([
        sys.executable,
        str(sibling_script("completion-report-check", root)),
        str(report),
        "--root", str(root),
    ])
    print("lower-tier-check: passed (completion)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
