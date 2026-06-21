---
module: p2p-frame
submodule: sn-distributed-directory
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-06-20T21:07:00+08:00
approved_content_sha256: 94f222ccce7279ded9a495b45c43533973a81b3d91233036b64360d34fe519ef
---

# SN Distributed Directory Testing

## Test Document Index
| Document | Topic | Scope |
|----------|-------|-------|
| `testing.md` | post-implementation test design | direct submodule |
| `testplan.yaml` | machine-readable test entry | direct submodule |

## Unified Test Entry
- Machine-readable plan: `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory unit`
- DV: disabled with recorded gap.
- Integration: disabled with recorded gap.
- Registration: targeted unit coverage is reachable through the unified entrypoint.

## Submodule Tests
| Submodule | Responsibility | Detailed Test Doc | Required Behaviors | Edge/Failure Cases | Test Type | Test Files | Status | Gap / Manual Reason |
|-----------|----------------|-------------------|--------------------|--------------------|-----------|------------|--------|---------------------|
| sn_directory | owner resolver, serving lease, owner store data model | this file | multi-serving lease, HRW owner set, local-only fallback | stale sequence and TTL expiry | unit | `p2p-frame/src/sn/directory/mod.rs` | ready | |
| sn_directory | OwnerMember heartbeat validity | this file | valid owner filtering and heartbeat recovery | expired member health is not returned to serving owner set | unit | `p2p-frame/src/sn/directory/mod.rs` | ready | |
| sn_directory | Owner control plane sessions | this file | leader election, leader-only mutation, quorum commit, Serving SN session renewal | stale leader and missing quorum reject mutation | unit | `p2p-frame/src/sn/directory/mod.rs` | ready | |
| sn_directory | owner network leader election | this file | owner-to-owner vote quorum, leader heartbeat propagation, leader failover after heartbeat timeout, and TTP owner command dispatch for election payloads | stale/missing leader cannot commit; no quorum blocks election | unit | `p2p-frame/src/sn/directory/election.rs`, `p2p-frame/src/sn/inter_sn/mod.rs` | ready | real multi-process TTP transport lifecycle remains a DV/integration gap. |
| sn_directory | owner leader session replication | this file | follower forwards Serving SN session mutation to leader; leader replicates renew/revoke to followers | leader revoke hides session on every owner node | unit | `p2p-frame/src/sn/directory/election.rs` | ready | real multi-process replication remains a DV/integration gap. |
| sn_directory | partitioned peer route | this file | route publish/query is filtered by committed Serving SN session/epoch | revoked session hides existing route | unit | `p2p-frame/src/sn/directory/mod.rs` | ready | |
| sn_directory | owner directory server and serving-side directory client | this file | directory server owns lease publish/query; `SnService` uses directory client only | validator reject; serving detail remains separate | unit | `p2p-frame/src/sn/directory/mod.rs`, `p2p-frame/src/sn/service/service.rs` | ready | |
| sn_directory | OwnerDirectoryServer serving-facing listener | this file | `OwnerDirectoryServer::new` creates owner-peer and serving-facing listen endpoints; serving-facing publish/query uses `DefaultCmdServerService + TtpServer`; serving-side client opens the serving connector path instead of registry shortcut | mismatched serving SN id is rejected before owner store write | unit | `p2p-frame/src/sn/directory/server.rs`, `p2p-frame/src/sn/directory/client.rs` | ready | real Serving SN to OwnerDirectoryServer TTP process workflow remains an integration gap. |
| sn_inter_service | inter-SN validator, registry fallback, and owner command node dispatch | this file | validator blocks side effects; registered serving SN can return detail; owner command code mapping and `DefaultCmdNodeService` handler dispatch are covered | connection/command reject; endpoint-less member falls back to local registry path | unit | `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/inter_sn/mod.rs` | ready | |

## Module-Level Tests
| Test Item | Covered Boundary | Entry | Expected Result | Test Type | Test File/Script | Status | Gap / Manual Reason |
|-----------|------------------|-------|-----------------|-----------|------------------|--------|---------------------|
| SN distributed directory targeted unit | directory/inter-SN/service integration | `cargo test -p p2p-frame sn_distributed_directory -- --nocapture` | 7 targeted tests pass | unit | inline Rust unit tests | ready | |
| SN directory client/server boundary targeted unit | directory server/client vs serving-only `SnService` | `cargo test -p p2p-frame sn_directory_client_server_boundary -- --nocapture` | directory client/server boundary test passes and serving detail remains separate from owner lease state | unit | inline Rust unit tests | ready | |
| SN owner member heartbeat targeted unit | OwnerMember health filtering and recovery | `cargo test -p p2p-frame sn_owner_member_heartbeat_validity -- --nocapture` | expired owner health is filtered and refreshed health is returned again | unit | inline Rust unit tests | ready | |
| SN owner control plane targeted unit | Owner control plane leader/quorum/session behavior | `cargo test -p p2p-frame sn_owner_control_plane_sessions_require_leader_and_quorum -- --nocapture` | non-leader mutation is rejected, leader mutation commits with quorum, and missing quorum rejects new mutation | unit | inline Rust unit tests | ready | |
| SN owner network election targeted unit | Owner network vote and leader failover behavior | `cargo test -p p2p-frame sn_owner_network -- --nocapture` | owner peers elect a leader by vote quorum and elect a new leader after old leader heartbeat timeout | unit | inline Rust unit tests | ready | |
| SN owner leader session replication targeted unit | leader-only Serving SN session replication | `cargo test -p p2p-frame sn_owner_leader_session_replication -- --nocapture` | follower forwards session renew/revoke to leader and all owner nodes converge on committed session state | unit | inline Rust unit tests | ready | |
| SN partitioned route targeted unit | Peer route filtering by Serving SN session/epoch | `cargo test -p p2p-frame sn_partitioned_peer_route_filters_revoked_serving_session -- --nocapture` | route is returned while session is alive and filtered after session revoke | unit | inline Rust unit tests | ready | |
| SN owner serving listener targeted unit | OwnerDirectoryServer serving-facing command server and serving-side connector selection | `cargo test -p p2p-frame owner_serving_listener -- --nocapture` | serving publish command is dispatched through the serving-facing command handler, mismatched serving SN id is rejected, and serving client opens the serving connector path | unit | inline Rust unit tests | ready | |

## External Interface Tests
| Interface | Responsibility | Success Cases | Failure/Edge Cases | Test Type | Test Doc/File | Status | Gap / Manual Reason |
|-----------|----------------|---------------|--------------------|-----------|---------------|--------|---------------------|
| `SnServiceConfig::set_owner_membership(...)` | configure static owner membership | package compiles and service tests construct membership-backed services | empty membership rejected by `OwnerMembership::new(...)` | unit | `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/directory/mod.rs` | ready | |
| `SnServiceConfig::set_inter_service_validator(...)` | configure inter-SN validation | default allow-all permits publish/query/detail | reject validator blocks owner write before side effect | unit | `p2p-frame/src/sn/service/service.rs` | ready | |
| `sn-miner --owner-members` | optional owner membership wiring | CLI parser accepts comma-separated member ids | malformed member exits before service start | manual | `sn-miner-rust/src/main.rs` | gap | owner: testing; risk: CLI parse regression; acceptance impact: `cargo check -p sn-miner` was attempted but blocked by long-running third-party build script. |

## Direct Change Coverage
| change_id | design_source | validation_id | testplan_level | testplan_step_id | Gap? | Gap / Manual Reason |
|-----------|---------------|---------------|----------------|------------------|------|---------------------|
| sn_distributed_directory | `design.md` Directly Mapped Change Items, Data and State | VAL-SN-DIST-DIR-UNIT | unit | sn-distributed-directory-unit | no | |
| sn_owner_serving_role_boundary | `design.md` Owner role vs Serving role runtime boundary, Owner lease publish/query flow | VAL-SN-ROLE-BOUNDARY-UNIT | unit | sn-directory-client-server-boundary-unit | no | |
| sn_directory_client_server_boundary | `design.md` `sn_directory` client/server split and serving-only `SnService` | VAL-SN-DIRECTORY-CLIENT-SERVER-UNIT | unit | sn-directory-client-server-boundary-unit | no | |
| sn_owner_member_heartbeat_validity | `design.md` OwnerMember health state, heartbeat refresh flow, and strict HRW owner filtering behavior | VAL-SN-OWNER-HEARTBEAT-UNIT | unit | sn-owner-member-heartbeat-validity-unit | no | |
| sn_distributed_query_merge | `design.md` Key Call Flows, Query merge detail | VAL-SN-DIST-QUERY-UNIT | unit | sn-distributed-directory-unit | no | |
| sn_distributed_relay_call | `design.md` Key Call Flows, Relay call | VAL-SN-DIST-CALL-UNIT | unit | sn-distributed-directory-unit | no | |
| sn_inter_service_validation | `design.md` validation and authorization interfaces | VAL-SN-INTER-AUTH-UNIT | unit | sn-distributed-directory-unit | no | |
| sn_owner_directory_peer_transport | `design.md` Owner peer transport and command node using `NetManager + TtpNode + DefaultCmdNodeService`; no `TtpClient`/`TtpServer` substitution | VAL-SN-OWNER-PEER-TRANSPORT-UNIT | unit | sn-distributed-directory-unit | no | |
| sn_owner_serving_listener_transport | `design.md` Serving-facing listener and command server using `NetManager + TtpServer + DefaultCmdServerService`; owner peer listener remains `NetManager + TtpNode + DefaultCmdNodeService` | VAL-SN-OWNER-SERVING-LISTENER-UNIT | unit | sn-owner-serving-listener-unit | no | |
| sn_owner_control_plane_sessions | `design.md` Owner control plane, Serving SN session/epoch, leader/quorum commit, revoke/expiry filtering | VAL-SN-OWNER-CONTROL-UNIT | unit | sn-owner-control-plane-unit | no | |
| sn_owner_network_leader_election | `design.md` Owner-to-owner vote request/response, candidate/follower/leader roles, leader heartbeat, heartbeat timeout, failover election, follower redirect/forward | VAL-SN-OWNER-NETWORK-ELECTION-UNIT | unit | sn-owner-network-election-unit | no | owner command dispatch adapter is unit-covered; real multi-process TTP lifecycle remains a DV/integration gap. |
| sn_owner_leader_session_replication | `design.md` leader-only Serving SN session mutation, follower forwarding, quorum replication, committed session apply on followers, offline/revoke propagation | VAL-SN-OWNER-SESSION-REPLICATION-UNIT | unit | sn-owner-leader-session-replication-unit | no | real multi-process replication remains a DV/integration gap. |
| sn_partitioned_peer_route | `design.md` Fixed partition route owner set, route publish/query, session/epoch filter, route miss behavior | VAL-SN-PARTITIONED-ROUTE-UNIT | unit | sn-partitioned-peer-route-unit | no | |

## Case-Type Coverage
| change_id | case_type | required | validation_id | level | status | gap_manual_reason |
|-----------|-----------|----------|---------------|-------|--------|-------------------|
| sn_distributed_directory | normal | yes | VAL-SN-DIST-DIR-UNIT | unit | covered | |
| sn_distributed_directory | boundary | yes | VAL-SN-DIST-DIR-UNIT | unit | covered | |
| sn_distributed_directory | negative | yes | VAL-SN-DIST-DIR-UNIT | unit | covered | |
| sn_distributed_directory | error | yes | VAL-SN-DIST-DIR-UNIT | unit | covered | |
| sn_distributed_directory | compatibility | yes | VAL-SN-DIST-DIR-UNIT | unit | covered | |
| sn_distributed_directory | lifecycle | yes | VAL-SN-DIST-DIR-UNIT | unit | covered | |
| sn_distributed_directory | cross-module | yes | VAL-SN-DIST-DIR-UNIT | unit | covered | |
| sn_owner_serving_role_boundary | normal | yes | VAL-SN-ROLE-BOUNDARY-UNIT | unit | covered | |
| sn_owner_serving_role_boundary | boundary | yes | VAL-SN-ROLE-BOUNDARY-UNIT | unit | covered | |
| sn_owner_serving_role_boundary | negative | yes | VAL-SN-ROLE-BOUNDARY-UNIT | unit | covered | |
| sn_owner_serving_role_boundary | error | yes | VAL-SN-ROLE-BOUNDARY-UNIT | unit | covered | |
| sn_owner_serving_role_boundary | compatibility | yes | VAL-SN-ROLE-BOUNDARY-UNIT | unit | covered | |
| sn_owner_serving_role_boundary | lifecycle | yes | VAL-SN-ROLE-BOUNDARY-UNIT | unit | covered | |
| sn_owner_serving_role_boundary | cross-module | yes | VAL-SN-ROLE-BOUNDARY-UNIT | unit | covered | |
| sn_directory_client_server_boundary | normal | yes | VAL-SN-DIRECTORY-CLIENT-SERVER-UNIT | unit | covered | |
| sn_directory_client_server_boundary | boundary | yes | VAL-SN-DIRECTORY-CLIENT-SERVER-UNIT | unit | covered | |
| sn_directory_client_server_boundary | negative | yes | VAL-SN-DIRECTORY-CLIENT-SERVER-UNIT | unit | covered | |
| sn_directory_client_server_boundary | error | yes | VAL-SN-DIRECTORY-CLIENT-SERVER-UNIT | unit | covered | |
| sn_directory_client_server_boundary | compatibility | yes | VAL-SN-DIRECTORY-CLIENT-SERVER-UNIT | unit | covered | |
| sn_directory_client_server_boundary | lifecycle | yes | VAL-SN-DIRECTORY-CLIENT-SERVER-UNIT | unit | covered | |
| sn_directory_client_server_boundary | cross-module | yes | VAL-SN-DIRECTORY-CLIENT-SERVER-UNIT | unit | covered | |
| sn_owner_member_heartbeat_validity | normal | yes | VAL-SN-OWNER-HEARTBEAT-UNIT | unit | covered | |
| sn_owner_member_heartbeat_validity | boundary | yes | VAL-SN-OWNER-HEARTBEAT-UNIT | unit | covered | |
| sn_owner_member_heartbeat_validity | negative | yes | VAL-SN-OWNER-HEARTBEAT-UNIT | unit | covered | |
| sn_owner_member_heartbeat_validity | error | yes | VAL-SN-OWNER-HEARTBEAT-UNIT | unit | covered | |
| sn_owner_member_heartbeat_validity | compatibility | yes | VAL-SN-OWNER-HEARTBEAT-UNIT | unit | covered | |
| sn_owner_member_heartbeat_validity | lifecycle | yes | VAL-SN-OWNER-HEARTBEAT-UNIT | unit | covered | |
| sn_owner_member_heartbeat_validity | cross-module | yes | VAL-SN-OWNER-HEARTBEAT-UNIT | unit | covered | |
| sn_distributed_query_merge | normal | yes | VAL-SN-DIST-QUERY-UNIT | unit | covered | |
| sn_distributed_query_merge | boundary | yes | VAL-SN-DIST-QUERY-UNIT | unit | covered | |
| sn_distributed_query_merge | negative | yes | VAL-SN-DIST-QUERY-UNIT | unit | covered | |
| sn_distributed_query_merge | error | yes | VAL-SN-DIST-QUERY-UNIT | unit | covered | |
| sn_distributed_query_merge | compatibility | yes | VAL-SN-DIST-QUERY-UNIT | unit | covered | |
| sn_distributed_query_merge | lifecycle | yes | VAL-SN-DIST-QUERY-UNIT | unit | covered | |
| sn_distributed_query_merge | cross-module | yes | VAL-SN-DIST-QUERY-UNIT | unit | covered | |
| sn_distributed_relay_call | normal | yes | VAL-SN-DIST-CALL-UNIT | unit | covered | |
| sn_distributed_relay_call | boundary | yes | VAL-SN-DIST-CALL-UNIT | unit | covered | |
| sn_distributed_relay_call | negative | yes | VAL-SN-DIST-CALL-UNIT | unit | covered | |
| sn_distributed_relay_call | error | yes | VAL-SN-DIST-CALL-UNIT | unit | covered | |
| sn_distributed_relay_call | compatibility | yes | VAL-SN-DIST-CALL-UNIT | unit | covered | |
| sn_distributed_relay_call | lifecycle | yes | VAL-SN-DIST-CALL-UNIT | unit | covered | |
| sn_distributed_relay_call | cross-module | yes | VAL-SN-DIST-CALL-UNIT | unit | covered | |
| sn_inter_service_validation | normal | yes | VAL-SN-INTER-AUTH-UNIT | unit | covered | |
| sn_inter_service_validation | boundary | yes | VAL-SN-INTER-AUTH-UNIT | unit | covered | |
| sn_inter_service_validation | negative | yes | VAL-SN-INTER-AUTH-UNIT | unit | covered | |
| sn_inter_service_validation | error | yes | VAL-SN-INTER-AUTH-UNIT | unit | covered | |
| sn_inter_service_validation | compatibility | yes | VAL-SN-INTER-AUTH-UNIT | unit | covered | |
| sn_inter_service_validation | lifecycle | yes | VAL-SN-INTER-AUTH-UNIT | unit | covered | |
| sn_inter_service_validation | cross-module | yes | VAL-SN-INTER-AUTH-UNIT | unit | covered | |
| sn_owner_directory_peer_transport | normal | yes | VAL-SN-OWNER-PEER-TRANSPORT-UNIT | unit | covered | owner command codes map heartbeat, publish lease, query lease, and detail requests. |
| sn_owner_directory_peer_transport | boundary | yes | VAL-SN-OWNER-PEER-TRANSPORT-UNIT | unit | covered | command code and payload mismatch path returns encoded command error. |
| sn_owner_directory_peer_transport | negative | yes | VAL-SN-OWNER-PEER-TRANSPORT-UNIT | unit | covered | dispatcher converts peer handler errors into `InterSnResponse::Error`. |
| sn_owner_directory_peer_transport | error | yes | VAL-SN-OWNER-PEER-TRANSPORT-UNIT | unit | covered | command body encode/decode failures are mapped to cmd errors by handler path. |
| sn_owner_directory_peer_transport | compatibility | yes | VAL-SN-OWNER-PEER-TRANSPORT-UNIT | unit | covered | owner peer command service is isolated from peer-facing SN command ids and serving-facing command ids. |
| sn_owner_directory_peer_transport | lifecycle | yes | VAL-SN-OWNER-PEER-TRANSPORT-UNIT | unit | gap | owner: testing; risk: real `TtpNode` listen/connect lifecycle regression; acceptance impact: no multi-process owner-to-owner DV exists yet. |
| sn_owner_directory_peer_transport | cross-module | yes | VAL-SN-OWNER-PEER-TRANSPORT-UNIT | unit | gap | owner: testing; risk: owner membership endpoint wiring regression; acceptance impact: no end-to-end owner peer transport workflow exists yet. |
| sn_owner_serving_listener_transport | normal | yes | VAL-SN-OWNER-SERVING-LISTENER-UNIT | unit | covered | |
| sn_owner_serving_listener_transport | boundary | yes | VAL-SN-OWNER-SERVING-LISTENER-UNIT | unit | covered | serving id must match lease owner before store write. |
| sn_owner_serving_listener_transport | negative | yes | VAL-SN-OWNER-SERVING-LISTENER-UNIT | unit | covered | mismatched serving SN id rejects publish. |
| sn_owner_serving_listener_transport | error | yes | VAL-SN-OWNER-SERVING-LISTENER-UNIT | unit | covered | permission-denied response path is exercised by the negative command case. |
| sn_owner_serving_listener_transport | compatibility | yes | VAL-SN-OWNER-SERVING-LISTENER-UNIT | unit | covered | owner-peer `DefaultCmdNodeService + TtpNode` path remains separate and existing distributed directory unit suite still passes. |
| sn_owner_serving_listener_transport | lifecycle | yes | VAL-SN-OWNER-SERVING-LISTENER-UNIT | unit | gap | owner: testing; risk: listener start/stop or port lifecycle regression; acceptance impact: no real `TtpServer` process lifecycle DV exists yet. |
| sn_owner_serving_listener_transport | cross-module | yes | VAL-SN-OWNER-SERVING-LISTENER-UNIT | unit | gap | owner: testing; risk: Serving SN remote transport regression; acceptance impact: no end-to-end Serving SN -> OwnerDirectoryServer integration workflow exists yet. |
| sn_owner_control_plane_sessions | normal | yes | VAL-SN-OWNER-CONTROL-UNIT | unit | covered | leader renews a Serving SN session and committed state becomes readable. |
| sn_owner_control_plane_sessions | boundary | yes | VAL-SN-OWNER-CONTROL-UNIT | unit | covered | mutation through a non-leader is rejected. |
| sn_owner_control_plane_sessions | negative | yes | VAL-SN-OWNER-CONTROL-UNIT | unit | covered | missing quorum rejects a new session mutation. |
| sn_owner_control_plane_sessions | error | yes | VAL-SN-OWNER-CONTROL-UNIT | unit | covered | no-quorum mutation returns an error and leaves committed state unchanged. |
| sn_owner_control_plane_sessions | compatibility | yes | VAL-SN-OWNER-CONTROL-UNIT | unit | covered | existing distributed directory lease tests still pass through the compatibility wrapper. |
| sn_owner_control_plane_sessions | lifecycle | yes | VAL-SN-OWNER-CONTROL-UNIT | unit | covered | session is renewed and later can be revoked by the route test. |
| sn_owner_control_plane_sessions | cross-module | yes | VAL-SN-OWNER-CONTROL-UNIT | unit | gap | owner: testing; risk: real multi-owner Raft transport regression; acceptance impact: only in-memory state machine coverage exists in this task. |
| sn_owner_network_leader_election | normal | yes | VAL-SN-OWNER-NETWORK-ELECTION-UNIT | unit | covered | owner peers vote and elect a leader. |
| sn_owner_network_leader_election | boundary | yes | VAL-SN-OWNER-NETWORK-ELECTION-UNIT | unit | covered | heartbeat timeout triggers a new election. |
| sn_owner_network_leader_election | negative | yes | VAL-SN-OWNER-NETWORK-ELECTION-UNIT | unit | covered | offline old leader cannot prevent quorum failover. |
| sn_owner_network_leader_election | error | yes | VAL-SN-OWNER-NETWORK-ELECTION-UNIT | unit | covered | network client returns errors for offline peers and election still requires quorum. |
| sn_owner_network_leader_election | compatibility | yes | VAL-SN-OWNER-NETWORK-ELECTION-UNIT | unit | covered | existing owner control plane and distributed directory tests still pass. |
| sn_owner_network_leader_election | lifecycle | yes | VAL-SN-OWNER-NETWORK-ELECTION-UNIT | unit | covered | follower -> candidate -> leader and old leader offline -> new leader. |
| sn_owner_network_leader_election | cross-module | yes | VAL-SN-OWNER-NETWORK-ELECTION-UNIT | unit | gap | owner: testing; risk: real owner-to-owner TTP listener/connect lifecycle regression; acceptance impact: no multi-process owner election DV exists yet. |
| sn_owner_leader_session_replication | normal | yes | VAL-SN-OWNER-SESSION-REPLICATION-UNIT | unit | covered | follower forwards renew to leader and all nodes observe alive session. |
| sn_owner_leader_session_replication | boundary | yes | VAL-SN-OWNER-SESSION-REPLICATION-UNIT | unit | covered | revoke is replicated and hides the session on all nodes. |
| sn_owner_leader_session_replication | negative | yes | VAL-SN-OWNER-SESSION-REPLICATION-UNIT | unit | covered | follower does not locally commit before forwarding to leader. |
| sn_owner_leader_session_replication | error | yes | VAL-SN-OWNER-SESSION-REPLICATION-UNIT | unit | covered | replication requires quorum ack before leader applies the entry. |
| sn_owner_leader_session_replication | compatibility | yes | VAL-SN-OWNER-SESSION-REPLICATION-UNIT | unit | covered | user-level `PeerRoute` replication remains separate and existing route tests pass. |
| sn_owner_leader_session_replication | lifecycle | yes | VAL-SN-OWNER-SESSION-REPLICATION-UNIT | unit | covered | alive -> replicated -> revoked across owner nodes. |
| sn_owner_leader_session_replication | cross-module | yes | VAL-SN-OWNER-SESSION-REPLICATION-UNIT | unit | gap | owner: testing; risk: real multi-process session replication regression; acceptance impact: no TTP-backed replication DV exists yet. |
| sn_partitioned_peer_route | normal | yes | VAL-SN-PARTITIONED-ROUTE-UNIT | unit | covered | route is returned while its Serving SN session is alive. |
| sn_partitioned_peer_route | boundary | yes | VAL-SN-PARTITIONED-ROUTE-UNIT | unit | covered | route query consults the matching serving epoch. |
| sn_partitioned_peer_route | negative | yes | VAL-SN-PARTITIONED-ROUTE-UNIT | unit | covered | revoked session hides a previously written route. |
| sn_partitioned_peer_route | error | yes | VAL-SN-PARTITIONED-ROUTE-UNIT | unit | covered | route write requires an alive session and returns false otherwise through implementation branch. |
| sn_partitioned_peer_route | compatibility | yes | VAL-SN-PARTITIONED-ROUTE-UNIT | unit | covered | existing `ServingLease` compatibility tests pass after route-store replacement. |
| sn_partitioned_peer_route | lifecycle | yes | VAL-SN-PARTITIONED-ROUTE-UNIT | unit | covered | route moves from visible to hidden when session is revoked. |
| sn_partitioned_peer_route | cross-module | yes | VAL-SN-PARTITIONED-ROUTE-UNIT | unit | gap | owner: testing; risk: real multi-owner route fanout regression; acceptance impact: no multi-process partitioned route DV exists yet. |

## Design Element Coverage
| element_type | design_source | derived_cases | level | status | gap_manual_reason |
|--------------|---------------|---------------|-------|--------|-------------------|
| parameter-domain | `OwnerMembership`, validator setters, SN ids | empty membership reject, owner count clamp, comma parsing noted | unit | covered | |
| state-transition | `ServingLease` absent -> fresh -> refreshed -> expired | multi-lease insert, stale sequence reject, TTL expiry | unit | covered | |
| state-transition | `OwnerMemberHealth` unknown -> fresh -> expired -> refreshed | valid owner filter, expired owner removal, heartbeat recovery | unit | covered | |
| state-transition | `OwnerControlPlane` follower/candidate/leader and committed session state | leader election, non-leader rejection, quorum loss rejection, session renewal | unit | covered | |
| state-transition | `OwnerElectionNode` follower/candidate/leader network role | vote quorum, leader heartbeat, heartbeat timeout, failover election, owner command payload dispatch | unit | covered | real multi-process owner-to-owner transport lifecycle remains a DV gap. |
| state-transition | leader session replication | follower forward, leader quorum replication, follower committed apply, revoke propagation | unit | covered | real multi-process replication remains a DV gap. |
| state-transition | `PeerRoute` visible -> hidden by dead session | route returned with live session and filtered after session revoke | unit | covered | |
| failure-path | publish/query/detail validator reject and missing remote registration | reject blocks owner write; missing registry ignored in implementation | unit | covered | |
| error-handling | `PermissionDenied`, `NotFound`, stale lease | reject returns permission denied; no owner accepted returns not found | unit | covered | |
| invariant | single SN fallback, client-visible response compatibility, owner/serving role separation | local-only owner resolver; no peer-facing command code replacement; directory client/server test keeps lease state separate from serving detail | unit | covered | |
| concurrency | store protected by mutex; global registry weak references | deterministic unit exercises mutex-backed writes | unit | covered | |
| invariant | OwnerDirectoryServer uses two transport/listener responsibilities | owner peer command path remains `DefaultCmdNodeService + TtpNode`; serving-facing command path uses separate `DefaultCmdServerService` handler set and local serving registry fallback | unit | covered | |
| failure-path | serving-facing publish/query validation in `design.md` Key Call Flows | mismatched serving SN id is rejected before lease write through `owner_serving_listener_rejects_publish_for_different_serving_sn` | unit | covered | |

## Validation Rationale
| Behavior or Risk | Validation Signal | Why This Is Sufficient | Gap / Manual Reason |
|------------------|-------------------|------------------------|---------------------|
| owner store must keep multiple serving SN leases | store unit asserts two serving SN ids for one peer | covers direct data invariant without network noise | |
| stale leases must not override newer leases | stale sequence insert returns false | directly covers overwrite rule | |
| expired leases must not be returned | TTL unit queries after expiry | directly covers owner store expiry | |
| inter-SN validator reject must block effects | service unit rejects publish and owner store remains empty | covers authorization side-effect boundary | |
| ownerSN and servingSN logic must stay separated | service unit publishes through `OwnerDirectoryClient` to `OwnerDirectoryServer` and verifies peer detail remains absent until `PeerManager` is explicitly populated | covers directory client/server boundary without allowing direct store/detail coupling | |
| registered serving SN can publish/query detail through inter-SN abstraction | service unit creates owner and serving services with membership | covers endpoint-less registry fallback path used by local tests and compatibility fallback | |
| owner peer command node maps and dispatches owner commands | `sn_distributed_directory_maps_requests_to_owner_cmd_codes`, heartbeat dispatch, and publish dispatch exercise the `DefaultCmdNodeService` handler boundary | covers command id selection, request decode, response encode, and side-effect dispatch without requiring multiple OS processes | |
| real owner-to-owner TTP node lifecycle | no automated DV/integration test yet | unit verifies command-node dispatch, but not real `NetManager + TtpNode` listen/connect behavior across OwnerDirectoryServer processes | owner: testing; risk: owner peer transport lifecycle regression; acceptance impact: true multi-process behavior remains a gap. |
| OwnerDirectoryServer must expose a serving-facing command listener distinct from owner peer communication | targeted serving listener unit dispatches publish through the serving-facing command handler, verifies bad serving id rejection, and verifies serving client opens the serving connector path | covers the command dispatch, command code, validation, client path selection, and store side-effect boundary; real `TtpServer` accept-loop lifecycle remains a separate DV/integration gap | owner: testing; risk: real listener lifecycle regression; acceptance impact: no multi-process Serving SN -> OwnerDirectoryServer transport evidence yet. |
| OwnerMember heartbeat validity must filter stale owner nodes | resolver unit refreshes one owner, verifies expiry removes it, then verifies refresh restores another owner | directly covers heartbeat validity lifecycle without conflating it with ServingLease TTL | |
| owner control plane must gate session writes through leader/quorum | targeted unit elects a leader, rejects stale leader mutation, commits session renewal, then rejects mutation after quorum loss | directly covers the first-version in-memory Raft-style state machine without requiring multi-process transport | |
| ownerSN network voting must elect and fail over leaders | targeted election unit uses an in-memory owner peer network to vote, propagate heartbeat, mark old leader offline, elect a new leader after timeout, and dispatch TTP owner command payloads | covers the control-plane election contract and TTP command dispatch independent of OS port lifecycle | owner: testing; risk: real owner-to-owner TTP listener/connect lifecycle may differ; acceptance impact: multi-process election remains a DV gap. |
| leader must own Serving SN session mutation and replicate to followers | targeted session replication unit sends renew/revoke through a follower, verifies forwarding to leader, and asserts all owner nodes converge | covers leader-only mutation and quorum replication semantics at the state-machine/network abstraction boundary | owner: testing; risk: real transport serialization/timeout path remains unproven; acceptance impact: multi-process replication remains a DV gap. |
| partitioned peer route must be filtered by Serving SN session/epoch | targeted unit writes a route under a live session, then revokes the session and verifies the route disappears from query | directly covers abnormal Serving SN disconnect semantics at the route-store layer | |
| real multi-process TTP-backed SN-to-SN transport | no automated DV/integration test yet | unit verifies TTP frame path, but not listener/client behavior across real SN processes | owner: testing; risk: distributed runtime regression; acceptance impact: true multi-process behavior remains a gap. |

## Unit Tests
| Function or Unit | Branch or Condition | Covered Behavior | Test File | Status | Gap / Manual Reason |
|------------------|---------------------|------------------|-----------|--------|---------------------|
| `OwnerDirectoryStore::put_lease` | fresh insert, multiple serving SN, stale sequence | owner store lease ownership | `p2p-frame/src/sn/directory/mod.rs` | covered | |
| `OwnerDirectoryStore::query` | expired retain/filter | TTL expiry not returned | `p2p-frame/src/sn/directory/mod.rs` | covered | |
| `OwnerResolver::owner_set` | local-only and membership HRW | single SN fallback and deterministic owner set | `p2p-frame/src/sn/directory/mod.rs` | covered | |
| `OwnerMemberHealth` and `OwnerResolver::owner_set_at` | fresh, expired, and refreshed owner member health | only valid owner members are returned to serving owner selection after TTL filtering | `p2p-frame/src/sn/directory/mod.rs` | covered | |
| `OwnerDirectoryServer` and `OwnerDirectoryClient` | directory client/server boundary | lease publish/query uses directory module and does not create serving detail in `SnService` | `p2p-frame/src/sn/directory/mod.rs`, `p2p-frame/src/sn/service/service.rs` | covered | |
| `SnService::publish_serving_lease` and inter trait methods | registered owner/serving SN | publish lease and query detail through registry | `p2p-frame/src/sn/service/service.rs` | covered | |
| `SnInterServiceValidator` use | reject connection before side effect | no owner write on reject | `p2p-frame/src/sn/service/service.rs` | covered | |
| `inter_sn_command_code` | heartbeat, publish lease, query lease, query detail | maps owner requests to `DefaultCmdNodeService` command ids | `p2p-frame/src/sn/inter_sn/mod.rs` | covered | |
| `dispatch_owner_cmd` | heartbeat and publish lease | owner command handler dispatches decoded requests and returns encoded owner responses | `p2p-frame/src/sn/inter_sn/mod.rs` | covered | |
| `TtpInterSnClient::new` / `OwnerTunnelFactory` | real `NetManager + TtpNode + DefaultCmdNodeService` listen/connect path | compile coverage confirms command node service and tunnel factory wiring; real OS port lifecycle is not exercised by the targeted unit | `p2p-frame/src/sn/inter_sn/mod.rs` | gap | owner: testing; risk: owner peer listener/connect regression; acceptance impact: no DV harness can start two real owner peers yet. |
| `OwnerDirectoryServer::attach_serving_listener` | constructs serving-facing listener and starts `DefaultCmdServerService` | command server creation and handler registration compile; runtime accept-loop lifecycle is not exercised by the targeted unit | `p2p-frame/src/sn/directory/server.rs` | gap | owner: testing; risk: real port listener lifecycle regression; acceptance impact: no DV harness can start a real `TtpServer` listener yet. |
| `OwnerDirectoryServer::new` | non-empty owner-peer and serving endpoint parameters | constructs separate owner-peer `TtpNode` and serving-facing `TtpServer` resources at construction time; actual OS port lifecycle remains a DV/integration gap | `p2p-frame/src/sn/directory/server.rs` | gap | owner: testing; risk: constructor-time listen failure, duplicate port, or shutdown regression; acceptance impact: no real dual-port DV harness exists yet. |
| `OwnerDirectoryService::publish_lease_from_serving_sn` | matching serving SN id | stores a serving lease and returns published success | `p2p-frame/src/sn/directory/server.rs` | covered | |
| `OwnerDirectoryService::publish_lease_from_serving_sn` | mismatched serving SN id | returns permission denied and does not write owner store | `p2p-frame/src/sn/directory/server.rs` | covered | |
| `OwnerServingCommandCode` and serving command dispatch | `PublishLease` and response decode path | publish command body is decoded, dispatched, and encoded as published response | `p2p-frame/src/sn/directory/server.rs` | covered | |
| `StaticOwnerDirectoryClient::publish_serving_lease` | configured serving connector | opens the owner serving control-stream purpose through the connector instead of using same-process registry fallback | `p2p-frame/src/sn/directory/client.rs` | covered | |
| `OwnerControlPlane::elect_leader` and `renew_serving_session` | leader election, stale leader mutation, quorum loss | first-version Raft-style owner control/session behavior | `p2p-frame/src/sn/directory/mod.rs` | covered | |
| `OwnerElectionNode::start_election` and vote handlers | quorum vote and heartbeat propagation | ownerSN network leader election | `p2p-frame/src/sn/directory/election.rs` | covered | |
| `OwnerElectionNode::tick` | heartbeat timeout after old leader is offline | ownerSN leader failover and re-election | `p2p-frame/src/sn/directory/election.rs` | covered | |
| `OwnerElectionNode::commit_or_forward_session` and replication handlers | follower forward, leader replicate, follower apply committed entry | Serving SN session renew/revoke converges on all owner nodes | `p2p-frame/src/sn/directory/election.rs` | covered | |
| `PeerRouteStore::query` through `OwnerDirectoryStore::query_peer_routes` | live session and revoked session | partitioned route is visible only while Serving SN session/epoch is alive | `p2p-frame/src/sn/directory/mod.rs` | covered | |

## DV Tests
| Workflow | Kind | Entry | Expected Result | Test File or Script | Status | Gap / Manual Reason |
|----------|------|-------|-----------------|---------------------|--------|---------------------|
| multi-SN service lifecycle | lifecycle | not-applicable: no automated DV harness exists yet | multiple SN processes publish/query/call through TTP | not-applicable: gap recorded for future DV harness | gap | owner: testing; risk: distributed runtime regression; acceptance impact: no automated DV exists yet. |
| OwnerDirectoryServer dual-listener lifecycle | lifecycle | not-applicable: no automated DV harness exists yet | owner peer listener and serving-facing listener can start on independent ports, reject wrong-purpose traffic, and shut down cleanly | not-applicable: gap recorded for future DV harness | gap | owner: testing; risk: serving listener port lifecycle regression; acceptance impact: no automated DV exists yet. |
| ownerSN network election lifecycle | lifecycle | unit owner peer network | owner peers vote, propagate leader heartbeat, and fail over after leader timeout | `cargo test -p p2p-frame sn_owner_network -- --nocapture` | covered | real TTP-backed multi-process election remains a gap. |
| ownerSN leader session replication | main | unit owner peer network | follower forwards Serving SN session mutation and leader replicates committed state to followers | `cargo test -p p2p-frame sn_owner_leader_session_replication -- --nocapture` | covered | real TTP-backed multi-process replication remains a gap. |
| multi-SN query and relay workflow | main | not-applicable: no automated DV harness exists yet | query merge and relay call cross serving SN boundaries | not-applicable: gap recorded for future DV harness | gap | owner: testing; risk: distributed runtime regression; acceptance impact: true multi-SN query/call remains unevidenced. |
| default no-config compatibility | config | unit fallback test | single SN owner set is local only | unit tests | covered | |
| validator reject workflow | failure | unit inter validator test | reject has no owner write side effect | unit tests | covered | |

## Integration Tests
| Contract or Flow | Modules Involved | Success Case | Failure Case | Test File | Status | Gap / Manual Reason |
|------------------|------------------|--------------|--------------|-----------|--------|---------------------|
| sn-miner owner membership config | `sn-miner-rust`, `p2p-frame` | CLI parser wires `OwnerMembership` into `SnServiceConfig` | invalid id rejects startup | `sn-miner-rust/src/main.rs` | gap | owner: testing; risk: deployment parse regression; acceptance impact: `cargo check -p sn-miner` did not complete because `libsecp256k1` build script ran too long. |
| peer-facing SN compatibility | `p2p-frame` SN protocol/client callers | existing client-visible command enum remains unchanged | no new client-visible query response shape | compile and unit tests | covered | |
| multi-process SN-to-SN transport | multiple SN services | remote serving SN detail/call over TTP | timeout/partial failure ignored | not-applicable: no integration test file exists yet | gap | owner: testing; risk: true network path regression; acceptance impact: implementation has TTP client/listener code and frame unit coverage, but no multi-process transport evidence. |
| serving SN to owner directory transport | `p2p-frame` Serving SN client and OwnerDirectoryServer | Serving SN publishes and queries leases through `NetManager + TtpServer + DefaultCmdServerService` serving-facing port | mismatched serving id and transport failure propagate as command errors | not-applicable: no integration test file exists yet | gap | owner: testing; risk: remote serving directory transport regression; acceptance impact: targeted command unit passes but real cross-process contract is unevidenced. |

## Regression Focus
- Preserve default single-SN behavior when OwnerMembership is absent.
- Preserve client-visible SN command and response shapes.
- Prevent validator reject from writing owner state or returning detail/relay side effects.
- Prevent serving-only `SnService` from depending on mixed owner/serving state.
- Prevent expired OwnerMember health from being returned to Serving SN owner selection while preserving ServingLease TTL semantics.
- Preserve two independent OwnerDirectoryServer listener responsibilities: owner peer `NetManager + TtpNode + DefaultCmdNodeService` and serving-facing `NetManager + TtpServer + DefaultCmdServerService`.

## Definition of Done
- [x] Testing docs cover the direct submodule.
- [x] `testplan.yaml` matches the declared targeted unit entrypoint.
- [x] Generated tests are reachable through `harness/scripts/test-run.py`.
- [x] Unit tests cover lease sequence, TTL, owner resolver, and validator reject branches.
- [x] Unit tests cover owner/serving role separation through the directory client/server boundary.
- [x] Unit tests cover the TTP inter-SN client control-stream frame path.
- [x] Unit tests cover OwnerMember heartbeat validity expiry and recovery.
- [x] Unit tests cover OwnerDirectoryServer serving-facing command dispatch, wrong-serving-id rejection, and serving-side connector selection.
- [x] Unit tests cover owner control plane leader/quorum/session behavior.
- [x] Unit tests cover ownerSN network voting, leader heartbeat propagation, and leader failover.
- [x] Unit tests cover follower-to-leader Serving SN session forwarding and leader replication to followers.
- [x] Unit tests cover partitioned peer route filtering by Serving SN session/epoch.
- [x] Each implemented change_id has direct validation coverage.
- [x] DV and integration gaps record owner, risk, and acceptance impact.
- [ ] Full multi-process DV/integration evidence exists for TTP-backed inter-SN transport.
- [ ] Full multi-process DV/integration evidence exists for Serving SN -> OwnerDirectoryServer `NetManager + TtpServer + DefaultCmdServerService` transport.
- [x] Relevant targeted unit tests pass.

## Approval Record
- approver:
- approval_date:
- user_statement: ""
