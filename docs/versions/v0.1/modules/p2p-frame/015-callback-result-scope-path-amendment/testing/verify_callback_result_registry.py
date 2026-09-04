#!/usr/bin/env python3
"""Verify callback-result resolves from crates.io 0.2.5 with no local vendor."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path


REGISTRY_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"


def validate_contents(
    root_manifest: bytes,
    p2p_manifest: bytes,
    lockfile: bytes,
    vendor_exists: bool,
) -> list[str]:
    errors: list[str] = []
    root = tomllib.loads(root_manifest.decode("utf-8"))
    p2p = tomllib.loads(p2p_manifest.decode("utf-8"))
    lock = tomllib.loads(lockfile.decode("utf-8"))

    patch = root.get("patch", {}).get("crates-io", {})
    if "callback-result" in patch:
        errors.append("root Cargo.toml still patches callback-result")

    dependency = p2p.get("dependencies", {}).get("callback-result")
    if dependency != "0.2.5":
        errors.append(f"p2p-frame requires {dependency!r}, expected '0.2.5'")

    packages = [package for package in lock.get("package", []) if package.get("name") == "callback-result"]
    if len(packages) != 1:
        errors.append(f"Cargo.lock contains {len(packages)} callback-result entries, expected 1")
    else:
        package = packages[0]
        if package.get("version") != "0.2.5":
            errors.append(f"Cargo.lock resolves callback-result {package.get('version')!r}, expected '0.2.5'")
        if package.get("source") != REGISTRY_SOURCE:
            errors.append("Cargo.lock callback-result source is not the crates.io registry")
        checksum = package.get("checksum", "")
        if not isinstance(checksum, str) or re.fullmatch(r"[0-9a-f]{64}", checksum) is None:
            errors.append("Cargo.lock callback-result checksum is missing or malformed")

    if vendor_exists:
        errors.append("third-party/callback-result still exists")
    return errors


def validate_workspace(root: Path) -> list[str]:
    return validate_contents(
        (root / "Cargo.toml").read_bytes(),
        (root / "p2p-frame" / "Cargo.toml").read_bytes(),
        (root / "Cargo.lock").read_bytes(),
        (root / "third-party" / "callback-result").exists(),
    )


def run_self_test() -> list[str]:
    root = b'[workspace]\nmembers = ["p2p-frame"]\n'
    p2p = b'[package]\nname = "p2p-frame"\nversion = "0.1.0"\n[dependencies]\ncallback-result = "0.2.5"\n'
    lock = (
        b'version = 4\n\n[[package]]\nname = "callback-result"\nversion = "0.2.5"\n'
        b'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
        b'checksum = "2f671a207a542ec897beadea9bd46ea74db13284c3e61b80c68e1e578d81bece"\n'
    )
    failures: list[str] = []

    if validate_contents(root, p2p, lock, False):
        failures.append("clean registry fixture was rejected")

    stale_patch = root + b'\n[patch.crates-io]\ncallback-result = { path = "third-party/callback-result" }\n'
    if not validate_contents(stale_patch, p2p, lock, False):
        failures.append("stale path patch was accepted")

    old_requirement = p2p.replace(b'"0.2.5"', b'"0.2.3"')
    if not validate_contents(root, old_requirement, lock, False):
        failures.append("old direct requirement was accepted")

    local_lock = lock.replace(
        b'source = "registry+https://github.com/rust-lang/crates.io-index"\n', b""
    ).replace(
        b'checksum = "2f671a207a542ec897beadea9bd46ea74db13284c3e61b80c68e1e578d81bece"\n',
        b"",
    )
    if not validate_contents(root, p2p, local_lock, False):
        failures.append("local lock entry without source/checksum was accepted")

    if not validate_contents(root, p2p, lock, True):
        failures.append("remaining vendor directory was accepted")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=("self-test", "workspace"), required=True)
    parser.add_argument("--workspace-root", default=".")
    args = parser.parse_args()

    errors = run_self_test() if args.mode == "self-test" else validate_workspace(Path(args.workspace_root))
    if errors:
        for error in errors:
            print(f"callback-result-registry-check: {error}", file=sys.stderr)
        return 1
    print(f"callback-result-registry-check: passed ({args.mode})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
