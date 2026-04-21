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
REQUIRED_TEST_LEVELS = ("unit", "dv", "integration")
REQUIRED_DIRECT_MAPPING_SECTIONS = {
    "proposal.md": "## 验收锚点",
    "design.md": "## 当前改动直接映射",
    "testing.md": "## 当前改动直接验证",
}
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
            text = path.read_text(encoding="utf-8")
            metadata = parse_front_matter(path)
            for key in REQUIRED_METADATA:
                if key not in metadata:
                    errors.append(f"missing metadata {key} in {path}")
            if metadata.get("module") not in ("", module):
                errors.append(f"wrong module metadata in {path}: {metadata.get('module')}")
            if metadata.get("version") not in ("", version):
                errors.append(f"wrong version metadata in {path}: {metadata.get('version')}")
            if metadata.get("status") == "approved":
                for key in ("approved_by", "approved_at"):
                    if not metadata.get(key, "").strip():
                        errors.append(f"approved document missing {key} in {path}")
            if name in REQUIRED_DIRECT_MAPPING_SECTIONS and module not in LEGACY_DIRECT_MAPPING_EXEMPT_MODULES:
                required_section = REQUIRED_DIRECT_MAPPING_SECTIONS[name]
                if required_section not in text:
                    errors.append(f"missing section {required_section} in {path}")
        elif name == "testplan.yaml":
            text = path.read_text(encoding="utf-8")
            for level in REQUIRED_TEST_LEVELS:
                if f"{level}:" not in text:
                    errors.append(f"missing test level {level} in {path}")

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
