#!/usr/bin/env python3

import pathlib
import sys


REQUIRED_DOCS = ("proposal.md", "design.md", "testing.md")
REQUIRED_APPROVAL_FIELDS = ("approved_by", "approved_at")
REQUIRED_DIRECT_MAPPING_SECTIONS = {
    "proposal.md": "## 验收锚点",
    "design.md": "## 当前改动直接映射",
    "testing.md": "## 当前改动直接验证",
}
# Temporary exemption for the legacy approved packet until it is refreshed in a doc-stage task.
LEGACY_DIRECT_MAPPING_EXEMPT_MODULES = {"p2p-frame"}


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
    invalid = []
    for name in REQUIRED_DOCS:
        path = module_dir / name
        if not path.exists():
            missing.append(str(path))
            continue
        metadata = parse_front_matter(path)
        if metadata.get("status") != "approved":
            draft.append(f"{path}: status={metadata.get('status', '<missing>')}")
            continue
        for field in REQUIRED_APPROVAL_FIELDS:
            if not metadata.get(field, "").strip():
                invalid.append(f"{path}: missing {field}")
        if module not in LEGACY_DIRECT_MAPPING_EXEMPT_MODULES:
            text = path.read_text(encoding="utf-8")
            required_section = REQUIRED_DIRECT_MAPPING_SECTIONS[name]
            if required_section not in text:
                invalid.append(f"{path}: missing direct mapping section {required_section}")

    testplan_path = module_dir / "testplan.yaml"
    if not testplan_path.exists():
        missing.append(str(testplan_path))

    if missing or draft or invalid:
        for item in missing:
            print(f"missing: {item}", file=sys.stderr)
        for item in draft:
            print(f"not approved: {item}", file=sys.stderr)
        for item in invalid:
            print(f"invalid approval metadata: {item}", file=sys.stderr)
        return 1

    print(f"implementation admission passed for {module} {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
