"""Replay unchanged real-socket assertions against pristine and fixed transports."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time


TASK = "081-tcp-error-close-data-connections"
TRANSPORTS = {
    "tcp": Path("p2p-frame/src/networks/tcp/tunnel.rs"),
    "quic": Path("p2p-frame/src/networks/quic/tunnel.rs"),
}
CASES = {
    "tcp": {
        "filter": "networks::tcp::tunnel::control_failure_tests::"
                  "dv_control_eof_wakes_borrowed_read_and_closes_data_socket",
        "assertion": "let result = timeout(Duration::from_secs(1), read).await.unwrap().unwrap();",
        "change_id": "CHG-tcp-error-close-owned-data",
    },
    "quic": {
        "filter": "networks::quic::tunnel::tests::control_failure_tests::"
                  "integration_command_failure_wakes_borrowed_data_io_after_success",
        "assertion": "assert!(timeout(Duration::from_secs(1), pending).await.unwrap().unwrap().is_err());",
        "change_id": "CHG-quic-control-error-close-connection",
    },
    "tcp-callers": {
        "protocol": "tcp",
        "baseline": "round1",
        "filter": "networks::tcp::tunnel::control_failure_tests::"
                  "dv_actual_claim_and_reverse_open_cancel_while_control_mutex_is_held",
        "assertion": 'let claim_result = timeout(Duration::from_secs(1), claim).await.expect(',
        "change_id": "CHG-tcp-error-close-owned-data",
    },
}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def test_wiring(source: str, protocol: str) -> tuple[str, str]:
    if protocol == "tcp":
        pattern = r'(#\[cfg\(test\)\]\s*include!\("([^"\n]*control_failure_tests\.rs)"\);)'
    else:
        pattern = (r'(#\[cfg\(feature = "x509"\)\]\s*mod control_failure_tests\s*'
                   r'\{\s*include!\("([^"\n]*control_failure_tests\.rs)"\);\s*\})')
    match = re.search(pattern, source)
    if not match:
        raise RuntimeError(f"current {protocol} control-failure test wiring missing")
    return match.group(1), match.group(2)


def baseline_with_test_wiring(source: str, wiring: str, protocol: str) -> str:
    if protocol == "tcp":
        # The included current file retains this existing test-only constant.
        pattern = r'#\[cfg\(test\)\]\s*const LEGACY_TCP_CONTROL_MAX_COMMAND_ID:\s*u8\s*=\s*11;'
        result, count = re.subn(pattern, lambda _: wiring, source)
    else:
        pattern = r'(#\[cfg\(test\)\]\s*mod tests\s*\{\s*use super::\*;)'
        result, count = re.subn(pattern, lambda match: match.group(1) + "\n\n    " + wiring, source)
    if count != 1:
        raise RuntimeError(f"pristine {protocol} source must have exactly one test-wiring anchor, got {count}")
    return result


def expected_assertion(test_source: Path, case: dict) -> dict:
    source = test_source.read_text(encoding="utf-8")
    function = case["filter"].rsplit("::", 1)[1]
    function_start = source.index(f"async fn {function}(")
    assertion = source.index(case["assertion"], function_start)
    following_test = source.find("#[tokio::test", function_start + 1)
    if following_test >= 0 and assertion > following_test:
        raise RuntimeError(f"expected assertion is outside selected test {function}")
    return {"test_source": str(test_source), "test_sha256": digest(test_source),
            "panic_line": source.count("\n", 0, assertion) + 1,
            "assertion": case["assertion"]}


def invoke(repo: Path, workspace: Path, case: dict, mode: str, log: Path, timeout: int) -> dict:
    # copytree preserves source mtimes; force Cargo to rebuild this variant and
    # its include! files instead of reusing another replay's cached test binary.
    for source in (workspace / "p2p-frame").rglob("*.rs"):
        source.touch()
    command = ["cargo", "test", "--offline", "--manifest-path", str(workspace / "Cargo.toml"),
               "--target-dir", str(repo / "target"), "-p", "p2p-frame", "--lib", "--features", "x509",
               case["filter"], "--", "--exact", "--nocapture", "--test-threads=1"]
    started = time.monotonic()
    try:
        process = subprocess.run(command, cwd=repo, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                 text=True, encoding="utf-8", errors="replace", timeout=timeout, check=False)
        output = process.stdout
        exit_code = process.returncode
    except subprocess.TimeoutExpired as error:
        output = error.stdout or ""
        if isinstance(output, bytes):
            output = output.decode("utf-8", errors="replace")
        output += f"\nWrapper timeout after {timeout} seconds.\n"
        exit_code = -1
    log.write_text(output, encoding="utf-8")
    normalized = output.replace("\\", "/")
    if mode == "baseline":
        assertion_location = f'control_failure_tests.rs:{case["panic_line"]}:'
        verified = (exit_code not in (0, -1)
                    and "running 1 test" in normalized
                    and "test result: FAILED. 0 passed; 1 failed;" in normalized
                    and case["filter"] in normalized
                    and assertion_location in normalized
                    and "Elapsed(())" in normalized)
        expectation = "one runtime failure at the selected terminal-operation timeout assertion"
    else:
        verified = (exit_code == 0 and "running 1 test" in normalized
                    and "test result: ok. 1 passed; 0 failed;" in normalized
                    and case["filter"] in normalized)
        expectation = "the identical selected test passes"
    print(f'{mode}: {case["filter"]}: {"verified" if verified else "FAILED expectation"}', flush=True)
    if not verified:
        print(output[-2500:], flush=True)
    return {"mode": mode, "command": command, "exit_code": exit_code,
            "duration_seconds": round(time.monotonic() - started, 3), "log": str(log),
            "expectation": expectation, "verified": verified}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", choices=["all", *CASES], default="all")
    parser.add_argument("--timeout", type=int, default=600, help="maximum seconds per Cargo invocation")
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    repo = Path(__file__).resolve().parents[2]
    snapshots = repo / ".harness/baselines/v0.1/p2p-frame" / TASK
    result_root = repo / ".harness/test-results/control-failure-red-green"
    result_root.mkdir(parents=True, exist_ok=True)
    run_dir = Path(tempfile.mkdtemp(prefix="run-", dir=result_root))
    workspace_root = repo / ".harness/control-failure-red-green"
    workspace_root.mkdir(parents=True, exist_ok=True)
    run_workspace = Path(tempfile.mkdtemp(prefix="run-", dir=workspace_root))
    lock = repo / "Cargo.lock"
    original_lock = digest(lock)
    report = {"task": TASK, "started_utc": datetime.now(timezone.utc).isoformat(),
              "selected_case": args.case, "workspace": str(run_workspace),
              "original_lock_sha256": original_lock, "cases": [], "status": "failed"}
    try:
        manifest = json.loads((snapshots / "manifest.json").read_text(encoding="utf-8"))
        recorded = {entry["path"]: entry for entry in manifest["files"]}
        # Both trees start from one snapshot, retaining identical Windows fixtures and tests.
        fixed = run_workspace / "fixed"
        fixed.mkdir()
        shutil.copytree(repo / "p2p-frame", fixed / "p2p-frame",
                        ignore=shutil.ignore_patterns("target", ".git", ".harness", "__pycache__"))
        (fixed / "Cargo.toml").write_text('[workspace]\nresolver = "2"\nmembers = ["p2p-frame"]\n', encoding="utf-8")
        shutil.copy2(lock, fixed / "Cargo.lock")
        baseline = run_workspace / "baseline"
        shutil.copytree(fixed, baseline)
        test_sources = {}
        for protocol, relative in TRANSPORTS.items():
            entry = recorded.get(relative.as_posix())
            if entry is None:
                raise RuntimeError(f"missing recorded pristine source: {relative}")
            snapshot = (snapshots / entry["snapshot"]).resolve()
            snapshot.relative_to(snapshots.resolve())
            live = fixed / relative
            wiring, include_path = test_wiring(live.read_text(encoding="utf-8"), protocol)
            test_sources[protocol] = (live.parent / include_path).resolve()
            test_sources[protocol].relative_to(fixed.resolve())
            pristine = snapshot.read_text(encoding="utf-8")
            (baseline / relative).write_text(baseline_with_test_wiring(pristine, wiring, protocol), encoding="utf-8")
            report.setdefault("production_sources", {})[protocol] = {
                "baseline_snapshot": str(snapshot), "pristine_sha256": digest(snapshot),
                "baseline_with_test_wiring_sha256": digest(baseline / relative),
                "fixed_sha256": digest(live), "test_only_wiring": wiring,
            }
        # Round 1 already contains data retirement, but still blocks actual
        # callers before their failed response receiver can be observed.
        round1 = run_workspace / "round1"
        shutil.copytree(fixed, round1)
        relative = TRANSPORTS["tcp"]
        round1_root = repo / ".harness/baselines/v0.1/p2p-frame" / f"{TASK}-testing-round-1"
        round1_manifest = json.loads((round1_root / "manifest.json").read_text(encoding="utf-8"))
        entry = next(item for item in round1_manifest["files"] if item["path"] == relative.as_posix())
        snapshot = (round1_root / entry["snapshot"]).resolve()
        snapshot.relative_to(round1_root.resolve())
        wiring, _ = test_wiring((fixed / relative).read_text(encoding="utf-8"), "tcp")
        (round1 / relative).write_text(
            baseline_with_test_wiring(snapshot.read_text(encoding="utf-8"), wiring, "tcp"), encoding="utf-8")
        report["production_sources"]["tcp-callers"] = {
            "baseline_snapshot": str(snapshot), "pristine_sha256": digest(snapshot),
            "baseline_with_test_wiring_sha256": digest(round1 / relative),
            "fixed_sha256": digest(fixed / relative), "test_only_wiring": wiring,
        }
        fixture = Path("p2p-frame/src/sn/client/sn_service.rs")
        report["shared_windows_fixture"] = {"path": fixture.as_posix(), "sha256": digest(fixed / fixture)}
        if digest(fixed / fixture) != digest(baseline / fixture):
            raise RuntimeError("baseline and fixed Windows fixtures differ")
        selected_cases = list(CASES) if args.case == "all" else [args.case]
        for selected_case in selected_cases:
            case = dict(CASES[selected_case])
            protocol = case.get("protocol", selected_case)
            selected_baseline = round1 if case.get("baseline") == "round1" else baseline
            case.update(expected_assertion(test_sources[protocol], case))
            relative_test = test_sources[protocol].relative_to(fixed)
            if digest(selected_baseline / relative_test) != case["test_sha256"]:
                raise RuntimeError("baseline and fixed assertions differ")
            red = invoke(repo, selected_baseline, case, "baseline", run_dir / f"{selected_case}-baseline.log", args.timeout)
            green = invoke(repo, fixed, case, "fixed", run_dir / f"{selected_case}-fixed.log", args.timeout)
            report["cases"].append({**case, "case": selected_case, "baseline": red, "fixed": green,
                                    "verified": red["verified"] and green["verified"]})
        if all(case["verified"] for case in report["cases"]):
            report["status"] = "passed"
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
        print(report["error"], file=sys.stderr, flush=True)
    finally:
        report["final_lock_sha256"] = digest(lock)
        report["root_lock_unchanged"] = report["final_lock_sha256"] == original_lock
        if not report["root_lock_unchanged"]:
            report["status"] = "failed"
        report["finished_utc"] = datetime.now(timezone.utc).isoformat()
        encoded = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
        (run_dir / "result.json").write_text(encoded, encoding="utf-8")
        (result_root / "latest.json").write_text(encoded, encoding="utf-8")
        print(f'red-green evidence: {run_dir / "result.json"}', flush=True)
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
