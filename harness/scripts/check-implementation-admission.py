#!/usr/bin/env python3
"""Compatibility wrapper for the current implementation admission checks."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) < 4:
        print(
            "usage: python3 ./harness/scripts/check-implementation-admission.py "
            "<version> <module> [--submodule <submodule>] "
            "[--target-module <project>] "
            "--evidence-file <path> <change_id> [<change_id>...]",
            file=sys.stderr,
        )
        print(
            "implementation admission requires explicit change_id coverage and "
            "task admission evidence; use schema-check.py and admission-check.py "
            "directly for new tasks.",
            file=sys.stderr,
        )
        return 2

    version, module, *rest = sys.argv[1:]
    submodule = None
    target_module = None
    evidence_file = None
    while rest[:1] and rest[0].startswith("--"):
        option = rest.pop(0)
        if option not in {"--submodule", "--target-module", "--evidence-file"}:
            print(f"unknown option: {option}", file=sys.stderr)
            return 2
        if not rest:
            print(f"{option} requires a value", file=sys.stderr)
            return 2
        value = rest.pop(0)
        if option == "--submodule":
            submodule = value
        elif option == "--target-module":
            target_module = value
        else:
            evidence_file = value
    if evidence_file is None:
        print("--evidence-file is required by the current admission gate", file=sys.stderr)
        return 2
    change_ids = rest
    root = Path(__file__).resolve().parents[2]

    schema_cmd = [
        sys.executable,
        str(root / "harness" / "scripts" / "schema-check.py"),
        "--root",
        str(root),
        "--version",
        version,
        "--module",
        module,
    ]
    if submodule:
        schema_cmd.extend(["--submodule", submodule])
    admission_cmd = [
        sys.executable,
        str(root / "harness" / "scripts" / "admission-check.py"),
        "--root",
        str(root),
        "--version",
        version,
        "--module",
        module,
    ]
    if submodule:
        admission_cmd.extend(["--submodule", submodule])
    if target_module:
        admission_cmd.extend(["--target-module", target_module])
    for change_id in change_ids:
        admission_cmd.extend(["--change-id", change_id])
    admission_cmd.extend(["--evidence-file", evidence_file])

    schema_result = subprocess.run(schema_cmd, check=False)
    if schema_result.returncode != 0:
        return schema_result.returncode
    admission_result = subprocess.run(admission_cmd, check=False)
    return admission_result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
