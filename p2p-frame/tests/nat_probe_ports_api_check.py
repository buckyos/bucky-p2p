#!/usr/bin/env python3

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path


POSITIVE = r"""
use p2p_frame::sn::protocol::{NatProbeDirective, ReportSnResp};
use p2p_frame::sn::service::SnServiceRef;

fn port_only_surface(
    mut directive: NatProbeDirective,
    mut response: ReportSnResp,
    service: &SnServiceRef,
) {
    directive.ports = vec![3600, 3601];
    response.nat_probe_ports = vec![3600, 3601];
    service.set_nat_probe_ports(vec![3600, 3601]);
}

fn main() {}
"""


NEGATIVE = {
    "directive_endpoints": (
        r"""
use p2p_frame::endpoint::Endpoint;
use p2p_frame::sn::protocol::NatProbeDirective;

fn legacy_directive(mut directive: NatProbeDirective, endpoints: Vec<Endpoint>) {
    directive.endpoints = endpoints;
}

fn main() {}
""",
        ["no field `endpoints`", "NatProbeDirective"],
    ),
    "response_endpoints": (
        r"""
use p2p_frame::endpoint::Endpoint;
use p2p_frame::sn::protocol::ReportSnResp;

fn legacy_response(mut response: ReportSnResp, endpoints: Vec<Endpoint>) {
    response.nat_probe_endpoints = endpoints;
}

fn main() {}
""",
        ["no field `nat_probe_endpoints`", "ReportSnResp"],
    ),
    "service_endpoints": (
        r"""
use p2p_frame::endpoint::Endpoint;
use p2p_frame::sn::service::SnServiceRef;

fn legacy_service(service: &SnServiceRef, endpoints: Vec<Endpoint>) {
    service.set_nat_probe_endpoints(endpoints);
}

fn main() {}
""",
        ["no method named `set_nat_probe_endpoints`"],
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


def write_fixture(fixture: Path, crate_path: Path, package_name: str, source: str) -> None:
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
    with tempfile.TemporaryDirectory(prefix="nat-probe-ports-api-positive-") as temp_dir:
        fixture = Path(temp_dir)
        write_fixture(fixture, crate_path, "nat-probe-ports-api-positive", POSITIVE)
        result = cargo_check(repo, fixture)
    if result.returncode != 0:
        print("NAT probe port-only public API fixture failed", file=sys.stderr)
        print(result.stdout + result.stderr, file=sys.stderr)
        return 1
    print("NAT probe public API accepts ports without advertised endpoints")
    return 0


def run_negative(repo: Path, crate_path: Path) -> int:
    for name, (source, expected) in NEGATIVE.items():
        with tempfile.TemporaryDirectory(prefix=f"nat-probe-ports-api-negative-{name}-") as temp_dir:
            fixture = Path(temp_dir)
            write_fixture(fixture, crate_path, f"nat-probe-ports-api-negative-{name}", source)
            result = cargo_check(repo, fixture)
        output = result.stdout + result.stderr
        if result.returncode == 0:
            print(f"legacy NAT probe {name} API unexpectedly compiled", file=sys.stderr)
            return 1
        missing = [token for token in expected if token not in output]
        if missing:
            print(
                f"legacy NAT probe {name} failed for an unexpected reason; missing: "
                + ", ".join(missing),
                file=sys.stderr,
            )
            print(output, file=sys.stderr)
            return 1
    print("legacy endpoint-bearing NAT probe fields and setter are unavailable")
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
