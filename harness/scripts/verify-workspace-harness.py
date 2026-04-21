#!/usr/bin/env python3

import pathlib
import sys


MODULES = (
    "p2p-frame",
    "cyfs-p2p",
    "cyfs-p2p-test",
    "sn-miner",
    "desc-tool",
)

REQUIRED_ROOT_FILES = (
    pathlib.Path("AGENTS.md"),
    pathlib.Path("docs/architecture/principles.md"),
    pathlib.Path("docs/architecture/workspace-constraints.md"),
    pathlib.Path("docs/architecture/validation-model.md"),
    pathlib.Path("harness/workspace-governance.yaml"),
    pathlib.Path("harness/rules/approval-metadata-rules.md"),
    pathlib.Path("harness/rules/acceptance-task-rules.md"),
    pathlib.Path("harness/rules/auto-pipeline-rules.md"),
    pathlib.Path("harness/rules/design-doc-rules.md"),
    pathlib.Path("harness/rules/direct-change-mapping-rules.md"),
    pathlib.Path("harness/rules/implementation-admission-rules.md"),
    pathlib.Path("harness/rules/proposal-first-acceptance-rules.md"),
    pathlib.Path("harness/rules/testing-doc-rules.md"),
    pathlib.Path("harness/rules/trigger-rules.md"),
    pathlib.Path("harness/rules/unified-test-entry-rules.md"),
    pathlib.Path("harness/process_rules/implementation-loop.md"),
    pathlib.Path("harness/process_rules/task_templates/pipeline-stage-task.md"),
    pathlib.Path("harness/process_rules/task_templates/pipeline-submodule-task.md"),
    pathlib.Path("harness/process_rules/task_templates/acceptance-return-task.md"),
    pathlib.Path("harness/checklists/proposal-approval-checklist.md"),
    pathlib.Path("harness/checklists/design-approval-checklist.md"),
    pathlib.Path("harness/checklists/testing-approval-checklist.md"),
    pathlib.Path("harness/checklists/module-acceptance-checklist.md"),
    pathlib.Path("harness/human-rules/contribution-modes.md"),
    pathlib.Path("harness/human-rules/module-tier-matrix.md"),
    pathlib.Path("harness/scripts/test-run.py"),
)

REQUIRED_PACKET_FILES = (
    "proposal.md",
    "design.md",
    "testing.md",
    "acceptance.md",
    "testplan.yaml",
)


def parse_front_matter(path: pathlib.Path) -> dict[str, str]:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    if len(lines) < 3 or lines[0].strip() != "---":
        raise ValueError(f"{path} is missing YAML front matter")

    data: dict[str, str] = {}
    for line in lines[1:]:
        stripped = line.strip()
        if stripped == "---":
            return data
        if ":" not in line:
            continue
        key, value = line.split(":", 1)
        data[key.strip()] = value.strip()

    raise ValueError(f"{path} front matter is not closed")


def main() -> int:
    version = sys.argv[1] if len(sys.argv) == 2 else "v0.1"
    errors: list[str] = []

    for path in REQUIRED_ROOT_FILES:
        if not path.exists():
            errors.append(f"missing root harness file: {path}")

    for module in MODULES:
        module_doc = pathlib.Path("docs/modules") / f"{module}.md"
        packet_dir = pathlib.Path("docs/versions") / version / "modules" / module
        if not module_doc.exists():
            errors.append(f"missing module doc: {module_doc}")
        if not packet_dir.exists():
            errors.append(f"missing packet dir: {packet_dir}")
            continue

        for name in REQUIRED_PACKET_FILES:
            path = packet_dir / name
            if not path.exists():
                errors.append(f"missing packet file: {path}")
                continue
            if name in ("proposal.md", "design.md", "testing.md"):
                metadata = parse_front_matter(path)
                if metadata.get("module") != module:
                    errors.append(f"wrong module metadata in {path}: {metadata.get('module')}")
                if metadata.get("version") != version:
                    errors.append(f"wrong version metadata in {path}: {metadata.get('version')}")

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    print(f"workspace harness structure is valid for {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
