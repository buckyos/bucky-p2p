#!/usr/bin/env python3

import pathlib
import sys


MODULES = (
    "p2p-frame",
    "cyfs-p2p",
    "cyfs-p2p-test",
    "sn-miner",
    "desc-tool",
)

STAGE_DOCS = ("proposal.md", "design.md", "testing.md")


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
    version = sys.argv[1] if len(sys.argv) == 2 else "v0.1"
    root = pathlib.Path("docs") / "versions" / version / "modules"

    print(f"approval status report for {version}")
    for module in MODULES:
        print(f"[{module}]")
        packet_dir = root / module
        if not packet_dir.exists():
            print("  missing packet dir")
            continue

        for name in STAGE_DOCS:
            path = packet_dir / name
            if not path.exists():
                print(f"  {name}: missing")
                continue
            metadata = parse_front_matter(path)
            status = metadata.get("status", "<missing>")
            approved_by = metadata.get("approved_by", "")
            approved_at = metadata.get("approved_at", "")
            print(
                f"  {name}: status={status} approved_by={approved_by or '-'} approved_at={approved_at or '-'}"
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
