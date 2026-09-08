# Remove NatProfile.observed_endpoint and Live-Only Prediction Acceptance Report

## Findings

| ID | Severity | Owning Stage | Correctness Category | Evidence | Problem | Blocking |
|----|----------|--------------|----------------------|----------|---------|----------|
| F-000 | none | none | overall | `proposal.md`; `design.md`; `testing.md`; `testplan.yaml`; run `20260908T092424Z-p2p-frame+071-remove-nat-profile-observed-endpoint-all.json` | No approved requirement, design, implementation, or testing defect found in the live-only prediction and field-removal scope. | no |

## Object and Scope

- Task manifest: task.yaml
- Review date: 2026-09-08
- In-scope implementation: remove `NatProfile.observed_endpoint`; make live prediction retrieve its anchor from the same live probe result; fail closed when cached prediction would otherwise be used; migrate all public/wire/test consumers to the breaking field removal without compatibility shims.
- Review mode: independent falsification by the acceptance owner against current primary sources and the task-scoped run.

## Requirement Coverage

| change_id | Requirement or Boundary | Source | Implementation Evidence | Finding | Status |
|-----------|-------------------------|--------|-------------------------|---------|--------|
| CHG-remove-observed-endpoint | Delete `NatProfile.observed_endpoint` from public structure and wire; keep profile classification, hint, and time-based freshness | `proposal.md` P-001; `design/nat_type.md`, `design/sn-client.md`, `design/sn-service.md` | `nat_type.rs` has no field; `is_fresh` uses only `observed_at`/`valid_until`; all repo consumers compile; `consumer-closure-check.py` passed in task run | No approved behavior missing. Old wire compatibility is intentionally excluded per user confirmation. | pass |
| CHG-live-prediction-only | Prediction candidates must come from a live probe and cache must no longer expand predicted candidates | `proposal.md` P-002; `design/networks-quic.md`, `design/tunnel.md` | `QuicTunnelListener::predict_traversal_endpoints` passes the same-probe base; `nat_candidates(Predicted)` returns NotFound; fallback test asserts zero cached dial | The only direct fallback path for cached predicted ports now fails closed and routes to proxy/legacy/error as designed. | pass |

## Independent Defect Discovery

| Category | Applicable Scope | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|------------------|--------------------|-------------------|----------------------------------|--------|
| requirement-and-behavior | field deletion and live-only prediction requirement | proposal, design, current implementation, task run | challenged deleting only the struct without removing cached candidate use; tested fallback path | `nat_candidates(Predicted)` no longer expands cached profiles; live prediction keeps a same-probe base. | pass |
| logic-and-control-flow | `probe_nat_profile`, `predict_traversal_endpoints`, `execute_nat_action`, `nat_candidates` | listener.rs, tunnel_manager.rs | traversed every branch where observed endpoint was once consumed | NonSymmetricLike live prediction receives the live base; SymmetricLike uses `hint.last_observed`; fallback Predicted fails closed. | pass |
| boundary-and-input | two-sample probe, no hint, NonSymmetricLike, TTL expiry, listener generation | nat_type.rs, listener tests, tunnel tests | attempted empty/unknown hint, expired profiles, and invalid base flows | No hint returns NotFound; freshness no longer depends on endpoint presence; TTL still controls expiry. | pass |
| state-and-data-integrity | SN peer profile storage and report/query payload | nat_probe_scheduler.rs, peer_manager.rs, service.rs, sn_client.rs | checked whether any stored profile still carries the deleted wire value | `NatProfile` struct and derived wire no longer carry an observed endpoint; peer storage only keeps classification/hint/time. | pass |
| error-handling-and-recovery | prediction timeout, fallback failure, no PN | QUIC listener, tunnel manager tests | challenged whether fallback could silently reuse cached hint after failure | Fallback returns typed NotFound before proxy/legacy and is covered by `rendezvous_deterministic_failure_rejects_cached_predicted_without_proxy`. | pass |
| resource-lifetime-and-cleanup | no new state or task | `git diff` of `p2p-frame/src/nat_type.rs`, `p2p-frame/src/networks/quic/listener.rs`, `p2p-frame/src/tunnel/tunnel_manager.rs`; current source scan | looked for newly retained endpoint/cache/task fields | No new field, cache, task, socket, timer, or retained endpoint was added; live base is method-local. | pass |
| concurrency-and-ordering | same-probe anchor with listener generation | `predict_traversal_endpoints`, validation test | checked if stale listener could carry forward a base | `validate_traversal_prediction` still binds socket generation and TTL; live probe occurs before endpoint expansion. | pass |
| interface-and-compatibility | breaking public/wire API | design API impact, testplan contract checks, `nat_profile_public_api`, `tunnel_rendezvous_protocol` | verified old field references fail and new public API compiles | contract checks include external-positive, external-negative, removed-symbol scan, and repository compile closure; all passed. | pass |
| security-and-capacity | no auth/trust boundary change | diff and existing probe signer/token paths | checked whether removal weakens signed probe verification or endpoint ownership | Probe signer, token, source verification, and endpoint ownership logic are untouched. | pass |
| test-adequacy | changed branches and fallback behavior | `testing.md`, `testplan.yaml`, task run artifact | checked whether tests prove field removal, freshness, live prediction, and cache fallback closure | 505 lib tests plus public/protocol/logging suites pass through the unified task runner; public NAT/multi-host remains an explicit non-goal. | pass |

## Document Consistency

| Document | Source | Implementation Consistency | Finding | Status |
|----------|--------|----------------------------|---------|--------|
| design | `design.md`; `design/nat_type.md`, `design/networks-quic.md`, `design/tunnel.md`, `design/sn-client.md`, `design/sn-service.md`, `design/tests.md` | implementation matches documented field removal, live anchor, and fallback fail-closed behavior | no mismatch | pass |
| testing | `testing.md`; `testplan.yaml` | testplan steps match unit/DV/integration evidence and task run | no mismatch | pass |

## Result Summary

- Overall result: accepted
- Outcome: `NatProfile` no longer carries `observed_endpoint`; prediction only comes from live probes; cached predicted-port fallback is removed. Existing direct NonSymmetricLike paths continue through endpoint_array Base candidates.
- Blocking issues: none
- Next action: complete the acceptance lifecycle receipt and remove the task from the unfinished index.

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: The breaking field removal, live-only prediction requirement, freshness semantics, and fallback closure all met the approved scope and passed the unified task runner; public NAT/multi-host deployment remains outside the local evidence and is recorded as a non-goal.
