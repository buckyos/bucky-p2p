#!/usr/bin/env python3
"""Unified test entrypoint for Harness Engineering repositories.

Adapt this template to the target repository's test commands. The contract is
stable: all test implementation must be reachable through this entrypoint.

Every real run (not --list / --dry-run) writes a machine-readable run artifact
to test-results/test-runs/<timestamp>-<module>-<level>.json recording each
executed command, its exit code, and the git state. test-results/ is generated
output and must be listed in .gitignore. Acceptance cites these artifacts
instead of pasted command output, and acceptance-report-check.py re-verifies
them, so test evidence cannot be claimed without an actual run.
"""

from __future__ import annotations

import argparse
import ast
import datetime
import json
import subprocess
import sys
import time
from pathlib import Path


LEVELS = ("unit", "dv", "integration")
RUN_ARTIFACT_SCHEMA = 1
EXCLUDED_TESTPLAN_MODULES = {"cyfs-p2p-test"}

# Optional direct registry for repository-local exceptions that do not generate testplan.yaml.
# Keys are module names. Each level contains a list of command argv lists.
TEST_COMMANDS: dict[str, dict[str, list[list[str]]]] = {
    "p2p-frame": {
        "unit": [["cargo", "test", "-p", "p2p-frame"]],
        "integration": [["cargo", "test", "--workspace"]],
    },
    "cyfs-p2p": {
        "unit": [["cargo", "test", "-p", "cyfs-p2p"]],
        "integration": [["cargo", "test", "--workspace"]],
    },
    "sn-miner": {
        "unit": [["cargo", "test", "-p", "sn-miner"]],
        "dv": [["cargo", "run", "-p", "sn-miner", "--", "--help"]],
        "integration": [["cargo", "test", "--workspace"]],
    },
    "desc-tool": {
        "unit": [["cargo", "test", "-p", "desc-tool"]],
        "dv": [["cargo", "run", "-p", "desc-tool", "--", "--help"]],
        "integration": [["cargo", "test", "--workspace"]],
    },
}


def fail(message: str) -> None:
    print(f"test-run: {message}", file=sys.stderr)
    raise SystemExit(1)


def parse_inline_list(value: str) -> list[str]:
    try:
        parsed = ast.literal_eval(value)
    except (SyntaxError, ValueError) as error:
        fail(f"invalid run command list {value!r}: {error}")
    if not isinstance(parsed, list) or not all(isinstance(item, str) for item in parsed):
        fail(f"run command must be a list of strings: {value!r}")
    return parsed


def commands_from_testplan(path: Path, level: str) -> list[list[str]]:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    in_levels = False
    in_level = False
    current_indent = ""
    commands: list[list[str]] = []

    for line in lines:
        stripped = line.strip()
        if stripped == "levels:":
            in_levels = True
            continue
        if not in_levels:
            continue

        if line.startswith("  ") and stripped == f"{level}:":
            in_level = True
            current_indent = line[: len(line) - len(line.lstrip())]
            continue
        if in_level and stripped.endswith(":") and line.startswith("  ") and not line.startswith(f"{current_indent}  "):
            break
        if in_level and "run:" in line:
            _, value = line.split("run:", 1)
            commands.append(parse_inline_list(value.strip()))

    return commands


def discover_testplans(root: Path) -> dict[str, Path]:
    plans: dict[str, Path] = {}
    for path in root.glob("docs/versions/*/modules/**/testplan.yaml"):
        if "_template" in path.parts:
            continue
        text = path.read_text(encoding="utf-8")
        module = None
        submodule = None
        for line in text.splitlines():
            if line.startswith("module:"):
                module = line.split(":", 1)[1].strip()
            elif line.startswith("submodule:"):
                submodule = line.split(":", 1)[1].strip()
        if module in EXCLUDED_TESTPLAN_MODULES:
            continue
        if module:
            key = f"{module}/{submodule}" if submodule else module
            plans[key] = path
    return plans


def known_modules(root: Path) -> list[str]:
    modules = set(TEST_COMMANDS)
    modules.update(discover_testplans(root))
    return sorted(modules)


def commands_for(root: Path, module: str, level: str) -> list[list[str]]:
    plan = discover_testplans(root).get(module)
    if plan:
        commands = commands_from_testplan(plan, level)
        if commands:
            return commands
    return list(TEST_COMMANDS.get(module, {}).get(level, []))


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
            "steps": steps,
            "exit_code": exit_code,
        }
        artifact_path.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
        print(f"test-run: run artifact written: {artifact_path}")
    except OSError as error:
        print(f"test-run: warning: failed to write run artifact: {error}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("module", help="module name, module/submodule, or all")
    parser.add_argument("level", choices=[*LEVELS, "all"], help="test level or all")
    parser.add_argument("--root", default=".")
    parser.add_argument("--list", action="store_true", help="list known modules and exit")
    parser.add_argument("--dry-run", action="store_true", help="print commands without running them")
    parser.add_argument(
        "--no-dedupe",
        action="store_true",
        help="execute repeated identical commands instead of reusing results within this run",
    )
    args = parser.parse_args()

    root = Path(args.root)
    modules = known_modules(root)
    if args.list:
        for module in modules:
            print(module)
        return 0

    selected_modules = modules if args.module == "all" else [args.module]
    selected_levels = list(LEVELS) if args.level == "all" else [args.level]
    if not selected_modules:
        fail("no test modules registered; add TEST_COMMANDS entries or generate testplan.yaml files")

    started_at = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
    steps: list[dict[str, object]] = []
    command_results: dict[tuple[str, tuple[str, ...]], tuple[int, int]] = {}
    exit_code = 0
    ran = 0
    for module in selected_modules:
        if module not in modules:
            fail(f"unknown module: {module}")
        for level in selected_levels:
            commands = commands_for(root, module, level)
            if not commands:
                print(f"test-run: no {level} tests for {module}")
                continue
            for command in commands:
                command_key = (root.resolve().as_posix(), tuple(command))
                if not args.no_dedupe and command_key in command_results:
                    code, reused_from_step = command_results[command_key]
                    print(
                        "+ "
                        + " ".join(command)
                        + f" # reused from step {reused_from_step}"
                    )
                    steps.append(
                        {
                            "module": module,
                            "level": level,
                            "command": command,
                            "exit_code": code,
                            "duration_s": 0.0,
                            "deduped": True,
                            "reused_from_step": reused_from_step,
                        }
                    )
                else:
                    step_start = time.monotonic()
                    code = run_command(command, root, args.dry_run)
                    step_index = len(steps)
                    steps.append(
                        {
                            "module": module,
                            "level": level,
                            "command": command,
                            "exit_code": code,
                            "duration_s": round(time.monotonic() - step_start, 3),
                            "deduped": False,
                        }
                    )
                    command_results[command_key] = (code, step_index)
                ran += 1
                if code != 0:
                    exit_code = code
                    break
            if exit_code != 0:
                break
        if exit_code != 0:
            break

    if exit_code == 0 and ran == 0:
        fail("no test commands matched the requested module/level")
    if not args.dry_run:
        write_run_artifact(root, args.module, args.level, started_at, steps, exit_code)
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
