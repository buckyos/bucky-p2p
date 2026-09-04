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
use p2p_frame::networks::{
    TunnelConnectIntent, TunnelNetwork, TraversalEndpointPrediction, UdpTunnelNetwork,
};
use p2p_frame::p2p_identity::P2pIdentityCertRef;
use p2p_frame::types::Timestamp;
use std::time::Duration;

fn udp_network_is_also_a_tunnel_network<T: UdpTunnelNetwork>(
    network: &T,
) -> &dyn TunnelNetwork {
    network
}

async fn use_udp_capability(
    network: &dyn TunnelNetwork,
    remote: &Endpoint,
    targets: &[Endpoint],
    expected_signer: &P2pIdentityCertRef,
    prediction: &TraversalEndpointPrediction,
    now: Timestamp,
) {
    if let Some(udp) = network.as_udp_tunnel_network() {
        let _ = udp
            .punch_only(remote, TunnelConnectIntent::default(), Duration::from_secs(1))
            .await;
        let _ = udp
            .predict_traversal_endpoints(
                targets,
                expected_signer,
                Duration::from_secs(1),
                Duration::from_secs(30),
            )
            .await;
        let _ = udp.validate_traversal_prediction(prediction, now);
    }
}

fn main() {}
"""


NEGATIVE = r"""
use p2p_frame::endpoint::Endpoint;
use p2p_frame::networks::{TunnelConnectIntent, TunnelNetwork, TraversalEndpointPrediction};
use p2p_frame::p2p_identity::P2pIdentityCertRef;
use p2p_frame::types::Timestamp;
use std::time::Duration;

async fn old_generic_surface(
    network: &dyn TunnelNetwork,
    remote: &Endpoint,
    targets: &[Endpoint],
    expected_signer: &P2pIdentityCertRef,
    prediction: &TraversalEndpointPrediction,
    now: Timestamp,
) {
    let _ = network
        .punch_only(remote, TunnelConnectIntent::default(), Duration::from_secs(1))
        .await;
    let _ = network
        .predict_traversal_endpoints(
            targets,
            expected_signer,
            Duration::from_secs(1),
            Duration::from_secs(30),
        )
        .await;
    let _ = network.validate_traversal_prediction(prediction, now);
}

fn main() {}
"""


def write_fixture(fixture: Path, crate_path: Path, source: str) -> None:
    source_path = fixture / "src" / "main.rs"
    source_path.parent.mkdir()
    (fixture / "Cargo.toml").write_text(
        "[package]\n"
        'name = "udp-tunnel-network-api-check"\n'
        'version = "0.0.0"\n'
        'edition = "2024"\n'
        "publish = false\n\n"
        "[workspace]\n\n"
        "[dependencies]\n"
        f'p2p-frame = {{ path = "{crate_path.as_posix()}", features = ["x509"] }}\n',
        encoding="utf-8",
    )
    source_path.write_text(source, encoding="utf-8")


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


def run(mode: str) -> int:
    repo = Path(__file__).resolve().parents[2]
    crate_path = repo / "p2p-frame"
    source = POSITIVE if mode == "positive" else NEGATIVE
    with tempfile.TemporaryDirectory(prefix=f"udp-tunnel-network-{mode}-") as temp_dir:
        fixture = Path(temp_dir)
        write_fixture(fixture, crate_path, source)
        result = cargo_check(repo, fixture)

    output = result.stdout + result.stderr
    if mode == "positive":
        if result.returncode != 0:
            print("UdpTunnelNetwork public API fixture failed", file=sys.stderr)
            print(output, file=sys.stderr)
            return 1
        print("UdpTunnelNetwork is exported, extends TunnelNetwork, and is discoverable")
        return 0

    expected = [
        "no method named `punch_only`",
        "no method named `predict_traversal_endpoints`",
        "no method named `validate_traversal_prediction`",
    ]
    if result.returncode == 0:
        print("removed TunnelNetwork traversal methods unexpectedly compiled", file=sys.stderr)
        return 1
    missing = [token for token in expected if token not in output]
    if missing:
        print(
            "old generic API failed for an unexpected reason; missing diagnostics: "
            + ", ".join(missing),
            file=sys.stderr,
        )
        print(output, file=sys.stderr)
        return 1
    print("TunnelNetwork no longer exposes UDP traversal methods")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=("positive", "negative"), required=True)
    args = parser.parse_args()
    return run(args.mode)


if __name__ == "__main__":
    raise SystemExit(main())
