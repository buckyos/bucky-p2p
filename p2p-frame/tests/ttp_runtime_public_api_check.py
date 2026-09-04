#!/usr/bin/env python3

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path


POSITIVE = r"""
use p2p_frame::error::P2pResult;
use p2p_frame::networks::NetManagerRef;
use p2p_frame::p2p_identity::P2pIdentityRef;
use p2p_frame::ttp::{TtpNode, TtpNodeRef, TtpRuntime, TtpServer, TtpServerRef};

fn public_surface() {
    let _: fn(P2pIdentityRef, NetManagerRef) -> P2pResult<TtpServerRef> = TtpServer::new;
    let _: fn(P2pIdentityRef, NetManagerRef) -> P2pResult<TtpNodeRef> = TtpNode::new;
    let _: fn(TtpRuntime) -> TtpServerRef = TtpServer::new_with_runtime;
    let _: fn(TtpRuntime) -> TtpNodeRef = TtpNode::new_with_runtime;
    let _: fn(&TtpServer) -> TtpRuntime = TtpServer::runtime;
    let _: fn(&TtpNode) -> TtpRuntime = TtpNode::runtime;
}

fn server_to_node(server: &TtpServer) {
    let _ = TtpNode::new_with_runtime(server.runtime());
}

fn node_to_server(node: &TtpNode) {
    let _ = TtpServer::new_with_runtime(node.runtime());
}

fn main() {}
"""

NEGATIVE = {
    "core": ("runtime.core();", ["core", "private"]),
    "attach": ("runtime.attach_tunnel(todo!());", ["attach_tunnel", "no method"]),
    "listen": ("runtime.listen_stream(todo!(), todo!());", ["listen_stream", "no method"]),
    "connect": ("runtime.get_or_create_tunnel(todo!());", ["get_or_create_tunnel", "no method"]),
    "deref": ("let _ = &*runtime;", ["cannot be dereferenced"]),
    "field": ("let _ = runtime.0;", ["field `0`", "private"]),
}


def cargo_check(repo: Path, fixture: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            "cargo",
            "check",
            "--offline",
            "--quiet",
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


def main() -> int:
    repo = Path(__file__).resolve().parents[2]
    crate_path = repo / "p2p-frame"
    with tempfile.TemporaryDirectory(prefix="ttp-runtime-public-api-") as temp_dir:
        fixture = Path(temp_dir)
        source = fixture / "src" / "main.rs"
        source.parent.mkdir()
        (fixture / "Cargo.toml").write_text(
            "[package]\n"
            'name = "ttp-runtime-public-api"\n'
            'version = "0.0.0"\n'
            'edition = "2024"\n'
            "publish = false\n\n"
            "[workspace]\n\n"
            "[dependencies]\n"
            f'p2p-frame = {{ path = "{crate_path.as_posix()}", features = ["x509"] }}\n',
            encoding="utf-8",
        )

        source.write_text(POSITIVE, encoding="utf-8")
        positive = cargo_check(repo, fixture)
        if positive.returncode != 0:
            print("positive public TTP runtime API fixture failed", file=sys.stderr)
            print(positive.stdout + positive.stderr, file=sys.stderr)
            return 1

        for name, (statement, expected) in NEGATIVE.items():
            source.write_text(
                "use p2p_frame::ttp::TtpRuntime;\n"
                f"fn probe(runtime: TtpRuntime) {{ {statement} }}\n"
                "fn main() {}\n",
                encoding="utf-8",
            )
            result = cargo_check(repo, fixture)
            output = result.stdout + result.stderr
            if result.returncode == 0:
                print(f"forbidden runtime {name} access unexpectedly compiled", file=sys.stderr)
                return 1
            missing = [token for token in expected if token not in output]
            if missing:
                print(
                    f"runtime {name} failed for an unexpected reason; missing: "
                    + ", ".join(missing),
                    file=sys.stderr,
                )
                print(output, file=sys.stderr)
                return 1

    print("public facade compiles and opaque TtpRuntime operations remain inaccessible")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
