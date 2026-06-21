# Pipeline Plan: SN Distributed Directory 5x5 Command Matrix

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "确认，自动完成后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): p2p-frame
- Submodule(s): sn-distributed-directory
- change_id values: sn_five_by_five_command_matrix

## Acceptance Baseline
- Final acceptance uses the approved `sn-distributed-directory` proposal as authority.
- The current pipeline is scoped to `sn_five_by_five_command_matrix`; previously admitted online-state refresh work remains existing worktree context but is not the new change authority.
- Automated evidence must construct or simulate 5 Owner SN, 5 Serving SN, and 5 user peers, with each user peer bound to a different Serving SN.
- The command matrix must cover peer-facing `ReportSn`/`ReportSnResp`, `SnQuery`/`SnQueryResp`, `SnCall`/`SnCallResp`, `SnCalled`/`SnCalledResp`, inter-SN `Heartbeat`, `PublishLease`, `QueryLease`, `QueryDetail`, `RelayCall`, and owner serving-facing `PublishLease`, `QueryLease`.
- Serving SN availability is modeled as online/offline state only; Serving SN session/epoch is not a required distinction.
- Serving SN online state is an independent low-frequency report/renew/offline flow and is not coupled to every client `ReportSn`.
- `ReportSn` no longer forces `PeerRoute` publish on every report; route publish is first-report, migration, repair, refresh-window, or explicit route-update driven.
- Owner SN leader/follower behavior, leader failover, and leader replication of Serving SN online/offline state remain required.
- OwnerDirectoryServer serving-facing access must be wired through server/listener transport rather than same-process shortcuts.
- Owner control forward must validate the remote owner identity before accepting forwarded owner control commands.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-SN-5X5-CMD-MATRIX-1 | proposal | confirmed | Approve 5 Owner SN, 5 Serving SN, 5 user peer command matrix requirement | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | root | user approval | approved proposal | proposal structure/schema passed and approval metadata records user statement |
| D-SN-5X5-CMD-MATRIX-1 | design | pending | Define command matrix topology, command coverage, testability seams, and admitted scope paths | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | root | P-SN-5X5-CMD-MATRIX-1 | approved design | design doc/check/schema passed, stage-scope result recorded, and auto-pipeline approval metadata recorded |
| I-SN-5X5-CMD-MATRIX-1 | implementation | pending | Implement minimal test support or runtime hooks admitted for the 5x5 command matrix | `p2p-frame/src/sn/**` and admission evidence as admitted by design | root | D-SN-5X5-CMD-MATRIX-1 | implementation/test support and admission evidence | schema/admission passed and targeted checks pass |
| T-SN-5X5-CMD-MATRIX-1 | testing | pending | Add post-implementation 5x5 command matrix testing docs, testplan step, and runnable test evidence | testing docs, testplan, test code, fixtures, runner wiring | root | I-SN-5X5-CMD-MATRIX-1 | runnable test evidence | coverage check and `test-run.py p2p-frame/sn-distributed-directory unit` pass or record concrete blocker |
| A-SN-5X5-CMD-MATRIX-1 | acceptance | pending | Audit 5x5 command matrix proposal/design/implementation/testing consistency | `docs/versions/v0.1/reviews/` | root | T-SN-5X5-CMD-MATRIX-1 | acceptance report | acceptance report check passed and conclusion is accepted |

## Return Routing
- Proposal issue: return to proposal only if the 5x5 topology, command-family coverage, or accepted evidence boundary changes.
- Design issue: return to design if command matrix topology, command coverage, failure handling, testability seams, or Scope Paths are incomplete.
- Implementation issue: return to implementation if test support/code does not satisfy approved design or violates admitted paths.
- Testing issue: return to testing if runnable evidence does not cover the approved 5x5 command matrix behavior.
- Acceptance issue: return to the owning stage named by the finding.

## Exit Condition
- [x] proposal approved
- [ ] design approved
- [ ] implementation admission passed
- [ ] implementation/test support updated
- [ ] testing completed
- [ ] acceptance completed

## Evidence
- User launch statement: `确认，自动完成后续步骤`
- Approved proposal hash: `09ed2a3390f2fbce1c81adf9212d44e4335c71a5a368aa8c295c52a0ab564e81`.
- Proposal path: `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md`.
- Current pipeline starts by refreshing design for `sn_five_by_five_command_matrix`.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --docs design` passed for refreshed draft design.
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory` passed for refreshed draft design.
- `python3 ./harness/scripts/stage-scope-check.py --stage design --version v0.1 --module p2p-frame --submodule sn-distributed-directory` failed because the shared worktree contains many pre-existing tracked/untracked modifications outside the design scope.
- User instructed to ignore untracked historical evidence/review files for this run.
- `python3 ./harness/scripts/stage-scope-check.py --stage design --version v0.1 --module p2p-frame --submodule sn-distributed-directory --ignore-untracked` passed after design auto-confirm.
- Design approval hash: `74ea5cb1239f49695122aa43c54dec59b9d9a29acdb7918db8c38f44b0e52c38`.
- Admission evidence: `harness/evidence/admission/20260621-sn-distributed-online-refresh.md`.
- Admission stamp: `harness/evidence/admission/20260621-sn-distributed-online-refresh.p2p-frame.sn-distributed-directory.stamp.json`.
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_owner_control_plane_online_state --change-id sn_owner_network_leader_election --change-id sn_owner_leader_online_replication --change-id sn_partitioned_peer_route --change-id sn_owner_serving_role_boundary --change-id sn_directory_client_server_boundary --change-id sn_owner_directory_peer_transport --change-id sn_owner_serving_listener_transport --change-id sn_distributed_query_merge --change-id sn_distributed_relay_call --change-id sn_inter_service_validation --evidence-file harness/evidence/admission/20260621-sn-distributed-online-refresh.md` passed.
- `cargo check -p p2p-frame` passed.
- `cargo test -p p2p-frame sn_owner -- --nocapture` passed.
- `cargo test -p p2p-frame sn_distributed_directory -- --nocapture` passed.
- `cargo test -p p2p-frame sn_report_updates_local_detail_without_publishing_route -- --nocapture` passed.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --docs testing` passed.
- `python3 ./harness/scripts/testing-coverage-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_owner_control_plane_online_state --change-id sn_owner_network_leader_election --change-id sn_owner_leader_online_replication --change-id sn_partitioned_peer_route --change-id sn_owner_serving_role_boundary --change-id sn_directory_client_server_boundary --change-id sn_owner_directory_peer_transport --change-id sn_owner_serving_listener_transport --change-id sn_distributed_query_merge --change-id sn_distributed_relay_call --change-id sn_inter_service_validation` passed.
- `python3 ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory unit` passed; latest artifact `test-results/test-runs/20260621T140807Z-p2p-frame+sn-distributed-directory-unit.json`.
- `python3 ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_owner_control_plane_online_state --change-id sn_owner_network_leader_election --change-id sn_owner_leader_online_replication --change-id sn_partitioned_peer_route --change-id sn_owner_serving_role_boundary --change-id sn_directory_client_server_boundary --change-id sn_owner_directory_peer_transport --change-id sn_owner_serving_listener_transport --change-id sn_distributed_query_merge --change-id sn_distributed_relay_call --change-id sn_inter_service_validation --ignore-untracked` failed only because `design.md` and `harness/pipeline-plan.md` are also modified in this uncommitted auto-pipeline worktree.
- `python3 ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --submodule sn-distributed-directory --ignore-untracked` failed because this uncommitted auto-pipeline worktree also contains design, pipeline, and production-code changes.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
| SCOPE-WORKTREE-MIXED | design | D-SN-DIST-ONLINE-REFRESH-1 | Stage scope sees unrelated tracked/untracked changes across docs, harness, production code, tests, and reviews, so the design stage cannot be mechanically confirmed. | Re-run the design stage scope check in a clean/isolated worktree or after unrelated changes are committed/stashed/removed; then auto-confirm design and continue implementation admission. |
| SCOPE-MULTISTAGE-WORKTREE | implementation/testing | I-SN-DIST-ONLINE-REFRESH-1 / T-SN-DIST-ONLINE-REFRESH-1 | Implementation and testing scope gates see earlier design/pipeline changes in the same uncommitted auto-pipeline worktree, even with untracked historical files ignored. | Commit or otherwise baseline completed design/pipeline artifacts, then rerun implementation/testing stage-scope checks; production code and runnable unit evidence already pass. |
