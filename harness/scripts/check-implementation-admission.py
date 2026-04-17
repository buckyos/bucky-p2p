#!/usr/bin/env python3

import pathlib
import sys


REQUIRED_DOCS = ("proposal.md", "design.md", "testing.md")


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
    if len(sys.argv) != 3:
        print("usage: python3 ./harness/scripts/check-implementation-admission.py <version> <module>", file=sys.stderr)
        return 2

    version, module = sys.argv[1], sys.argv[2]
    module_dir = pathlib.Path("docs") / "versions" / version / "modules" / module

    missing = []
    draft = []
    for name in REQUIRED_DOCS:
        path = module_dir / name
        if not path.exists():
            missing.append(str(path))
            continue
        metadata = parse_front_matter(path)
        if metadata.get("status") != "approved":
            draft.append(f"{path}: status={metadata.get('status', '<missing>')}")

    if missing or draft:
        for item in missing:
            print(f"missing: {item}", file=sys.stderr)
        for item in draft:
            print(f"not approved: {item}", file=sys.stderr)
        return 1

    print(f"implementation admission passed for {module} {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
