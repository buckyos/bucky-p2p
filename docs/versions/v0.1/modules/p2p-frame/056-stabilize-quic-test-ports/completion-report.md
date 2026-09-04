# Lightweight Acceptance Report

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/056-stabilize-quic-test-ports.md

## Delivery Summary

- Outcome: QUIC network tests now allocate monotonic, process-unique loopback ports from `20000..=24999` instead of asking independent `sfo-reuseport` listeners to bind `127.0.0.1:0`; the reported TLS identity failure no longer reproduces under the original parallel pressure.
- Handoff: only the existing `#[cfg(all(test, feature = "x509"))]` fixture changed. Production QUIC/TLS behavior, identity verification, timeout values, dependencies, and the user's pre-existing dirty changes remain untouched.

## Proposal Consistency

| change_id | Requirement or Boundary | Proposal Source | Delivery Evidence | Finding | Status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| quic_network_test_unique_listener_ports | Parallel QUIC network listeners must not share a dynamically selected UDP reuse-port group; production and TLS behavior must remain unchanged | proposal.md `P-056-PORT`, Scope, and Success Criteria | `network.rs` test-only `NEXT_TEST_QUIC_PORT` and `loopback_quic_ep`; baseline diff contains exactly this helper/import/constants change; six 21-thread module runs pass | Delivery matches the approved test-only boundary and removes the reproduced cross-fixture handshake failure | pass |

## Independent Defect Discovery

| Category | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|--------------------|-------------------|----------------------------------|--------|
| behavior-and-logic | Baseline-to-current diff of `p2p-frame/src/networks/quic/network.rs`; every `loopback_quic_ep()` call site; `sfo-reuseport` Linux socket setup | Checked whether relaxed atomic allocation can duplicate a port, whether the range can silently wrap into an invalid port, and whether any production constructor consumes the helper | `fetch_add` returns a distinct value per call; the inclusive range assertion fails before an out-of-range endpoint is used; helper and state are confined to the x509 test module | pass |
| boundaries-and-failure-paths | QUIC range constants, SN test allocator constants, repository scan for ports 20000/24999, listener `listen(...).unwrap()` behavior | Challenged range exhaustion, overlap with the SN `25025+` range, host port occupation, and loss of TLS/timeout coverage | Capacity is 5000 ports versus fewer than 64 fixture allocations per test process; no repository listener allocator uses this range; host conflict remains an explicit bind failure rather than a connection to an unintended identity; real TLS and the 3-second timeout remain active | pass |
| regression-and-side-effects | Exact reported test, six full QUIC network module runs at 21 threads, full 467-test x509 lib run, canonical baseline changed-path output | Looked for serialized execution, disabled verification, changed timeout, production code edits, contamination from the dirty worktree, and failures outside the target module | Target passed 1/1 and parallel module passed 126/126 aggregate; changed-path evidence lists only `network.rs`. The broad run's four SN failures are outside this helper and one reproduces alone; every QUIC test passed | pass |

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 networks::quic::network::tests::quic_tunnel_open_stream_without_listen_stays_pending --lib -- --exact`; six consecutive `cargo test -q -p p2p-frame --features x509 networks::quic::network::tests --lib -- --test-threads=21` runs; `git diff --check` on task paths
- Result: passed
- Exception reason: not-applicable; the broader x509 lib run was additionally attempted and its unrelated 4 SN failures are recorded in the change report rather than treated as targeted verification failures

## Findings

| ID | Severity | Evidence | Problem | Blocking |
|----|----------|----------|---------|----------|
| F-1 | none | Baseline-isolated implementation diff, port-range scan, and 126/126 parallel QUIC results | No defect found in the approved delivery; unrelated deterministic SN failures remain outside this task | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: the minimal test-only change eliminates the reproduced reuse-port identity crossover, retains the approved production and security boundaries, passes repeated parallel regression, and introduces no additional changed path beyond the scoped fixture file.
