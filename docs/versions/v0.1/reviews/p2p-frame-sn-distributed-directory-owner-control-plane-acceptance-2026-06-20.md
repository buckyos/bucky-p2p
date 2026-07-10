# p2p-frame SN Distributed Directory Owner Control Plane Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-OWNER-CONTROL-SCOPE-001 | high | implementation/testing | `stage-scope-check.py` implementation and testing runs failed | The shared dirty worktree contains unrelated and cross-stage paths outside this task's admitted implementation/testing scopes, so the implementation diff cannot be mechanically isolated to `sn_owner_control_plane_sessions` / `sn_partitioned_peer_route`. | implementation diff was not bound to admitted design Scope Paths; single-stage scope checks failed |
| F-OWNER-CONTROL-DV-001 | medium | testing | `testing.md` and `testplan.yaml` DV/integration levels remain disabled | The delivered Raft-style behavior is validated as an in-memory state machine and route-store unit behavior, but no multi-process owner-to-owner Raft transport DV exists. | runtime/integration evidence gap remains |

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- change_id values reviewed: `sn_owner_control_plane_sessions`, `sn_partitioned_peer_route`
- Review date: 2026-06-20
- In scope: approved proposal/design/testing/admission, owner control plane state machine, Serving SN session/epoch, partitioned peer route store, targeted unit evidence.
- Out of scope: unrelated dirty worktree paths, complete etcd-compatible Raft, persistent WAL/snapshot, membership migration, full multi-process DV.

## Optional Diff / Status Evidence
- `git status --short` summary: mixed dirty worktree includes this task's docs/code plus pre-existing harness, TTP, QUIC, p2p-frame, cyfs-p2p-test, sn-miner, and review/evidence paths.
- `git diff --stat` summary: not used as the final acceptance basis.
- `git diff --name-status` summary: task-relevant changed paths include `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/{proposal,design,testing,testplan.yaml}`, `p2p-frame/src/sn/directory/mod.rs`, `harness/pipeline-plan.md`, and the admission evidence.
- `git diff --check` result: not run.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| Owner SN control/session small-state sync | `proposal.md` P-SN-DIST-CONTROL-1 and `design.md` Directly Mapped Change Items | `OwnerControlPlane`, `ServingSnSession`, leader-only mutation, quorum checks, renew/revoke APIs | `sn_owner_control_plane_sessions_require_leader_and_quorum`; unified artifact `test-results/test-runs/20260620T080012Z-p2p-frame+sn-distributed-directory-unit.json` | implemented |
| Serving SN session/epoch hides old routes | proposal success evidence and design Data and State | `OwnerDirectoryStore::revoke_serving_session`, `PeerRouteStore::query` session filtering | `sn_partitioned_peer_route_filters_revoked_serving_session`; unified artifact `test-results/test-runs/20260620T080012Z-p2p-frame+sn-distributed-directory-unit.json` | implemented |
| Existing distributed directory compatibility | proposal client-visible compatibility boundary | old `ServingLease` wrapper routes through new session/route storage | `cargo test -p p2p-frame sn_distributed_directory -- --nocapture` and unified artifact | implemented |
| Implementation scope isolation | task-entry and acceptance rules | admitted scope paths are recorded in `20260620-sn-owner-control-plane...stamp.json` | implementation/testing scope checks failed because mixed dirty paths remain | inconsistent |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `sn_owner_control_plane_sessions` | normal / boundary / negative / error / compatibility / lifecycle / cross-module | `testing.md` Direct Change Coverage, Case-Type Coverage, Unit Tests; `testplan.yaml` step `sn-owner-control-plane-unit` | `test-results/test-runs/20260620T080012Z-p2p-frame+sn-distributed-directory-unit.json` | adequate for unit; cross-module DV gap |
| `sn_partitioned_peer_route` | normal / boundary / negative / error / compatibility / lifecycle / cross-module | `testing.md` Direct Change Coverage, Case-Type Coverage, Unit Tests; `testplan.yaml` step `sn-partitioned-peer-route-unit` | `test-results/test-runs/20260620T080012Z-p2p-frame+sn-distributed-directory-unit.json` | adequate for unit; cross-module DV gap |
| Stage scope isolation | harness/process | pipeline plan records dirty worktree issue | implementation/testing stage-scope commands failed | gap |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-OWNER-CONTROL-001 | proposal P-SN-DIST-CONTROL-1 | owner control/session mutation is leader/quorum gated | code inspection plus targeted unit | pass |
| AR-OWNER-CONTROL-002 | proposal P-SN-DIST-CONTROL-1 | Serving SN revoke/expiry makes old route unavailable | code inspection plus targeted unit | pass |
| AR-OWNER-CONTROL-003 | proposal P-SN-DIST-ROUTE-1 | peer route is partition-store data, not full ownerSN broadcast | code inspection and Directly Mapped Change Items | pass |
| AR-HARNESS-001 | task-entry and acceptance rules | implementation/testing diffs are mechanically isolated to admitted stage scope | passing stage-scope checks | fail |
| AR-DV-001 | runtime/integration trigger | multi-owner control-plane transport is exercised across processes or recorded as a gap | DV/integration evidence | gap |

## Inputs
- `proposal.md`
- `design.md`
- test implementation and optional `testing.md`
- `testplan.yaml` for completed testing work
- optional `acceptance.md`: not used
- long-lived module doc: not changed in this task
- implementation: `p2p-frame/src/sn/directory/mod.rs`
- test code: inline Rust unit tests in `p2p-frame/src/sn/directory/mod.rs`
- test results: `test-results/test-runs/20260620T080012Z-p2p-frame+sn-distributed-directory-unit.json`
- optional git diff/status evidence
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Review approved requirements and acceptance boundaries
2. Review design against proposal
3. Generate or finalize acceptance rules and expected results from proposal, design, implementation, and test implementation
4. Review implementation against proposal and design
5. Review whether test design reasonably covers proposal/design/code behavior, including normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases where applicable
6. Review tests and results against generated test evidence, optional `testing.md`, and required completed-testing `testplan.yaml`
7. Review document and implementation logic for contradictions, invalid assumptions, impossible states, and correctness defects
8. Use diff/status output only when helpful to locate evidence
9. Produce conclusion

## Consistency Summary
- Proposal authority check: approved proposal covers `sn_owner_control_plane_sessions` and `sn_partitioned_peer_route`.
- Proposal vs design: consistent; design keeps structure details in design and scopes first version to in-memory Raft-style state machine.
- Design vs testing implementation: testing metadata maps both change_ids to targeted unit steps.
- Design vs long-lived boundary doc: p2p-frame owns `src/sn/**`; no contradiction found for directory-local implementation.
- Design vs implementation: implementation matches first-version owner control plane, session renewal/revoke, and route filtering.
- Test implementation vs test code vs results: inline Rust tests are registered through `testplan.yaml` and passed via unified test-run artifact.
- Test design adequacy: adequate for unit-level state machine and route filtering; DV/integration remains a recorded gap.
- change_id traceability: proposal, design, admission evidence, testing.md, testplan.yaml, and pipeline plan include the reviewed change_ids.
- Acceptance criteria traceability: AR-OWNER-CONTROL-001 through AR-OWNER-CONTROL-003 pass; AR-HARNESS-001 fails; AR-DV-001 remains gap.
- Cross-module admission: p2p-frame/sn-distributed-directory schema and admission checks passed for the reviewed change_ids.
- Public API / codec / runtime semantics review: no client-visible SN protocol shape change identified; new types are internal directory-side behavior.
- Document logic review: proposal/design/testing consistently separate control/session small state from partitioned peer route.
- Implementation logic review: leader/quorum state machine is in-memory and testable; route query filters revoked sessions.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed after proposal/design/testing approval hash updates.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): failed because the worktree includes mixed unrelated and cross-stage dirty paths.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not-applicable; this is feature implementation, not bugfix regression work.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version <version> --module <module>`: `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory` passed.
- `uv run --active python ./harness/scripts/admission-check.py --version <version> --module <module> --change-id <change_id> --evidence-file harness/evidence/admission/<task-id>.md`: `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_owner_control_plane_sessions --change-id sn_partitioned_peer_route --evidence-file harness/evidence/admission/20260620-sn-owner-control-plane.md` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage <stage>`: implementation and testing stage-scope checks failed due mixed dirty worktree; see F-OWNER-CONTROL-SCOPE-001.
- Unit test command through `harness/scripts/test-run.py`: `python3 ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory unit` passed with artifact `test-results/test-runs/20260620T080012Z-p2p-frame+sn-distributed-directory-unit.json`.
- DV test command through `harness/scripts/test-run.py`: DV is disabled in `testplan.yaml` with owner/risk/acceptance impact recorded; not run.
- Integration test command through `harness/scripts/test-run.py`: integration is disabled in `testplan.yaml` with owner/risk/acceptance impact recorded; not run.
- Module all command through `harness/scripts/test-run.py <module> all`: not run because acceptance is already needs changes due failing stage-scope gate.
- Project all command through `harness/scripts/test-run.py all all`: not run because acceptance is already needs changes due failing stage-scope gate.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): not available for this acceptance attempt because whole-project tests were not run after the scope-gate failure.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed; `harness/quality-gates.yaml` declares `gates: []`.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not-applicable because no quality gates are configured and no quality artifact was written.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not run because acceptance is already needs changes due failing stage-scope gate.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: pending at report creation time.
- Targeted migration search, when applicable: not-applicable; no public migration search required for the reviewed internal directory change.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: owner control/session and partitioned route proposal/design/code/testing evidence is coherent and targeted unit tests pass, but implementation/testing stage-scope gates fail because the worktree is not isolated to this admitted task.
- Supporting test evidence: `test-results/test-runs/20260620T080012Z-p2p-frame+sn-distributed-directory-unit.json`
- Residual risk: mixed worktree scope prevents clean mechanical acceptance; multi-process ownerSN Raft transport remains a recorded DV/integration gap.

## Follow-Up Tasks
- Requirement task: none for the approved first-version control/session and route semantics.
- Design task: none for the approved first-version scope.
- Implementation task: isolate or commit unrelated dirty paths, then rerun implementation stage-scope for `sn_owner_control_plane_sessions` and `sn_partitioned_peer_route`.
- Testing task: isolate or commit unrelated dirty paths, then rerun testing stage-scope for `sn-distributed-directory`.
- Testing return reason if coverage is incomplete: unit coverage is adequate for first-version state machine and route filtering; multi-process DV remains a recorded gap.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not-applicable
