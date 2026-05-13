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
            "<version> <module> <change_id> [<change_id>...]",
            file=sys.stderr,
        )
        print(
            "implementation admission now requires explicit change_id coverage; "
            "use schema-check.py and admission-check.py directly for new tasks.",
            file=sys.stderr,
        )
        return 2

    version, module, *change_ids = sys.argv[1:]
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
    for change_id in change_ids:
        admission_cmd.extend(["--change-id", change_id])

    schema_result = subprocess.run(schema_cmd, check=False)
    if schema_result.returncode != 0:
        return schema_result.returncode
    admission_result = subprocess.run(admission_cmd, check=False)
    return admission_result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
