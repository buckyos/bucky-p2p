#!/usr/bin/env python3
"""Unified test entrypoint for Harness Engineering repositories.

Adapt this template to the target repository's test commands. The contract is
stable: all test implementation must be reachable through this entrypoint.

Every real run (not --list / --dry-run) writes a machine-readable run artifact
to test-results/test-runs/<timestamp>-<module>-<level>.json recording each
executed command, its exit code, and repository_state_sha256. test-results/ is
generated output and must be listed in .gitignore. Acceptance cites these
artifacts instead of pasted command output and rejects a repository-state
mismatch, so results created before later repository changes cannot be reused.
"""

from __future__ import annotations

import argparse
import ast
import datetime
import hashlib
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path


LEVELS = ("unit", "dv", "integration")
RUN_ARTIFACT_SCHEMA = 1
STATE_EXCLUDED_DIRS = {".git", ".venv", "__pycache__", ".pytest_cache", "test-results"}

# Canonical module-level regression suites. Task testplan.yaml files are
# discovered separately and are registered only as <module>/<task-name>.
# `module all` and `all all` use only each module's explicit `all` suite.
MODULE_SUITES: dict[str, dict[str, list[list[str]]]] = {
    "p2p-frame": {
        "unit": [["cargo", "test", "-p", "p2p-frame"]],
        "dv": [
            ["cargo", "test", "-p", "p2p-frame", "--features", "x509", "sn_client"],
            ["cargo", "test", "-p", "p2p-frame", "--features", "x509", "quic_tunnel_create_tunnel_no_listener", "--", "--nocapture"],
            ["cargo", "test", "-p", "p2p-frame", "sn_tcp_observed_endpoint"],
            ["cargo", "test", "-p", "p2p-frame", "ttp::", "--", "--nocapture"],
            ["cargo", "test", "-p", "p2p-frame", "pn_connection_validator", "--", "--nocapture"],
        ],
        "integration": [["cargo", "test", "--workspace"]],
        "all": [
            ["cargo", "test", "-p", "p2p-frame"],
            ["cargo", "test", "-p", "p2p-frame", "--features", "x509", "sn_client"],
            ["cargo", "test", "-p", "p2p-frame", "--features", "x509", "quic_tunnel_create_tunnel_no_listener", "--", "--nocapture"],
            ["cargo", "test", "-p", "p2p-frame", "sn_tcp_observed_endpoint"],
            ["cargo", "test", "-p", "p2p-frame", "ttp::", "--", "--nocapture"],
            ["cargo", "test", "-p", "p2p-frame", "pn_connection_validator", "--", "--nocapture"],
            ["cargo", "test", "--workspace"],
        ],
    },
    "cyfs-p2p": {
        "unit": [["cargo", "test", "-p", "cyfs-p2p"]],
        "dv": [["cargo", "check", "-p", "cyfs-p2p"]],
        "integration": [["cargo", "test", "--workspace"]],
        "all": [["cargo", "test", "-p", "cyfs-p2p"], ["cargo", "check", "-p", "cyfs-p2p"], ["cargo", "test", "--workspace"]],
    },
    "cyfs-p2p-test": {
        "unit": [["cargo", "test", "-p", "cyfs-p2p-test"]],
        "dv": [["cargo", "run", "-p", "cyfs-p2p-test", "--", "all-in-one"]],
        "integration": [["cargo", "test", "--workspace"]],
        "all": [["cargo", "test", "-p", "cyfs-p2p-test"], ["cargo", "run", "-p", "cyfs-p2p-test", "--", "all-in-one"], ["cargo", "test", "--workspace"]],
    },
    "sn-miner": {
        "unit": [["cargo", "test", "-p", "sn-miner"]],
        "dv": [["cargo", "run", "-p", "sn-miner", "--", "--help"]],
        "integration": [["cargo", "test", "-p", "sn-miner", "--test", "real_process", "--", "--test-threads=1"]],
        "all": [["cargo", "test", "-p", "sn-miner"], ["cargo", "run", "-p", "sn-miner", "--", "--help"], ["cargo", "test", "-p", "sn-miner", "--test", "real_process", "--", "--test-threads=1"]],
    },
    "desc-tool": {
        "unit": [["cargo", "test", "-p", "desc-tool"]],
        "dv": [["cargo", "run", "-p", "desc-tool", "--", "--help"]],
        "integration": [["cargo", "test", "--workspace"]],
        "all": [["cargo", "test", "-p", "desc-tool"], ["cargo", "run", "-p", "desc-tool", "--", "--help"], ["cargo", "test", "--workspace"]],
    },
}


def fail(message: str) -> None:
    print(f"test-run: {message}", file=sys.stderr)
    raise SystemExit(1)


def state_path_excluded(path: Path) -> bool:
    if any(part in STATE_EXCLUDED_DIRS for part in path.parts):
        return True
    leaf = path.name.lower()
    return (
        leaf == "acceptance-report.md"
        or leaf.endswith("-acceptance-report.md")
        or (leaf == "state.json" and path.parent.name == "pipeline")
    )


def update_state_hash(hasher: "hashlib._Hash", root: Path, relative: Path) -> None:
    if state_path_excluded(relative):
        return
    path = root / relative
    hasher.update(relative.as_posix().encode("utf-8", errors="surrogateescape") + b"\0")
    try:
        stat_result = path.lstat()
    except FileNotFoundError:
        hasher.update(b"deleted\0")
        return
    hasher.update(f"{stat_result.st_mode:o}".encode("ascii") + b"\0")
    if path.is_symlink():
        hasher.update(b"symlink\0" + os.readlink(path).encode("utf-8", errors="surrogateescape") + b"\0")
    elif path.is_file():
        hasher.update(b"file\0" + path.read_bytes() + b"\0")
    else:
        hasher.update(b"directory\0")


def repository_state_sha256(root: Path) -> str:
    """Hash the current repository content that test evidence must remain bound to."""
    root = root.resolve()
    hasher = hashlib.sha256(b"harness-repository-state-v1\0")
    try:
        head = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True
        )
        tracked = subprocess.run(
            ["git", "diff", "--name-only", "-z", "HEAD", "--"],
            cwd=root, capture_output=True, text=False
        )
        untracked = subprocess.run(
            ["git", "ls-files", "--others", "--exclude-standard", "-z"],
            cwd=root, capture_output=True, text=False
        )
    except OSError:
        head = tracked = untracked = None
    if (
        head is not None
        and tracked is not None
        and untracked is not None
        and head.returncode == tracked.returncode == untracked.returncode == 0
    ):
        hasher.update(b"git\0" + head.stdout.strip().encode("utf-8") + b"\0")
        paths = sorted(
            {
                Path(raw.decode("utf-8", errors="surrogateescape"))
                for raw in (tracked.stdout + untracked.stdout).split(b"\0")
                if raw
            },
            key=lambda path: path.as_posix(),
        )
    else:
        hasher.update(b"filesystem\0")
        paths = sorted(
            (path.relative_to(root) for path in root.rglob("*") if not path.is_dir()),
            key=lambda path: path.as_posix(),
        )
    for relative in paths:
        update_state_hash(hasher, root, relative)
    return hasher.hexdigest()


def parse_inline_list(value: str) -> list[str]:
    try:
        parsed = ast.literal_eval(value)
    except (SyntaxError, ValueError) as error:
        fail(f"invalid run command list {value!r}: {error}")
    if not isinstance(parsed, list) or not all(isinstance(item, str) for item in parsed):
        fail(f"run command must be a list of strings: {value!r}")
    return parsed


def steps_from_testplan(path: Path, level: str) -> list[tuple[list[str], str, list[str]]]:
    text = path.read_text(encoding="utf-8")
    start = re.search(rf"(?m)^  {re.escape(level)}:\s*$", text)
    if not start:
        return []
    next_level = re.search(r"(?m)^  [A-Za-z0-9_-]+:\s*$", text[start.end() :])
    end = start.end() + next_level.start() if next_level else len(text)
    body = text[start.end() : end]
    starts = list(re.finditer(r"(?m)^      - id:\s*(\S+)\s*$", body))
    steps: list[tuple[list[str], str, list[str]]] = []
    for index, match in enumerate(starts):
        block_end = starts[index + 1].start() if index + 1 < len(starts) else len(body)
        block = body[match.end() : block_end]
        change_ids = re.search(r"(?m)^        change_ids:\s*(\[[^\n]*\])\s*$", block)
        run = re.search(r"(?m)^        run:\s*(\[[^\n]*\])\s*$", block)
        if not change_ids or not run:
            fail(f"testplan step {level}/{match.group(1)} must define inline change_ids and run lists")
        steps.append(
            (
                parse_inline_list(run.group(1)),
                match.group(1),
                parse_inline_list(change_ids.group(1)),
            )
        )
    return steps


def non_executed_levels_from_testplan(path: Path, requested_level: str) -> list[dict[str, str]]:
    text = path.read_text(encoding="utf-8")
    levels = list(LEVELS) if requested_level == "all" else [requested_level]
    records: list[dict[str, str]] = []
    for level in levels:
        start = re.search(rf"(?m)^  {re.escape(level)}:\s*$", text)
        if not start:
            return []
        next_level = re.search(r"(?m)^  [A-Za-z0-9_-]+:\s*$", text[start.end() :])
        end = start.end() + next_level.start() if next_level else len(text)
        body = text[start.end() : end]
        mode_match = re.search(r"(?m)^    mode:\s*(manual|disabled)\s*$", body)
        reason_match = re.search(r"(?m)^    reason:\s*(.+)\s*$", body)
        if not mode_match or not reason_match or not reason_match.group(1).strip():
            return []
        records.append(
            {
                "level": level,
                "mode": mode_match.group(1),
                "reason": reason_match.group(1).strip(),
            }
        )
    return records


def discover_testplans(root: Path) -> dict[str, Path]:
    plans: dict[str, Path] = {}
    for path in root.glob("docs/versions/*/modules/**/testplan.yaml"):
        relative = path.relative_to(root / "docs" / "versions")
        if any(part.startswith("_") for part in relative.parts):
            continue
        text = path.read_text(encoding="utf-8")
        module = None
        task_name = None
        legacy_submodule = None
        for line in text.splitlines():
            if line.startswith("module:"):
                module = line.split(":", 1)[1].strip()
            elif line.startswith("task_name:"):
                task_name = line.split(":", 1)[1].strip()
            elif line.startswith("submodule:"):
                legacy_submodule = line.split(":", 1)[1].strip()
        if module:
            # Version-level module packets predate task-local plans. Their
            # commands are preserved above as canonical MODULE_SUITES and the
            # historical YAML remains documentation rather than a registry.
            packet_relative = path.relative_to(root / "docs" / "versions")
            if len(packet_relative.parts) == 4:
                continue
            task_name = task_name or legacy_submodule
            if not task_name:
                fail(f"task testplan must declare task_name and cannot register as a top-level module: {path}")
            key = f"{module}/{task_name}"
            if key in plans:
                fail(f"duplicate task testplan registration for {key}: {plans[key]} and {path}")
            plans[key] = path
    return plans


def validate_module_suites() -> None:
    allowed_levels = {*LEVELS, "all"}
    for module, suite in MODULE_SUITES.items():
        if not module or "/" in module:
            fail(f"invalid canonical module name: {module!r}")
        unknown = set(suite) - allowed_levels
        if unknown:
            fail(f"module {module} has unknown canonical levels: {', '.join(sorted(unknown))}")
        missing = allowed_levels - set(suite)
        if missing:
            fail(f"module {module} is missing canonical levels: {', '.join(sorted(missing))}")
        if not suite.get("all"):
            fail(f"module {module} must define a non-empty canonical all suite")
        for level, commands in suite.items():
            if not isinstance(commands, list):
                fail(f"module {module} canonical level {level} must be a list of argv lists")
            for command in commands:
                if not command or not isinstance(command, list) or not all(
                    isinstance(item, str) and item for item in command
                ):
                    fail(f"module {module} canonical level {level} has invalid argv: {command!r}")


def command_sources_for(
    root: Path,
    scope: str,
    requested_level: str,
    task_plans: dict[str, Path],
) -> list[tuple[list[str], dict[str, object]]]:
    entries: list[tuple[list[str], dict[str, object]]] = []
    if scope in MODULE_SUITES:
        suite = MODULE_SUITES[scope]
        if requested_level == "all":
            if "all" not in suite or not suite["all"]:
                fail(f"module {scope} must define a non-empty canonical all suite")
            levels = ["all"]
        else:
            levels = [requested_level]
        for level in levels:
            for command_index, command in enumerate(suite.get(level, [])):
                entries.append(
                    (
                        command,
                        {
                            "scope": scope,
                            "kind": "module-suite",
                            "level": level,
                            "registration_index": str(command_index),
                        },
                    )
                )
        return entries

    plan = task_plans.get(scope)
    if plan is None:
        fail(f"unknown module or task scope: {scope}")
    levels = list(LEVELS) if requested_level == "all" else [requested_level]
    try:
        plan_rel = plan.resolve().relative_to(root.resolve()).as_posix()
    except ValueError:
        plan_rel = plan.as_posix()
    for level in levels:
        for command_index, (command, step_id, change_ids) in enumerate(steps_from_testplan(plan, level)):
            entries.append(
                (
                    command,
                    {
                        "scope": scope,
                        "kind": "task-testplan",
                        "level": level,
                        "testplan": plan_rel,
                        "step_id": step_id,
                        "change_ids": change_ids,
                        "registration_index": str(command_index),
                    },
                )
            )
    return entries


def selected_scopes(requested_module: str, task_plans: dict[str, Path]) -> list[str]:
    if requested_module == "all":
        modules = sorted(MODULE_SUITES)
        if not modules:
            fail("no canonical module suites registered in MODULE_SUITES")
        return modules
    if requested_module in MODULE_SUITES or requested_module in task_plans:
        return [requested_module]
    fail(f"unknown module or task scope: {requested_module}")


def deduplicated_execution_plan(
    root: Path,
    scopes: list[str],
    requested_level: str,
    task_plans: dict[str, Path],
) -> list[dict[str, object]]:
    by_argv: dict[tuple[str, ...], dict[str, object]] = {}
    for scope in scopes:
        for command, source in command_sources_for(root, scope, requested_level, task_plans):
            key = tuple(command)
            entry = by_argv.setdefault(key, {"command": command, "sources": []})
            sources = entry["sources"]
            if isinstance(sources, list) and source not in sources:
                sources.append(source)
    return list(by_argv.values())


def run_command(command: list[str], root: Path, dry_run: bool) -> int:
    print("+ " + " ".join(command))
    if dry_run:
        return 0
    completed = subprocess.run(command, cwd=root)
    return completed.returncode


def git_state(root: Path) -> tuple[str | None, bool | None]:
    try:
        head = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True
        )
        status = subprocess.run(
            ["git", "status", "--porcelain"], cwd=root, capture_output=True, text=True
        )
    except OSError:
        return None, None
    if head.returncode != 0 or status.returncode != 0:
        return None, None
    return head.stdout.strip(), bool(status.stdout.strip())


def write_run_artifact(
    root: Path,
    requested_module: str,
    requested_level: str,
    started_at: str,
    steps: list[dict[str, object]],
    exit_code: int,
    non_executed_levels: list[dict[str, str]] | None = None,
) -> None:
    artifact_dir = root / "test-results" / "test-runs"
    try:
        artifact_dir.mkdir(parents=True, exist_ok=True)
        timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        slug_module = requested_module.replace("/", "+")
        artifact_path = artifact_dir / f"{timestamp}-{slug_module}-{requested_level}.json"
        head, dirty = git_state(root)
        artifact = {
            "schema": RUN_ARTIFACT_SCHEMA,
            "requested_module": requested_module,
            "requested_level": requested_level,
            "started_at": started_at,
            "finished_at": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
            "git_head": head,
            "worktree_dirty": dirty,
            "repository_state_sha256": repository_state_sha256(root),
            "testplans": sorted(
                {
                    source["testplan"]
                    for step in steps
                    for source in step.get("sources", [])
                    if isinstance(source, dict) and isinstance(source.get("testplan"), str)
                }
            ),
            "change_ids": sorted(
                {
                    change_id
                    for step in steps
                    for source in step.get("sources", [])
                    if isinstance(source, dict)
                    for change_id in source.get("change_ids", [])
                    if isinstance(change_id, str)
                }
            ),
            "steps": steps,
            "non_executed_levels": non_executed_levels or [],
            "exit_code": exit_code,
        }
        artifact_path.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
        print(f"test-run: run artifact written: {artifact_path}")
    except OSError as error:
        print(f"test-run: warning: failed to write run artifact: {error}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("module", help="module name, module/task-name, or all")
    parser.add_argument("level", choices=[*LEVELS, "all"], help="test level or all")
    parser.add_argument("--root", default=".")
    parser.add_argument("--list", action="store_true", help="list known modules and exit")
    parser.add_argument("--dry-run", action="store_true", help="print commands without running them")
    args = parser.parse_args()

    root = Path(args.root)
    validate_module_suites()
    task_plans = discover_testplans(root)
    modules = sorted({*MODULE_SUITES, *task_plans})
    if args.list:
        for module in modules:
            print(module)
        return 0

    scopes = selected_scopes(args.module, task_plans)
    plan = deduplicated_execution_plan(root, scopes, args.level, task_plans)
    if not plan:
        non_executed_levels = (
            non_executed_levels_from_testplan(task_plans[args.module], args.level)
            if len(scopes) == 1 and args.module in task_plans
            else []
        )
        if not non_executed_levels:
            fail("no test commands matched the requested module/task scope and level")
        started_at = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
        print(
            "test-run: passed without automated commands; selected levels are manual/disabled: "
            + ", ".join(
                f"{item['level']}={item['mode']} ({item['reason']})"
                for item in non_executed_levels
            )
        )
        if not args.dry_run:
            write_run_artifact(
                root,
                args.module,
                args.level,
                started_at,
                [],
                0,
                non_executed_levels,
            )
        return 0
    source_count = sum(len(entry["sources"]) for entry in plan if isinstance(entry.get("sources"), list))
    if source_count > len(plan):
        print(
            f"test-run: exact argv deduplication merged {source_count} registrations "
            f"into {len(plan)} commands"
        )

    started_at = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
    steps: list[dict[str, object]] = []
    exit_code = 0
    for entry in plan:
        command = entry["command"]
        sources = entry["sources"]
        if not isinstance(command, list) or not all(isinstance(item, str) for item in command):
            fail(f"invalid registered command: {command!r}")
        if not isinstance(sources, list) or not sources:
            fail(f"registered command has no sources: {command!r}")
        first_source = sources[0]
        step_start = time.monotonic()
        code = run_command(command, root, args.dry_run)
        steps.append(
            {
                "module": first_source.get("scope") if isinstance(first_source, dict) else None,
                "level": first_source.get("level") if isinstance(first_source, dict) else None,
                "command": command,
                "sources": sources,
                "exit_code": code,
                "duration_s": round(time.monotonic() - step_start, 3),
            }
        )
        if code != 0:
            exit_code = code
            break

    if not args.dry_run:
        write_run_artifact(root, args.module, args.level, started_at, steps, exit_code)
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
