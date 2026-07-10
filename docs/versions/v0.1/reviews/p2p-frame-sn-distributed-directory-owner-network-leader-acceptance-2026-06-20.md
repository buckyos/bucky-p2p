# p2p-frame SN Distributed Directory Owner Network Leader Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-OWNER-NET-SCOPE-001 | high | implementation/testing | implementation and testing `stage-scope-check.py` runs failed | The shared worktree still contains unrelated and cross-stage dirty paths, so the current diff cannot be mechanically isolated to `sn_owner_network_leader_election` / `sn_owner_leader_session_replication`. | implementation diff was not bound to admitted design Scope Paths; single-stage scope checks failed |
| F-OWNER-NET-DV-001 | medium | testing | `testing.md` and `testplan.yaml` record DV/integration gaps | Network voting, failover, follower forwarding, session replication, and TTP owner command payload dispatch are covered by unit harnesses, but not by real TTP-backed multi-process OwnerDirectoryServer DV. | runtime/integration evidence gap remains |

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- change_id values reviewed: `sn_owner_network_leader_election`, `sn_owner_leader_session_replication`
- Review date: 2026-06-20
- In scope: approved proposal/design/testing/admission, owner network election abstraction, leader heartbeat/failover, follower session forwarding, leader session replication, targeted unit evidence.
- Out of scope: complete etcd/Raft persistence, WAL, snapshot, joint consensus, linearizable reads, route full-cluster replication, unrelated dirty worktree paths.

## Optional Diff / Status Evidence
- `git status --short` summary: mixed dirty worktree includes this task's docs/code plus pre-existing harness, TTP, QUIC, p2p-frame, cyfs-p2p-test, sn-miner, and review/evidence paths.
- Task-relevant changed paths include `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/{proposal,design,testing,testplan.yaml}`, `p2p-frame/src/sn/directory/{control_plane,election,mod}.rs`, `harness/evidence/admission/20260620-owner-network-leader-election.md`, and `harness/pipeline-plan.md`.
- Diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| Owner network leader election | `proposal.md` P-SN-DIST-CONTROL-2 and `design.md` Directly Mapped Change Items | `OwnerElectionNode`, `OwnerPeerControlClient`, vote request/response, role/term/vote state, heartbeat timeout, TTP owner command payload dispatch | `cargo test -p p2p-frame sn_owner_network -- --nocapture`; unified artifact `test-results/test-runs/20260620T130419Z-p2p-frame+sn-distributed-directory-unit.json` | implemented at unit abstraction and command-dispatch level |
| Leader failover | same | `tick()` timeout starts a new election; offline old leader is ignored if quorum remains | `sn_owner_network_leader_failover_elects_new_leader_after_timeout` via unified artifact | implemented at unit abstraction level |
| Follower-to-leader session forwarding | `proposal.md` P-SN-DIST-CONTROL-3 and `design.md` Directly Mapped Change Items | `commit_or_forward_session`, `receive_session_forward` | `cargo test -p p2p-frame sn_owner_leader_session_replication -- --nocapture`; unified artifact | implemented at unit abstraction level |
| Leader session replication | same | `OwnerSessionReplication`, quorum ack before leader apply, follower `apply_committed_session_entry` | `sn_owner_leader_session_replication_forwards_and_replicates` via unified artifact | implemented at unit abstraction level |
| Implementation scope isolation | task-entry and acceptance rules | admission stamp exists for the two reviewed change_ids | implementation/testing scope checks failed because mixed dirty paths remain | inconsistent |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `sn_owner_network_leader_election` | normal / boundary / negative / error / compatibility / lifecycle / cross-module | `testing.md` Direct Change Coverage, Case-Type Coverage, Unit Tests; `testplan.yaml` step `sn-owner-network-election-unit` | `test-results/test-runs/20260620T130419Z-p2p-frame+sn-distributed-directory-unit.json` | adequate for unit and command dispatch; real TTP multi-process DV gap |
| `sn_owner_leader_session_replication` | normal / boundary / negative / error / compatibility / lifecycle / cross-module | `testing.md` Direct Change Coverage, Case-Type Coverage, Unit Tests; `testplan.yaml` step `sn-owner-leader-session-replication-unit` | `test-results/test-runs/20260620T130419Z-p2p-frame+sn-distributed-directory-unit.json` | adequate for unit and command dispatch; real TTP multi-process DV gap |
| Stage scope isolation | harness/process | pipeline plan records dirty worktree issue | implementation/testing stage-scope commands failed | gap |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-OWNER-NET-001 | proposal P-SN-DIST-CONTROL-2 | owner peers elect a leader via vote quorum and owner command dispatch can carry election payloads | code inspection plus targeted unit | pass |
| AR-OWNER-NET-002 | proposal P-SN-DIST-CONTROL-2 | leader heartbeat/failure timeout enables re-election when quorum remains | code inspection plus targeted unit | pass |
| AR-OWNER-NET-003 | proposal P-SN-DIST-CONTROL-3 | follower forwards Serving SN session mutation to leader instead of directly committing | code inspection plus targeted unit | pass |
| AR-OWNER-NET-004 | proposal P-SN-DIST-CONTROL-3 | leader replicates Serving SN session renew/revoke to followers and nodes converge on committed state | code inspection plus targeted unit | pass |
| AR-HARNESS-001 | task-entry and acceptance rules | implementation/testing diffs are mechanically isolated to admitted stage scope | passing stage-scope checks | fail |
| AR-DV-001 | runtime/integration trigger | owner election/session replication is exercised through real owner-to-owner transport or recorded as a gap | DV/integration evidence | gap |

## Inputs
- `proposal.md`
- `design.md`
- `testing.md`
- `testplan.yaml`
- admission evidence: `harness/evidence/admission/20260620-owner-network-leader-election.md`
- implementation: `p2p-frame/src/sn/directory/control_plane.rs`, `p2p-frame/src/sn/directory/election.rs`, `p2p-frame/src/sn/directory/mod.rs`
- test code: inline Rust unit tests in `p2p-frame/src/sn/directory/election.rs`
- test results: `test-results/test-runs/20260620T130419Z-p2p-frame+sn-distributed-directory-unit.json`
- optional git diff/status evidence
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Review approved requirements and acceptance boundaries
2. Review design against proposal
3. Generate acceptance rules and expected results from proposal, design, implementation, and test implementation
4. Review implementation against proposal and design
5. Review whether test design covers normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases
6. Review tests and results against `testing.md` and `testplan.yaml`
7. Review document and implementation logic for contradictions, invalid assumptions, impossible states, and correctness defects
8. Use diff/status output only when helpful to locate evidence
9. Produce conclusion

## Consistency Summary
- Proposal authority check: approved proposal covers network leader election and leader session replication.
- Proposal vs design: consistent; design excludes full etcd/Raft persistence but requires runtime vote, heartbeat, failover, forwarding, and replication.
- Design vs implementation: implementation provides `OwnerElectionNode` and `OwnerPeerControlClient` abstraction with vote/heartbeat/replicate/forward semantics; `TtpInterSnClient` implements the peer client and inter-SN command dispatch carries owner election/session payloads; session entries are applied through `OwnerControlPlane::apply_committed_session_entry`.
- Design vs testing implementation: testing metadata maps both reviewed change_ids to targeted unit steps and records real TTP multi-process gaps.
- Test implementation vs results: targeted unit tests passed directly and through unified `test-run.py`.
- Test design adequacy: unit-level normal, boundary, negative, error, compatibility, and lifecycle cases are covered for both reviewed change_ids; cross-module real transport remains a recorded DV gap.
- change_id traceability: proposal, design, admission evidence, testing.md, testplan.yaml, and pipeline plan include the reviewed change_ids.
- Public API / codec / runtime semantics review: no client-visible SN protocol shape change identified; new exports are directory-side owner control abstractions.
- Document logic review: proposal, design, and testing consistently scope this as runtime network election/replication without claiming full etcd/Raft persistence or route full-cluster replication.
- Implementation logic review: election and replication logic is separated into `election.rs`, uses quorum vote/ack paths, steps down on higher term responses, applies session entries through the control plane, and the real owner command dispatcher routes vote/heartbeat/replicate/forward payloads to `InterSnPeer`.
- Module facade rule review: `p2p-frame/src/sn/directory/mod.rs` remains facade-only and implementation lives in responsibility files.
- Implementation diff bound to design Scope Paths: failed because shared worktree contains mixed unrelated paths.

## Required Command Evidence
- schema-check.py: `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory` passed.
- admission-check.py: `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_owner_network_leader_election --change-id sn_owner_leader_session_replication --evidence-file harness/evidence/admission/20260620-owner-network-leader-election.md` passed.
- `cargo check -p p2p-frame` passed.
- Targeted unit: `cargo test -p p2p-frame sn_owner_network -- --nocapture` passed, including `sn_owner_network_ttp_command_dispatches_election_payloads`.
- Targeted unit: `cargo test -p p2p-frame sn_owner_leader_session_replication -- --nocapture` passed.
- Regression unit: `cargo test -p p2p-frame sn_distributed_directory -- --nocapture` passed.
- Testing docs: `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --docs testing` passed.
- Testing coverage: `python3 ./harness/scripts/testing-coverage-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_owner_network_leader_election --change-id sn_owner_leader_session_replication` passed.
- Unified unit: `python3 ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory unit` passed with artifact `test-results/test-runs/20260620T130419Z-p2p-frame+sn-distributed-directory-unit.json`.
- stage-scope-check.py: implementation and testing stage scope checks failed due mixed dirty worktree and unrelated paths outside admitted design/testing scope.
- test-run.py <module> all: not run because unit evidence already passed and acceptance is blocked by stage-scope and DV gaps.
- test-run.py all all: not run because acceptance is blocked by stage-scope and DV gaps.
- quality-check.py: `python3 ./harness/scripts/quality-check.py` passed with no configured gates.
- DV/integration: disabled with recorded owner/risk/acceptance-impact gaps; not run.
- Acceptance report check: pending at report creation time.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: network voting, failover, follower forwarding, leader session replication, and real owner command payload dispatch are implemented and covered by targeted/unit harness evidence, but clean acceptance is blocked by failing stage-scope checks in a mixed worktree and by the absence of real TTP-backed multi-process DV for owner-to-owner election/replication.
- Supporting test evidence: `test-results/test-runs/20260620T130419Z-p2p-frame+sn-distributed-directory-unit.json`
- Residual risk: the transport adapter is unit-covered at command dispatch level, but not yet validated through real multi-process TTP listener/connect lifecycle.

## Follow-Up Tasks
- Requirement task: none for the approved first-version network leader requirement.
- Design task: none for the approved first-version unit-abstraction boundary; add design if real TTP adapter semantics need new wire fields.
- Implementation task: isolate unrelated worktree changes before rerunning scope gates; no additional unit-level TTP command adapter work is pending for this change_id.
- Testing task: add a DV harness that starts multiple OwnerDirectoryServer instances and validates election/failover/session replication through real owner peer transport.
- Testing return reason if coverage is incomplete: unit coverage is adequate for the new state-machine/network abstraction; multi-process DV remains a recorded gap.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not-applicable
