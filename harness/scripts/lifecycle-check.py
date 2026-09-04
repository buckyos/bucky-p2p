#!/usr/bin/env python3
"""Validate durable high-risk stage-completion receipts.

`acceptance-report-check.py` owns only the acceptance document.  This checker
owns the separate invariant that every required high-risk stage completed in
order.  Manual-stage receipts are written by `task-transition.py`; automatic
stages are proved by the auto-pipeline runtime state.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

from task_manifest import (
    PIPELINE_STAGES,
    TaskManifestError,
    parse_task_manifest,
    stage_is_automatic,
)


STAGES = ("proposal", "design", "implementation", "testing", "acceptance")
STATE_FIELDS = {"schema_version", "task_manifest", "stages"}
TASK_BINDING_SCHEMA = 2


def fail(message: str) -> None:
    print(f"lifecycle-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def safe_repo_path(root: Path, value: str, label: str) -> Path:
    relative = Path(value)
    if relative.is_absolute() or ".." in relative.parts:
        fail(f"unsafe {label}: {value}")
    resolved = (root / relative).resolve()
    try:
        resolved.relative_to(root.resolve())
    except ValueError:
        fail(f"{label} resolves outside repository: {value}")
    return resolved


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    try:
        return sha256_bytes(path.read_bytes())
    except OSError as error:
        fail(f"cannot read receipt input {path}: {error}")


def task_binding_payload(
    task: dict[str, object], *, prelaunch_manual_policy: bool = False
) -> dict[str, object]:
    """Return receipt-bound identity and policy, excluding runtime evidence paths."""
    fields = (
        "schema_version", "workflow_tier", "version", "packet_module", "task_name",
        "mode", "auto_pipeline_start_stage", "proposal", "design", "testing",
        "testplan", "acceptance_report", "pipeline_plan", "risk_profile",
        "completion_report", "change_record", "lifecycle_state",
    )
    payload = {field: task.get(field) for field in fields}
    if prelaunch_manual_policy:
        payload["mode"] = "manual"
        payload["auto_pipeline_start_stage"] = None
    changes = task.get("changes")
    if not isinstance(changes, list):
        fail("task binding requires canonical changes")
    payload["changes"] = [
        {
            "id": change.get("id"),
            "target_module": change.get("target_module"),
            "scope_paths": change.get("scope_paths"),
        }
        for change in changes
        if isinstance(change, dict)
    ]
    return payload


def task_binding(task: dict[str, object]) -> str:
    """Hash stable identity and frozen policy while excluding stage/evidence paths."""
    encoded = json.dumps(
        task_binding_payload(task),
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return sha256_bytes(encoded)


def prelaunch_manual_binding(task: dict[str, object]) -> str:
    """Reconstruct the one allowed manual -> auto-pipeline policy transition."""
    encoded = json.dumps(
        task_binding_payload(task, prelaunch_manual_policy=True),
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return sha256_bytes(encoded)


def legacy_task_binding(task: dict[str, object]) -> str:
    """Reproduce the unversioned binding used before policy schema 2."""
    fields = (
        "workflow_tier", "version", "packet_module", "task_name", "mode",
        "auto_pipeline_start_stage", "proposal", "design", "testing",
        "testplan", "acceptance_report", "pipeline_plan", "risk_profile",
        "completion_report", "change_record", "lifecycle_state", "changes",
    )
    encoded = json.dumps(
        {field: task.get(field) for field in fields},
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return sha256_bytes(encoded)


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


def automatic_stage(task: dict[str, object], stage: str) -> bool:
    return stage in PIPELINE_STAGES and stage_is_automatic(task_policy(task), stage)


def validate_task(task_path: Path) -> dict[str, object]:
    try:
        task = parse_task_manifest(task_path)
    except TaskManifestError as error:
        fail(str(error))
    if task.get("workflow_tier") != "high-risk":
        fail(f"{task_path} lifecycle receipts apply only to workflow_tier: high-risk")
    if task.get("stage") not in STAGES:
        fail(f"{task_path} has invalid or missing stage")
    if task.get("mode") not in {"manual", "auto-pipeline"}:
        fail(f"{task_path} has invalid or missing mode")
    return task


def state_path(task_path: Path, task: dict[str, object]) -> Path:
    value = task.get("lifecycle_state") or "lifecycle.json"
    if value != "lifecycle.json":
        fail("lifecycle_state must use canonical task-packet path lifecycle.json")
    return task_path.parent / str(value)


def load_state(task_path: Path, task: dict[str, object], *, required: bool) -> dict[str, object]:
    path = state_path(task_path, task)
    if not path.is_file():
        if required:
            fail(f"missing lifecycle state: {path}")
        return {"schema_version": 1, "task_manifest": "task.yaml", "stages": {}}
    try:
        state = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"invalid lifecycle state {path}: {error}")
    if not isinstance(state, dict) or set(state) != STATE_FIELDS:
        fail(f"{path} must contain exactly schema_version, task_manifest, and stages")
    if state.get("schema_version") != 1 or state.get("task_manifest") != "task.yaml":
        fail(f"{path} has invalid schema_version or task_manifest binding")
    stages = state.get("stages")
    if not isinstance(stages, dict) or any(stage not in STAGES for stage in stages):
        fail(f"{path} stages must be an object containing only canonical stage names")
    return state


def artifact_inputs(root: Path, task_path: Path, task: dict[str, object], stage: str) -> list[Path]:
    fields: dict[str, tuple[str, ...]] = {
        "proposal": ("proposal",),
        "design": ("design", "risk_profile"),
        "implementation": (),
        "testing": ("testing", "testplan"),
        "acceptance": ("acceptance_report",),
    }
    result: list[Path] = []
    for field in fields[stage]:
        value = task.get(field)
        if value in {None, ""}:
            fail(f"stage {stage} requires task field {field}")
        relative = Path(str(value))
        if relative.is_absolute() or ".." in relative.parts or str(value).startswith(".harness/"):
            fail(f"stage artifact {field} must be a safe task-packet path")
        path = (task_path.parent / relative).resolve()
        try:
            path.relative_to(root.resolve())
        except ValueError:
            fail(f"stage artifact {field} resolves outside repository: {path}")
        if not path.is_file():
            fail(f"stage {stage} completion input is missing: {path}")
        result.append(path)
    return result


def successful_test_run(
    artifact: object, task_scope: str, expected_testplan: str, change_ids: set[str]
) -> bool:
    if not isinstance(artifact, dict) or artifact.get("schema") != 1:
        return False
    if artifact.get("requested_module") != task_scope or artifact.get("requested_level") != "all":
        return False
    if artifact.get("exit_code") != 0:
        return False
    testplans = artifact.get("testplans")
    artifact_changes = artifact.get("change_ids")
    steps = artifact.get("steps")
    if not isinstance(testplans, list) or expected_testplan not in testplans:
        return False
    if (
        not isinstance(artifact_changes, list)
        or not all(isinstance(item, str) for item in artifact_changes)
        or not change_ids <= set(artifact_changes)
    ):
        return False
    if not isinstance(steps, list):
        return False
    if steps:
        return all(
            isinstance(step, dict)
            and step.get("exit_code") == 0
            and isinstance(step.get("command"), list)
            and bool(step.get("command"))
            and isinstance(step.get("sources"), list)
            and bool(step.get("sources"))
            for step in steps
        )
    non_executed = artifact.get("non_executed_levels")
    return (
        isinstance(non_executed, list)
        and len(non_executed) == 3
        and {item.get("level") for item in non_executed if isinstance(item, dict)}
        == {"unit", "dv", "integration"}
        and all(
            isinstance(item, dict)
            and item.get("mode") in {"manual", "disabled"}
            and bool(str(item.get("reason") or "").strip())
            for item in non_executed
        )
    )


def latest_test_artifact(root: Path, task_path: Path, task: dict[str, object]) -> Path:
    testplan = task_path.parent / str(task.get("testplan") or "testplan.yaml")
    expected = testplan.relative_to(root).as_posix()
    scope = f"{task['packet_module']}/{task['task_name']}"
    change_ids = {
        str(change["id"])
        for change in task.get("changes", [])
        if isinstance(change, dict) and change.get("id")
    }
    matches: list[Path] = []
    artifact_root = root / ".harness" / "test-results" / "test-runs"
    for path in artifact_root.glob("*.json") if artifact_root.is_dir() else ():
        try:
            artifact = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, json.JSONDecodeError):
            continue
        if successful_test_run(artifact, scope, expected, change_ids):
            matches.append(path)
    if not matches:
        fail(f"testing completion requires a successful task test-run artifact for {scope} all")
    return max(matches, key=lambda path: (path.stat().st_mtime_ns, path.name))


def receipt_payload(root: Path, task_path: Path, task: dict[str, object], stage: str) -> dict[str, object]:
    inputs = {
        path.relative_to(root).as_posix(): sha256_file(path)
        for path in artifact_inputs(root, task_path, task, stage)
    }
    payload: dict[str, object] = {
        "status": "complete",
        "task_binding_schema": TASK_BINDING_SCHEMA,
        "task_binding_sha256": task_binding(task),
        "inputs": inputs,
    }
    if stage == "testing":
        artifact = latest_test_artifact(root, task_path, task)
        payload["test_run"] = artifact.relative_to(root).as_posix()
        payload["test_run_sha256"] = sha256_file(artifact)
    return payload


def write_state(path: Path, state: dict[str, object]) -> None:
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(
        json.dumps(state, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    temporary.replace(path)


def record_stage(root: Path, task_path: Path, task: dict[str, object], stage: str) -> None:
    if task.get("stage") != stage:
        fail(f"cannot record {stage} while task.yaml stage is {task.get('stage')}")
    if automatic_stage(task, stage):
        fail(f"automatic stage {stage} must be recorded in pipeline runtime state")
    state = load_state(task_path, task, required=False)
    stages = state["stages"]
    assert isinstance(stages, dict)
    index = STAGES.index(stage)
    missing = [name for name in STAGES[:index] if not automatic_stage(task, name) and name not in stages]
    if missing:
        fail(f"cannot record {stage} before prior manual stages: {', '.join(missing)}")
    stages[stage] = receipt_payload(root, task_path, task, stage)
    for later in STAGES[index + 1 :]:
        stages.pop(later, None)
    write_state(state_path(task_path, task), state)


def clear_from(task_path: Path, task: dict[str, object], stage: str) -> None:
    state = load_state(task_path, task, required=False)
    stages = state["stages"]
    assert isinstance(stages, dict)
    for name in STAGES[STAGES.index(stage) :]:
        stages.pop(name, None)
    write_state(state_path(task_path, task), state)


def verify_receipts(root: Path, task_path: Path, task: dict[str, object], required: tuple[str, ...]) -> None:
    if not required:
        return
    state = load_state(task_path, task, required=True)
    stages = state["stages"]
    assert isinstance(stages, dict)
    binding = task_binding(task)
    for stage in required:
        receipt = stages.get(stage)
        if not isinstance(receipt, dict) or receipt.get("status") != "complete":
            fail(f"high-risk stage has no completion receipt: {stage}")
        receipt_schema = receipt.get("task_binding_schema")
        receipt_binding = receipt.get("task_binding_sha256")
        if receipt_schema is None and receipt_binding == legacy_task_binding(task):
            verify_receipt_evidence(root, task_path, task, stage, receipt)
            continue
        if receipt_schema != TASK_BINDING_SCHEMA:
            fail(
                f"high-risk stage receipt uses a legacy task binding: {stage}; "
                "only an explicit auto-pipeline launch may migrate manual-prefix receipts"
            )
        if receipt_binding != binding:
            fail(f"high-risk stage receipt has stale task binding: {stage}")
        verify_receipt_evidence(root, task_path, task, stage, receipt)


def expected_receipt_inputs(
    root: Path, task_path: Path, task: dict[str, object], stage: str
) -> dict[str, str]:
    return {
        path.relative_to(root).as_posix(): sha256_file(path)
        for path in artifact_inputs(root, task_path, task, stage)
    }


def verify_receipt_evidence(
    root: Path,
    task_path: Path,
    task: dict[str, object],
    stage: str,
    receipt: dict[str, object],
) -> None:
    if receipt.get("inputs") != expected_receipt_inputs(root, task_path, task, stage):
        fail(f"high-risk stage receipt inputs are missing or stale: {stage}")
    if stage == "testing":
        value = receipt.get("test_run")
        digest = receipt.get("test_run_sha256")
        if not isinstance(value, str) or not isinstance(digest, str):
            fail("testing receipt is missing its task test-run binding")
        artifact = safe_repo_path(root, value, "testing receipt test_run")
        if not artifact.is_file() or sha256_file(artifact) != digest:
            fail("testing receipt task test-run artifact is missing or stale")


def sibling_script(name: str) -> Path:
    installed = Path(__file__).with_name(f"{name}.py")
    return installed if installed.is_file() else Path(__file__).with_name(f"{name}.template.py")


def run_pipeline_check(root: Path, task_path: Path, task: dict[str, object], *, complete: bool) -> None:
    plan = task_path.parent / str(task.get("pipeline_plan") or "pipeline/plan.md")
    command = [sys.executable, str(sibling_script("pipeline-plan-check")), str(plan), "--root", str(root)]
    if complete:
        command.append("--require-complete")
    completed = subprocess.run(command, capture_output=True, text=True)
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "unknown error"
        fail(f"auto-pipeline lifecycle validation failed: {detail}")


def required_manual_stages(task: dict[str, object], *, before: str | None = None) -> tuple[str, ...]:
    limit = STAGES.index(before) if before is not None else len(STAGES)
    policy = task_policy(task)
    return tuple(
        stage for stage in STAGES[:limit]
        if not (stage in PIPELINE_STAGES and stage_is_automatic(policy, stage))
    )


def refresh_manual_bindings(
    root: Path, task_path: Path, task: dict[str, object]
) -> tuple[str, ...]:
    """Migrate legacy or exact prelaunch manual receipts to frozen policy."""
    if task.get("mode") != "auto-pipeline":
        fail("--refresh-manual-bindings applies only to auto-pipeline tasks")
    run_pipeline_check(root, task_path, task, complete=False)
    required = required_manual_stages(task)
    state = load_state(task_path, task, required=True)
    stages = state["stages"]
    assert isinstance(stages, dict)
    current_binding = task_binding(task)
    allowed_prelaunch_binding = prelaunch_manual_binding(task)
    receipts: list[dict[str, object]] = []
    for stage in required:
        receipt = stages.get(stage)
        if not isinstance(receipt, dict) or receipt.get("status") != "complete":
            fail(f"high-risk stage has no completion receipt: {stage}")
        receipt_binding = receipt.get("task_binding_sha256")
        receipt_schema = receipt.get("task_binding_schema")
        if receipt_schema is None:
            if (
                not isinstance(receipt_binding, str)
                or len(receipt_binding) != 64
                or any(character not in "0123456789abcdef" for character in receipt_binding)
            ):
                fail(f"manual receipt has an invalid legacy task binding: {stage}")
        elif receipt_schema != TASK_BINDING_SCHEMA:
            fail(f"manual receipt has unsupported task binding schema: {stage}")
        elif receipt_binding not in {current_binding, allowed_prelaunch_binding}:
            fail(
                "manual receipt cannot be migrated because its binding differs "
                f"from the exact prelaunch policy: {stage}"
            )
        verify_receipt_evidence(root, task_path, task, stage, receipt)
        receipts.append(receipt)
    for receipt in receipts:
        receipt["task_binding_schema"] = TASK_BINDING_SCHEMA
        receipt["task_binding_sha256"] = current_binding
    write_state(state_path(task_path, task), state)
    return required


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--task", required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--require-prior", choices=STAGES)
    mode.add_argument("--require-complete", action="store_true")
    mode.add_argument("--refresh-manual-bindings", action="store_true")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    task_path = Path(args.task)
    if not task_path.is_absolute():
        task_path = root / task_path
    task_path = task_path.resolve()
    try:
        task_path.relative_to(root)
    except ValueError:
        fail(f"task manifest resolves outside repository: {task_path}")
    task = validate_task(task_path)
    if args.refresh_manual_bindings:
        refreshed = refresh_manual_bindings(root, task_path, task)
        print(
            "lifecycle-check: refreshed manual receipt bindings: "
            + ", ".join(refreshed)
        )
    elif args.require_prior:
        verify_receipts(
            root, task_path, task,
            required_manual_stages(task, before=args.require_prior),
        )
        if task.get("mode") == "auto-pipeline":
            run_pipeline_check(root, task_path, task, complete=False)
    else:
        verify_receipts(root, task_path, task, required_manual_stages(task))
        if task.get("mode") == "auto-pipeline":
            run_pipeline_check(root, task_path, task, complete=True)
    print("lifecycle-check: passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
