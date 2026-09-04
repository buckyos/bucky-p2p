#!/usr/bin/env python3
"""Validate that a generated Harness Engineering scaffold is complete.

This checker is intentionally dependency-free. It verifies the default files
that the bootstrap kit expects every generated repository to contain.
"""

from __future__ import annotations

import argparse
import ast
import re
import subprocess
import sys
import tempfile
from pathlib import Path


REQUIRED_RULES = (
    "task-entry-gate-rules.md",
    "proposal-doc-rules.md",
    "design-doc-rules.md",
    "testing-doc-rules.md",
    "test-design-rules.md",
    "implementation-rules.md",
    "schema-validation-rules.md",
    "unified-test-entry-rules.md",
    "acceptance-task-rules.md",
    "acceptance-review-rules.md",
    "quality-gate-rules.md",
    "auto-pipeline-rules.md",
    "triggers/contract-protocol.md",
    "triggers/data-schema.md",
    "triggers/security.md",
    "triggers/runtime-integration.md",
    "triggers/build-config-deployment.md",
    "triggers/ui-workflow.md",
    "triggers/harness-process.md",
)

REQUIRED_SCRIPTS = (
    "test-run.py",
    "context.py",
    "harness-check.py",
    "lifecycle-check.py",
    "task-transition.py",
    "task_manifest.py",
    "schema-check.py",
    "task-seq.py",
    "task-index.py",
    "risk-profile-check.py",
    "baseline-snapshot.py",
    "stage-scope-check.py",
    "harness-self-check.py",
    "doc-structure-check.py",
    "architecture-doc-check.py",
    "testing-coverage-check.py",
    "consumer-closure-check.py",
    "acceptance-report-check.py",
    "completion-report-check.py",
    "lower-tier-check.py",
    "pipeline-plan-check.py",
    "check-all.py",
    "quality-check.py",
)

REQUIRED_TASK_TEMPLATES = (
    "pipeline-stage-task.md",
    "pipeline-submodule-task.md",
    "acceptance-return-task.md",
)


def fail(message: str) -> None:
    print(f"harness-self-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def warn(message: str) -> None:
    print(f"harness-self-check: warning: {message}", file=sys.stderr)


def require_path(path: Path, *, directory: bool | None = None) -> None:
    if not path.exists():
        fail(f"missing required path: {path}")
    if directory is True and not path.is_dir():
        fail(f"expected directory: {path}")
    if directory is False and not path.is_file():
        fail(f"expected file: {path}")


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError as error:
        fail(f"{path} is not valid utf-8: {error}")


def require_contains(path: Path, patterns: tuple[str, ...]) -> None:
    text = read_text(path)
    missing = [pattern for pattern in patterns if pattern not in text]
    if missing:
        fail(f"{path} missing required references: {', '.join(missing)}")


def require_any(path: Path, patterns: tuple[str, ...], description: str) -> None:
    text = read_text(path)
    if not any(pattern in text for pattern in patterns):
        fail(f"{path} missing required reference: {description}")


def configured_module_suites(path: Path) -> dict[object, object]:
    """Read the literal MODULE_SUITES registration without executing the runner."""
    try:
        tree = ast.parse(read_text(path), filename=str(path))
    except SyntaxError as error:
        fail(f"{path} is not valid Python: {error}")
    for node in tree.body:
        target = node.target if isinstance(node, ast.AnnAssign) else None
        if isinstance(target, ast.Name) and target.id == "MODULE_SUITES":
            try:
                suites = ast.literal_eval(node.value)
            except (ValueError, TypeError) as error:
                fail(f"{path} MODULE_SUITES must be a literal mapping: {error}")
            if not isinstance(suites, dict):
                fail(f"{path} MODULE_SUITES must be a mapping")
            return suites
    fail(f"{path} does not define MODULE_SUITES")


def require_configured_module_suites(path: Path) -> None:
    suites = configured_module_suites(path)
    if not suites:
        fail(
            f"{path} has no canonical MODULE_SUITES; adapt the template to "
            "register each project module's explicit all suite"
        )
    for module, suite in suites.items():
        if not isinstance(module, str) or not module:
            fail(f"{path} has an invalid canonical module name: {module!r}")
        if not isinstance(suite, dict):
            fail(f"{path} module {module} suite must be a mapping")
        all_commands = suite.get("all")
        if not isinstance(all_commands, list) or not all_commands:
            fail(f"{path} module {module} must define a non-empty canonical all suite")


def check_root(root: Path) -> None:
    require_path(root / "AGENTS.md", directory=False)
    require_path(root / "test-run.bat", directory=False)
    require_path(root / "test-run.sh", directory=False)
    require_path(root / "docs", directory=True)
    require_path(root / "docs" / "architecture", directory=True)
    require_path(root / "docs" / "changes", directory=True)
    require_path(root / "docs" / "changes" / "_template.md", directory=False)
    require_path(root / "docs" / "modules", directory=True)
    require_path(root / "docs" / "versions", directory=True)
    require_path(root / "harness", directory=True)
    require_path(root / "harness" / "rules", directory=True)
    require_path(root / "harness" / "custom-rules", directory=True)
    require_path(root / "harness" / "scripts", directory=True)
    require_path(root / "harness" / "process_rules", directory=True)
    require_path(root / "harness" / "templates" / "evidence", directory=True)
    require_path(
        root / "harness" / "templates" / "evidence" / "stage-scope-manifest-meta.json",
        directory=False,
    )
    require_path(root / "harness" / "templates" / "pipeline", directory=True)
    require_path(
        root / "harness" / "templates" / "pipeline" / "state.json",
        directory=False,
    )
    require_path(root / "harness" / "quality-gates.yaml", directory=False)
    require_path(root / ".harness", directory=True)
    check_version_scaffold(root)
    check_generated_output_ignored(root)
    check_no_legacy_runtime_locations(root)


def check_version_scaffold(root: Path) -> None:
    versions_dir = root / "docs" / "versions"
    version_dirs = [path for path in versions_dir.iterdir() if path.is_dir()]
    if not version_dirs:
        fail("docs/versions must contain at least one version directory")
    for version_dir in version_dirs:
        tasks_index = root / ".harness" / "tasks" / version_dir.name / "tasks.json"
        require_path(tasks_index, directory=False)
        task_index = root / "harness" / "scripts" / "task-index.py"
        completed = subprocess.run(
            [sys.executable, str(task_index), "--root", str(root), "validate", "--version", version_dir.name],
            text=True,
            capture_output=True,
        )
        if completed.returncode != 0:
            detail = completed.stderr.strip() or completed.stdout.strip() or "unknown error"
            fail(f"unfinished-task index validation failed: {detail}")
        task_template = version_dir / "modules" / "_template" / "task.yaml"
        profile_template = version_dir / "modules" / "_template" / "risk-profile.yaml"
        require_path(task_template, directory=False)
        require_path(profile_template, directory=False)
        require_contains(
            task_template,
            ("workflow_tier: pending", "risk_profile: risk-profile.yaml"),
        )
        if re.search(r"(?m)^\s+triggers:\s*", read_text(task_template)):
            fail(f"{task_template} duplicates task risk triggers; use only risk-profile.yaml")
        profile = read_text(profile_template)
        for category in ("contract", "data", "security", "runtime", "build", "ui", "harness"):
            if len(re.findall(rf"(?m)^  {category}:\s*$", profile)) != 1:
                fail(f"{profile_template} must contain exactly one {category} risk category")


def check_generated_output_ignored(root: Path) -> None:
    """All generated Harness runtime state must stay untracked."""
    gitignore = root / ".gitignore"
    if not gitignore.is_file():
        fail("missing .gitignore: .harness/ must be git-ignored")
    entries = {
        line.strip().lstrip("/").rstrip("/")
        for line in read_text(gitignore).splitlines()
    }
    missing = [entry for entry in (".harness",) if entry not in entries]
    if missing:
        fail(
            ".gitignore must ignore generated Harness output: "
            + ", ".join(f"{entry}/" for entry in missing)
        )


def check_no_legacy_runtime_locations(root: Path) -> None:
    """Report grandfathered runtime state outside the current `.harness/` tree."""
    legacy: list[Path] = []
    if (root / "test-results").exists():
        legacy.append(root / "test-results")
    versions = root / "docs" / "versions"
    if versions.is_dir():
        for version_dir in versions.iterdir():
            if not version_dir.is_dir():
                continue
            if (version_dir / "evidence").exists():
                legacy.append(version_dir / "evidence")
            modules = version_dir / "modules"
            if modules.is_dir():
                if (modules / "tasks.json").exists():
                    legacy.append(modules / "tasks.json")
                legacy.extend(modules.glob("**/pipeline/state.json"))
    if legacy:
        warn(
            "legacy Harness runtime state remains outside .harness/; "
            "new runtime state must not use these locations: "
            + ", ".join(str(path) for path in sorted(legacy)[:10])
            + (" ..." if len(legacy) > 10 else "")
        )


def check_rules(root: Path) -> None:
    rules_dir = root / "harness" / "rules"
    require_path(rules_dir / "index.yaml", directory=False)
    for name in REQUIRED_RULES:
        require_path(rules_dir / name, directory=False)

    custom_dir = root / "harness" / "custom-rules"
    reserved_names = {Path(name).name for name in REQUIRED_RULES}
    for path in custom_dir.rglob("*.md"):
        if path.name in reserved_names:
            fail(f"skill-managed rule appears under harness/custom-rules: {path}")


def check_rule_index(root: Path) -> None:
    context = root / "harness" / "scripts" / "context.py"
    completed = subprocess.run(
        [sys.executable, str(context), "--root", str(root), "--validate-index"],
        text=True,
        capture_output=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "unknown error"
        fail(f"rule index validation failed: {detail}")


def check_rule_script_contracts(root: Path) -> None:
    """Ensure routed rules reference real scripts with the required CLI contract."""
    referenced: set[str] = set()
    for path in sorted((root / "harness" / "rules").rglob("*.md")):
        referenced.update(
            re.findall(r"(?<![A-Za-z0-9_-])([A-Za-z0-9][A-Za-z0-9_-]*\.py)", read_text(path))
        )
    missing = [
        name for name in sorted(referenced)
        if not (root / "harness" / "scripts" / name).is_file()
    ]
    if missing:
        fail("generated rules reference missing scripts: " + ", ".join(missing))

    schema = root / "harness" / "scripts" / "schema-check.py"
    help_run = subprocess.run(
        [sys.executable, str(schema), "--help"],
        text=True,
        capture_output=True,
    )
    if help_run.returncode != 0 or "--require-approved" not in help_run.stdout:
        fail("schema-check.py must expose --require-approved")

    with tempfile.TemporaryDirectory(prefix="harness-schema-contract-") as tmp:
        probe = Path(tmp)
        task_rel = "docs/versions/v-probe/modules/probe/001-schema-contract/task.yaml"
        packet = probe / Path(task_rel).parent
        packet.mkdir(parents=True)
        task_text = """schema_version: 1
workflow_tier: high-risk
version: v-probe
packet_module: probe
task_name: 001-schema-contract
stage: proposal
mode: manual
auto_pipeline_start_stage:
proposal: proposal.md
completion_report: completion-report.md
change_record:
design: design.md
testing: testing.md
testplan: testplan.yaml
acceptance_report: acceptance-report.md
pipeline_plan: pipeline/plan.md
risk_profile: risk-profile.yaml
lifecycle_state: lifecycle.json
changed_paths_file: .harness/evidence/v-probe/stage-scope/001-schema-contract.paths
baseline_manifest:
changes:
  - id: CHG-schema-contract
    target_module: probe
    scope_paths: ["src/**"]
    changed_paths_file:
"""
        (packet / "task.yaml").write_text(task_text, encoding="utf-8")
        (packet / "proposal.md").write_text(
            "---\ntask_manifest: task.yaml\nstatus: approved\n---\n",
            encoding="utf-8",
        )
        index = probe / ".harness" / "tasks" / "v-probe" / "tasks.json"
        index.parent.mkdir(parents=True)
        index.write_text(
            '{"schema_version":1,"version":"v-probe","tasks":['
            '{"task_id":"001-schema-contract","task_manifest":"'
            + task_rel
            + '"}]}\n',
            encoding="utf-8",
        )
        command = [
            sys.executable,
            str(schema),
            "--root",
            str(probe),
            "--version",
            "v-probe",
            "--module",
            "probe",
            "--submodule",
            "001-schema-contract",
            "--require-approved",
        ]
        accepted = subprocess.run(command, text=True, capture_output=True)
        if accepted.returncode != 0:
            detail = accepted.stderr.strip() or accepted.stdout.strip()
            fail(
                "schema-check.py must accept approved manifest-bound proposal "
                f"without repeated module/version: {detail}"
            )
        (packet / "proposal.md").write_text(
            "---\ntask_manifest: task.yaml\nstatus: approved\n"
            "module: probe\nversion: v-probe\n---\n",
            encoding="utf-8",
        )
        rejected = subprocess.run(command, text=True, capture_output=True)
        if rejected.returncode == 0 or "duplicates canonical task identity fields" not in (
            rejected.stderr + rejected.stdout
        ):
            fail("schema-check.py must reject proposal module/version duplication")


def check_generated_script_runtime_paths(root: Path) -> None:
    """Reject active generated scripts that still write legacy runtime locations."""
    violations: list[str] = []
    scripts = root / "harness" / "scripts"
    for path in sorted(scripts.glob("*.py")):
        if path.name in {"harness-self-check.py"}:
            continue
        text = read_text(path)
        checks = {
            "docs/versions/<version>/evidence/": "legacy docs evidence path",
            "docs/versions/{version}/evidence/": "legacy docs evidence path",
            'root / "test-results"': "root test-results path",
            'with_name("state.json")': "task-local pipeline state path",
        }
        for needle, description in checks.items():
            if needle in text:
                violations.append(f"{path}: {description}")
    if violations:
        fail("generated scripts contain legacy runtime paths: " + "; ".join(violations))


def check_scripts(root: Path) -> None:
    scripts_dir = root / "harness" / "scripts"
    for name in REQUIRED_SCRIPTS:
        path = scripts_dir / name
        require_path(path, directory=False)
        text = read_text(path)
        if "raise SystemExit(main())" not in text:
            warn(f"{path} does not expose the standard main() exit pattern")

    require_contains(
        root / "test-run.bat",
        ("uv", ".venv", "uv run", "--active", "python", "UV_CACHE_DIR", ".harness\\uv-cache"),
    )
    require_any(
        root / "test-run.bat",
        ("harness/scripts/test-run.py", "harness\\scripts\\test-run.py"),
        "harness/scripts/test-run.py",
    )
    require_contains(
        root / "test-run.sh",
        (
            "uv",
            ".venv",
            "uv run",
            "--active",
            "python",
            "harness/scripts/test-run.py",
            "UV_CACHE_DIR",
            ".harness/uv-cache",
            "mkdir -p",
        ),
    )
    require_configured_module_suites(scripts_dir / "test-run.py")
    require_contains(
        scripts_dir / "test-run.py",
        ("contract_steps_from_testplan", "contract_kind", "evidence_inputs"),
    )
    require_contains(
        scripts_dir / "baseline-snapshot.py",
        (
            "working-tree-content-baseline",
            "modified_tracked_paths",
            "untracked_paths",
            "EXCLUDED_ROOTS",
            "capture_hybrid",
            "diff_hybrid",
        ),
    )
    require_contains(
        scripts_dir / "harness-check.py",
        ("baseline_pre_edit_command", "baseline_completion_command"),
    )


def check_process_templates(root: Path) -> None:
    require_path(root / "harness" / "process_rules" / "task-template.md", directory=False)
    template_dir = root / "harness" / "process_rules" / "task_templates"
    require_path(template_dir, directory=True)
    for name in REQUIRED_TASK_TEMPLATES:
        require_path(template_dir / name, directory=False)


def check_agents_references(root: Path) -> None:
    agents = root / "AGENTS.md"
    require_contains(
        agents,
        (
            "harness/rules/task-entry-gate-rules.md",
            "harness/rules/index.yaml",
            "harness/scripts/context.py",
            "Harness rules never restrict reading or writing project files",
            "router output as recommended starting context",
            "## Harness Step Agent Mapping",
            "## Rule Ownership",
            "harness/scripts/harness-check.py",
            "## Workflow Tiers",
            "docs/changes/<change>.md",
            "harness/scripts/schema-check.py",
            "harness/scripts/stage-scope-check.py",
        ),
    )
    agents_text = read_text(agents)
    forbidden = (
        "docs/versions/<version>/evidence/",
        "docs/versions/v0.1/evidence/",
        "pipeline/state.json",
    )
    present = [value for value in forbidden if value in agents_text]
    if present:
        fail("AGENTS.md references legacy runtime paths: " + ", ".join(present))


def check_markdown_path_references(root: Path) -> None:
    """Catch obvious stale generated path references in default harness files."""

    checked_roots = [root / "AGENTS.md", root / "harness" / "rules", root / "harness" / "process_rules"]
    pattern = re.compile(r"`((?:harness|docs|test-run)[^`]+?)`")
    missing: list[tuple[Path, str]] = []
    for base in checked_roots:
        paths = [base] if base.is_file() else sorted(base.rglob("*.md"))
        for path in paths:
            text = read_text(path)
            for match in pattern.finditer(text):
                raw = match.group(1).strip()
                if any(token in raw for token in ("<", ">", "|", "*", " ", "\n")):
                    continue
                candidate = root / raw.replace("\\", "/")
                if not candidate.exists() and raw.startswith(("harness/", "docs/")):
                    missing.append((path, raw))
    if missing:
        for path, raw in missing[:20]:
            print(f"  - {path}: {raw}", file=sys.stderr)
        fail("generated docs reference missing concrete paths")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument(
        "--skip-reference-check",
        action="store_true",
        help="skip best-effort markdown path reference validation",
    )
    args = parser.parse_args()

    root = Path(args.root).resolve()
    check_root(root)
    check_rules(root)
    check_scripts(root)
    check_rule_index(root)
    check_rule_script_contracts(root)
    check_generated_script_runtime_paths(root)
    check_process_templates(root)
    check_agents_references(root)
    if not args.skip_reference_check:
        check_markdown_path_references(root)

    print("harness-self-check: passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
