# Lightweight Acceptance Report

## Object and Scope
- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/061-symmetric-profile-without-prediction.md

## Delivery Summary
- Outcome: SN-probe NAT classification no longer degrades to Unknown for unpredictable symmetric mappings. `QuicTunnelListener` now exposes `probe_nat_profile` (probe + classify, errors only on real probe failures), `predict_traversal_endpoints` builds candidate endpoints on top of it and keeps its existing `NotFound` behavior when no candidates exist, `UdpTunnelNetwork` gained the required `probe_nat_profile` method (implemented by `QuicTunnelNetwork` and stubbed in all three in-repo test mocks), and `SNService::probe_endpoints` uses the profile-only API. New listener-level regression tests reproduce both issue patterns: ports 40000/40003/40007 keep `SymmetricLike` with no hint while prediction still errors; ports 40000/40003/40006 keep a usable hint and 8 predicted candidates.
- Handoff: delivered behavior matches the approved proposal; a `SymmetricLike`-without-hint peer now feeds `select_connect_plan`'s existing `BoundedBestEffort` branch instead of the legacy plan. Residual follow-up: none. The working tree also carries the separate in-flight task 060 changes that predate this task and are captured in the pre-edit baseline; they are not part of this task's delivery.

## Proposal Consistency
| change_id | Requirement or Boundary | Proposal Source | Delivery Evidence | Finding | Status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-symmetric-profile-without-prediction | Unpredictable symmetric mapping keeps its SymmetricLike classification in the client probe path instead of degrading to Unknown | proposal.md Proposal Items P-001 | p2p-frame/src/networks/quic/listener.rs `probe_nat_profile` + sn_service.rs `probe_endpoints` now call the profile-only API; regression test `nat_profile_probe_keeps_symmetric_classification_without_prediction_hint` asserts Ok(SymmetricLike), hint None, observed port 40007 | matches | pass |
| CHG-symmetric-profile-without-prediction | Rendezvous prediction API keeps its existing candidate requirement and errors when none exist | proposal.md Proposal Items P-001 boundary | p2p-frame/src/networks/quic/listener.rs `predict_traversal_endpoints` still returns NotFound "listener NAT prediction is unavailable" for empty candidates; regression test asserts NotFound for the same 40000/40003/40007 observation, and 40000/40003/40006 still yields 8 predicted ports | matches | pass |

## Independent Defect Discovery
| Category | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|--------------------|-------------------|----------------------------------|--------|
| behavior-and-logic | p2p-frame/src/networks/quic/listener.rs probe_nat_profile/predict_traversal_endpoints split, sn_service.rs probe_endpoints call, nat_type.rs from_observations/predicted_ports freshness math | Searched for lost validation, changed error semantics, and stale timestamp misuse: confirmed param validation, closed/timeout/IPv4/no-observed-endpoint errors are unchanged; `profile.observed_at` equals the original local `observed_at` passed to from_observations so hint freshness math is identical; found and fixed the None-branch error text still saying "traversal prediction" (now "UDP NAT profile probing") | concrete finding fixed during this pass: stale error text in sn_service.rs probe_endpoints None branch corrected; no behavioral defect remains | pass |
| boundaries-and-failure-paths | nat_probe_scheduler.rs result acceptance (`observation == Unknown or is_fresh`), nat_connect_plan.rs select_connect_plan Unknown/SymmetricLike branches, mock NotSupport stubs | Challenged failure handling: real probe failures (timeout/closed/interrupted) still produce Err so the client still reports Unknown when nothing was observed; SN scheduler accepts fresh SymmetricLike without re-probe loops; SymmetricLike-without-hint now selects BoundedBestEffort with Base candidates via existing `prediction_or_base(false)`; prediction-mode rendezvous failures keep the pre-existing fallback path | no defect found | pass |
| regression-and-side-effects | All in-repo `UdpTunnelNetwork` implementers (grep: QuicTunnelNetwork + 3 test mocks), tunnel_manager.rs rendezvous callers at lines 1188/1475/1709, public API check scripts (udp_tunnel_network_api_check.py updated), full p2p-frame lib + integration suites, workspace check with x509 | Searched for compatibility regressions and stale consumers: trait gained a required method so every implementer was updated in the same change; rendezvous prediction callers untouched; wire encoding of NatProfile unchanged; repo-wide fmt/clippy drift and warnings verified pre-existing (unrelated files, task 051 precedent); API contract scripts pass in positive and negative modes | no defect found | pass |

## Verification
- Targeted check: `cargo test -p p2p-frame --features x509 --lib` (487 passed, including the two new `nat_profile_probe_*` regression tests); `cargo test -p p2p-frame --features x509 --tests` (all integration targets passed); `cargo check --workspace --all-targets --features p2p-frame/x509`; `python3 p2p-frame/tests/udp_tunnel_network_api_check.py --mode positive|negative`, `nat_probe_ports_api_check.py --mode positive|negative`, `signed_pnat_api_check.py --mode positive|negative`.
- Result: passed
- Exception reason: not-applicable

## Findings
| ID | Severity | Evidence | Problem | Blocking |
|----|----------|----------|---------|----------|
| F-1 | low | sn_service.rs probe_endpoints None branch before this pass | error text referenced "traversal prediction" although the call had become profile probing; corrected to "UDP NAT profile probing" and re-verified with the full lib suite | no |

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: delivery satisfies the approved proposal P-001 end to end: the unpredictable symmetric sample (40000/40003/40007) keeps its SymmetricLike classification through the SN-probe path (new regression test), the prediction API retains its candidate requirement and error semantics (same test), all proposal-scope boundaries held (no wire/scheduler/plan changes, rendezvous unchanged), and targeted verification plus full suites pass with no blocking findings.
