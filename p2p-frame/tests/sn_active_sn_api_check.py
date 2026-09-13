#!/usr/bin/env python3
"""Compiler-backed external consumer check for the ActiveSN public surface.

Positive mode proves an external crate can consume the delivered `ActiveSN`
identity/profile surface. Negative mode proves the removed `conn_id` field is
rejected with the expected diagnostics instead of only being absent from text.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path


POSITIVE = r"""
use p2p_frame::sn::client::ActiveSN;

fn active_sn_surface(active: &ActiveSN) -> bool {
    let _latest = active.latest_time;
    active.sn_peer_id == active.sn_peer_id
}

fn main() {
    let _ = active_sn_surface;
}
"""

NEGATIVE = r"""
use p2p_frame::sn::client::ActiveSN;

fn legacy_active_sn_conn(active: &ActiveSN) -> u32 {
    active.conn_id.value()
}

fn main() {
    let _ = legacy_active_sn_conn;
}
"""

NEGATIVE_EXPECTED = ["no field `conn_id`", "ActiveSN"]


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
    with tempfile.TemporaryDirectory(prefix="sn-active-sn-api-positive-") as temp_dir:
        fixture = Path(temp_dir)
        write_fixture(fixture, crate_path, "sn-active-sn-api-positive", POSITIVE)
        result = cargo_check(repo, fixture)
    if result.returncode != 0:
        print("active SN identity surface fixture failed to compile", file=sys.stderr)
        print(result.stdout + result.stderr, file=sys.stderr)
        return 1
    print("external consumers can use the active SN identity surface without conn_id")
    return 0


def run_negative(repo: Path, crate_path: Path) -> int:
    with tempfile.TemporaryDirectory(prefix="sn-active-sn-api-negative-") as temp_dir:
        fixture = Path(temp_dir)
        write_fixture(fixture, crate_path, "sn-active-sn-api-negative", NEGATIVE)
        result = cargo_check(repo, fixture)
    output = result.stdout + result.stderr
    if result.returncode == 0:
        print("legacy active SN conn_id access unexpectedly compiled", file=sys.stderr)
        return 1
    missing = [token for token in NEGATIVE_EXPECTED if token not in output]
    if missing:
        print(
            "legacy active SN conn_id access failed for an unexpected reason; missing: "
            + ", ".join(missing),
            file=sys.stderr,
        )
        print(output, file=sys.stderr)
        return 1
    print("legacy active SN conn_id access is rejected for the expected removed field")
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
