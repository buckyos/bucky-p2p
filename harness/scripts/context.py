#!/usr/bin/env python3
"""Route a Harness task to only the rules and documents it needs.

The router intentionally supports a small, dependency-free YAML subset used by
`harness/rules/index.yaml` and optional `harness/custom-rules/index.yaml`.
List values must use inline YAML/Python list syntax.
"""

from __future__ import annotations

import argparse
import ast
import fnmatch
import json
import re
import sys
from pathlib import Path


HIGH_RISK_STAGES = {"proposal", "design", "implementation", "testing", "acceptance"}
STAGES = HIGH_RISK_STAGES | {"general"}
PIPELINE_STAGES = ("design", "implementation", "testing", "acceptance")
MODES = {"manual", "auto-pipeline"}
WORKFLOW_TIERS = {"trivial", "standard", "high-risk"}
ACTIVATIONS = {"always", "bootstrap", "mode", "stage", "trigger"}
LIST_FIELDS = {"tiers", "stages", "modes", "triggers", "path_patterns"}
SCALAR_FIELDS = {"activation", "file"}


def fail(message: str) -> None:
    print(f"context: {message}", file=sys.stderr)
    raise SystemExit(1)


def parse_inline_list(value: str, *, path: Path, line_number: int) -> list[str]:
    try:
        parsed = ast.literal_eval(value)
    except (SyntaxError, ValueError):
        if not (value.startswith("[") and value.endswith("]")):
            fail(f"{path}:{line_number}: invalid inline list {value!r}")
        body = value[1:-1].strip()
        parsed = [] if not body else [
            item.strip().strip('"').strip("'") for item in body.split(",")
        ]
    if not isinstance(parsed, list) or not all(isinstance(item, str) for item in parsed):
        fail(f"{path}:{line_number}: value must be an inline list of strings")
    if any(not item for item in parsed):
        fail(f"{path}:{line_number}: inline list contains an empty item")
    return parsed


def parse_index(path: Path) -> list[dict[str, object]]:
    if not path.is_file():
        fail(f"missing rule index: {path}")
    text = path.read_text(encoding="utf-8")
    if not re.search(r"(?m)^schema_version:\s*1\s*$", text):
        fail(f"{path}: missing or unsupported schema_version (expected 1)")
    rules_match = re.search(r"(?m)^rules:\s*$", text)
    if not rules_match:
        fail(f"{path}: missing top-level rules list")

    rules: list[dict[str, object]] = []
    current: dict[str, object] | None = None
    for line_number, line in enumerate(text[rules_match.end() :].splitlines(), start=3):
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        id_match = re.fullmatch(r"-\s+id:\s*([A-Za-z0-9][A-Za-z0-9_.-]*)", stripped)
        if id_match:
            current = {"id": id_match.group(1)}
            rules.append(current)
            continue
        field_match = re.fullmatch(r"([a-z_]+):\s*(.*?)\s*", stripped)
        if not field_match or current is None:
            fail(f"{path}:{line_number}: unrecognized index entry {stripped!r}")
        field, value = field_match.groups()
        if field in LIST_FIELDS:
            current[field] = parse_inline_list(value, path=path, line_number=line_number)
        elif field in SCALAR_FIELDS:
            current[field] = value.strip().strip('"').strip("'")
        else:
            fail(f"{path}:{line_number}: unsupported rule field {field!r}")

    if not rules:
        fail(f"{path}: rules list must not be empty")
    validate_entries(path, rules)
    return rules


def validate_entries(path: Path, rules: list[dict[str, object]]) -> None:
    ids = [str(rule.get("id", "")) for rule in rules]
    if len(ids) != len(set(ids)):
        fail(f"{path}: duplicate rule ids")
    files: list[str] = []
    for rule in rules:
        rule_id = str(rule["id"])
        missing = [field for field in ("file", "activation") if not rule.get(field)]
        if missing:
            fail(f"{path}: rule {rule_id} missing fields: {', '.join(missing)}")
        activation = str(rule["activation"])
        if activation not in ACTIVATIONS:
            fail(f"{path}: rule {rule_id} has unsupported activation {activation!r}")
        tiers = set(rule.get("tiers", WORKFLOW_TIERS))
        stages = set(rule.get("stages", []))
        modes = set(rule.get("modes", []))
        if not tiers:
            fail(f"{path}: rule {rule_id} must not declare an empty tiers list")
        if not tiers <= WORKFLOW_TIERS:
            fail(
                f"{path}: rule {rule_id} has unsupported workflow tiers: "
                f"{sorted(tiers - WORKFLOW_TIERS)}"
            )
        if not stages <= STAGES:
            fail(f"{path}: rule {rule_id} has unsupported stages: {sorted(stages - STAGES)}")
        if not modes <= MODES:
            fail(f"{path}: rule {rule_id} has unsupported modes: {sorted(modes - MODES)}")
        if activation == "stage" and not stages:
            fail(f"{path}: stage rule {rule_id} must declare stages")
        if activation == "mode" and not modes:
            fail(f"{path}: mode rule {rule_id} must declare modes")
        if activation == "trigger" and not (rule.get("triggers") or rule.get("path_patterns")):
            fail(f"{path}: trigger rule {rule_id} needs triggers or path_patterns")
        relative = Path(str(rule["file"]))
        if relative.is_absolute() or ".." in relative.parts:
            fail(f"{path}: rule {rule_id} has unsafe file path {relative}")
        files.append(relative.as_posix())
    if len(files) != len(set(files)):
        fail(f"{path}: each rule file must have exactly one index entry")


def normalized_triggers(values: list[str]) -> set[str]:
    return {value.strip().lower().replace("_", "-") for value in values if value.strip()}


def path_matches(pattern: str, changed_path: str) -> bool:
    # A declared Scope Path may itself be a glob. Match that declaration only
    # when it is already specific enough to satisfy the rule pattern. A broad
    # scope such as src/** must not activate every leading-** risk rule; the
    # reviewed risk profile supplies pre-edit triggers for those cases.
    candidates = [pattern]
    if pattern.startswith("**/"):
        candidates.append(pattern[3:])
    return any(fnmatch.fnmatchcase(changed_path, candidate) for candidate in candidates)


def entry_matches(
    rule: dict[str, object],
    *,
    workflow_tier: str,
    stage: str,
    mode: str,
    triggers: set[str],
    changed_paths: list[str],
    include_bootstrap: bool,
) -> tuple[bool, list[str]]:
    tiers = set(rule.get("tiers", WORKFLOW_TIERS))
    stages = set(rule.get("stages", []))
    modes = set(rule.get("modes", []))
    if workflow_tier not in tiers:
        return False, []
    if stages and stage not in stages:
        return False, []
    if modes and mode not in modes:
        return False, []

    activation = str(rule["activation"])
    reasons: list[str] = []
    if activation == "always":
        reasons.append("always")
    elif activation == "bootstrap":
        if include_bootstrap:
            reasons.append("bootstrap")
    elif activation == "stage":
        reasons.append(f"stage:{stage}")
    elif activation == "mode":
        reasons.append(f"mode:{mode}")
    elif activation == "trigger":
        declared = normalized_triggers(list(rule.get("triggers", [])))
        for trigger in sorted(triggers & declared):
            reasons.append(f"trigger:{trigger}")
        for changed_path in changed_paths:
            for pattern in rule.get("path_patterns", []):
                if path_matches(str(pattern), changed_path):
                    reasons.append(f"path:{changed_path}")
                    break
    return bool(reasons), reasons


def safe_relative(root: Path, value: str, *, description: str) -> Path:
    relative = Path(value)
    if relative.is_absolute() or ".." in relative.parts:
        fail(f"unsafe {description} path: {value}")
    candidate = (root / relative).resolve()
    try:
        candidate.relative_to(root.resolve())
    except ValueError:
        fail(f"{description} path resolves outside repository: {value}")
    return candidate


def add_result(
    results: dict[str, dict[str, object]],
    root: Path,
    relative: str,
    *,
    source: str,
    reasons: list[str],
    require_exists: bool = True,
) -> None:
    path = safe_relative(root, relative, description=source)
    if require_exists and not path.is_file():
        fail(f"indexed {source} file does not exist: {relative}")
    if not path.is_file():
        return
    key = path.relative_to(root.resolve()).as_posix()
    existing = results.setdefault(key, {"path": key, "sources": [], "reasons": []})
    if source not in existing["sources"]:
        existing["sources"].append(source)
    for reason in reasons:
        if reason not in existing["reasons"]:
            existing["reasons"].append(reason)


def add_packet_documents(
    results: dict[str, dict[str, object]],
    root: Path,
    packet: str | None,
    *,
    stage: str,
    mode: str,
    auto_pipeline_start_stage: str | None,
) -> None:
    if not packet:
        return
    packet_path = safe_relative(root, packet, description="task packet")
    if not packet_path.is_dir():
        fail(f"task packet directory does not exist: {packet}")
    names: list[str] = ["proposal.md", "risk-profile.yaml"]
    if mode == "auto-pipeline":
        names.append("pipeline/plan.md")
        if auto_pipeline_start_stage != "design" and stage in {
            "design", "implementation", "testing", "acceptance"
        }:
            names.append("design.md")
    elif stage in {"design", "implementation", "testing", "acceptance"}:
        names.append("design.md")
    if stage == "testing":
        if (
            mode != "auto-pipeline"
            or PIPELINE_STAGES.index("testing")
            < PIPELINE_STAGES.index(str(auto_pipeline_start_stage))
        ):
            names.append("testing.md")
        names.append("testplan.yaml")
    elif stage == "acceptance":
        if (
            mode != "auto-pipeline"
            or PIPELINE_STAGES.index("testing")
            < PIPELINE_STAGES.index(str(auto_pipeline_start_stage))
        ):
            names.append("testing.md")
        names.extend(["testplan.yaml", "acceptance-report.md"])
    for name in names:
        relative = (Path(packet) / name).as_posix()
        add_result(
            results,
            root,
            relative,
            source="task-packet",
            reasons=[f"stage:{stage}"],
            require_exists=False,
        )


def expand_scope_files(root: Path, values: list[str]) -> list[str]:
    """Expand task Scope Paths into existing repository files."""
    result: list[str] = []
    root = root.resolve()
    for value in values:
        relative = Path(value)
        if relative.is_absolute() or ".." in relative.parts:
            fail(f"unsafe task scope path: {value}")
        normalized = relative.as_posix().rstrip("/")
        wildcard_positions = [
            normalized.find(character)
            for character in "*?["
            if character in normalized
        ]
        if wildcard_positions:
            prefix = normalized[: min(wildcard_positions)].rstrip("/")
            search_root = safe_relative(
                root,
                prefix or ".",
                description="task scope prefix",
            )
            if not search_root.exists():
                continue
            candidates = search_root.rglob("*") if search_root.is_dir() else [search_root]
            for candidate in candidates:
                if candidate.is_symlink() or not candidate.is_file():
                    continue
                candidate_relative = candidate.relative_to(root).as_posix()
                if fnmatch.fnmatchcase(candidate_relative, normalized):
                    result.append(candidate_relative)
            continue

        candidate = safe_relative(root, normalized, description="task scope")
        if candidate.is_symlink() or not candidate.exists():
            continue
        if candidate.is_file():
            result.append(candidate.relative_to(root).as_posix())
        else:
            result.extend(
                child.relative_to(root).as_posix()
                for child in candidate.rglob("*")
                if child.is_file() and not child.is_symlink()
            )
    return list(dict.fromkeys(sorted(result)))


def validate_index_coverage(root: Path, main_rules: list[dict[str, object]]) -> None:
    missing_tiers = sorted(
        str(rule["id"]) for rule in main_rules if "tiers" not in rule
    )
    if missing_tiers:
        fail(
            "generated rule index entries missing tiers: "
            + ", ".join(missing_tiers)
        )
    indexed = {Path(str(rule["file"])).as_posix() for rule in main_rules}
    for relative in sorted(indexed):
        path = safe_relative(root, relative, description="generated rule")
        if not path.is_file():
            fail(f"index references missing generated rule file: {relative}")
    actual = {
        path.relative_to(root).as_posix()
        for path in (root / "harness" / "rules").rglob("*.md")
    }
    missing = sorted(actual - indexed)
    stale = sorted(indexed - actual)
    if missing:
        fail("unindexed generated rule files: " + ", ".join(missing))
    if stale:
        fail("index references missing generated rule files: " + ", ".join(stale))


def load_custom_rules(root: Path) -> list[dict[str, object]]:
    custom_dir = root / "harness" / "custom-rules"
    custom_files = sorted(custom_dir.rglob("*.md")) if custom_dir.is_dir() else []
    custom_index = custom_dir / "index.yaml"
    if not custom_index.exists():
        if custom_files:
            fail("custom rule Markdown exists but harness/custom-rules/index.yaml is missing")
        return []
    if custom_index.is_symlink():
        fail("harness/custom-rules/index.yaml must not be a symlink")
    rules = parse_index(custom_index)
    for rule in rules:
        relative = Path(str(rule["file"]))
        if relative.parts[:2] != ("harness", "custom-rules"):
            fail(f"{custom_index}: custom rule {rule['id']} must stay under harness/custom-rules")
        custom_path = safe_relative(root, relative.as_posix(), description="custom rule")
        if not custom_path.is_file():
            fail(f"custom index references missing file: {relative.as_posix()}")
    indexed = {Path(str(rule["file"])).as_posix() for rule in rules}
    actual = {path.relative_to(root).as_posix() for path in custom_files}
    if indexed != actual:
        missing = sorted(actual - indexed)
        stale = sorted(indexed - actual)
        if missing:
            fail("unindexed custom rule files: " + ", ".join(missing))
        if stale:
            fail("custom index references missing files: " + ", ".join(stale))
    return rules


def route_context(
    root: Path,
    rules: list[dict[str, object]],
    custom_rules: list[dict[str, object]],
    *,
    workflow_tier: str,
    stage: str,
    mode: str,
    triggers: set[str],
    changed_paths: list[str],
    include_bootstrap: bool,
    packet: str | None,
    module: str | None,
    architecture_docs: list[str],
    auto_pipeline_start_stage: str | None = None,
    scope_paths: list[str] | None = None,
    evidence_paths: list[str] | None = None,
) -> list[dict[str, object]]:
    if workflow_tier == "high-risk" and stage == "general":
        fail("--workflow-tier high-risk requires a responsibility stage, not general")
    results: dict[str, dict[str, object]] = {}
    # Preserve policy precedence in router output: every matching project-added
    # custom rule is emitted before every generated Harness rule.
    for source, entries in (("custom-rule", custom_rules), ("generated-rule", rules)):
        for rule in entries:
            matched, reasons = entry_matches(
                rule,
                workflow_tier=workflow_tier,
                stage=stage,
                mode=mode,
                triggers=triggers,
                changed_paths=changed_paths,
                include_bootstrap=include_bootstrap,
            )
            if matched:
                add_result(
                    results,
                    root,
                    str(rule["file"]),
                    source=f"{source}:{rule['id']}",
                    reasons=reasons,
                )
    if workflow_tier == "high-risk":
        add_packet_documents(
            results,
            root,
            packet,
            stage=stage,
            mode=mode,
            auto_pipeline_start_stage=(
                auto_pipeline_start_stage
                or ("design" if mode == "auto-pipeline" else None)
            ),
        )
    for relative in expand_scope_files(root, scope_paths or []):
        add_result(
            results,
            root,
            relative,
            source="task-source",
            reasons=["task-scope"],
        )
    for relative in evidence_paths or []:
        add_result(
            results,
            root,
            relative,
            source="task-evidence",
            reasons=["task-evidence"],
        )
    if module:
        add_result(
            results,
            root,
            f"docs/modules/{module}.md",
            source="module-doc",
            reasons=[f"module:{module}"],
            require_exists=False,
        )
    for relative in architecture_docs:
        normalized = Path(relative).as_posix()
        if not normalized.startswith("docs/architecture/"):
            fail(f"architecture document must be under docs/architecture/: {relative}")
        add_result(
            results,
            root,
            normalized,
            source="architecture-doc",
            reasons=["explicit-reference"],
        )
    return list(results.values())


def changed_paths_from_args(root: Path, direct: list[str], manifests: list[str]) -> list[str]:
    values = list(direct)
    for manifest in manifests:
        path = safe_relative(root, manifest, description="changed paths manifest")
        if not path.is_file():
            fail(f"changed paths manifest does not exist: {manifest}")
        values.extend(
            line.strip()
            for line in path.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        )
    normalized: list[str] = []
    for value in values:
        candidate = Path(value)
        if candidate.is_absolute() or ".." in candidate.parts:
            fail(f"unsafe changed path: {value}")
        normalized.append(candidate.as_posix())
    return list(dict.fromkeys(normalized))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--workflow-tier", choices=sorted(WORKFLOW_TIERS))
    parser.add_argument("--stage", choices=sorted(STAGES))
    parser.add_argument("--mode", choices=sorted(MODES), default="manual")
    parser.add_argument("--auto-pipeline-start-stage", choices=PIPELINE_STAGES)
    parser.add_argument("--packet", help="repo-relative active task packet directory")
    parser.add_argument("--module", help="project module name for docs/modules/<module>.md")
    parser.add_argument("--architecture-doc", action="append", default=[])
    parser.add_argument("--scope-path", action="append", default=[])
    parser.add_argument("--evidence-path", action="append", default=[])
    parser.add_argument("--trigger", action="append", default=[])
    parser.add_argument("--changed-path", action="append", default=[])
    parser.add_argument("--changed-paths-file", action="append", default=[])
    parser.add_argument("--include-bootstrap", action="store_true")
    parser.add_argument("--format", choices=("paths", "json"), default="paths")
    parser.add_argument("--validate-index", action="store_true")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    main_index = root / "harness" / "rules" / "index.yaml"
    if main_index.is_symlink():
        fail("harness/rules/index.yaml must not be a symlink")
    rules = parse_index(main_index)
    custom_rules = load_custom_rules(root)
    validate_index_coverage(root, rules)
    if args.validate_index:
        print("context: rule indexes valid")
        return 0
    if not args.workflow_tier:
        fail("--workflow-tier is required unless --validate-index is used")
    if not args.stage:
        fail("--stage is required unless --validate-index is used")
    if args.mode == "auto-pipeline" and args.workflow_tier != "high-risk":
        fail("--mode auto-pipeline requires --workflow-tier high-risk")
    if args.mode == "auto-pipeline" and not args.auto_pipeline_start_stage:
        fail("--mode auto-pipeline requires --auto-pipeline-start-stage")
    changed_paths = changed_paths_from_args(root, args.changed_path, args.changed_paths_file)
    results = route_context(
        root,
        rules,
        custom_rules,
        workflow_tier=args.workflow_tier,
        stage=args.stage,
        mode=args.mode,
        auto_pipeline_start_stage=args.auto_pipeline_start_stage,
        triggers=normalized_triggers(args.trigger),
        changed_paths=changed_paths,
        include_bootstrap=args.include_bootstrap,
        packet=args.packet,
        module=args.module,
        architecture_docs=args.architecture_doc,
        scope_paths=args.scope_path,
        evidence_paths=args.evidence_path,
    )
    if args.format == "json":
        print(json.dumps({
            "schema_version": 1,
            "workflow_tier": args.workflow_tier,
            "stage": args.stage,
            "mode": args.mode,
            "context": results,
        }, indent=2))
    else:
        for result in results:
            print(result["path"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
