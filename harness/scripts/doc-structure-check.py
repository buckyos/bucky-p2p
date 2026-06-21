#!/usr/bin/env python3
"""Validate Harness Engineering module packet document structure."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


MAX_HUMAN_DOC_LINES = 1000
TABLE_SEPARATOR_RE = re.compile(r"^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?\s*$")
EMPTY_VALUES = {"", "-", "n/a", "na", "none", "tbd", "todo"}

REQUIRED_SECTIONS = {
    "proposal.md": (
        "Background and Goal",
        "Scope",
        "Assumptions and Ambiguities",
        "Constraints",
        "Requirement Challenge",
        "Large Module Submodule Decision",
        "Trigger Matrix",
        "High-Level Outcomes",
        "Proposal Items",
        "Success Criteria",
        "Risks",
        "Downstream Follow-Up",
    ),
    "design.md": (
        "Design Scope",
        "Overall Approach",
        "Simplicity Check",
        "Current Structure",
        "Invariants to Preserve",
        "Submodules",
        "Boundary Rationale",
        "Boundary Decision Matrix",
        "Dependency Graph",
        "Key Call Flows",
        "Large Module Submodule Decision",
        "Trigger Matrix",
        "Directly Mapped Change Items",
        "Implementation Order",
        "Key Decisions",
        "Data and State",
        "Testability",
        "Interfaces and Dependencies",
        "Document Index",
        "Risks and Rollback",
    ),
    "testing.md": (
        "Test Document Index",
        "Unified Test Entry",
        "Submodule Tests",
        "Module-Level Tests",
        "External Interface Tests",
        "Direct Change Coverage",
        "Case-Type Coverage",
        "Design Element Coverage",
        "Validation Rationale",
        "Unit Tests",
        "DV Tests",
        "Integration Tests",
        "Definition of Done",
    ),
}

REQUIRED_TABLE_COLUMNS = {
    ("proposal.md", "Proposal Items"): ("proposal_id", "change_id", "outcome", "success_evidence"),
    ("proposal.md", "Requirement Challenge"): (
        "question",
        "evaluation",
        "risk_or_tradeoff",
        "decision",
    ),
    ("proposal.md", "Large Module Submodule Decision"): (
        "submodule",
        "new_or_existing",
        "responsibility",
        "proposal_packet",
        "reason",
    ),
    ("proposal.md", "Trigger Matrix"): (
        "trigger_category",
        "applies",
        "evidence",
        "required_checks",
        "deferred_checks_and_reason",
    ),
    ("design.md", "Submodules"): ("submodule", "type", "responsibility", "depends_on"),
    ("design.md", "Boundary Decision Matrix"): (
        "boundary",
        "classification",
        "business_responsibility",
        "shared_logic_or_technical_area",
        "decision",
    ),
    ("design.md", "Dependency Graph"): ("source", "depends_on", "reason", "cycle_check"),
    ("design.md", "Key Call Flows"): (
        "flow",
        "caller",
        "callee_submodule_path",
        "purpose",
        "failure_handling",
    ),
    ("design.md", "Large Module Submodule Decision"): (
        "submodule",
        "source_proposal",
        "decision",
        "design_packet",
        "reason",
    ),
    ("design.md", "Trigger Matrix"): (
        "trigger_category",
        "applies",
        "evidence",
        "design_coverage",
        "required_checks",
        "deferred_checks_and_reason",
    ),
    ("design.md", "Directly Mapped Change Items"): (
        "change_id",
        "proposal_id",
        "design_coverage",
        "scope_paths",
    ),
    ("testing.md", "Direct Change Coverage"): (
        "change_id",
        "design_source",
        "validation_id",
        "testplan_level",
        "testplan_step_id",
        "gap",
        "gap_manual_reason",
    ),
    ("testing.md", "Case-Type Coverage"): (
        "change_id",
        "case_type",
        "required",
        "validation_id",
        "level",
        "status",
        "gap_manual_reason",
    ),
    ("testing.md", "Design Element Coverage"): (
        "element_type",
        "design_source",
        "derived_cases",
        "level",
        "status",
        "gap_manual_reason",
    ),
    ("testing.md", "Unit Tests"): (
        "function_or_unit",
        "branch_or_condition",
        "covered_behavior",
        "test_file",
        "status",
        "gap_manual_reason",
    ),
    ("testing.md", "DV Tests"): (
        "workflow",
        "kind",
        "entry",
        "expected_result",
        "test_file_or_script",
        "status",
        "gap_manual_reason",
    ),
    ("testing.md", "Integration Tests"): (
        "contract_or_flow",
        "modules_involved",
        "success_case",
        "failure_case",
        "test_file",
        "status",
        "gap_manual_reason",
    ),
}

TRIGGER_CATEGORIES = {
    "contract/protocol",
    "data/schema",
    "security/privacy/permission",
    "runtime/integration",
    "build/dependency/config/deployment",
    "ui/datamodel/workflow",
    "harness/process",
}
CASE_TYPES = {"normal", "boundary", "negative", "error", "compatibility", "lifecycle", "cross-module"}
TEST_LEVELS = {"unit", "dv", "integration"}
DESIGN_ELEMENT_TYPES = {
    "parameter-domain",
    "state-transition",
    "failure-path",
    "error-handling",
    "invariant",
    "concurrency",
}
COVERAGE_STATUSES = {"covered", "gap", "manual", "disabled", "not-applicable"}
DV_KINDS = {"lifecycle", "main", "failure", "config", "persistence"}
LEVEL_TABLE_HEADINGS = ("Unit Tests", "DV Tests", "Integration Tests")
SUBMODULE_TYPES = {"business", "shared", "technical", "assembly"}
GRAB_BAG_SHARED_NAMES = {"common", "utils", "util", "misc", "helpers", "helper"}
COMPATIBILITY_VALUES = {"new", "backward-compatible", "migration-required", "breaking"}
NOT_APPLICABLE_RE = re.compile(r"(?i)^not-applicable\s*:\s*\S.*$")


def fail(message: str) -> None:
    print(f"doc-structure-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def read_text(path: Path) -> str:
    if not path.exists():
        fail(f"missing required file: {path}")
    return path.read_text(encoding="utf-8")


def normalize_column(value: str) -> str:
    return re.sub(r"[^a-z0-9]+", "_", value.strip().lower()).strip("_")


def split_table_row(line: str) -> list[str]:
    parts = [part.strip() for part in line.strip().split("|")]
    if parts and parts[0] == "":
        parts = parts[1:]
    if parts and parts[-1] == "":
        parts = parts[:-1]
    return parts


def section_body(text: str, heading: str, path: Path) -> str:
    pattern = re.compile(rf"(?m)^##\s+{re.escape(heading)}\s*$")
    match = pattern.search(text)
    if not match:
        fail(f"{path} missing required section: ## {heading}")
    next_heading = re.search(r"(?m)^##\s+", text[match.end() :])
    end = match.end() + next_heading.start() if next_heading else len(text)
    return text[match.end() : end]


def table_after_heading(text: str, heading: str, path: Path) -> list[dict[str, str]]:
    body = section_body(text, heading, path)
    lines = body.splitlines()
    table_start = None
    for index, line in enumerate(lines):
        if "|" in line and index + 1 < len(lines) and TABLE_SEPARATOR_RE.match(lines[index + 1]):
            table_start = index
            break
    if table_start is None:
        fail(f"{path} section ## {heading} missing required table")

    headers = [normalize_column(cell) for cell in split_table_row(lines[table_start])]
    rows: list[dict[str, str]] = []
    for line in lines[table_start + 2 :]:
        if not line.strip() or not line.lstrip().startswith("|"):
            break
        values = split_table_row(line)
        row = {header: values[pos].strip() if pos < len(values) else "" for pos, header in enumerate(headers)}
        rows.append(row)
    if not rows:
        fail(f"{path} section ## {heading} has no data rows")
    return rows


def non_empty(value: str) -> bool:
    return value.strip().lower() not in EMPTY_VALUES


def check_line_count(path: Path, max_lines: int) -> None:
    line_count = len(read_text(path).splitlines())
    if line_count > max_lines:
        fail(f"{path} has {line_count} lines; split docs above {max_lines} lines")


def check_sections(path: Path, text: str) -> None:
    for heading in REQUIRED_SECTIONS[path.name]:
        section_body(text, heading, path)


def check_tables(path: Path, text: str) -> None:
    for (name, heading), columns in REQUIRED_TABLE_COLUMNS.items():
        if name != path.name:
            continue
        rows = table_after_heading(text, heading, path)
        available = set(rows[0])
        missing = [column for column in columns if column not in available]
        if missing:
            fail(f"{path} ## {heading} missing columns: {', '.join(missing)}")
        for index, row in enumerate(rows, start=1):
            if all(not non_empty(value) for value in row.values()):
                fail(f"{path} ## {heading} row {index} is empty placeholder content")
            first_value = row.get(columns[0], "").strip()
            if is_not_applicable(first_value):
                require_not_applicable_reason(path, f"## {heading} row {index}", first_value)
                continue
            for column in columns:
                if column in {
                    "depends_on",
                    "gap_manual_reason",
                    "deferred_checks_and_reason",
                    "required_checks",
                    "testplan_step_id",
                    "type",
                    "test_file",
                    "test_file_or_script",
                    "derived_cases",
                    "design_source",
                    "level",
                }:
                    continue
                if not non_empty(row.get(column, "")):
                    fail(f"{path} ## {heading} row {index} column {column} is empty placeholder content")


def split_dependencies(value: str) -> list[str]:
    if not non_empty(value):
        return []
    if value.strip().lower() in {"none", "no", "n/a", "na"}:
        return []
    parts = re.split(r"[,;/]+|\band\b", value)
    return [part.strip().strip("`") for part in parts if non_empty(part)]


def check_dependency_graph(path: Path, text: str) -> None:
    rows = table_after_heading(text, "Dependency Graph", path)
    graph: dict[str, set[str]] = {}
    for row in rows:
        source = row.get("source", "").strip().strip("`")
        if not non_empty(source):
            continue
        graph.setdefault(source, set())
        for dependency in split_dependencies(row.get("depends_on", "")):
            graph[source].add(dependency)
            graph.setdefault(dependency, set())

    visiting: set[str] = set()
    visited: set[str] = set()
    stack: list[str] = []

    def visit(node: str) -> None:
        if node in visited:
            return
        if node in visiting:
            cycle = " -> ".join([*stack, node])
            fail(f"{path} dependency graph contains a cycle: {cycle}")
        visiting.add(node)
        stack.append(node)
        for dependency in sorted(graph.get(node, ())):
            visit(dependency)
        stack.pop()
        visiting.remove(node)
        visited.add(node)

    for node in sorted(graph):
        visit(node)


def is_not_applicable(value: str) -> bool:
    return value.strip().lower().startswith("not-applicable")


def require_not_applicable_reason(path: Path, context: str, value: str) -> None:
    if not NOT_APPLICABLE_RE.match(value.strip()):
        fail(f"{path} {context}: not-applicable entries need a concrete reason: `not-applicable: <reason>`")


def check_submodule_layering(path: Path, text: str) -> None:
    """Enforce submodule type vocabulary and dependency direction.

    Dependencies flow business -> shared/technical; nothing depends on
    assembly; a shared submodule needs >= 2 consumers in the graph.
    """
    types: dict[str, str] = {}
    for index, row in enumerate(table_after_heading(text, "Submodules", path), start=1):
        name = row.get("submodule", "").strip().strip("`")
        if not non_empty(name):
            continue
        if is_not_applicable(name):
            require_not_applicable_reason(path, f"## Submodules row {index}", name)
            continue
        type_value = row.get("type", "").strip().lower()
        if type_value not in SUBMODULE_TYPES:
            fail(
                f"{path} ## Submodules row {index} type must be one of "
                f"business/shared/technical/assembly: {type_value or '<empty>'}"
            )
        if type_value == "shared" and name.lower() in GRAB_BAG_SHARED_NAMES:
            fail(
                f"{path} ## Submodules row {index} shared submodule '{name}' is a grab-bag name; "
                "name the single responsibility it owns"
            )
        types[name.lower()] = type_value

    edges: list[tuple[str, str]] = []
    for row in table_after_heading(text, "Dependency Graph", path):
        source = row.get("source", "").strip().strip("`")
        if not non_empty(source):
            continue
        for dependency in split_dependencies(row.get("depends_on", "")):
            edges.append((source.lower(), dependency.lower()))

    for source, target in edges:
        source_type = types.get(source)
        target_type = types.get(target)
        if source_type in {"shared", "technical"} and target_type == "business":
            fail(
                f"{path} dependency direction violation: {source_type} submodule '{source}' depends on "
                f"business submodule '{target}'; dependencies flow business -> shared/technical"
            )
        if target_type == "assembly":
            fail(
                f"{path} dependency direction violation: '{source}' depends on assembly submodule "
                f"'{target}'; assembly is the composition root and nothing may depend on it"
            )

    consumers: dict[str, set[str]] = {}
    for source, target in edges:
        consumers.setdefault(target, set()).add(source)
    for name, type_value in types.items():
        if type_value != "shared":
            continue
        count = len(consumers.get(name, set()))
        if count < 2:
            fail(
                f"{path} shared submodule '{name}' has {count} consumer(s) in the dependency graph; "
                "a shared submodule needs >= 2 consumers (record external module consumers as edges) "
                "or its logic belongs inside its single consumer"
            )


def check_exported_interfaces(path: Path, text: str) -> None:
    rows = table_after_heading(text, "Interfaces and Dependencies", path)
    required = ("interface", "consumer", "compatibility")
    missing = [column for column in required if column not in set(rows[0])]
    if missing:
        fail(f"{path} exported-interface table missing columns: {', '.join(missing)}")
    for index, row in enumerate(rows, start=1):
        interface = row.get("interface", "").strip()
        if is_not_applicable(interface):
            require_not_applicable_reason(path, f"exported-interface row {index}", interface)
            continue
        for column in required:
            if not non_empty(row.get(column, "")):
                fail(f"{path} exported-interface row {index} column {column} is empty placeholder content")
        compatibility = row.get("compatibility", "").strip().lower()
        if compatibility not in COMPATIBILITY_VALUES:
            fail(
                f"{path} exported-interface row {index} compatibility must be one of: "
                f"{', '.join(sorted(COMPATIBILITY_VALUES))}"
            )
        if compatibility in {"breaking", "migration-required"} and not non_empty(row.get("notes", "")):
            fail(
                f"{path} exported-interface row {index} {compatibility} change must list affected "
                "callers and migration path in notes"
            )


def check_data_ownership(path: Path, text: str) -> None:
    rows = table_after_heading(text, "Data and State", path)
    required = ("data_or_state", "owner_submodule", "access_for_others", "state_transitions")
    missing = [column for column in required if column not in set(rows[0])]
    if missing:
        fail(f"{path} ## Data and State missing columns: {', '.join(missing)}")
    seen: dict[str, int] = {}
    for index, row in enumerate(rows, start=1):
        data = row.get("data_or_state", "").strip()
        if is_not_applicable(data):
            require_not_applicable_reason(path, f"## Data and State row {index}", data)
            continue
        for column in required:
            if not non_empty(row.get(column, "")):
                fail(f"{path} ## Data and State row {index} column {column} is empty placeholder content")
        key = data.lower()
        if key in seen:
            fail(
                f"{path} ## Data and State rows {seen[key]} and {index} both declare '{data}'; "
                "every datum has exactly one owning submodule"
            )
        seen[key] = index


def check_key_decisions(path: Path, text: str) -> None:
    rows = table_after_heading(text, "Key Decisions", path)
    required = ("decision", "chosen", "alternatives_considered", "rejection_reason")
    missing = [column for column in required if column not in set(rows[0])]
    if missing:
        fail(f"{path} ## Key Decisions missing columns: {', '.join(missing)}")
    for index, row in enumerate(rows, start=1):
        for column in ("decision", "chosen", "alternatives_considered"):
            if not non_empty(row.get(column, "")):
                fail(f"{path} ## Key Decisions row {index} column {column} is empty placeholder content")
        alternatives = row.get("alternatives_considered", "").strip()
        if is_not_applicable(alternatives):
            require_not_applicable_reason(path, f"## Key Decisions row {index}", alternatives)
            continue
        if not non_empty(row.get("rejection_reason", "")):
            fail(f"{path} ## Key Decisions row {index} column rejection_reason is empty placeholder content")


def check_trigger_matrix(path: Path, text: str) -> None:
    if path.name not in {"proposal.md", "design.md"}:
        return
    rows = table_after_heading(text, "Trigger Matrix", path)
    seen: set[str] = set()
    for index, row in enumerate(rows, start=1):
        category = row.get("trigger_category", "").strip().lower()
        applies = row.get("applies", "").strip().lower()
        seen.add(category)
        if category not in TRIGGER_CATEGORIES:
            fail(f"{path} ## Trigger Matrix row {index} has unknown trigger category: {category}")
        if applies not in {"yes", "no"}:
            fail(f"{path} ## Trigger Matrix row {index} Applies must be yes or no")
        if not non_empty(row.get("evidence", "")):
            fail(f"{path} ## Trigger Matrix row {index} must include evidence")
        if applies == "yes" and not non_empty(row.get("required_checks", "")):
            fail(f"{path} ## Trigger Matrix row {index} applies=yes requires checks")
        deferred = row.get("deferred_checks_and_reason", "")
        if non_empty(deferred) and not re.search(r"(?i)\b(owner|risk|acceptance)\b", deferred):
            fail(f"{path} ## Trigger Matrix row {index} deferred checks must include owner, risk, or acceptance impact")
    missing = sorted(TRIGGER_CATEGORIES - seen)
    if missing:
        fail(f"{path} ## Trigger Matrix missing categories: {', '.join(missing)}")


def check_case_type_coverage(path: Path, text: str) -> None:
    if path.name != "testing.md":
        return
    rows = table_after_heading(text, "Case-Type Coverage", path)
    seen: set[str] = set()
    for index, row in enumerate(rows, start=1):
        case_type = row.get("case_type", "").strip().lower()
        required = row.get("required", "").strip().lower()
        status = row.get("status", "").strip().lower()
        seen.add(case_type)
        if case_type not in CASE_TYPES:
            fail(f"{path} ## Case-Type Coverage row {index} has unknown case_type: {case_type}")
        if required not in {"yes", "no"}:
            fail(f"{path} ## Case-Type Coverage row {index} Required must be yes or no")
        if status not in {"covered", "gap", "manual", "disabled", "not-applicable"}:
            fail(f"{path} ## Case-Type Coverage row {index} has invalid status: {status}")
        if required == "yes" and status == "not-applicable":
            fail(f"{path} ## Case-Type Coverage row {index} cannot mark required coverage not-applicable")
        if status in {"gap", "manual", "disabled", "not-applicable"} and not non_empty(row.get("gap_manual_reason", "")):
            fail(f"{path} ## Case-Type Coverage row {index} requires Gap / Manual Reason")
        level = row.get("level", "").strip().lower()
        if status == "covered" and level not in TEST_LEVELS:
            fail(
                f"{path} ## Case-Type Coverage row {index} covered cases must declare the implementing "
                f"level (unit/dv/integration): {level or '<empty>'}"
            )
    missing = sorted(CASE_TYPES - seen)
    if missing:
        fail(f"{path} ## Case-Type Coverage missing case types: {', '.join(missing)}")


def check_design_element_coverage(path: Path, text: str) -> None:
    if path.name != "testing.md":
        return
    rows = table_after_heading(text, "Design Element Coverage", path)
    seen: set[str] = set()
    for index, row in enumerate(rows, start=1):
        element_type = row.get("element_type", "").strip().lower()
        status = row.get("status", "").strip().lower()
        reason = row.get("gap_manual_reason", "")
        seen.add(element_type)
        if element_type not in DESIGN_ELEMENT_TYPES:
            fail(f"{path} ## Design Element Coverage row {index} has unknown element_type: {element_type}")
        if status not in COVERAGE_STATUSES:
            fail(f"{path} ## Design Element Coverage row {index} has invalid status: {status}")
        if status == "covered":
            if not non_empty(row.get("design_source", "")):
                fail(f"{path} ## Design Element Coverage row {index} covered rows must name the design source")
            if not non_empty(row.get("derived_cases", "")):
                fail(f"{path} ## Design Element Coverage row {index} covered rows must list derived cases")
            level = row.get("level", "").strip().lower()
            if level not in TEST_LEVELS:
                fail(
                    f"{path} ## Design Element Coverage row {index} covered rows must declare the "
                    f"implementing level (unit/dv/integration): {level or '<empty>'}"
                )
        elif not non_empty(reason):
            fail(f"{path} ## Design Element Coverage row {index} requires Gap / Manual Reason")
    missing = sorted(DESIGN_ELEMENT_TYPES - seen)
    if missing:
        fail(f"{path} ## Design Element Coverage missing element types: {', '.join(missing)}")


def check_level_tables(path: Path, text: str) -> None:
    if path.name != "testing.md":
        return
    for heading in LEVEL_TABLE_HEADINGS:
        rows = table_after_heading(text, heading, path)
        first_column = REQUIRED_TABLE_COLUMNS[(path.name, heading)][0]
        seen_kinds: set[str] = set()
        escaped_rows = 0
        for index, row in enumerate(rows, start=1):
            if is_not_applicable(row.get(first_column, "").strip()):
                escaped_rows += 1
                continue
            status = row.get("status", "").strip().lower()
            if status not in COVERAGE_STATUSES:
                fail(f"{path} ## {heading} row {index} has invalid status: {status}")
            if status != "covered" and not non_empty(row.get("gap_manual_reason", "")):
                fail(f"{path} ## {heading} row {index} requires Gap / Manual Reason")
            if status == "covered" and "test_file" in row and not non_empty(row.get("test_file", "")):
                fail(f"{path} ## {heading} row {index} covered rows must name the test file")
            if heading == "DV Tests":
                kind = row.get("kind", "").strip().lower()
                if kind not in DV_KINDS:
                    fail(
                        f"{path} ## DV Tests row {index} kind must be one of "
                        f"{'/'.join(sorted(DV_KINDS))}: {kind or '<empty>'}"
                    )
                seen_kinds.add(kind)
        if heading == "DV Tests" and escaped_rows < len(rows):
            missing_kinds = sorted({"lifecycle", "main", "failure"} - seen_kinds)
            if missing_kinds:
                fail(
                    f"{path} ## DV Tests must include lifecycle, main, and failure workflow rows "
                    f"(covered or with recorded gaps); missing kinds: {', '.join(missing_kinds)}"
                )


def check_submodule_doc_placement(packet: Path) -> None:
    for folder_name in ("design", "testing"):
        folder = packet / folder_name
        if not folder.exists():
            continue
        for name in ("proposal.md", "design.md", "testing.md", "testplan.yaml"):
            matches = sorted(folder.rglob(name))
            if matches:
                rendered = ", ".join(str(path) for path in matches[:5])
                fail(f"submodule packet docs belong directly under the submodule packet, not {folder_name}/: {rendered}")


def check_doc(path: Path, max_lines: int) -> None:
    text = read_text(path)
    check_line_count(path, max_lines)
    check_sections(path, text)
    check_tables(path, text)
    if path.name == "design.md":
        check_dependency_graph(path, text)
        check_submodule_layering(path, text)
        check_exported_interfaces(path, text)
        check_data_ownership(path, text)
        check_key_decisions(path, text)
    check_trigger_matrix(path, text)
    check_case_type_coverage(path, text)
    check_design_element_coverage(path, text)
    check_level_tables(path, text)
    if path.name == "testing.md" and "harness/scripts/test-run.py" not in text:
        fail(f"{path} must reference the unified test entrypoint")


def packet_path(root: Path, version: str, module: str, submodule: str | None) -> Path:
    packet = root / "docs" / "versions" / version / "modules" / module
    if submodule:
        packet = packet / submodule
    return packet


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--version", required=True)
    parser.add_argument("--module", required=True)
    parser.add_argument("--submodule")
    parser.add_argument("--max-lines", type=int, default=MAX_HUMAN_DOC_LINES)
    parser.add_argument(
        "--docs",
        choices=("all", "mandatory", "proposal", "design", "testing"),
        default="all",
    )
    args = parser.parse_args()

    packet = packet_path(Path(args.root), args.version, args.module, args.submodule)
    if args.docs in {"all", "mandatory", "proposal"}:
        check_doc(packet / "proposal.md", args.max_lines)
    if args.docs in {"all", "mandatory", "design"}:
        check_doc(packet / "design.md", args.max_lines)
    if args.docs in {"all", "testing"}:
        testing = packet / "testing.md"
        if testing.exists():
            check_doc(testing, args.max_lines)
        elif args.docs == "testing":
            fail(f"missing required file: {testing}")

    check_submodule_doc_placement(packet)
    print("doc-structure-check: passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
