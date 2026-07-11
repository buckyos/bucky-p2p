#!/usr/bin/env python3
"""Run every discoverable repository harness gate in one command.

This is the explicit repository-wide enforcement entrypoint for all generated
harness invariants:

- `harness-self-check.py` validates scaffold completeness.
- packet schema and document structure
- completed testing coverage
- existing admission evidence/stamps
- stage-scope manifests with machine-readable sidecars
- active auto-pipeline plans
- architecture and declared quality gates
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path


TASK_NAME_RE = re.compile(r"^\d{3,}-[a-z0-9][a-z0-9_.-]*$")


def run(cmd: list[str]) -> bool:
    print(f"check-all: running {' '.join(cmd)}")
    return subprocess.run(cmd).returncode == 0


def discover_packets(root: Path) -> list[tuple[str, str, str | None]]:
    packets: set[tuple[str, str, str | None]] = set()
    versions = root / "docs" / "versions"
    if not versions.is_dir():
        return []
    for version_dir in sorted(path for path in versions.iterdir() if path.is_dir()):
        modules_dir = version_dir / "modules"
        if not modules_dir.is_dir():
            continue
        for module_dir in sorted(path for path in modules_dir.iterdir() if path.is_dir()):
            if module_dir.name.startswith("_"):
                continue
            if (module_dir / "proposal.md").is_file():
                packets.add((version_dir.name, module_dir.name, None))
            for packet_dir in sorted(path for path in module_dir.rglob("*") if path.is_dir()):
                if any(part.startswith("_") for part in packet_dir.relative_to(module_dir).parts):
                    continue
                if TASK_NAME_RE.fullmatch(packet_dir.name):
                    packets.add(
                        (
                            version_dir.name,
                            module_dir.name,
                            packet_dir.relative_to(module_dir).as_posix(),
                        )
                    )
    for proposal in sorted(versions.glob("*/modules/**/proposal.md")):
        relative_to_versions = proposal.relative_to(versions)
        version = relative_to_versions.parts[0]
        if len(relative_to_versions.parts) < 4 or relative_to_versions.parts[1] != "modules":
            continue
        relative = Path(*relative_to_versions.parts[2:])
        if any(part.startswith("_") for part in relative.parts):
            continue
        module = relative.parts[0]
        packet_parts = relative.parts[1:-1]
        submodule = "/".join(packet_parts) if packet_parts else None
        packets.add((version, module, submodule))
    return sorted(packets, key=lambda item: (item[0], item[1], item[2] or ""))


def has_value(value: str) -> bool:
    return value.strip().strip('"').strip("'").lower() not in {
        "", "-", "n/a", "na", "none", "tbd", "todo", "pending"
    }


def pipeline_trigger_value(text: str, label: str) -> str | None:
    match = re.search(rf"(?mi)^\s*-\s*{re.escape(label)}:\s*(.+)$", text)
    return match.group(1).strip() if match else None


def pipeline_no_stage_docs(
    root: Path, version: str, module: str, task_name: str | None
) -> bool:
    if not task_name:
        return False
    plan = root / "docs" / "versions" / version / "modules" / module / task_name / "pipeline" / "plan.md"
    if not plan.is_file():
        return False
    text = plan.read_text(encoding="utf-8")
    launch = (pipeline_trigger_value(text, "User launch confirmed") or "").lower()
    launch_statement = pipeline_trigger_value(text, "User launch statement") or ""
    policy = (pipeline_trigger_value(text, "Auto-pipeline document policy") or "").lower()
    return (
        launch in {"yes", "true", "confirmed"}
        and len(launch_statement.strip()) >= 8
        and pipeline_trigger_value(text, "Version") == version
        and pipeline_trigger_value(text, "Packet module") == module
        and pipeline_trigger_value(text, "Task name") == task_name
        and (pipeline_trigger_value(text, "Proposal") or "").strip("`")
        == f"docs/versions/{version}/modules/{module}/{task_name}/proposal.md"
        and "no design/testing markdown docs" in policy
        and "testplan.yaml required" in policy
    )


def packet_path(root: Path, version: str, module: str, submodule: str | None) -> Path:
    packet = root / "docs" / "versions" / version / "modules" / module
    return packet / submodule if submodule else packet


def testing_started(packet: Path) -> bool:
    return (packet / "testing.md").exists() or (packet / "testplan.yaml").exists()


def should_run_document_structure(
    root: Path, version: str, module: str, submodule: str | None
) -> bool:
    return not pipeline_no_stage_docs(root, version, module, submodule)


def load_json(path: Path, label: str) -> dict[str, object]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"{label} is not valid JSON: {path}: {error}") from error
    if not isinstance(data, dict):
        raise ValueError(f"{label} must be a JSON object: {path}")
    return data


def admission_commands(root: Path, scripts: Path) -> tuple[list[list[str]], list[str]]:
    commands: list[list[str]] = []
    errors: list[str] = []
    evidence_root = root / "docs" / "versions"
    stamp_evidence: set[str] = set()
    for stamp_path in sorted(evidence_root.glob("*/evidence/admission/*.stamp.json")):
        try:
            stamp = load_json(stamp_path, "admission stamp")
            if stamp.get("schema") != 1:
                raise ValueError(f"unsupported admission stamp schema: {stamp_path}")
            version = stamp.get("version")
            module = stamp.get("module")
            evidence_file = stamp.get("evidence_file")
            change_ids = stamp.get("change_ids")
            if not isinstance(version, str) or not isinstance(module, str):
                raise ValueError(f"admission stamp missing version/module: {stamp_path}")
            if not isinstance(evidence_file, str) or not evidence_file:
                raise ValueError(f"admission stamp missing evidence_file: {stamp_path}")
            if not isinstance(change_ids, list) or not change_ids or not all(isinstance(item, str) for item in change_ids):
                raise ValueError(f"admission stamp missing change_ids: {stamp_path}")
            stamp_evidence.add(evidence_file)
            cmd = [
                sys.executable,
                str(scripts / "admission-check.py"),
                "--root", str(root),
                "--version", version,
                "--module", module,
                "--evidence-file", evidence_file,
                "--verify-only",
            ]
            submodule = stamp.get("submodule")
            if isinstance(submodule, str) and submodule:
                cmd += ["--submodule", submodule]
            target_module = stamp.get("target_module")
            if isinstance(target_module, str) and target_module:
                cmd += ["--target-module", target_module]
            for change_id in change_ids:
                cmd += ["--change-id", change_id]
            commands.append(cmd)
        except ValueError as error:
            errors.append(str(error))

    for evidence_path in sorted(evidence_root.glob("*/evidence/admission/*.md")):
        try:
            rel = evidence_path.resolve().relative_to(root.resolve()).as_posix()
        except ValueError:
            rel = evidence_path.as_posix()
        if rel not in stamp_evidence:
            errors.append(f"admission evidence has no machine-written stamp: {rel}")
    return commands, errors


def stage_scope_commands(root: Path, scripts: Path) -> tuple[list[list[str]], list[str]]:
    commands: list[list[str]] = []
    errors: list[str] = []
    for manifest in sorted((root / "docs" / "versions").glob("*/evidence/stage-scope/*.paths")):
        meta_path = Path(str(manifest) + ".meta.json")
        if not meta_path.is_file():
            errors.append(f"stage-scope manifest missing sidecar metadata: {meta_path}")
            continue
        try:
            meta = load_json(meta_path, "stage-scope metadata")
            if meta.get("schema") != 1:
                raise ValueError(f"unsupported stage-scope metadata schema: {meta_path}")
            stage = meta.get("stage")
            version = meta.get("version")
            module = meta.get("module")
            if stage not in {"proposal", "design", "testing", "implementation", "acceptance"}:
                raise ValueError(f"stage-scope metadata has invalid stage: {meta_path}")
            if not isinstance(version, str) or not isinstance(module, str):
                raise ValueError(f"stage-scope metadata missing version/module: {meta_path}")
            cmd = [
                sys.executable,
                str(scripts / "stage-scope-check.py"),
                "--root", str(root),
                "--stage", stage,
                "--version", version,
                "--module", module,
                "--changed-paths-file", str(manifest),
            ]
            submodule = meta.get("submodule")
            if isinstance(submodule, str) and submodule:
                cmd += ["--submodule", submodule]
            target_module = meta.get("target_module")
            if stage == "implementation" and module == "globals" and (
                not isinstance(target_module, str) or not target_module or target_module == "globals"
            ):
                raise ValueError(
                    f"globals implementation stage-scope metadata requires target_module: {meta_path}"
                )
            if isinstance(target_module, str) and target_module:
                cmd += ["--target-module", target_module]
            change_ids = meta.get("change_ids", [])
            if not isinstance(change_ids, list) or not all(isinstance(item, str) for item in change_ids):
                raise ValueError(f"stage-scope metadata change_ids must be a string list: {meta_path}")
            for change_id in change_ids:
                cmd += ["--change-id", change_id]
            commands.append(cmd)
        except ValueError as error:
            errors.append(str(error))
    manifests = {
        str(path) + ".meta.json"
        for path in (root / "docs" / "versions").glob("*/evidence/stage-scope/*.paths")
    }
    for meta_path in sorted((root / "docs" / "versions").glob("*/evidence/stage-scope/*.paths.meta.json")):
        if str(meta_path) not in manifests:
            errors.append(f"orphan stage-scope metadata has no .paths manifest: {meta_path}")
    return commands, errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    args = parser.parse_args()

    root = Path(args.root)
    scripts = root / "harness" / "scripts"
    failed = 0

    self_check = scripts / "harness-self-check.py"
    if self_check.exists():
        if not run([sys.executable, str(self_check), "--root", str(root)]):
            failed += 1
    else:
        print(f"check-all: missing {self_check}", file=sys.stderr)
        failed += 1

    architecture_check = scripts / "architecture-doc-check.py"
    if architecture_check.exists():
        if not run([sys.executable, str(architecture_check), "--root", str(root)]):
            failed += 1
    else:
        print(f"check-all: missing {architecture_check}", file=sys.stderr)
        failed += 1

    packets = discover_packets(root)
    if not packets:
        print("check-all: no module packets discovered under docs/versions/")
    for version, module, submodule in packets:
        auto_pipeline_packet = not should_run_document_structure(
            root, version, module, submodule
        )
        cmd = [
            sys.executable,
            str(scripts / "schema-check.py"),
            "--root",
            str(root),
            "--version",
            version,
            "--module",
            module,
        ]
        if submodule:
            cmd += ["--submodule", submodule]
        if not run(cmd):
            failed += 1

        if not auto_pipeline_packet:
            doc_cmd = [
                sys.executable,
                str(scripts / "doc-structure-check.py"),
                "--root", str(root),
                "--version", version,
                "--module", module,
                "--docs", "mandatory",
            ]
            if submodule:
                doc_cmd += ["--submodule", submodule]
            if not run(doc_cmd):
                failed += 1

        packet = packet_path(root, version, module, submodule)
        if testing_started(packet):
            testing_cmd = [
                sys.executable,
                str(scripts / "testing-coverage-check.py"),
                "--root", str(root),
                "--version", version,
                "--module", module,
            ]
            if submodule:
                testing_cmd += ["--submodule", submodule]
            if not run(testing_cmd):
                failed += 1

    for cmd, errors in (
        admission_commands(root, scripts),
        stage_scope_commands(root, scripts),
    ):
        for error in errors:
            print(f"check-all: {error}", file=sys.stderr)
            failed += 1
        for discovered_cmd in cmd:
            if not run(discovered_cmd):
                failed += 1

    for pipeline_plan in sorted((root / "docs" / "versions").glob("*/modules/**/pipeline/plan.md")):
        if any(part.startswith("_") for part in pipeline_plan.relative_to(root).parts):
            continue
        if not run([
            sys.executable,
            str(scripts / "pipeline-plan-check.py"),
            pipeline_plan.relative_to(root).as_posix(),
            "--root", str(root),
        ]):
            failed += 1

    if not run([
        sys.executable,
        str(scripts / "quality-check.py"),
        "--root", str(root),
    ]):
        failed += 1

    if failed:
        print(f"check-all: FAILED ({failed} check(s) failed)", file=sys.stderr)
        return 1
    print("check-all: passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
