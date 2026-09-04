#!/usr/bin/env python3
"""Dependency-free parser for the canonical Harness task manifest subset.

All generated lifecycle checkers import this module so quoted scalars, null
values, inline lists, and change bindings have one interpretation.
"""

from __future__ import annotations

import ast
import re
from pathlib import Path


PIPELINE_STAGES = ("design", "implementation", "testing", "acceptance")


class TaskManifestError(ValueError):
    pass


def scalar(value: str) -> str | None:
    value = value.strip()
    if not value or value.lower() in {"null", "none", "~"}:
        return None
    if value[:1] in {"\"", "'"}:
        if value[-1:] != value[:1]:
            raise TaskManifestError(f"unterminated quoted scalar {value!r}")
        try:
            parsed = ast.literal_eval(value)
        except (SyntaxError, ValueError) as error:
            raise TaskManifestError(f"invalid quoted scalar {value!r}: {error}") from error
        if not isinstance(parsed, str):
            raise TaskManifestError(f"expected string scalar, got {value!r}")
        return parsed
    return value


def inline_list(value: str, path: Path, line_number: int) -> list[str]:
    try:
        parsed = ast.literal_eval(value)
    except (SyntaxError, ValueError):
        if not (value.startswith("[") and value.endswith("]")):
            raise TaskManifestError(f"{path}:{line_number}: expected an inline list")
        body = value[1:-1].strip()
        parsed = [] if not body else [
            item.strip().strip("\"").strip("'") for item in body.split(",")
        ]
    if not isinstance(parsed, list) or not all(
        isinstance(item, str) and item for item in parsed
    ):
        raise TaskManifestError(
            f"{path}:{line_number}: expected a list of non-empty strings"
        )
    return parsed


def parse_task_manifest(path: Path) -> dict[str, object]:
    """Parse top-level scalars plus the canonical changes list."""
    if not path.is_file():
        raise TaskManifestError(f"missing task manifest: {path}")
    task: dict[str, object] = {}
    changes: list[dict[str, object]] = []
    current: dict[str, object] | None = None
    in_changes = False
    for line_number, raw_line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        stripped = raw_line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if stripped == "changes:":
            if in_changes:
                raise TaskManifestError(f"{path}:{line_number}: duplicate changes section")
            in_changes = True
            current = None
            continue
        if not in_changes:
            match = re.fullmatch(r"([a-z_]+):\s*(.*?)\s*", stripped)
            if not match:
                raise TaskManifestError(
                    f"{path}:{line_number}: unsupported task manifest entry {stripped!r}"
                )
            key, value = match.groups()
            if key in task:
                raise TaskManifestError(f"{path}:{line_number}: duplicate field {key}")
            task[key] = 1 if key == "schema_version" and value == "1" else scalar(value)
            continue

        item = re.fullmatch(r"-\s+id:\s*(.*?)\s*", stripped)
        if item:
            current = {"id": scalar(item.group(1))}
            changes.append(current)
            continue
        field = re.fullmatch(r"([a-z_]+):\s*(.*?)\s*", stripped)
        if not field or current is None:
            raise TaskManifestError(
                f"{path}:{line_number}: malformed changes entry {stripped!r}"
            )
        key, value = field.groups()
        if key in current:
            raise TaskManifestError(
                f"{path}:{line_number}: duplicate change field {key}"
            )
        current[key] = (
            inline_list(value, path, line_number)
            if value.strip().startswith("[")
            else scalar(value)
        )
    task["changes"] = changes
    return task


def task_policy(path: Path) -> dict[str, str | None]:
    task = parse_task_manifest(path)
    return {
        "stage": str(task["stage"]) if task.get("stage") is not None else None,
        "mode": str(task["mode"]) if task.get("mode") is not None else None,
        "start": (
            str(task["auto_pipeline_start_stage"])
            if task.get("auto_pipeline_start_stage") is not None
            else None
        ),
    }


def stage_is_automatic(policy: dict[str, str | None], stage: str) -> bool:
    start = policy.get("start")
    return (
        policy.get("mode") == "auto-pipeline"
        and start in PIPELINE_STAGES
        and PIPELINE_STAGES.index(stage) >= PIPELINE_STAGES.index(str(start))
    )
