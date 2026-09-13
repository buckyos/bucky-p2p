---
task_manifest: task.yaml
status: approved
---

# SN Client Multi-Channel Command Testing

Risk profile: ./risk-profile.yaml

## Test Document Index
| Document | Topic | Scope |
|----------|-------|-------|
| `testing.md` | classified SN command-stream dispatch, peer-keyed active-SN lifecycle, server stream selection, QA-timeout stream closure, `conn_id` removal | full task |

## Unified Test Entry
- Machine-readable task plan: `docs/versions/v0.1/modules/p2p-frame/073-sn-client-multi-channel-commands/testplan.yaml`
- Task all: `UV_CACHE_DIR=.harness/uv-cache uv run --active python ./harness/scripts/test-run.py p2p-frame/073-sn-client-multi-channel-commands all`
- Single-task boundary: only this task's contract, unit, DV and integration steps are selected; no package/module runtime suite and no `all all` run.

## Repository Consumer Closure
`ActiveSN.conn_id` is a breaking public field removal. Migrated consumers are the production SN client dispatch in `p2p-frame/src/sn/client/sn_service.rs` and the SN tests in `p2p-frame/src/sn/tests.rs`, `p2p-frame/tests/unit/sn_tests/client/nat_probe_directive_tests.rs`, `p2p-frame/tests/nat_type_aware/sn_profile_flow_tests.rs`, `p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs`, `p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs`, `p2p-frame/tests/sn_protocol_real_network.rs`, and `p2p-frame/tests/real_p2p_tunnel_flow/collision_cross_sn.rs`.

## Submodule Tests
| Submodule | Responsibility | Detailed Test Doc | Required Behaviors | Edge/Failure Cases | Test Type | Test Files | Status | Gap / Manual Reason |
|-----------|----------------|-------------------|--------------------|--------------------|-----------|------------|--------|---------------------|
| sn/client | classified dispatch and active-SN lifecycle | `testing.md` | report/call/query/rendezvous use a pool-selected stream; responses stay on that stream; timeout disables the selected stream | peer mismatch, timeout, stale response, closed stream | unit | `p2p-frame/src/sn/tests.rs`, `p2p-frame/tests/unit/sn_tests/client/nat_probe_directive_tests.rs` | ready | none |
| sn/service | server-side selection among accepted client streams | `testing.md` | round-robin notify across the target's existing streams; close only the failed stream | no live stream, send failure, response failure | unit | `p2p-frame/tests/unit/sn_tests/service/distributed_nat_profile_tests.rs` | ready | none |
| sn/types | classification-based stream selection | `testing.md` | classification keys reuse and on-demand creation | missing local endpoint, cap reached | unit | `p2p-frame/tests/unit/sn_tests/service/service/nat_probe_scheduler_tests.rs` | ready | none |
| sn command matrix | end-to-end SN command surface | `testing.md` | five-by-five command matrix and real TCP/QUIC flows keep working | slow handler, forbidden report, notification dedup | integration | `p2p-frame/tests/sn_command_matrix/five_by_five_command_matrix_tests.rs`, `p2p-frame/tests/sn_protocol_real_network.rs` | ready | none |

## Module-Level Tests
| Test Item | Covered Boundary | Entry | Expected Result | Test Type | Test File/Script | Status | Gap / Manual Reason |
|-----------|------------------|-------|-----------------|-----------|------------------|--------|---------------------|
| p2p-frame lib suite | client dispatch, server selection, active-SN lifecycle, timeout cleanup, unreachable-SN recovery | `cargo test -p p2p-frame --features x509 --lib` | 508 tests pass | unit | `p2p-frame/src/sn/tests.rs` and lib test modules | ready | none |
| real SN protocol matrix | whole-module TCP/QUIC SN registration and query surface | `cargo test -p p2p-frame --features x509 --test sn_protocol_real_network` | 3 tests pass | dv | `p2p-frame/tests/sn_protocol_real_network.rs` | ready | none |
| cross-SN real-socket flow | SN rendezvous, report and query across two SN services | `cargo test -p p2p-frame --features x509 --test real_p2p_tunnel_flow` | 6 tests pass | integration | `p2p-frame/tests/real_p2p_tunnel_flow.rs` | ready | none |
| active SN public surface | external consumer compile closure after field removal | `python3 p2p-frame/tests/sn_active_sn_api_check.py --mode positive|negative` | positive compiles, negative rejects `conn_id` | integration | `p2p-frame/tests/sn_active_sn_api_check.py` | ready | none |

## External Interface Tests
| Interface | Responsibility | Success Cases | Failure/Edge Cases | Test Type | Test Doc/File | Status | Gap / Manual Reason |
|-----------|----------------|---------------|--------------------|-----------|---------------|--------|---------------------|
| `ActiveSN` public struct | exported identity/profile state without a pinned stream handle | external crate reads `latest_time` and `sn_peer_id` | `conn_id` access fails to compile | integration | `p2p-frame/tests/sn_active_sn_api_check.py` | ready | none |
| classified command client (`sfo-cmd-server`) | reuse idle streams, create on demand, disable timed-out streams | report/call/query/rendezvous through `get_send_by_classified` | timeout and transport error disable only that stream | unit | `p2p-frame/src/sn/tests.rs`, `p2p-frame/tests/unit/sn_tests/service/distributed_nat_profile_tests.rs` | ready | The dependency version is unchanged; the consumed API surface is exercised through p2p-frame tests. |
| SN rendezvous protocol | notify/response correlation over existing client streams | real TCP/QUIC rendezvous arm and response | forged or unowned target prediction rejected | integration | `p2p-frame/tests/real_p2p_tunnel_flow/collision_cross_sn.rs`, `p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs` | ready | none |

## Direct Change Coverage
| change_id | design_source | validation_id | testplan_level | testplan_step_id | gap | gap_manual_reason |
|-----------|---------------|---------------|----------------|------------------|-----|-------------------|
| CHG-sn-client-classified-command-channel | design.md Overall Approach, File-Level Interfaces, Key Flows; risk-profile runtime checks | VAL-classified-dispatch; VAL-real-protocol | unit | sn-command-unit-lib | no | none |
| CHG-sn-command-active-identity-lifecycle | design.md State and Ownership, Invariants to Preserve; risk-profile security checks | VAL-peer-lifecycle; VAL-cross-sn | unit | sn-command-unit-lib | no | none |
| CHG-sn-server-command-stream-selection | design.md second Key Flow, State and Ownership; risk-profile runtime checks | VAL-server-selection | integration | sn-command-integration-suites | no | none |
| CHG-qa-timeout-command-stream-close | design.md timeout Key Flow, command-stream state diagram; risk-profile runtime checks | VAL-timeout-close | unit | sn-command-unit-lib | no | none |
| CHG-remove-active-sn-conn-id | design.md File-Level Interfaces, Consumer Migration Closure, State and Ownership; risk-profile contract and build checks | VAL-public-surface; VAL-compile-closure; VAL-unreachable-recovery | unit | sn-command-unit-lib | no | none |

## Case-Type Coverage
| change_id | case_type | required | validation_id | level | status | gap_manual_reason |
|-----------|-----------|----------|---------------|-------|--------|-------------------|
| CHG-sn-client-classified-command-channel | normal | yes | VAL-classified-dispatch | unit | covered | report/call/query/rendezvous each acquire a pooled command stream and complete on it |
| CHG-sn-client-classified-command-channel | boundary | yes | VAL-classified-dispatch | unit | covered | no idle stream below the configured cap creates exactly one new command stream |
| CHG-sn-client-classified-command-channel | negative | yes | VAL-classified-dispatch | unit | covered | pooled stream whose remote peer is not the active SN is disabled instead of used |
| CHG-sn-client-classified-command-channel | error | yes | VAL-classified-dispatch | unit | covered | failed QA disables the selected stream and reports a connect failure |
| CHG-sn-client-classified-command-channel | compatibility | yes | VAL-real-protocol | dv | covered | real TCP and QUIC SN command flows keep their existing wire and command codes |
| CHG-sn-client-classified-command-channel | lifecycle | yes | VAL-classified-dispatch | unit | covered | an evicted or cleared command stream is replaced on the next command |
| CHG-sn-client-classified-command-channel | cross-module | yes | VAL-compile-closure | integration | covered | every repository consumer of the classified client compiles and the command matrix passes |
| CHG-sn-command-active-identity-lifecycle | normal | yes | VAL-peer-lifecycle | unit | covered | notifications on any authenticated stream of the active SN are accepted |
| CHG-sn-command-active-identity-lifecycle | boundary | yes | VAL-peer-lifecycle | unit | covered | stale replacement from another endpoint still cannot replace a published active SN |
| CHG-sn-command-active-identity-lifecycle | negative | yes | VAL-peer-lifecycle | unit | covered | notification from an SN that is not active is answered with a failure response |
| CHG-sn-command-active-identity-lifecycle | error | yes | VAL-peer-lifecycle | unit | covered | closed command streams no longer delete the active SN registration |
| CHG-sn-command-active-identity-lifecycle | compatibility | yes | VAL-real-protocol | dv | covered | registration still exposes one active SN per authenticated SN peer |
| CHG-sn-command-active-identity-lifecycle | lifecycle | yes | VAL-peer-lifecycle | unit | covered | peer-keyed updates keep probe profile and signer state after stream replacement |
| CHG-sn-command-active-identity-lifecycle | cross-module | yes | VAL-cross-sn | integration | covered | cross-SN real-socket report, query and rendezvous keep the active SN usable |
| CHG-sn-server-command-stream-selection | normal | yes | VAL-server-selection | unit | covered | server notify QA selects a live client stream and returns the target action |
| CHG-sn-server-command-stream-selection | boundary | yes | VAL-server-selection | unit | covered | cursor rotation distributes sequential notifies across the target's streams |
| CHG-sn-server-command-stream-selection | negative | yes | VAL-server-selection | unit | covered | target without a live command stream yields NotConnected instead of creating a stream |
| CHG-sn-server-command-stream-selection | error | yes | VAL-server-selection | unit | covered | failed notify closes only the selected stream and falls back to another candidate |
| CHG-sn-server-command-stream-selection | compatibility | yes | VAL-cross-sn | integration | covered | rendezvous notify/response wire semantics are unchanged across two SN services |
| CHG-sn-server-command-stream-selection | lifecycle | yes | VAL-server-selection | unit | covered | finished receive tasks disappear from the candidate list before the next selection |
| CHG-sn-server-command-stream-selection | cross-module | yes | VAL-cross-sn | integration | covered | SN service selection interoperates with the client's accepted command streams |
| CHG-qa-timeout-command-stream-close | normal | yes | VAL-timeout-close | unit | covered | successful QA leaves its command stream reusable |
| CHG-qa-timeout-command-stream-close | boundary | yes | VAL-timeout-close | unit | covered | timeout at the configured deadline disables exactly the selected stream |
| CHG-qa-timeout-command-stream-close | negative | yes | VAL-timeout-close | unit | covered | late response cannot complete or restore state after the stream is disabled |
| CHG-qa-timeout-command-stream-close | error | yes | VAL-timeout-close | unit | covered | transport error disables its stream and reports failure to the caller |
| CHG-qa-timeout-command-stream-close | compatibility | yes | VAL-real-protocol | dv | covered | bearer TTP tunnel survives a single SN command timeout |
| CHG-qa-timeout-command-stream-close | lifecycle | yes | VAL-timeout-close | unit | covered | other command streams and the active SN remain usable after one timeout |
| CHG-qa-timeout-command-stream-close | cross-module | yes | VAL-cross-sn | integration | covered | timeout cleanup in the client is observable to the SN service stream list |
| CHG-remove-active-sn-conn-id | normal | yes | VAL-public-surface | integration | covered | external crate consumes the active SN identity surface without `conn_id` |
| CHG-remove-active-sn-conn-id | boundary | yes | VAL-compile-closure | integration | covered | lib and integration test targets compile with `--all-targets` |
| CHG-remove-active-sn-conn-id | negative | yes | VAL-public-surface | integration | covered | legacy `conn_id` field access fails with the expected compiler diagnostic |
| CHG-remove-active-sn-conn-id | error | yes | VAL-unreachable-recovery | unit | covered | a SN with no obtainable command stream is evicted and its QA reports connect failure |
| CHG-remove-active-sn-conn-id | compatibility | yes | VAL-public-surface | integration | covered | remaining active-SN fields keep their public shape and behaviour |
| CHG-remove-active-sn-conn-id | lifecycle | yes | VAL-unreachable-recovery | unit | covered | active-SN records are created and updated without a stored stream handle and are evicted only when no command stream can be obtained |
| CHG-remove-active-sn-conn-id | cross-module | yes | VAL-cross-sn | integration | covered | workspace consumers of `ActiveSN` compile and the cross-SN flow passes |

## Design Element Coverage
| element_type | design_source | derived_cases | level | status | gap_manual_reason |
|--------------|---------------|---------------|-------|--------|-------------------|
| parameter-domain | design.md File-Level Interfaces (`send_sn_qa` classification, timeout) | missing local endpoint, at-cap wait, peer mismatch, zero-length body, concurrent call/query/rendezvous | unit | covered | none |
| state-transition | design.md command-stream state diagram and State and Ownership | Idle->Borrowed->Idle, Borrowed->Disabled, cleared stream replaced, active SN retained while reachable, unreachable SN evicted and re-registered | unit | covered | none |
| failure-path | design.md Key Flows timeout branch and server selection branch | QA timeout, transport error, notify send failure, target without live stream | unit | covered | none |
| error-handling | design.md file-level interfaces and invariants | ConnectFailed/InvalidData/NotConnected categories and late-response rejection | unit | covered | none |
| invariant | design.md Invariants to Preserve | response stream match, timeout closes only its stream, stale stream cannot update state, stream failure keeps active SN | unit | covered | none |
| concurrency | design.md Overall Approach concurrency intent | concurrent commands on distinct streams over one bearer, notify cursor under concurrent requests, cancellation during pending QA | unit | covered | none |

## Validation Rationale
| Behavior or Risk | Validation Signal | Why This Is Sufficient | Gap / Manual Reason |
|------------------|-------------------|------------------------|---------------------|
| one slow SN QA must not serialize other commands | lib suite command matrix and real TCP/QUIC matrix pass with classified dispatch | the suites exercise concurrent commands and notify paths through the real SN service | public NAT and multi-host SN deployments remain outside this environment |
| timeout must close one stream and keep the bearer | lib suite timeout and stream-failure cases plus real protocol matrix | cleanup is asserted on the selected guard while other streams and the active SN stay usable | none |
| server must distribute notify QA over existing client streams | distributed rendezvous service tests and cross-SN real-socket rendezvous | both the selection code and its real rendezvous consumer are executed | none |
| breaking `ActiveSN.conn_id` removal | external positive/negative compile fixtures, removed-symbol scan and `--all-targets` compile closure | compiler-backed external consumers plus repository closure prove the migration | no downstream out-of-repo consumer inventory |
| no wire or dependency change | real SN protocol matrix and command matrix suites | command codes, versions and payload shapes are asserted unchanged | none |
| unreachable SN must not leave the client permanently online | `unreachable_sn_command_stream_evicts_stale_active_sn_and_recovers` plus the removed-symbol scan and compile closure | the test fails against the pre-fix implementation (observed assertion failure at `p2p-frame/src/sn/tests.rs:1276` with the eviction disabled) and passes after the fix, then observes automatic re-registration of the serving SN | none |

## Unit Tests
| Function or Unit | Branch or Condition | Covered Behavior | Test File | Status | Gap / Manual Reason |
|------------------|---------------------|------------------|-----------|--------|---------------------|
| `SNClientService::send_sn_qa` | peer mismatch, success, send failure | pool-selected stream validated against the active SN peer, disabled on failure | `p2p-frame/src/sn/tests.rs` | covered | none |
| `SNClientService::report` | response sequence/peer mismatch, timeout | report QA uses the pooled stream and rejects mismatched responses | `p2p-frame/src/sn/tests.rs` | covered | none |
| `SNClientService::call_inner` | success, timeout, closed stream | call QA keeps its stream or re-creates one without dropping the active SN | `p2p-frame/src/sn/tests.rs` | covered | none |
| `SNClientService::query_with_context` | success, error continuation | query QA iterates active SNs with pooled streams | `p2p-frame/src/sn/tests.rs` | covered | none |
| `SNClientService::rendezvous_via_sn` | success, failure response | rendezvous QA uses the pooled stream and validates the responder | `p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs` | covered | none |
| `SNClientService::active_sn_matches` | matching peer, non-matching peer | inbound notify accepted only for the authenticated active SN | `p2p-frame/tests/unit/sn_tests/service/distributed_nat_profile_tests.rs` | covered | none |
| `update_active_sn` | present peer, absent peer | peer-keyed profile update cannot be written by a stale stream identity | `p2p-frame/tests/unit/sn_tests/client/nat_probe_directive_tests.rs` | covered | none |
| `publish_active_sn` | newer registration, stale registration | profile publication remains peer-keyed and monotonic | `p2p-frame/tests/nat_type_aware/sn_profile_flow_tests.rs` | covered | none |
| `SnService::deliver_rendezvous_to_local_peer` | candidate list empty, success, send failure, response failure | round-robin selection, single-stream cleanup on failure, NotConnected when no live stream | `p2p-frame/tests/unit/sn_tests/service/distributed_nat_profile_tests.rs` | covered | none |
| `ActiveSN` construction and removal | field set after removal | no `conn_id` in the public struct or its construction sites | `p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs` | covered | none |
| `SNClientService::remove_active_sn` | acquisition failure, no record, record present | eviction only fires when no command stream can be obtained; re-registration recovers the serving SN | `p2p-frame/src/sn/tests.rs` | covered | none |
| `SNClientService::send_sn_qa` | stream acquisition failure vs request failure | acquisition failure evicts the unusable SN record; a per-request timeout or stream failure keeps a reachable SN | `p2p-frame/src/sn/tests.rs` | covered | none |

## DV Tests
| Workflow | Kind | Entry | Expected Result | Test File or Script | Status | Gap / Manual Reason |
|----------|------|-------|-----------------|---------------------|--------|---------------------|
| SN command dispatch | main | lib command matrix and SN client tests | commands complete on pooled streams across all client/node variants | `p2p-frame/tests/sn_command_matrix/five_by_five_command_matrix_tests.rs` | covered | none |
| QA timeout cleanup | failure | lib timeout tests over the real SN service | selected stream disabled, other streams and active SN remain usable | `p2p-frame/src/sn/tests.rs` | covered | none |
| real TCP/QUIC registration | main | `sn_protocol_real_network` matrix | clients register and query with one active SN and no pinned stream handle | `p2p-frame/tests/sn_protocol_real_network.rs` | covered | none |
| stream replacement | lifecycle | clear all cached command streams, then call and query | both commands re-create a stream and the active SN registration survives | `p2p-frame/src/sn/tests.rs` | covered | none |
| unreachable SN recovery | failure | active SN records a closed endpoint, then a QA is issued | the unusable record is evicted and the ping loop re-registers the serving SN from the configured SN list | `p2p-frame/src/sn/tests.rs` | covered | none |

## Integration Tests
| Contract or Flow | Modules Involved | Success Case | Failure Case | Test File | Status | Gap / Manual Reason |
|------------------|------------------|--------------|--------------|-----------|--------|---------------------|
| classified command client contract | p2p-frame sn client and `sfo-cmd-server` | idle stream reuse and on-demand creation through the consumed API | timeout disables only the selected stream | `p2p-frame/src/sn/tests.rs` | covered | none |
| server notify selection contract | p2p-frame sn service and client | notify QA reaches the target on an accepted stream | target without a live stream returns NotConnected | `p2p-frame/tests/real_p2p_tunnel_flow/collision_cross_sn.rs` | covered | none |
| rendezvous protocol contract | SN service, SN client, tunnel manager | rendezvous arm and response over real sockets | forged or unowned prediction rejected | `p2p-frame/tests/tunnel_rendezvous_protocol.rs` | covered | none |
| public SN protocol surface | p2p-frame exported protocol types | protocol version and public API suites pass unchanged | removed field rejected by the compiler | `p2p-frame/tests/sn_protocol_version_public_api.rs` | covered | none |
| public NAT/multi-host SN deployment | deployed SN fleet | not executed | public NAT, multi-host and deployed SN environments are unavailable here | `p2p-frame/tests/real_p2p_tunnel_flow.rs` | gap | 公网 NAT、多主机与部署 SN 环境不在本机能力范围内，本地仅覆盖 loopback 真实 socket 流程 |

## Definition of Done
- [x] Every changed branch and behavior is mapped to a runnable case or a recorded gap.
- [x] `testplan.yaml` maps every `change_id` to contract, unit, DV and integration steps.
- [x] Task-scoped runner selects only this task's steps.
- [x] Breaking public field removal has compiler-backed positive/negative fixtures, removed-symbol scan and compile closure.
- [x] Task-scoped automated tests pass.
