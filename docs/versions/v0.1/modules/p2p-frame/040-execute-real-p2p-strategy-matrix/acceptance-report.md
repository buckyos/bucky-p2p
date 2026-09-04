# Execute Real P2P Strategy Matrix Acceptance Report

## Findings
| ID | Severity | Owning Stage | Correctness Category | Evidence | Problem | Blocking |
|----|----------|--------------|----------------------|----------|---------|----------|
| F-040-001 closed | none | none | test-adequacy | Current six-row matrix source in `p2p-frame/tests/real_p2p_tunnel_flow/strategy_matrix.rs` asserts `event=sn_rendezvous_requesting`, `event=sn_rendezvous_action_armed`, `event=sn_rendezvous_target_finished`, connected direction, and unique payload per row; fresh task artifact `20260902T090811Z-...-all.json` exit 0 | The original hardcoded `not-observable` matrix cover was replaced by real production-branch execution; no blocking finding remains for this P1 item. | no |
| F-040-002 closed | none | none | logic-and-control-flow | Feature-gated seams at `p2p-frame/src/endpoint.rs`, `tunnel_manager.rs`, `sn/protocol/sn.rs`, `networks/quic/listener.rs`, and `sn/service/service.rs` keep default-build behavior unchanged (default `cargo check -p p2p-frame` passed) while the feature build reaches rendezvous request/action for all six rows | Target-side prediction initially could not run because Report never advertised probe endpoints; the test-mode Report backfill is feature-gated and the full matrix now passes. | no |

## Object and Scope
- Task manifest: task.yaml
- Module: p2p-frame
- Version: v0.1
- Task name: 040-execute-real-p2p-strategy-matrix
- change_id values reviewed: CHG-040-nat-matrix-test-seam, CHG-040-nat-matrix-fixture, CHG-040-nat-matrix-execution
- Review date: 2026-09-02
- Review mode: fresh independent falsification after implementation and testing; prior task findings and the previous fake matrix conclusion were discarded before choosing the result.

## Requirement Coverage
| change_id | Requirement or Boundary | Source | Implementation Evidence | Finding | Status |
|-----------|-------------------------|--------|-------------------------|---------|--------|
| CHG-040-nat-matrix-test-seam | Feature-gated loopback/private eligibility and SN observation/report prediction seams without changing default builds | `proposal.md` P-040-1 and `pipeline/plan.md` seam binding | `p2p-frame/Cargo.toml` feature; `endpoint.rs` `rendezvous_ipv4_eligible`/`rendezvous_eligible_area`; four predicate consumers; `sn/service/service.rs` loopback observation and test-mode Report probe endpoint backfill; default `cargo check -p p2p-frame` passes | Implemented and default-parity verified; with the feature enabled all six rows complete request/action and reachable payload. | pass |
| CHG-040-nat-matrix-fixture | One logical internal source socket per node with per-destination Stable/Changed mapping through real probe reflectors and the production Report/Query chain | `proposal.md` P-040-2 and `pipeline/plan.md` fixture binding | `strategy_matrix.rs` MappingProbeFixture/reflector loops; SN `set_nat_probe_endpoints`; profile readiness waits on `query_with_context`; matrix shape assertions verify SymmetricLike hint and per-target port delta | Real profile chain produces the expected observations; SymmetricLike uses a single source socket with changed ports across two reflector targets. | pass |
| CHG-040-nat-matrix-execution | All six matrix rows execute the production NatConnectPlan, rendezvous request, target action, and (for reachable rows) connected payload | `proposal.md` P-040-3 and `pipeline/plan.md` matrix binding | `strategy_matrix::real_strategy_matrix_executes_production_branches` asserts operation/predict/endpoint-count, request-sent, action-armed, target completion, `ConnectDirection`, and unique payload; task run artifact exit 0 | Six rows (callee-public, caller-public, non/non, non/sym, sym/non, sym/sym) all record production request/action and connected evidence. | pass |

## Independent Defect Discovery
| Category | Applicable Scope | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|------------------|--------------------|-------------------|----------------------------------|--------|
| requirement-and-behavior | P-040-1..3 acceptance boundaries and the original P1 complaint | Current proposal/plan, seam sources, reflector fixture, matrix assertions, fresh task artifact | Reconstructed every row's expected operation/predict/flow and checked that no row can pass without production request/action events | Every change_id is delivered as required; the fake hardcoded matrix and two-source-socket SymmetricLike evidence are gone. | pass |
| logic-and-control-flow | Predicate branching, dedup ordering, report backfill, rendezvous action flow | `endpoint.rs`, `tunnel_manager.rs`, `sn/protocol/sn.rs`, `networks/quic/listener.rs`, `sn/service/service.rs`, matrix event assertions | Switched to feature-disabled builds; traced NotFound before the Report backfill fix and verified the seam restores base candidates and prediction | No wrong-branch or fallthrough defect remains; default and feature builds behave as designed. | pass |
| boundary-and-input | Loopback/private eligibility, endpoint count 0 vs 1, predict true/false, port windows, event timeouts | Feature predicates, matrix `expect_zero_request_endpoints`/`expect_predict` assertions, reflector port-overflow guard, 8-second event waits | Tried endpoint-count mismatch and predict-flag mismatch; tested overflow window rejection; checked absent event fails the row | All six row boundaries assert real production values; symmetric port window was valid in every run. | pass |
| state-and-data-integrity | Profile storage/query freshness, connection-info publishing, rendezvous owner completion | `wait_for_profiles`, `query_with_context` checks, `ConnectionInfoRecorder`, event traces | Checked profiles must match expected observation before flow; connected rows require `ConnectDirection` and payload; stale/missing profile paths remain legacy | No stale-profile or ownership leak observed; profile evidence is fresh and production-query-sourced. | pass |
| error-handling-and-recovery | Rendezvous fallback, bounded action failure, event waits, retry/exhaustion | Trace `sn_rendezvous_fallback`, matrix row `connected` handling, `SETUP_MAX_RETRIES`, absolute deadlines | Considered missing action events and early local rejection; rows now fail with explicit evidence rather than `not-observable` | Bounded fallback/error paths are observable and fail closed; no swallowed error remains in matrix evidence. | pass |
| resource-lifetime-and-cleanup | Reflector tasks, sockets, stacks, SN server, profile wait loops | `MappingProbeFixture::drop`, `NatMatrixTopology::drop`, `stop_partial`, deadline rechecks | Reran the suite serially and the full binary; no port leak, leaked task, or dangling listener appeared | tasks/sockets are RAII-aborted/stopped; repeated runs stay clean. | pass |
| concurrency-and-ordering | Serial matrix execution, log correlation, background probe/report timing | `--test-threads=1`, per-case trace clearing, bounded polling, `wait_for_profiles` | Ran matrix standalone and as part of the 5-test DV set; no cross-case trace contamination or order dependency found | Concurrency is handled by serial scope and correlation waits; no race surface was introduced. | pass |
| interface-and-compatibility | Public API/build surface, default builds, existing fallback/collision consumers | Cargo feature list, default compile, full DV suite (5 passed), `test-run.py` task scope | Verified default feature set unchanged; existing real-socket tests pass under the feature build | Additive opt-in feature is backward-compatible; no consumer migration is required (`verified-none`). | pass |
| security-and-capacity | External input, amplification, unbounded work, secret exposure | Reflector fixed 32-byte PNAT scheme, port caps, bounded endpoint budgets, loopback-only sockets | Inspected reflector response size/token handling and event/relay bounds | No secret, authorization, amplification, or unbounded-allocation surface was introduced. | pass |
| test-adequacy | Normal/boundary/negative/error/lifecycle/compatibility/cross-module behavior for all three change_ids | `testplan.yaml` unit/dv/integration/contract steps, state case-type coverage, fresh task artifact, direct cargo runs | Re-read every row assertion and attempted to weaken request/action evidence (e.g. allow absent events); such rows now fail | Tests can expose damaged NatConnectPlan, prediction, or rendezvous branches; the P1 fake-coverage defect is closed. | pass |

## Document Consistency
| Document | Source | Implementation Consistency | Finding | Status |
|----------|--------|---------------------------|---------|--------|
| proposal | `docs/versions/v0.1/modules/p2p-frame/040-execute-real-p2p-strategy-matrix/proposal.md` | Proposal boundaries match delivered seam, fixture, and matrix behavior; build seam and probe-report backfill are recorded in scope | No contradiction found with the delivered behavior. | pass |
| design | `docs/versions/v0.1/modules/p2p-frame/040-execute-real-p2p-strategy-matrix/pipeline/plan.md` | Plan file sequence, exported interfaces, failure flows, and scope bindings match all edited files | Plan remains valid and reflects service.rs prediction support. | pass |
| testing | `docs/versions/v0.1/modules/p2p-frame/040-execute-real-p2p-strategy-matrix/testplan.yaml` | Testplan steps exactly matched the commands executed in the fresh task artifact; coverage check passed | No stale or unreachable step remains. | pass |

## Result Summary
- Overall result: accepted
- Outcome: The P1 fake-matrix finding is closed: all six production strategy rows now execute the real NatConnectPlan and SN rendezvous/action branches with observable request-sent/action-armed/connected evidence, and SymmetricLike evidence comes from one logical source socket with per-destination mapping changes through real reflectors.
- What was verified: six matrix rows, two fallback cases, two same-SN collision cases, default-build parity, task-scoped runner artifact, coverage/lifecycle/plan checks.
- Evidence used: fresh artifact `.harness/test-results/test-runs/20260902T090811Z-p2p-frame+040-execute-real-p2p-strategy-matrix-all.json`, implementation-admission stamp, testing coverage rows, and all task packet sources.
- Blocking issues: none
- Next action: record accepted completion and remove 040 from unfinished-task bookkeeping.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: Every task change_id is covered by concrete implementation and runnable evidence; all independent defect-discovery categories are pass or task-specific not-applicable; no blocking finding remains.
