# Isolate UDP Tunnel Traversal Acceptance Report

## Findings
| ID | Severity | Owning Stage | Correctness Category | Evidence | Problem | Blocking |
|----|----------|--------------|----------------------|----------|---------|----------|
| F-050-A1-000 | none | none | overall | Approved P-001; `TunnelNetwork` and `UdpTunnelNetwork` declarations; QUIC, SN, and `TunnelManager` call paths; capability/API tests; migrated legacy fixtures; and `20260904T034147Z-p2p-frame+050-isolate-udp-tunnel-traversal-all.json` | The fresh post-return falsification found no remaining requirement, design, implementation, or testing defect in the approved scope. | no |

## Prior Finding Closure
| Prior ID | Closure Evidence | Status |
|----------|------------------|--------|
| F-050-A1-001 | `p2p-frame/tests/signed_pnat_api_check.py` now accepts `&dyn UdpTunnelNetwork` in both the current signed call and legacy unsigned-signature fixture. Both modes are registered in `testplan.yaml` and exit successfully in the fresh all-level artifact. | closed |
| F-050-A1-002 | `p2p-frame/tests/unit/networks/network/punch_only_default_tests.rs` now asserts the default accessor returns `None` instead of invoking removed generic behavior; `network.rs` loads it under the capability suite, whose three tests pass in the fresh artifact. | closed |

## Object and Scope
- Task manifest: task.yaml
- Review date: 2026-09-04
- In-scope implementation: public generic/UDP trait boundary, QUIC capability opt-in, SN prediction, tunnel prediction/validation/punch callers, test doubles, API fixtures, and module boundary documentation
- Review mode: separate post-testing acceptance pass using primary sources first; an independent reviewer was unavailable in this execution environment, so the acceptance owner ignored prior conclusions, generated counterexamples, and inspected the concrete evidence recorded below before selecting the result

## Requirement Coverage
| change_id | Requirement or Boundary | Source | Implementation Evidence | Finding | Status |
|-----------|-------------------------|--------|-------------------------|---------|--------|
| CHG-isolate-udp-tunnel-traversal | Remove `punch_only`, `predict_traversal_endpoints`, and `validate_traversal_prediction` from `TunnelNetwork`; define them on `UdpTunnelNetwork: TunnelNetwork`; add default-`None` object-safe capability discovery; make QUIC return the same instance; migrate callers without runtime or wire changes | approved `proposal.md` P-001 and `pipeline/plan.md` acceptance baseline | `p2p-frame/src/networks/network.rs::TunnelNetwork`, `udp_network.rs::UdpTunnelNetwork`, `quic/network.rs` trait implementations, `sn_service.rs`, `tunnel_manager.rs`, migrated tests, and fresh task artifact | No missing requested behavior or boundary remains | pass |

## Independent Defect Discovery
| Category | Applicable Scope | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|------------------|--------------------|-------------------|----------------------------------|--------|
| requirement-and-behavior | generic versus UDP-specific tunnel capabilities and behavior preservation | approved proposal, pipeline acceptance baseline, both trait declarations, QUIC implementation, SN/tunnel callers | searched for any of the three methods still declared on `TunnelNetwork`, accidental PN/TCP opt-in, missing QUIC opt-in, and algorithm changes hidden in the move | The generic trait contains only the optional accessor; the UDP supertrait owns exactly the three requested methods; only QUIC opts in; method bodies retain the existing listener operations | pass |
| logic-and-control-flow | capability discovery before prediction, validation, and punch execution | `sn_service.rs::handle_nat_probe_directive`; `TunnelManager::predict_owned_rendezvous_endpoints`, rendezvous request/notify validation, and `punch_candidates` | followed `Some` and `None` branches and checked whether any call can bypass discovery or invoke a different network instance | Every generic-network call requires `as_udp_tunnel_network()`; prediction/validation return `NotSupport` when absent, while punch candidates stop and log as the prior default-error path did | pass |
| boundary-and-input | optional capability boundary and existing traversal arguments | default accessor, QUIC accessor, UDP method signatures, prediction validation tests | challenged a generic non-UDP implementation, a capable implementation, expired prediction, closed generation/listener, and missing capability | Default absence and same-object opt-in are asserted; signer, timeout, TTL, generation, and timestamp values pass unchanged to existing QUIC listener validation | pass |
| state-and-data-integrity | network registry identity and prediction generation ownership | `NetManager` generic lookup usage, borrowed accessor, QUIC `Some(self)`, same-pointer unit assertion | looked for a second UDP registry, copied adapter, independently owned capability, or changed prediction state | No registry or stored state was added; the capability borrow is tied to the selected generic network and QUIC returns the same object | pass |
| error-handling-and-recovery | unsupported capability plus existing prediction/punch failures | SN and tunnel `ok_or_else` branches, QUIC traversal bodies, punch candidate logging path | forced conceptual `None`, listener-not-found, expiry, timeout, and punch error paths and compared prior fallback behavior | Required prediction/validation paths surface `NotSupport`; candidate punch remains best-effort; downstream QUIC errors and rendezvous recovery remain unchanged | pass |
| resource-lifetime-and-cleanup | borrowed trait object across async calls and existing listener/punch lifecycle | `p2p-frame/src/networks/network.rs:85`, `p2p-frame/src/networks/quic/network.rs:607`, `p2p-frame/src/tunnel/tunnel_manager.rs:1180,2188`, and `punch_only_stops_on_incoming_success_and_owner_drop` | checked whether the capability borrow can outlive its owner or alter cancellation/owner-drop cleanup | Callers retain the owning network `Arc` while awaiting through the borrow; no new task, socket, handle, or cleanup owner exists; owner-drop test still passes | pass |
| concurrency-and-ordering | async prediction and concurrent punch candidate orchestration | `p2p-frame/src/tunnel/tunnel_manager.rs:1180-1196,2174-2201` and `p2p-frame/src/networks/quic/network.rs:607-707` | looked for a borrow crossing mutation, new lock lifetime, reordered validation, lost cancellation, or serialization | Trait dispatch adds no lock or state mutation; candidate concurrency and prediction-before-validation order are preserved | pass |
| interface-and-compatibility | exported Rust API removal, supertrait rule, object safety, repository consumers, and prior signed-PNAT contract | facade export, positive/negative UDP API fixture, consumer-closure check, all-target build, migrated signed-PNAT fixture | compiled new external discovery/supertrait usage, required all three old generic calls to fail, scanned qualified consumers, and reran the older signed/unsigned signature contract | The intended source break is explicit and closed across known consumers; `UdpTunnelNetwork: TunnelNetwork` and borrowed discovery compile; no forwarding shim remains | pass |
| security-and-capacity | existing signer trust and UDP work limits are transported through the new interface | unchanged expected-signer parameter and QUIC listener delegation, signed-PNAT positive/negative fixture | checked for dropped signer input, bypassed validation, new unauthenticated entry point, allocations, loops, or registries | The trait move preserves the signer argument and validation delegation and introduces no wire/parser/authentication/capacity behavior or additional work source | pass |
| test-adequacy | API, normal, boundary, negative, error, lifecycle, compatibility, and cross-module behavior | test sources, `testplan.yaml`, and `20260904T034147Z-p2p-frame+050-isolate-udp-tunnel-traversal-all.json` commands/results | mapped likely regressions to assertions, then specifically searched older task fixtures for stale generic-trait calls; this exposed and closed F-050-A1-001 and F-050-A1-002 | Fresh evidence covers default and opt-in branches, same identity, old API rejection, existing signed API semantics, all-target compilation, validation failures, punch cleanup, and real loopback SN/QUIC flow | pass |

## Document Consistency
| Document | Source | Implementation Consistency | Finding | Status |
|----------|--------|----------------------------|---------|--------|
| design | `pipeline/plan.md` | The public split, single-registry borrowed capability, QUIC same-instance implementation, caller migration, failure paths, and compatibility closure match current code | No inconsistency found after adding the previously omitted signed-PNAT fixture to the migration mapping | pass |
| testing | `testplan.yaml` | Contract checks and unit/DV/integration steps match current test sources and the fresh all-level artifact, including both signed-PNAT fixture modes | No missing declared evidence or unsupported environment claim found | pass |

## Result Summary
- Overall result: accepted
- Outcome: P-001 is satisfied: UDP traversal operations are isolated on `UdpTunnelNetwork: TunnelNetwork`, discoverable through the generic trait, implemented by the same QUIC instance, and consumed through the explicit capability boundary.
- Blocking issues: none; both acceptance-discovered stale test fixtures are closed and covered by fresh evidence.
- Next action: complete the auto-pipeline runtime state and remove the task from the unfinished index.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: after the acceptance returns closed the stale signed-PNAT consumer and unloaded legacy default-method test, a fresh counterexample review found the trait boundary, caller control flow, failure semantics, lifecycle behavior, compatibility migration, and test evidence consistent with the approved requirement.
