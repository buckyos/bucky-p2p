# Wan/Mapped Rendezvous Reverse Connect Acceptance Report

Risk profile: ./risk-profile.yaml

## Findings
| ID | Severity | Owning Stage | Correctness Category | Evidence | Problem | Blocking |
|----|----------|--------------|----------------------|----------|---------|----------|
| F-044-001 closed | none | none | logic-and-control-flow | `TunnelManager::rendezvous_base_endpoints` now selects TCP or QUIC for `ReverseConnectOnly`, while punch-capable operations remain QUIC-only; the fresh task artifact passes the dedicated candidate and protocol cases | The first acceptance pass found that TCP Wan/Mapped endpoints were still filtered before request construction. The pipeline returned to design, fixed the transport policy, added regressions, and reran all task checks successfully. | no |

## Object and Scope
- Task manifest: task.yaml
- Module: p2p-frame
- Version: v0.1
- Task name: 044-wan-mapped-reverse-connect
- change_id values reviewed: wan_mapped_reverse_connect_eligibility, wan_mapped_rendezvous_protocol_validation, production_default_reverse_connect_regression_tests
- Review date: 2026-09-03
- Review mode: fresh independent falsification after the design return, implementation correction, and complete test rerun

## Requirement Coverage
| change_id | Requirement or Boundary | Source | Implementation Evidence | Finding | Status |
|-----------|-------------------------|--------|-------------------------|---------|--------|
| wan_mapped_reverse_connect_eligibility | Wan and Mapped endpoints are usable for reverse connect without broadening pure punch or prediction | `proposal.md` P-WMRC-1 | `endpoint.rs::rendezvous_reverse_connect_eligible_area`; operation-specific transport and area filtering in `tunnel_manager.rs::rendezvous_base_endpoints`; dedicated endpoint and candidate tests | ReverseConnectOnly accepts TCP/QUIC ServerReflexive, Wan, and Mapped; PunchOnly keeps the existing production area rule and remains QUIC-only. | pass |
| wan_mapped_rendezvous_protocol_validation | Request and notify validation accept Wan and Mapped for reverse operations while retaining protocol bounds | `proposal.md` P-WMRC-2 | `sn/protocol/sn.rs::validate_rendezvous_endpoints`; `tunnel_rendezvous_protocol.rs` positive and negative cases | ReverseConnectOnly accepts TCP/QUIC Wan/Mapped and PunchAndReverseConnect accepts QUIC Wan/Mapped; TCP punch, LAN, invalid address/port/count, duplicates, and mixed transport remain rejected. | pass |
| production_default_reverse_connect_regression_tests | Default-build tests expose production eligibility and real sockets cover the caller-public action | `proposal.md` P-WMRC-3 | dedicated default-feature unit files, protocol integration target, real strategy matrix, and fresh unified task artifact | Production-default tests cover Wan/Mapped and TCP/QUIC policy; the real-socket caller-public row reaches rendezvous action and direct payload over QUIC. | pass |

## Independent Defect Discovery
| Category | Applicable Scope | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|------------------|--------------------|-------------------|----------------------------------|--------|
| requirement-and-behavior | all three approved change items | proposal, corrected plan, endpoint helper, candidate builder, validator, fresh artifact | tested both public endpoint areas across every transport admitted by pure reverse connect | Wan and Mapped are now constructible and valid for TCP/QUIC reverse connect; no approved behavior remains missing. | pass |
| logic-and-control-flow | operation-specific area and transport branches | `rendezvous_base_endpoints`, `operation.punches`, `operation.reverse_connects`, validator branch | compared ReverseConnectOnly, PunchAndReverseConnect, and PunchOnly decisions | Pure reverse permits TCP/QUIC; any punching operation remains QUIC-only; area selection follows reverse capability without fallthrough. | pass |
| boundary-and-input | area, protocol, address, port, count, uniqueness domains | endpoint predicates and protocol integration target | combined Wan/Mapped with TCP/QUIC and retained invalid inputs | Positive equivalence classes pass and LAN, TCP punch, mixed transports, invalid address/port/count, and duplicate endpoints remain rejected. | pass |
| state-and-data-integrity | immutable endpoint snapshot and authenticated ownership | `reverse_endpoints_for_sn`, `rendezvous_endpoints_owned_by`, request cloning | checked whether broader areas bypass authenticated endpoint ownership or mutate shared state | SN still binds requested IPs to the authenticated peer's reported/descriptor endpoints; no state mutation was added. | pass |
| error-handling-and-recovery | candidate exhaustion and PN fallback | candidate construction and existing NotFound/fallback path | supplied only TCP Wan or Mapped to the pure reverse path | Valid TCP public candidates no longer become an empty request; genuinely ineligible inputs still take the existing bounded fallback. | pass |
| resource-lifetime-and-cleanup | reverse action owner, waiter, and socket tasks | unchanged owner-token and bounded action paths; real-socket test artifact | inspected changed paths for new allocation, task ownership, or cleanup behavior | The change consists of pure filtering/validation and tests; no resource owner or cleanup path changed. | pass |
| concurrency-and-ordering | request construction and existing rendezvous owner lifecycle | immutable filtering before owner installation; owner-token paths | checked for new shared mutation, stale completion, or ordering dependency | No shared state or lifecycle transition was added; existing owner-token ordering is unchanged. | pass |
| interface-and-compatibility | existing wire shape and rolling peer behavior | operation methods, validator, candidate builder, protocol tests | compared sender construction with receiver validation and considered old peers | Sender and current receiver now agree on TCP/QUIC pure reverse. Wire fields/version are unchanged; an older validator may reject Wan/Mapped and fall back, which is a rolling-upgrade limitation rather than corruption. | pass |
| security-and-capacity | authenticated ownership and endpoint budgets | `service.rs::rendezvous_endpoints_owned_by`, count/duplicate/address validation | reasoned through third-party IP injection and oversized/duplicate endpoint sets | Accepting Wan/Mapped does not bypass ownership, endpoint limits, address/port checks, or deduplication. | pass |
| test-adequacy | unit, integration, DV, and compile closure | testplan, dedicated tests, protocol target, strategy matrix, fresh artifact | repeated the first-pass TCP counterexample and checked pure-punch non-regression | TCP Wan/Mapped now has candidate and protocol coverage; the original production-feature mismatch and retained negative classes are directly asserted. | pass |

## Document Consistency
| Document | Source | Implementation Consistency | Finding | Status |
|----------|--------|----------------------------|---------|--------|
| proposal | `proposal.md` | Approved outcome and non-goals match the operation-specific implementation | No contradiction found. | pass |
| design | `pipeline/plan.md` | Corrected plan specifies TCP/QUIC for pure reverse and QUIC-only for punch-capable operations | The design return for F-044-001 is reflected in implementation and tests. | pass |
| testing | `testplan.yaml` | All declared commands ran through the unified task runner and produced the fresh successful artifact | Coverage and stage-scope checks pass; no stale test step remains. | pass |

## Result Summary
- Overall result: accepted
- Outcome: Production-default rendezvous now carries Wan and Mapped endpoints through candidate construction and SN request/notify validation for reverse connect. ReverseConnectOnly supports TCP and QUIC; punch-capable operations remain QUIC-only, and pure punch/prediction eligibility is unchanged.
- What was verified: endpoint policy, candidate construction, request and notify validation, retained negative bounds, caller-public real-socket action/direct payload, all-target compilation, and the correction of F-044-001.
- Evidence used: fresh artifact `.harness/test-results/test-runs/20260903T075042Z-p2p-frame+044-wan-mapped-reverse-connect-all.json`, implementation/testing stage-scope evidence, coverage checks, and task packet sources.
- Residual validation boundary: the real-socket matrix proves the QUIC caller-public production branch locally; TCP and Mapped are proven by default-feature unit/integration tests, not a deployed public-NAT or multi-SN environment.
- Blocking issues: none
- Next action: record accepted pipeline completion and remove 044 from unfinished-task bookkeeping.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: Every approved change_id has matching implementation and runnable evidence, the acceptance-return defect is closed, all defect-discovery categories pass, and no blocking finding remains.
