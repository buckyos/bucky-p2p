#!/usr/bin/env python3

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path


POSITIVE = r"""
use p2p_frame::endpoint::Endpoint;
use p2p_frame::networks::UdpTunnelNetwork;
use p2p_frame::p2p_identity::{P2pIdentityCertRef, P2pIdentityRef};
use p2p_frame::sn::nat_probe::NatProbeReflector;
use std::net::SocketAddr;
use std::time::Duration;

async fn signed_pnat_surface(
    network: &dyn UdpTunnelNetwork,
    bind_addr: SocketAddr,
    identity: P2pIdentityRef,
    expected_signer: P2pIdentityCertRef,
    targets: &[Endpoint],
) {
    let _ = NatProbeReflector::bind(bind_addr, identity).await;
    let _ = network
        .predict_traversal_endpoints(
            targets,
            &expected_signer,
            Duration::from_secs(1),
            Duration::from_secs(10),
        )
        .await;
}

fn main() {}
"""


NEGATIVE = {
    "bind": (
        r"""
use p2p_frame::sn::nat_probe::NatProbeReflector;
use std::net::SocketAddr;

async fn old_bind(bind_addr: SocketAddr) {
    let _ = NatProbeReflector::bind(bind_addr).await;
}

fn main() {}
""",
        ["error[E0061]", "NatProbeReflector::bind", "takes 2 arguments but 1 argument was supplied"],
    ),
    "prediction": (
        r"""
use p2p_frame::endpoint::Endpoint;
use p2p_frame::networks::UdpTunnelNetwork;
use std::time::Duration;

async fn old_prediction(network: &dyn UdpTunnelNetwork, targets: &[Endpoint]) {
    let _ = network
        .predict_traversal_endpoints(
            targets,
            Duration::from_secs(1),
            Duration::from_secs(10),
        )
        .await;
}

fn main() {}
""",
        ["error[E0061]", "predict_traversal_endpoints", "takes 4 arguments but 3 arguments were supplied"],
    ),
}


def cargo_check(repo: Path, fixture: Path) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["CARGO_NET_OFFLINE"] = "true"
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
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )


def write_fixture(
    fixture: Path, crate_path: Path, package_name: str, source: str
) -> None:
    source_path = fixture / "src" / "main.rs"
    source_path.parent.mkdir(exist_ok=True)
    (fixture / "Cargo.toml").write_text(
        "[package]\n"
        f'name = "{package_name}"\n'
        'version = "0.0.0"\n'
        'edition = "2024"\n'
        "publish = false\n\n"
        "[workspace]\n\n"
        "[dependencies]\n"
        f'p2p-frame = {{ path = "{crate_path.as_posix()}", features = ["x509"] }}\n',
        encoding="utf-8",
    )
    source_path.write_text(source, encoding="utf-8")


def run_positive(repo: Path, crate_path: Path) -> int:
    with tempfile.TemporaryDirectory(prefix="signed-pnat-api-positive-") as temp_dir:
        fixture = Path(temp_dir)
        write_fixture(fixture, crate_path, "signed-pnat-api-positive", POSITIVE)
        result = cargo_check(repo, fixture)
    if result.returncode != 0:
        print("signed PNAT public API fixture failed", file=sys.stderr)
        print(result.stdout + result.stderr, file=sys.stderr)
        return 1
    print("signed PNAT public API accepts the identity and expected-signer arguments")
    return 0


def run_negative(repo: Path, crate_path: Path) -> int:
    for name, (source, expected) in NEGATIVE.items():
        with tempfile.TemporaryDirectory(prefix=f"signed-pnat-api-negative-{name}-") as temp_dir:
            fixture = Path(temp_dir)
            write_fixture(
                fixture,
                crate_path,
                f"signed-pnat-api-negative-{name}",
                source,
            )
            result = cargo_check(repo, fixture)
        output = result.stdout + result.stderr
        if result.returncode == 0:
            print(f"legacy signed PNAT {name} call unexpectedly compiled", file=sys.stderr)
            return 1
        missing = [token for token in expected if token not in output]
        if missing:
            print(
                f"legacy signed PNAT {name} failed for an unexpected reason; missing: "
                + ", ".join(missing),
                file=sys.stderr,
            )
            print(output, file=sys.stderr)
            return 1
    print("legacy PNAT bind and prediction calls fail with the expected E0061 diagnostics")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=("positive", "negative"), required=True)
    args = parser.parse_args()

    repo = Path(__file__).resolve().parents[2]
    crate_path = repo / "p2p-frame"
    if args.mode == "positive":
        return run_positive(repo, crate_path)
    return run_negative(repo, crate_path)


if __name__ == "__main__":
    raise SystemExit(main())
