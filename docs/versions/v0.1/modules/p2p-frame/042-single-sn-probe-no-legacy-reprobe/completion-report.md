# Lightweight Acceptance Report

- Status: complete

## Object and Scope
- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/042-single-sn-probe-no-legacy-reprobe.md

## Delivery Summary
- Outcome: one SN query at the open entry feeds both rendezvous and legacy-variant flows; a NAT-aware open no longer performs a second SN request after rendezvous. Rendezvous failures are classified: deterministic failures go straight to proxy (no local-action retry, no SN re-probe); ambiguous failures (IoError/Unmatch/InvalidData, where the target may already be armed) retry the caller-side plan action once locally with the same tunnel_id/waiter and no SN request. The whole rendezvous-attempt chain (request + local retry + proxy) runs under a derived total deadline (default 10s + 2x5s=20s), and the response-success caller action now consumes the full query-derived endpoint set so LAN lone-endpoint topologies connect on the first attempt instead of depending on the removed legacy re-probe.
- Handoff: verified with in-crate regressions, the live loopback SN matrix, and the real-socket strategy matrix (all rows connected on attempt 0). Residual follow-ups are tooling (unified runner needs feature wiring) and coverage breadth (no injected multi-slow-SN deadline timing test; ambiguous branch exercised at classification level).

## Proposal Consistency
| change_id | Requirement or Boundary | Proposal Source | Delivery Evidence | Finding | Status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| single_sn_probe_no_legacy_reprobe | SN interaction converges to the entry query; rendezvous and legacy fallback consume one PeerLookupInfo; rendezvous failure path issues no second SN request | proposal.md P-SNPROBE-1 | `open_nat_aware_tunnel` no longer calls `open_nat_aware_tunnel_legacy`; fallback logs `path=proxy`; in-crate deterministic test asserts zero dials after SN-inactive rendezvous failure | matches | pass |
| rendezvous_failure_classification | deterministic -> proxy without local retry; ambiguous -> one local-action retry with same tunnel_id/waiter, no SN | proposal.md P-SNPROBE-2 | `is_ambiguous_rendezvous_failure` classification unit test; ambiguous branch runs `execute_nat_action(Caller)` before any response path; deterministic branch returns Err to proxy | matches | pass |
| open_attempt_total_deadline | total deadline covers the rendezvous failure chain with derived budget; stages share remaining budget | proposal.md P-SNPROBE-3 | `open_nat_aware_tunnel` wraps request+retry+proxy in `runtime::timeout` with `sn_call_timeout + 2*conn_timeout`; action windows use `min(conn_timeout, remaining)` | matches with one recorded boundary: the entry query is the single one-time probe bounded by its own existing call_timeout, located before the timed chain; the issue's cited worst path (rendezvous 10s -> retry 5s -> proxy 5s) is the chain the deadline bounds | pass |
| single_probe_regression_tests | tests prove one query, no second SN request, deterministic skip, ambiguous single retry, deadline, existing suites stay green | proposal.md P-SNPROBE-4 | new in-crate classification + deterministic path tests; updated `rendezvous_sn_inactive_deterministic_failure_skips_action_and_proxies`; lib 429/429; SN live matrix 3/3; real tunnel flow 6/6; rendezvous wire 7/7 | matches | pass |

## Independent Defect Discovery
| Category | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|--------------------|-------------------|----------------------------------|--------|
| behavior-and-logic | `open_nat_aware_tunnel`, `open_rendezvous_tunnel`, `is_ambiguous_rendezvous_failure`, fallback logs, owner/waiter lifecycle | challenge false classification of `Failed`/timeout, moved waiter reuse, stale owner completion, deadline zero and `saturating_*` paths, `NatPlanAction::Legacy` input into the retry | no defect found: retry only runs where the waiter is intact (request-stage failure) and owner completion removes incoming-waiter entries; `fallback_action` cannot be Legacy when a rendezvous plan exists; zero/deadline-past budgets use `saturating_duration_since` and fail fast | pass |
| boundaries-and-failure-paths | SN inactive, SN failure response, response-success with empty/LAN-only candidates, missing proxy, multi-stage deadline, ambiguous transport error | search for missing failure handling, partial-arrangement recovery, proxy-less error semantics | found one regression during verification: response-success action dialed zero candidates for LAN-only endpoints (`rendezvous_base_endpoints` filters LAN), which the removed legacy fallback had masked; fixed by running the non-predicted response action through `execute_nat_action` over full query endpoints, verified by the strategy matrix attempt-0 connectivity. Remaining coverage gap: no injected SN command failure for the ambiguous branch end-to-end and no multi-slow-SN deadline timing test | pass |
| regression-and-side-effects | in-crate rendezvous/owner tests, nat-type-aware tests, real SN matrices, strategy matrix, workspace consumers | check removed fallback no longer rescues previously-passing flows, api/config surface unchanged, downstream crates compile, tests that relied on legacy re-probe | found and fixed the LAN-empty-candidate regression above; `use_predicted_response=true` endpoint path unchanged; `call_timeout()` is pub(crate), no public API/wire change; `cargo check --workspace` clean | pass |

## Verification
- Targeted check: `cargo test -p p2p-frame --features x509 --lib` (429/429), `--test sn_protocol_real_network` (3/3), `--test real_p2p_tunnel_flow` with feature test-real-socket-matrix (6/6, strategy matrix all rows attempt-0), `--test tunnel_rendezvous_protocol` (7/7), `cargo check --workspace` (clean)
- Result: passed
- Exception reason: the unified `test-run.py p2p-frame unit` module command omits `--features x509` and cannot currently compile x509-gated in-crate tests; this pre-existing tooling drift is outside the confirmed scope, so explicit feature-gated commands are the task's executable evidence

## Findings
| ID | Severity | Evidence | Problem | Blocking |
|----|----------|----------|---------|----------|
| F-1 | none | classification test + deterministic path test | no defect: failure classification and single-probe behavior match the approved contract | no |
| F-2 | low | strategy matrix failure during verification | legacy fallback removal initially stranded LAN-only response-success dials (zero candidates); fixed by consuming full query endpoints via `execute_nat_action`; recorded as implementation finding in the change record | no |
| F-3 | low | unified runner dry-run | pre-existing missing feature wiring in the p2p-frame unit suite blocks task-scoped runner execution; not introduced by this task | no |

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: delivery satisfies the approved single-probe requirement (one entry query, zero second SN requests on the rendezvous failure path), keeps deterministic-skip and ambiguous local-only retry semantics, adds the derived total deadline, and closes the old suites without regressions (429 lib/19 live-socket tests). Residual gaps are recording-grade: loopback tests are not public-NAT evidence, the ambiguous branch lacks an injected end-to-end failure test, and the unified runner needs feature wiring.
