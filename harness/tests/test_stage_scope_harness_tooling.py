#!/usr/bin/env python3
"""Mutation-sensitive tests for globals Harness-tooling scope bindings."""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
CHECKER = REPO_ROOT / "harness" / "scripts" / "stage-scope-check.py"
VERSION = "v0.1"
VALID_TASK = "031-fixture-harness-task"
VALID_TARGET = "p2p-frame"
VALID_CHANGE = "fixture_harness_tooling_change"


class StageScopeHarnessToolingBindingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)
        self.root = Path(self.temp_dir.name)
        placeholder = self.root / "harness" / "scripts" / "placeholder.py"
        placeholder.parent.mkdir(parents=True)
        placeholder.write_text("# fixture\n", encoding="utf-8")

    def _write_target_marker(self, target_module: str = VALID_TARGET) -> None:
        marker = self.root / "docs" / "modules" / f"{target_module}.md"
        marker.parent.mkdir(parents=True, exist_ok=True)
        marker.write_text(f"# {target_module}\n", encoding="utf-8")

    def _write_manifest(
        self,
        *,
        directory_module: str = "globals",
        directory_task: str = VALID_TASK,
        version: str = VERSION,
        packet_module: str = "globals",
        task_name: str = VALID_TASK,
        workflow_tier: str = "high-risk",
        changes: tuple[tuple[str, str], ...] = ((VALID_CHANGE, VALID_TARGET),),
    ) -> None:
        manifest = (
            self.root
            / "docs"
            / "versions"
            / VERSION
            / "modules"
            / directory_module
            / directory_task
            / "task.yaml"
        )
        manifest.parent.mkdir(parents=True, exist_ok=True)
        change_lines = "\n".join(
            f"  - id: {change_id}\n    target_module: {target_module}\n"
            "    scope_paths: [\"harness/scripts/placeholder.py\"]"
            for change_id, target_module in changes
        )
        manifest.write_text(
            "schema_version: 1\n"
            f"workflow_tier: {workflow_tier}\n"
            f"version: {version}\n"
            f"packet_module: {packet_module}\n"
            f"task_name: {task_name}\n"
            "changes:\n"
            f"{change_lines}\n",
            encoding="utf-8",
        )

    def _run_checker(
        self,
        *,
        module: str = "globals",
        submodule: str = VALID_TASK,
        target_module: str = VALID_TARGET,
        change_ids: tuple[str, ...] = (VALID_CHANGE,),
    ) -> subprocess.CompletedProcess[str]:
        command = [
            sys.executable,
            str(CHECKER),
            "--root",
            str(self.root),
            "--stage",
            "implementation",
            "--version",
            VERSION,
            "--module",
            module,
            "--submodule",
            submodule,
            "--target-module",
            target_module,
        ]
        for change_id in change_ids:
            command.extend(("--change-id", change_id))
        command.extend(("--changed-path", "harness/scripts/placeholder.py"))
        return subprocess.run(command, capture_output=True, text=True, check=False)

    def _assert_rejected(self, result: subprocess.CompletedProcess[str]) -> None:
        if result.returncode == 0:
            self.fail(
                "stage-scope checker unexpectedly accepted the negative fixture\n"
                f"stdout:\n{result.stdout}\n"
                f"stderr:\n{result.stderr}"
            )

    def test_accepts_fully_bound_globals_harness_tooling_fixture(self) -> None:
        self._write_target_marker()
        self._write_manifest()
        result = self._run_checker()
        self.assertEqual(result.returncode, 0, msg=result.stderr or result.stdout)

    def test_rejects_non_globals_packet_with_otherwise_valid_binding(self) -> None:
        self._write_target_marker()
        self._write_manifest()
        self._assert_rejected(self._run_checker(module=VALID_TARGET))

    def test_rejects_invalid_sibling_name_even_when_manifest_exists(self) -> None:
        invalid_task = "invalid-sibling-name"
        self._write_target_marker()
        self._write_manifest(directory_task=invalid_task, task_name=invalid_task)
        self._assert_rejected(self._run_checker(submodule=invalid_task))

    def test_rejects_missing_target_module_marker(self) -> None:
        missing_target = "missing-target-module"
        self._write_manifest(changes=((VALID_CHANGE, missing_target),))
        self._assert_rejected(self._run_checker(target_module=missing_target))

    def test_rejects_manifest_version_mismatch(self) -> None:
        self._write_target_marker()
        self._write_manifest(version="v9.9")
        self._assert_rejected(self._run_checker())

    def test_rejects_manifest_packet_module_mismatch(self) -> None:
        self._write_target_marker()
        self._write_manifest(packet_module=VALID_TARGET)
        self._assert_rejected(self._run_checker())

    def test_rejects_manifest_task_name_mismatch(self) -> None:
        self._write_target_marker()
        self._write_manifest(task_name="032-different-harness-task")
        self._assert_rejected(self._run_checker())

    def test_rejects_non_high_risk_manifest(self) -> None:
        self._write_target_marker()
        self._write_manifest(workflow_tier="standard")
        self._assert_rejected(self._run_checker())

    def test_rejects_duplicate_change_id_entries(self) -> None:
        self._write_target_marker()
        self._write_manifest(
            changes=(
                (VALID_CHANGE, VALID_TARGET),
                (VALID_CHANGE, VALID_TARGET),
            )
        )
        self._assert_rejected(self._run_checker())


if __name__ == "__main__":
    unittest.main()
