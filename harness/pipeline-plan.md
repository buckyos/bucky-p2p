# Pipeline Plan: SN Distributed Directory Online State Refresh

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "自动处理后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): p2p-frame
- Submodule(s): sn-distributed-directory
- change_id values: sn_owner_control_plane_online_state, sn_owner_network_leader_election, sn_owner_leader_online_replication, sn_partitioned_peer_route, sn_owner_serving_role_boundary, sn_directory_client_server_boundary, sn_owner_directory_peer_transport, sn_owner_serving_listener_transport, sn_distributed_query_merge, sn_distributed_relay_call, sn_inter_service_validation

## Acceptance Baseline
- Final acceptance uses the approved `sn-distributed-directory` proposal as authority.
- Serving SN availability is modeled as online/offline state only; Serving SN session/epoch is not a required distinction.
- Serving SN online state is an independent low-frequency report/renew/offline flow and is not coupled to every client `ReportSn`.
- `ReportSn` no longer forces `PeerRoute` publish on every report; route publish is first-report, migration, repair, refresh-window, or explicit route-update driven.
- Owner SN leader/follower behavior, leader failover, and leader replication of Serving SN online/offline state remain required.
- OwnerDirectoryServer serving-facing access must be wired through server/listener transport rather than same-process shortcuts.
- Owner control forward must validate the remote owner identity before accepting forwarded owner control commands.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-SN-DIST-ONLINE-REFRESH-1 | proposal | confirmed | Approved Serving SN online/offline and route publish decoupling requirements | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | root | user approval | approved proposal | proposal structure/schema passed and approval metadata records user statement |
| D-SN-DIST-ONLINE-REFRESH-1 | design | blocked | Refresh design for online/offline state, route publish decoupling, SnServer wiring, and remote owner validation | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | root | P-SN-DIST-ONLINE-REFRESH-1 | draft design | design doc/check/schema passed, but stage-scope is blocked by pre-existing mixed worktree changes |
| I-SN-DIST-ONLINE-REFRESH-1 | implementation | pending | Implement the approved design under admitted directory/inter-SN/service/sn-miner scope | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/service/**`, `p2p-frame/src/sn/protocol/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` as admitted by design | root | D-SN-DIST-ONLINE-REFRESH-1 | production code and admission evidence | schema/admission passed before code edits; targeted tests pass |
| T-SN-DIST-ONLINE-REFRESH-1 | testing | pending | Add post-implementation tests/evidence for online/offline, route publish decoupling, server wiring, validation, query, and call behavior | tests, testing docs, fixtures, runner wiring | root | I-SN-DIST-ONLINE-REFRESH-1 | runnable evidence | coverage check and targeted unit/DV/integration tests pass or recorded gaps are accepted by acceptance |
| A-SN-DIST-ONLINE-REFRESH-1 | acceptance | pending | Audit proposal/design/implementation/testing consistency and final behavior | `docs/versions/v0.1/reviews/` | root | T-SN-DIST-ONLINE-REFRESH-1 | acceptance report | acceptance report check passed and conclusion is accepted |

## Return Routing
- Proposal issue: return to proposal only if Serving SN online/offline, route publish cadence, role boundary, or authorization requirements change.
- Design issue: return to design if online-state flow, route publish policy, SnServer wiring, owner validation, or Scope Paths are incomplete.
- Implementation issue: return to implementation if code does not satisfy approved design or violates admitted paths.
- Testing issue: return to testing if runnable evidence does not cover the approved behavior.
- Acceptance issue: return to the owning stage named by the finding.

## Exit Condition
- [x] proposal approved
- [ ] design approved
- [ ] implementation admission passed
- [ ] production code updated
- [ ] testing completed
- [ ] acceptance completed

## Evidence
- User launch statement: `自动处理后续步骤`
- Approved proposal hash: `5b8cba1a856fc438f76ba1b83ef56ce1c604ff39787a7228048b342f30f0c349`.
- Proposal path: `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md`.
- Current pipeline starts by refreshing stale design that still used Serving SN session/epoch semantics.
- `python3 ./harness/scripts/pipeline-plan-check.py harness/pipeline-plan.md` passed after recording the user launch.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --docs design` passed for refreshed draft design.
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory` passed for refreshed draft design.
- `python3 ./harness/scripts/stage-scope-check.py --stage design --version v0.1 --module p2p-frame --submodule sn-distributed-directory` failed because the shared worktree contains many pre-existing tracked/untracked modifications outside the design scope.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
| SCOPE-WORKTREE-MIXED | design | D-SN-DIST-ONLINE-REFRESH-1 | Stage scope sees unrelated tracked/untracked changes across docs, harness, production code, tests, and reviews, so the design stage cannot be mechanically confirmed. | Re-run the design stage scope check in a clean/isolated worktree or after unrelated changes are committed/stashed/removed; then auto-confirm design and continue implementation admission. |
