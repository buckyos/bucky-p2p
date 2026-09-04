# Bind Rendezvous Response Endpoints to the Target Owner Acceptance Report

## Findings

| ID | Severity | Owning Stage | Correctness Category | Evidence | Problem | Blocking |
|----|----------|--------------|----------------------|----------|---------|----------|
| F-000 | none | none | overall | `proposal.md`; `design.md`; `p2p-frame/src/sn/service/service.rs:822-904`; both dedicated regression files; task run `20260904T050049Z-p2p-frame+052-bind-rendezvous-response-owner-all.json` | No requirement, design, implementation, or testing defect found within the approved IP-ownership scope. | no |

## Object and Scope

- Task manifest: task.yaml
- Review date: 2026-09-04
- In-scope implementation: target serving-SN validation of predicted endpoint IPs against current authenticated command-tunnel observations, plus same-SN and cross-SN regression coverage.
- Review mode: independent falsification by the acceptance owner after rereading current primary sources; no separate reviewer process was available under the current execution constraints, and the conclusion was selected only after the category review below.

## Requirement Coverage

| change_id | Requirement or Boundary | Source | Implementation Evidence | Finding | Status |
|-----------|-------------------------|--------|-------------------------|---------|--------|
| CHG-bind-rendezvous-response-owner | Reject every predicted response IP not currently observed for authenticated target B at B's serving SN; allow predicted ports to differ; cover same-SN and inter-SN delivery without a wire change. | `proposal.md` P-001 and Success Criteria | `SnService::validate_rendezvous_response_owner`; call from `deliver_rendezvous_to_local_peer`; same-SN tests at `p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs`; service/inter-SN tests at `p2p-frame/tests/unit/sn_tests/service/distributed_nat_profile_tests.rs` | No missing approved behavior. The stronger same-public-IP/per-port proof remains explicitly out of scope. | pass |

## Independent Defect Discovery

| Category | Applicable Scope | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|------------------|--------------------|-------------------|----------------------------------|--------|
| requirement-and-behavior | approved arbitrary-third-party-IP prevention and stated non-goals | proposal Scope, Requirement Review, P-001, Success Criteria; design Key Flows | challenged whether exact port equality, self-reported endpoints, or initiator-SN validation would satisfy ownership | IP-only matching at the target serving SN is the approved minimum; port equality would break prediction, and report/certificate assertions are intentionally excluded. | pass |
| logic-and-control-flow | empty prediction, owned list, mixed list, same-SN and cross-SN paths | `validate_rendezvous_response_owner`, `deliver_rendezvous_to_local_peer`, `relay_rendezvous_from_sn`, `relay_rendezvous_to_serving_sn`, `process_rendezvous_request` | traced every early return and whether any response can bypass local target delivery | Empty responses preserve no-prediction behavior; every non-empty target response is checked before local return or inter-SN completion; any unowned member rejects the entire list. | pass |
| boundary-and-input | target-controlled endpoint arrays and live observation set | `SnTunnelRendezvousResp::validate`; `get_peer_observed_ep`; ownership helper; same-SN mixed-list test | challenged empty observations, mixed owned/unowned entries, different ports, non-IPv4/LAN/duplicate/oversize input | Existing structural validation rejects invalid shape/domain before ownership; contextual validation rejects empty trusted sets and mixed lists while accepting same-IP/different-port prediction. | pass |
| state-and-data-integrity | live tunnel registry and response replay cache | `get_peer_observed_ep`; `RendezvousState::begin`, `cache_response`, `remove_peer` | checked stale cached predictions, disconnect races, and whether target report cache can seed trusted ownership | The helper reads the live command-tunnel registry and never reads `peer_mgr`; prediction-bearing requests cache a failure rather than the endpoint response, preventing predicted endpoint replay from the attempt cache. | pass |
| error-handling-and-recovery | permission failure through local and inter-SN orchestration | helper error, delivery `?`, inter-SN relay loop, `process_rendezvous_request`, caller client response handling | checked whether an error could still return endpoints or be treated as success | `PermissionDenied` prevents the response from leaving B's serving SN; outer orchestration returns a generic failed rendezvous with no endpoints, retaining existing higher-level fallback behavior. | pass |
| resource-lifetime-and-cleanup | snapshot allocation and async command response | helper-local `HashSet`; existing command tunnel futures | looked for new retained state, task, waiter, lock, socket, or cleanup path | Not applicable: the implementation adds only a bounded local set and no persistent allocation, owner, background task, socket, timer, or cleanup obligation. | not-applicable |
| concurrency-and-ordering | response arrival followed by live observation lookup | `deliver_rendezvous_to_local_peer` ordering; `get_peer_observed_ep` tunnel snapshot | challenged target disconnect/removal during validation and response-before-check ordering | Validation occurs after authenticated response receipt and before return. Concurrent disappearance can only cause a safe rejection or leave the already authenticated response tied to the captured live tunnel object; no shared state is mutated. | pass |
| interface-and-compatibility | response wire and caller semantics | protocol response fields/validator; design API impact; call sites in service and tunnel manager | checked for signature, encoding, result-code, feature, and downstream call changes | No wire or public API changes. Successful owned predictions retain the same response, and rejected responses use the established failure path. | pass |
| security-and-capacity | reflection/scan target control and work bounds | ownership helper, structural endpoint cap, authenticated peer tunnel lookup | attempted self-report seeding, one-valid-plus-one-malicious masking, cross-SN bypass, and unbounded endpoint work | Self-reported cache data is ignored, any unowned IP fails the list, the serving-SN relay entry is covered, and work remains bounded by the existing maximum of eight response endpoints plus the active tunnel set. Same-public-IP port ownership remains the documented non-goal. | pass |
| test-adequacy | changed branches and affected runtime boundaries | red failure output; `testing.md`; `testplan.yaml`; two test files; run artifact `20260904T050049Z-p2p-frame+052-bind-rendezvous-response-owner-all.json` | checked whether tests distinguish IP from socket-address equality, bypass via report cache, partial list acceptance, and cross-SN placement | The selected task run covers empty/no-observation, same-IP/different-port success, mixed-list rejection, and cross-SN serving-boundary rejection. It does not claim deployed public NAT or per-port proof. | pass |

## Document Consistency

| Document | Source | Implementation Consistency | Finding | Status |
|----------|--------|----------------------------|---------|--------|
| design | `design.md`; `design/sn-service.md` | private service helper and unified target delivery call match the documented enforcement point and preserve protocol/tunnel boundaries | no mismatch | pass |
| testing | `testing.md`; `testplan.yaml` | planned unit, DV, and integration steps match the implemented dedicated tests and successful task-scoped run | no mismatch | pass |

## Result Summary

- Overall result: accepted
- Outcome: The target serving SN now prevents a rendezvous target from steering prediction traffic to an unobserved third-party IP, while preserving legitimate predicted port variation and the existing wire contract.
- Blocking issues: none
- Next action: complete the acceptance lifecycle receipt and remove the task from the unfinished index.

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: Direct inspection and adversarial tests cover the approved ownership boundary, both routing placements, failure behavior, and relevant cache/lifecycle interactions without an unresolved blocking finding.
