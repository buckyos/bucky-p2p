#!/usr/bin/env python3

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path


def main() -> int:
    repo = Path(__file__).resolve().parents[2]
    crate_path = repo / "p2p-frame"
    with tempfile.TemporaryDirectory(prefix="removed-rendezvous-api-") as temp_dir:
        fixture = Path(temp_dir)
        (fixture / "src").mkdir()
        (fixture / "Cargo.toml").write_text(
            "[package]\n"
            'name = "removed-rendezvous-api"\n'
            'version = "0.0.0"\n'
            'edition = "2024"\n'
            "publish = false\n\n"
            "[workspace]\n\n"
            "[dependencies]\n"
            f'p2p-frame = {{ path = "{crate_path.as_posix()}" }}\n',
            encoding="utf-8",
        )
        (fixture / "src" / "main.rs").write_text(
            "use p2p_frame::sn::protocol::{"
            "SnTunnelRendezvousEnvelope, SnTunnelRendezvousTerminal};\n\n"
            "fn main() {\n"
            "    let _: Option<SnTunnelRendezvousEnvelope> = None;\n"
            "    let _: Option<SnTunnelRendezvousTerminal> = None;\n"
            "}\n",
            encoding="utf-8",
        )
        result = subprocess.run(
            [
                "cargo",
                "check",
                "--offline",
                "--manifest-path",
                str(fixture / "Cargo.toml"),
                "--target-dir",
                str(repo / "target"),
            ],
            cwd=repo,
            capture_output=True,
            text=True,
            check=False,
        )
    output = result.stdout + result.stderr
    expected = [
        "SnTunnelRendezvousEnvelope",
        "SnTunnelRendezvousTerminal",
        "unresolved import",
    ]
    if result.returncode == 0:
        print("removed rendezvous API unexpectedly compiled", file=sys.stderr)
        return 1
    missing = [token for token in expected if token not in output]
    if missing:
        print(
            "compile failed for an unexpected reason; missing diagnostics: "
            + ", ".join(missing),
            file=sys.stderr,
        )
        print(output, file=sys.stderr)
        return 1
    print("removed rendezvous API is rejected for the expected symbols")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
