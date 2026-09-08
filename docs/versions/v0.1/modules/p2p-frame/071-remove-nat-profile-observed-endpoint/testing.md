---
task_manifest: task.yaml
status: approved
---

# Remove NatProfile.observed_endpoint and Live-Only Prediction Testing

Risk profile: ./risk-profile.yaml

## Test Document Index
| Document | Topic | Scope |
|----------|-------|-------|
| `testing.md` | profile structure/freshness/wire removal and live-only prediction control | full task |

## Unified Test Entry
- Machine-readable task plan: `docs/versions/v0.1/modules/p2p-frame/071-remove-nat-profile-observed-endpoint/testplan.yaml`
- Task all: `UV_CACHE_DIR=.harness/uv-cache uv run --active python ./harness/scripts/test-run.py p2p-frame/071-remove-nat-profile-observed-endpoint all`
- Single-task boundary: only task-plan steps are selected; no package/module runtime suite or `all all`.

## Repository Consumer Closure
`NatProfile.observed_endpoint` is a breaking public API removal. Consumer files migrated in this task: `p2p-frame/src/nat_type.rs`, `p2p-frame/src/networks/quic/listener.rs`, `p2p-frame/src/networks/quic/network.rs`, `p2p-frame/src/networks/udp_network.rs`, `p2p-frame/src/sn/client/sn_service.rs`, `p2p-frame/src/sn/service/nat_probe_scheduler.rs`, `p2p-frame/src/sn/service/peer_manager.rs`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/tunnel/tunnel_manager.rs`, and all NatProfile-consuming tests. `rg "\.observed_endpoint"` remains only for SN candidate helpers, not `NatProfile`.

## Submodule Tests
| Submodule | Responsibility | Detailed Test Doc | Required Behaviors | Edge/Failure Cases | Test Type | Test Files | Status | Gap / Manual Reason |
|-----------|----------------|-------------------|--------------------|--------------------|-----------|------------|--------|---------------------|
| nat_type | profile without endpoint | `testing.md` | freshness by time only; hint still produced | expired profile, no hint | unit | `p2p-frame/tests/unit/nat_type/tests.rs` | ready | none |
| networks/quic | live prediction only | `testing.md` | live probe returns endpoints from same probe | no hint, listener generation mismatch | unit | `p2p-frame/src/networks/quic/listener/rendezvous_prediction_tests.rs` | ready | none |
| tunnel | cached predicted no longer expands | `testing.md` | fallback Predicted fails closed | no PN, no live rendezvous | unit | `p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs` | ready | none |
| sn client/service | profile publication | `testing.md` | report/query stores no endpoint | stale profile invalidation | unit | existing SN profile tests | ready | none |

## Module-Level Tests
| Test Item | Covered Boundary | Entry | Expected Result | Test Type | Test File/Script | Status | Gap / Manual Reason |
|-----------|------------------|-------|-----------------|-----------|------------------|--------|---------------------|
| lib profile suite | all `NatProfile` consumers compile and behave | `cargo test -p p2p-frame --features x509 --lib` | 505 pass, no direct field references | unit | `p2p-frame/tests/unit/nat_type/tests.rs` and lib modules | ready | none |
| fallback prediction | cached profile must not dial predicted ports | changed `rendezvous_deterministic_failure_rejects_cached_predicted_without_proxy` | NotFound/NotSupport and zero dial | unit | `p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs` | ready | none |

## External Interface Tests
| Interface | Responsibility | Success Cases | Failure/Edge Cases | Test Type | Test Doc/File | Status | Gap / Manual Reason |
|-----------|----------------|---------------|--------------------|-----------|---------------|--------|---------------------|
| `NatProfile` public API | exported profile still constructible without endpoint | external consumer builds profile from observations | field absent in public struct | integration | `p2p-frame/tests/nat_profile_public_api.rs` | ready | none |
| SN rendezvous wire | no expected endpoint field changes | rendezvous protocol round-trips unchanged | old hidden endpoints unused | integration | `p2p-frame/tests/tunnel_rendezvous_protocol.rs` | ready | none |

## Direct Change Coverage
| change_id | design_source | validation_id | testplan_level | testplan_step_id | gap | gap_manual_reason |
|-----------|---------------|---------------|----------------|------------------|-----|-------------------|
| CHG-remove-observed-endpoint | design.md File-Level Interfaces, API impact; design/nat_type.md; design/sn-client.md; design/sn-service.md | VAL-profile-structure; VAL-wire-public | unit | nat-profile-unit-lib | no | none |
| CHG-live-prediction-only | design.md Key Flows, Directly Mapped Change Items; design/networks-quic.md; design/tunnel.md | VAL-prediction-live; VAL-fallback-failclosed | unit | nat-profile-unit-lib | no | none |

## Case-Type Coverage
| change_id | case_type | required | validation_id | level | status | gap_manual_reason |
|-----------|-----------|----------|---------------|-------|--------|-------------------|
| CHG-remove-observed-endpoint | normal | yes | VAL-profile-structure | unit | covered | profile built without field and freshness passes within TTL |
| CHG-remove-observed-endpoint | boundary | yes | VAL-profile-structure | unit | covered | expiry boundary before/after observed_at remains unchanged |
| CHG-remove-observed-endpoint | negative | yes | VAL-profile-structure | unit | covered | no observed endpoint field exists |
| CHG-remove-observed-endpoint | error | yes | VAL-wire-public | integration | covered | old wire compatibility is intentionally not retained |
| CHG-remove-observed-endpoint | compatibility | yes | VAL-wire-public | integration | covered | new public wire round-trip parity asserted |
| CHG-remove-observed-endpoint | lifecycle | yes | VAL-profile-structure | unit | covered | stale profile invalidation remains time-based with no stored endpoint |
| CHG-remove-observed-endpoint | cross-module | yes | VAL-wire-public | integration | covered | SN report/query/inter-SN profile consumers all compile without the field |
| CHG-live-prediction-only | normal | yes | VAL-prediction-live | unit | covered | live prediction still produces symmetric candidates from same-probe base |
| CHG-live-prediction-only | boundary | yes | VAL-prediction-live | unit | covered | no hint returns NotFound and fallback no longer falls to cached expansion |
| CHG-live-prediction-only | negative | yes | VAL-fallback-failclosed | unit | covered | Predicted nat_candidates rejects cached hint expansion |
| CHG-live-prediction-only | error | yes | VAL-fallback-failclosed | unit | covered | deterministic rendezvous failure without PN is not a direct success |
| CHG-live-prediction-only | compatibility | yes | VAL-prediction-live | unit | covered | rendezvous response prediction path and wire remain unchanged |
| CHG-live-prediction-only | lifecycle | yes | VAL-prediction-live | unit | covered | prediction uses same-probe base after listener generation validation |
| CHG-live-prediction-only | cross-module | yes | VAL-fallback-failclosed | unit | covered | tunnel manager no longer expands cached remote profile |

## Design Element Coverage
| element_type | design_source | derived_cases | level | status | gap_manual_reason |
|--------------|---------------|---------------|-------|--------|-------------------|
| parameter-domain | design/nat_type.md | profile classification with two+ observations; missing hint | unit | covered | none |
| state-transition | design/nat_type.md | freshness by observed_at/valid_until only | unit | covered | none |
| failure-path | design/tunnel.md | fallback Predicted fails closed | unit | covered | none |
| error-handling | design/networks-quic.md | unavailable live prediction returns NotFound | unit | covered | none |
| invariant | design/sn-client.md | report/query profile never contains observed_endpoint | integration | covered | raw field scan in consumer closure |
| concurrency | design/sn-service.md | stale profile invalidation remains time-based without shared endpoint state | unit | covered | none |

## Validation Rationale
| Behavior or Risk | Validation Signal | Why This Is Sufficient | Gap / Manual Reason |
|------------------|-------------------|------------------------|---------------------|
| breaking public API removal | all in-repo consumers compile and rg scan finds no field | repository closure plus public API test proves exported surface | no public NAT deployment |
| freshness stays time-based | unit boundary before/after TTL | directly asserts `is_fresh` no longer needs endpoint | none |
| prediction only live | prediction test and fallback test | proves both live path works and cached path fails closed | none |
| wire break accepted | protocol/public API tests | newer profile blob decodes by new schema; old blob intentionally incompatible | mixed-version live deployment remains outside evidence |

## Unit Tests
| Function or Unit | Branch or Condition | Covered Behavior | Test File | Status | Gap / Manual Reason |
|------------------|---------------------|------------------|-----------|--------|---------------------|
| `NatProfile::from_observations` | two completed observations | profile structure has no endpoint | `p2p-frame/tests/unit/nat_type/tests.rs` | covered | none |
| `NatProfile::is_fresh` | TTL boundary | no endpoint precondition | `p2p-frame/tests/unit/nat_type/tests.rs` | covered | none |
| `QuicTunnelListener::predict_traversal_endpoints` | symmetric arithmetic probe | endpoints from same-probe hint | `p2p-frame/src/networks/quic/listener/rendezvous_prediction_tests.rs` | covered | none |
| `TunnelManager::nat_candidates` | Predicted mode | returns error, no cached expansion | `p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs` | covered | none |

## DV Tests
| Workflow | Kind | Entry | Expected Result | Test File or Script | Status | Gap / Manual Reason |
|----------|------|-------|-----------------|---------------------|--------|---------------------|
| live prediction | main | symmetric probe through QUIC listener prediction | predicted endpoints produced from same-probe base | `p2p-frame/src/networks/quic/listener/rendezvous_prediction_tests.rs` | covered | none |
| NAT-aware fallback | failure | deterministic rendezvous failure without PN | no predicted dial, not success | `p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs` | covered | none |
| prediction validation | lifecycle | listener generation current/expired/closed | validation accepts current, rejects others | `p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs` | covered | none |

## Integration Tests
| Contract or Flow | Modules Involved | Success Case | Failure Case | Test File | Status | Gap / Manual Reason |
|------------------|------------------|--------------|--------------|-----------|--------|---------------------|
| public profile API | p2p-frame export surface | external consumer constructs profile without endpoint | old field access fails compile | `p2p-frame/tests/nat_profile_public_api.rs` | covered | none |
| rendezvous protocol unrelated to field | SN protocol | wire round-trip unchanged | no endpoint leakage | `p2p-frame/tests/tunnel_rendezvous_protocol.rs` | covered | none |
| real NAT socket matrix | QUIC listener/snapshot | loopback real-socket flow | public NAT/multi-SN | `p2p-frame/tests/real_p2p_tunnel_flow.rs` | gap | 公网 NAT、多主机与部署网络不在本环境，未纳入自动验收 |

## Definition of Done
- [x] Direct submodule behavior, branches, and wire impact recorded.
- [x] `testplan.yaml` maps every validation to the two change_ids.
- [x] Tests live in existing crate test reachable through task plan.
- [x] Task-scoped runner selects only task unit/DV/integration steps.
- [x] No `cyfs-p2p-test` file, command, scenario, or artifact used.
- [x] Task-scoped automated tests pass.
