#!/usr/bin/env python3
"""Prepare and validate the single task-level risk profile.

The checker is dependency-free and intentionally accepts only the generated
profile schema. `--prepare` refreshes machine-owned task/change/source paths.
"""

from __future__ import annotations

import argparse
import ast
import importlib.util
import re
import sys
from pathlib import Path


RISK_KEYS = ("contract", "data", "security", "runtime", "build", "ui", "harness")
RISK_TRIGGERS = {
    "contract": "contract-protocol",
    "data": "data-schema",
    "security": "security",
    "runtime": "runtime-integration",
    "build": "build-config-deployment",
    "ui": "ui-workflow",
    "harness": "harness-process",
}
POST_PROPOSAL_STAGES = {"design", "implementation", "testing", "acceptance"}
EMPTY = {"", "-", "none", "null", "pending", "tbd", "todo", "n/a", "na"}


def fail(message: str) -> None:
    print(f"risk-profile-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_harness_check():
    path = Path(__file__).with_name("harness-check.py")
    if not path.is_file():
        path = Path(__file__).with_name("harness-check.template.py")
    spec = importlib.util.spec_from_file_location("harness_check", path)
    if spec is None or spec.loader is None:
        fail(f"cannot load required sibling script: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def inline_list(value: str, path: Path, line_number: int) -> list[str]:
    try:
        parsed = ast.literal_eval(value)
    except (SyntaxError, ValueError) as error:
        fail(f"{path}:{line_number}: invalid inline list: {error}")
    if not isinstance(parsed, list) or not all(isinstance(item, str) and item.strip() for item in parsed):
        fail(f"{path}:{line_number}: expected an inline list of non-empty strings")
    return [item.strip() for item in parsed]


def parse_profile(path: Path) -> dict[str, object]:
    if not path.is_file():
        fail(f"missing task risk profile: {path}")
    lines = path.read_text(encoding="utf-8").splitlines()
    profile: dict[str, object] = {"source_bindings": {}, "risks": {}}
    section: str | None = None
    category: str | None = None
    list_field: str | None = None
    for line_number, raw in enumerate(lines, start=1):
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        indent = len(raw) - len(raw.lstrip())
        stripped = raw.strip()
        if indent == 0:
            list_field = None
            category = None
            if stripped in {"source_bindings:", "risks:"}:
                section = stripped[:-1]
                continue
            match = re.fullmatch(r"([a-z0-9_]+):\s*(.*?)\s*", stripped)
            if not match:
                fail(f"{path}:{line_number}: unsupported top-level entry")
            key, value = match.groups()
            if key == "schema_version":
                if value != "1":
                    fail(f"{path}:{line_number}: schema_version must be 1")
                profile[key] = 1
            elif key == "task_manifest":
                profile[key] = value.strip("\"'")
            elif key == "change_ids":
                profile[key] = inline_list(value, path, line_number)
            else:
                fail(f"{path}:{line_number}: unsupported top-level field {key}")
            section = None
            continue
        if section == "source_bindings" and indent == 2:
            match = re.fullmatch(r"([a-z0-9_]+):\s*(.*?)\s*", stripped)
            if not match:
                fail(f"{path}:{line_number}: invalid source binding")
            profile["source_bindings"][match.group(1)] = match.group(2).strip().strip("\"'")
            continue
        if section == "risks" and indent == 2 and stripped.endswith(":"):
            category = stripped[:-1]
            profile["risks"][category] = {}
            list_field = None
            continue
        if (
            section == "risks"
            and category
            and list_field
            and indent in {4, 6}
            and stripped.startswith("-")
        ):
            item = re.fullmatch(r"-\s+(.+)", stripped)
            if not item:
                fail(f"{path}:{line_number}: invalid list item")
            profile["risks"][category][list_field].append(
                item.group(1).strip().strip("\"'")
            )
            continue
        if section == "risks" and category and indent == 4:
            match = re.fullmatch(r"([a-z_]+):\s*(.*?)\s*", stripped)
            if not match:
                fail(f"{path}:{line_number}: invalid risk field")
            field, value = match.groups()
            row = profile["risks"][category]
            if field == "applies":
                if value not in {"true", "false"}:
                    fail(f"{path}:{line_number}: applies must be true or false")
                row[field] = value == "true"
                list_field = None
            elif field in {"evidence", "required_checks"}:
                if not value:
                    row[field] = []
                    list_field = field
                elif value.startswith("["):
                    row[field] = inline_list(value, path, line_number) if value != "[]" else []
                    list_field = None
                else:
                    row[field] = value.strip().strip("\"'")
                    list_field = None
            else:
                fail(f"{path}:{line_number}: unsupported risk field {field}")
            continue
        fail(f"{path}:{line_number}: unsupported indentation or entry")
    return profile


def replace_scalar(text: str, key: str, value: str, *, indent: int = 0) -> str:
    pattern = re.compile(rf"(?m)^{' ' * indent}{re.escape(key)}:[ \t]*[^\n]*$")
    replacement = f"{' ' * indent}{key}: {value}"
    if not pattern.search(text):
        fail(f"risk profile missing machine-owned field: {key}")
    return pattern.sub(replacement, text, count=1)


def active_design_source(task: dict[str, object], task_path: Path) -> Path | None:
    if str(task["stage"]) == "proposal":
        return None
    harness_check = load_harness_check()
    field = "pipeline_plan" if harness_check.uses_pipeline_design(task) else "design"
    value = task.get(field)
    if not value:
        fail(f"task stage {task['stage']} requires {field}")
    path = task_path.parent / str(value)
    if not path.is_file():
        fail(f"missing active design source: {path}")
    return path


def prepare(profile_path: Path, task_path: Path, task: dict[str, object]) -> None:
    if not profile_path.is_file():
        fail(f"missing risk profile template: {profile_path}")
    proposal = task_path.parent / str(task["proposal"])
    if not proposal.is_file():
        fail(f"missing proposal source: {proposal}")
    design = active_design_source(task, task_path)
    text = profile_path.read_text(encoding="utf-8")
    change_ids = [str(change["id"]) for change in task["changes"]]
    text = replace_scalar(text, "change_ids", repr(change_ids).replace("'", '"'))
    text = replace_scalar(text, "proposal", str(task["proposal"]), indent=2)
    design_value = "" if design is None else design.relative_to(task_path.parent).as_posix()
    text = replace_scalar(text, "design_source", design_value, indent=2)
    profile_path.write_text(text, encoding="utf-8")


def values(value: object) -> list[str]:
    if isinstance(value, list):
        return [str(item).strip() for item in value]
    return [str(value).strip()] if value is not None else []


def placeholder(value: str) -> bool:
    lowered = value.strip().lower()
    return lowered in EMPTY or any(lowered.startswith(prefix) for prefix in ("todo ", "tbd ", "pending "))


def require_reference(path: Path, expected: str = "Risk profile: ./risk-profile.yaml") -> None:
    if not path.is_file():
        return
    if expected not in path.read_text(encoding="utf-8"):
        fail(f"{path} must reference the task-level profile with `{expected}`")


def validate(profile_path: Path, task_path: Path, task: dict[str, object]) -> None:
    profile = parse_profile(profile_path)
    if profile.get("task_manifest") != "task.yaml":
        fail("risk profile must bind task_manifest: task.yaml")
    expected_changes = [str(change["id"]) for change in task["changes"]]
    if profile.get("change_ids") != expected_changes:
        fail(f"risk profile change_ids do not match task.yaml: expected {expected_changes}")
    bindings = profile["source_bindings"]
    proposal_rel = str(bindings.get("proposal", ""))
    if proposal_rel != str(task["proposal"]):
        fail("risk profile proposal binding does not match task.yaml")
    proposal = task_path.parent / proposal_rel
    if not proposal.is_file():
        fail(f"missing bound proposal source: {proposal}")
    design = active_design_source(task, task_path)
    if design is None:
        if bindings.get("design_source"):
            fail("proposal-stage risk profile must not bind a design source")
    else:
        design_rel = design.relative_to(task_path.parent).as_posix()
        if bindings.get("design_source") != design_rel:
            fail("risk profile active design-source path does not match task.yaml")

    risks = profile["risks"]
    if set(risks) != set(RISK_KEYS):
        fail(f"risk profile must contain exactly these categories: {', '.join(RISK_KEYS)}")
    for key in RISK_KEYS:
        row = risks[key]
        if set(row) != {"applies", "evidence", "required_checks"}:
            fail(f"risk {key} must contain applies, evidence, and required_checks")
        evidence = values(row["evidence"])
        checks = values(row["required_checks"])
        if not evidence or any(placeholder(item) for item in evidence):
            fail(f"risk {key} requires concrete evidence or a concrete non-applicability reason")
        if row["applies"]:
            if task["stage"] in POST_PROPOSAL_STAGES and not isinstance(row["evidence"], list):
                fail(f"applicable risk {key} requires design-owned evidence path list before {task['stage']} completion")
            if task["stage"] in POST_PROPOSAL_STAGES and (not checks or any(placeholder(item) for item in checks)):
                fail(f"applicable risk {key} requires design-owned required_checks before {task['stage']} completion")
        else:
            if isinstance(row["evidence"], list):
                fail(f"non-applicable risk {key} requires a prose evidence reason, not a path list")
            if checks:
                fail(f"non-applicable risk {key} must use required_checks: []")

    require_reference(task_path.parent / str(task["proposal"]))
    harness_check = load_harness_check()
    if harness_check.uses_pipeline_design(task) and task["stage"] in POST_PROPOSAL_STAGES:
        require_reference(task_path.parent / str(task["pipeline_plan"]))
    elif task["stage"] in POST_PROPOSAL_STAGES:
        require_reference(task_path.parent / str(task["design"]))
    if task["stage"] in {"testing", "acceptance"}:
        testing = task_path.parent / str(task.get("testing") or "testing.md")
        if testing.is_file():
            require_reference(testing)
        testplan = task_path.parent / str(task.get("testplan") or "testplan.yaml")
        if testplan.is_file() and not re.search(r"(?m)^risk_profile:\s*risk-profile\.yaml\s*$", testplan.read_text(encoding="utf-8")):
            fail(f"{testplan} must reference risk_profile: risk-profile.yaml")
    if task["stage"] == "acceptance":
        require_reference(task_path.parent / str(task.get("acceptance_report") or "acceptance-report.md"))


def applicable_triggers(profile_path: Path) -> list[str]:
    profile = parse_profile(profile_path)
    return [RISK_TRIGGERS[key] for key in RISK_KEYS if profile["risks"].get(key, {}).get("applies") is True]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--task", required=True)
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--print-triggers", action="store_true")
    args = parser.parse_args()
    root = Path(args.root).resolve()
    task_path = Path(args.task)
    if not task_path.is_absolute():
        task_path = root / task_path
    task_path = task_path.resolve()
    harness_check = load_harness_check()
    task = harness_check.parse_task(task_path)
    expected = (harness_check.task_packet(root, task) / "task.yaml").resolve()
    if task_path != expected:
        fail(f"task manifest path must match its identity: {expected}")
    profile_value = task.get("risk_profile")
    if profile_value != "risk-profile.yaml":
        fail("task.yaml must declare risk_profile: risk-profile.yaml")
    profile_path = task_path.parent / str(profile_value)
    if args.prepare:
        prepare(profile_path, task_path, task)
        print(f"risk-profile-check: refreshed source bindings in {profile_path}")
        print("next: review all seven applies/evidence judgments and applicable required_checks")
        return 0
    if args.print_triggers:
        for trigger in applicable_triggers(profile_path):
            print(trigger)
        return 0
    validate(profile_path, task_path, task)
    print("risk-profile-check: passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
