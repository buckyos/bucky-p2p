"""Compile real external consumers of the ordered TTP target field."""
from pathlib import Path
import json
import os
import re
import subprocess
import sys


def main():
    mode = sys.argv[1]
    assert mode in {"positive", "negative", "docs"}
    root = Path(__file__).resolve().parents[2]
    crate = root / ".harness" / "test-fixtures" / "ttp-ordered-endpoints" / mode
    (crate / "src").mkdir(parents=True, exist_ok=True)
    dependency = (root / "p2p-frame").as_posix()
    (crate / "Cargo.toml").write_text(
        '[package]\nname = "ttp-ordered-' + mode + '"\nversion = "0.0.0"\nedition = "2024"\n'
        '[workspace]\n[dependencies]\np2p-frame = { path = "' + dependency + '" }\n',
        encoding="utf-8",
    )
    value = "vec![first, backup]" if mode == "positive" else "first"
    source = (
        "use p2p_frame::{endpoint::Endpoint, p2p_identity::P2pId, ttp::TtpTarget};\n"
        "pub fn target(remote_id: P2pId, first: Endpoint, backup: Endpoint) -> TtpTarget {\n"
        "    TtpTarget { local_ep: None, remote_ep: " + value + ", remote_id, remote_name: None }\n}\n"
    )
    if mode == "docs":
        documentation = (root / "p2p-frame/docs/ttp_module_design.md").read_text(encoding="utf-8")
        blocks = re.findall(r"```rust\s+([\s\S]*?)```", documentation)
        target = next(block for block in blocks if "pub struct TtpTarget" in block)
        source = "use p2p_frame::{endpoint::Endpoint, p2p_identity::P2pId};\n" + target
    (crate / "src" / "lib.rs").write_text(source, encoding="utf-8")
    env = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"))
    result = subprocess.run(
        ["cargo", "check", "--offline", "--manifest-path", str(crate / "Cargo.toml"), "--message-format=json"],
        cwd=root, env=env, text=True, capture_output=True,
    )
    errors = []
    for line in result.stdout.splitlines():
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        if entry.get("reason") == "compiler-message" and entry["message"]["level"] == "error":
            errors.append(entry["message"])
    if mode != "negative":
        if result.returncode:
            print(result.stderr + result.stdout)
            return 1
    else:
        matching = [error for error in errors if error.get("code", {}).get("code") == "E0308"
                    and "Vec<Endpoint>" in error.get("rendered", "")
                    and "remote_ep: first" in error.get("rendered", "")]
        if result.returncode == 0 or len(errors) != 1 or len(matching) != 1:
            print(result.stderr + result.stdout)
            return 1
    print(f"external TtpTarget {mode} contract passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
