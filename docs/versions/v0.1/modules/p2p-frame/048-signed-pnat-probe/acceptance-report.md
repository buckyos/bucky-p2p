# Signed PNAT Probe Acceptance Report

## Findings
| ID | Severity | Owning Stage | Correctness Category | Evidence | Problem | Blocking |
|----|----------|--------------|----------------------|----------|---------|----------|
| F-048-A3-000 | none | none | overall | Approved P-001; current signed codec, SN trust path, shared signer budget, owner-bound listener lifecycle, migrated consumers, focused security tests, production state refresh, feature-gated real strategy matrix, and `20260903T190209Z-p2p-frame+048-signed-pnat-probe-all.json` | Fresh iteration-three falsification found no remaining requirement, design, implementation, or testing defect in the approved scope. | no |

## Prior Finding Closure
| Prior ID | Closure Evidence | Status |
|----------|------------------|--------|
| F-048-A1-001 | `nat_probe.rs:103-175` admits against one rolling 128/s and four-in-flight context before blocking-pool signing; `service.rs:1936-1947` shares it across reflector sockets; `listener.rs:183-211,319-369` verifies outside the waiter lock, rechecks ownership, and yields after four auxiliary packets. Capacity, timer, race, and poll-fairness tests pass in the latest artifact. | closed |
| F-048-A1-002 | `listener.rs:109-180` gives the prediction receiver an `Arc`-identified Drop guard and uses pointer identity for cleanup. Outer-future cancellation, replacement-owner ABA, and replay tests pass. | closed |
| F-048-A1-003 | `nat_probe.rs:139-165,402-424` signs bytes 0 through 31, including `signature_len`. Empty, oversized, drift, nonzero-padding, short/enlarged, and trailing-tolerant verifier vectors all fail closed. | closed |
| F-048-A1-004 | `periodic_report_update_clears_and_restores_live_active_sn_signer` drives a real SN/client through the production periodic report assignment and observes the live snapshot clear for malformed, wrong-ID, and self-invalid certificates, then restore for a valid certificate. All other original security/lifecycle gaps have direct tests in the fresh artifact. | closed |
| F-048-A2-001 | `strategy_matrix.rs` now accepts fixed 1200-byte v2 requests and signs v2 responses with the same `sn_identity` published by the real topology. The latest runner compiles `x509,test-real-socket-matrix` and the six-row production strategy matrix exits successfully within its bounded retry policy. | closed |

## Object and Scope
- Task manifest: task.yaml
- Review date: 2026-09-04
- In-scope implementation: signed-only PNAT v2 codec, SN signer publication/validation, reflector resource bounds, QUIC waiter verification/lifecycle, tunnel prediction, repository consumer migration, and task-scoped tests
- Review mode: third fresh independent read-only falsification after two acceptance-return cycles; the reviewer did not implement or test the change

## Requirement Coverage
| change_id | Requirement or Boundary | Source | Implementation Evidence | Finding | Status |
|-----------|-------------------------|--------|-------------------------|---------|--------|
| CHG-signed-pnat-probe | Accept only fixed-size v2 responses signed by the expected active SN; authenticate all fields including signature length; fail closed; bound unauthenticated work; clean every waiter exit; migrate all consumers; provide current real-path evidence; retain no v1 compatibility | approved `proposal.md` and `pipeline/plan.md` | `nat_probe.rs`, `service.rs`, `sn_service.rs`, `listener.rs`, `tunnel_manager.rs`, migrated matrix fixture, testplan, and `20260903T190209Z-p2p-frame+048-signed-pnat-probe-all.json` | No missing behavior or scope boundary found | pass |

## Independent Defect Discovery
| Category | Applicable Scope | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|------------------|--------------------|-------------------|----------------------------------|--------|
| requirement-and-behavior | signed-only PNAT and expected active-SN trust | proposal, plan, codec, service, client, listener, tunnel, and tests | traced valid, malformed, v1, wrong source/signer, timeout, cancellation, report refresh, and fallback branches | Production accepts only expected-SN signed v2 and preserves fail-closed fallback behavior | pass |
| logic-and-control-flow | request decode, admission, response encode, verification, and waiter completion | `nat_probe.rs:103-175,265-315,361-451`; `listener.rs:183-211,684-731` | enumerated every header/token/source/signature/owner branch and the valid completion path | Admission precedes private work; only the same live owner completes after verification | pass |
| boundary-and-input | packet/version/kind/reserved/padding, address/port, signature length, and certificate identity | codec validators, client trust validator, and boundary tests | exercised v1, wrong sizes, zero/oversized/drifting/trailing signatures, tampering, malformed certs, and wrong IDs | Fixed v2 format and all trust/length boundaries fail closed | pass |
| state-and-data-integrity | atomic active-SN endpoint/certificate snapshot and pending-token ownership | `sn_service.rs:576-631,1068-1082,1214-1225`; `listener.rs:95-217` | challenged stale signer, report invalidation/restoration, replacement owner, replay, and verification races | Snapshot updates clear and restore trust; stale work cannot consume or remove a replacement waiter | pass |
| error-handling-and-recovery | signing failure/budget, missing trust, invalid response, timeout, cancellation, and tunnel fallback | reflector loop, report handler, prediction future, and TunnelManager caller | dropped the enclosing future and injected signing/trust/packet failures | Budget drops do not amplify logs; actual errors warn; prediction returns Unknown/fallback without trusting invalid input | pass |
| resource-lifetime-and-cleanup | signing permit/context, waiter sender/certificate, listener and reflector tasks | signing permit lifetime, service shared context, Drop guard, and listener close paths | cancelled outer work, held blocking signing, replaced tokens, completed tokens, and listener shutdown | Private work stays counted until it exits; every waiter lifecycle releases only its exact resources | pass |
| concurrency-and-ordering | aggregate signing, blocking pool, waiter lock, owner recheck, and Quinn poll fairness | codec/service/listener concurrency paths and deterministic tests | saturated rate/in-flight capacity, blocked a signer, replaced during verification, and queued more than four auxiliary datagrams | One shared context bounds all service sockets; crypto does not hold the waiter lock; bounded polling self-wakes | pass |
| interface-and-compatibility | breaking reflector/prediction APIs and strict v1 removal across feature-gated consumers | public signatures, repository search, API fixtures, matrix fixture, all-target build | compiled new external calls, required old calls to fail E0061, searched consumers, and ran both-feature matrix | All consumers are migrated; there is no v1 decoder, unsigned fallback, negotiation, or shim | pass |
| security-and-capacity | authenticity, replay/correlation, anti-amplification, spoof-driven signing, verifier CPU, and lock contention | signature preimage, report trust anchor, shared limits, listener dispatch, tests | altered every signed field/length, replayed completed tokens, flooded admission and poll boundaries | Domain and signer ID bind the full response fields; equal 1200-byte request/response prevents byte amplification; work is bounded | pass |
| test-adequacy | unit, DV, integration, API closure, state lifecycle, feature matrix, and run provenance | test sources, `testplan.yaml`, latest task-run JSON | mapped every proposal/design transition and prior finding to mutation-sensitive assertions and verified recorded commands/exit codes | The latest artifact contains all required x509-enabled steps, including production periodic refresh and the opt-in real matrix; no prohibited cyfs-p2p-test evidence is used | pass |

## Document Consistency
| Document | Source | Implementation Consistency | Finding | Status |
|----------|--------|----------------------------|---------|--------|
| design | `pipeline/plan.md` | Authenticated signature length, expected-SN report trust, shared budget, blocking signing, owner-bound cleanup, unlocked verification, bounded polling, and strict cutover match current code | No inconsistency found | pass |
| testing | `testplan.yaml` | Focused security/lifecycle steps, stateful report refresh, both-feature compilation, and six-row real matrix match the current tests and latest artifact | No missing declared evidence or unsupported claim found | pass |

## Result Summary
- Overall result: accepted
- Outcome: P-001 is satisfied by a strict identity-signed PNAT v2 protocol, authenticated expected-SN trust delivery, bounded computation, cancellation-safe waiter ownership, complete consumer migration, and fresh real-path validation.
- Blocking issues: none
- Next action: parent orchestrator may complete the auto-pipeline lifecycle and remove the task from the unfinished index.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: fresh counterexample review closed all five prior findings and found the implementation, compatibility cutover, lifecycle behavior, capacity controls, tests, and task evidence consistent with the approved proposal.
