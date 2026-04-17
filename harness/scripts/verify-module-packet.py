#!/usr/bin/env python3

import pathlib
import sys


REQUIRED_PACKET_FILES = (
    "proposal.md",
    "design.md",
    "testing.md",
    "acceptance.md",
    "testplan.yaml",
)

REQUIRED_METADATA = ("module", "version", "status", "approved_by", "approved_at")


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
        print("usage: python3 ./harness/scripts/verify-module-packet.py <version> <module>", file=sys.stderr)
        return 2

    version, module = sys.argv[1], sys.argv[2]
    module_dir = pathlib.Path("docs") / "versions" / version / "modules" / module
    module_doc = pathlib.Path("docs") / "modules" / f"{module}.md"

    errors: list[str] = []
    if not module_dir.exists():
        errors.append(f"missing module dir: {module_dir}")

    for name in REQUIRED_PACKET_FILES:
        path = module_dir / name
        if not path.exists():
            errors.append(f"missing required file: {path}")
            continue
        if name.endswith(".md") and name != "acceptance.md":
            metadata = parse_front_matter(path)
            for key in REQUIRED_METADATA:
                if key not in metadata:
                    errors.append(f"missing metadata {key} in {path}")

    if not module_doc.exists():
        errors.append(f"missing long-lived module doc: {module_doc}")

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    print(f"module packet is structurally valid: {module} {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
