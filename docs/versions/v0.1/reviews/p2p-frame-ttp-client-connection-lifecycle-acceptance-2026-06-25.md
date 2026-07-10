# p2p-frame TTP Client Connection Lifecycle Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-001 | high | acceptance | `python3 ./harness/scripts/test-run.py all all` attempted twice and was interrupted with exit 130 while waiting in `cargo test -p cyfs-p2p`; no fresh `test-results/test-runs/*-all-all.json` artifact was produced | Final accepted conclusion requires a fresh passing whole-project `all all` artifact; current evidence only proves targeted and p2p-frame unit coverage. | accepted conclusion requires whole-project test run artifact |
| F-002 | high | implementation | `admission-check.py` passed and production diff is limited to `p2p-frame/src/ttp/client.rs`, but `stage-scope-check.py --stage implementation --change-id ttp_client_connection_lifecycle` reports `docs/versions/v0.1/modules/p2p-frame/design.md` as an implementation-stage scope violation in the uncommitted multi-stage workspace | Implementation scope binding could not be mechanically recorded in the final combined worktree, even though admission scope paths were written and code is inside the admitted production path. | implementation diff was not bound to admitted design Scope Paths |

## Object and Scope
- Module: `p2p-frame`
- Version: `v0.1`
- change_id values reviewed: `ttp_client_connection_lifecycle`
- Review date: 2026-06-25
- In scope: approved proposal/design/testing evidence, `p2p-frame/src/ttp/client.rs`, `p2p-frame/src/ttp/tests.rs`, admission evidence, targeted/unit command results, pipeline plan.
- Out of scope: unrelated pre-existing untracked files and older review/admission artifacts already present in the workspace.

## Optional Diff / Status Evidence
- `git status --short` summary: tracked changes in proposal/design/testing/testplan/pipeline plus `p2p-frame/src/ttp/client.rs` and `p2p-frame/src/ttp/tests.rs`; many pre-existing untracked review/admission files remain unrelated.
- `git diff --stat` summary: 7 tracked files changed, 526 insertions and 115 deletions before this report.
- `git diff --name-status` summary: modified proposal, design, testing, testplan, pipeline plan, TTP client, and TTP tests.
- `git diff --check` result: passed for changed docs and TTP code/test files.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| Maintained server target deletion | `proposal.md` row `P-TTP-CLIENT-CONNECTION-LIFECYCLE-1`; `design.md` row `ttp_client_connection_lifecycle` | `TtpClient::remove_server(&TtpTarget)` removes exact `(remote_id, remote_ep)` target and refreshes cache maintained state | `cargo test -p p2p-frame ttp_client -- --nocapture` and `python3 ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260625T042003Z-p2p-frame-unit.json` | implemented |
| Non-maintained idle cache release | same proposal/design rows | `TtpClient` cache entries track `maintained`, `last_used`, and `leases`; idle release removes only non-maintained zero-lease local cache entries | Added unit tests cover release after test idle timeout and re-create behavior | implemented |
| Active channel retention | same proposal/design rows | Returned stream/control/datagram write handles are wrapped with lease guards that decrement on final drop | Added unit test holds stream read/write across idle release and verifies no re-create until after drop and second release | implemented |
| Maintained target retention | same proposal/design rows | Cache entry maintained state is refreshed after `connect_server` / `remove_server`; maintained entries are retained by idle release | Added unit test verifies maintained target is not released after idle timeout | implemented |
| Full acceptance execution evidence | `acceptance-review-rules.md` required harness commands | Implementation and p2p-frame unit evidence exist | `all all` run did not complete and no fresh all-all artifact exists | missing |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `ttp_client_connection_lifecycle` | normal / boundary / negative / error / compatibility / lifecycle / cross-module | `testing.md` Direct Change Coverage and Unit Tests rows map remove, idle release, active lease, maintained retention, and compatibility boundaries | Targeted 4-test run passed; unified p2p-frame unit run passed with 152 tests | adequate |
| Whole-workspace compatibility | compatibility / cross-module | `testplan.yaml` integration/all evidence is required by acceptance | `python3 ./harness/scripts/test-run.py all all` did not complete; no fresh all-all artifact | not runnable |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| A-TTP-CLIENT-1 | proposal/design/code/tests | Maintained target can be removed idempotently without peer deletion or global tunnel close | `remove_server` implementation and unit test evidence | pass |
| A-TTP-CLIENT-2 | proposal/design/code/tests | Non-maintained idle cache release only removes local cache entries with zero leases | code review plus idle release unit tests | pass |
| A-TTP-CLIENT-3 | proposal/design/code/tests | Active stream/control/datagram/pending lease prevents idle release | lease wrapper implementation plus active stream unit test | pass |
| A-TTP-CLIENT-4 | acceptance rules | Final accepted conclusion has fresh passing whole-project `all all` artifact | `test-results/test-runs/*-all-all.json` from current run | gap |
| A-TTP-CLIENT-5 | acceptance rules | Implementation diff is mechanically bound to admitted design scope paths | passing implementation `stage-scope-check.py --change-id ttp_client_connection_lifecycle` | fail |

## Inputs
- `proposal.md`: reviewed approved `ttp_client_connection_lifecycle` row.
- `design.md`: reviewed approved Directly Mapped Change Items row and lifecycle API/cache constraints.
- test implementation and optional `testing.md`: reviewed new TTP client lifecycle tests and testing coverage rows.
- `testplan.yaml` for completed testing work, or a versioned local exception with reason, owner, risk, and acceptance impact: reviewed updated unit/integration change ids; no exception used.
- optional `acceptance.md`: not present for this packet.
- long-lived module doc: `docs/modules/p2p-frame.md` boundary was not changed by this task.
- implementation: reviewed `p2p-frame/src/ttp/client.rs`.
- test code: reviewed `p2p-frame/src/ttp/tests.rs`.
- test results: targeted TTP client test and p2p-frame unit run.
- optional git diff/status evidence: recorded above.
- `harness/rules/acceptance-review-rules.md`: reviewed and applied.

## Review Order
1. Review approved requirements and acceptance boundaries
2. Review design against proposal
3. Generate or finalize acceptance rules and expected results from proposal, design, implementation, and test implementation
4. Review implementation against proposal and design
5. Review whether test design reasonably covers proposal/design/code behavior, including normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases where applicable
6. Review tests and results against generated test evidence, optional `testing.md`, and required completed-testing `testplan.yaml` or its versioned exception
7. Review document and implementation logic for contradictions, invalid assumptions, impossible states, and correctness defects
8. Use diff/status output only when helpful to locate evidence
9. Produce conclusion

## Consistency Summary
- Proposal authority check: `schema-check.py --version v0.1 --module p2p-frame` passed after proposal/design/testing approvals.
- Proposal vs design: design directly maps `P-TTP-CLIENT-CONNECTION-LIFECYCLE-1` to `ttp_client_connection_lifecycle` with matching scope and non-goals.
- Design vs testing implementation: testing rows cover remove, idle release, active lease retention, maintained retention, and compatibility boundaries.
- Design vs long-lived boundary doc: no long-lived module boundary change was required; behavior remains within TTP client.
- Design vs implementation: `client.rs` implements exact target removal, local metadata cache, lease wrappers, and local-only idle release.
- Test implementation vs test code vs results: four new `ttp_client_*` tests passed directly and through p2p-frame unit entry.
- Test design adequacy: adequate for changed TTP client behavior, but whole-workspace acceptance execution evidence is missing.
- change_id traceability: proposal, design, admission evidence, testing.md, and testplan.yaml all include `ttp_client_connection_lifecycle`.
- Acceptance criteria traceability: maintained deletion, idle release, active lease retention, maintained retention, and no trait/wire/server change are all traced to code/test evidence.
- Cross-module admission: implementation admission passed for `p2p-frame`; no other module carries implementation evidence for this change.
- Public API / codec / runtime semantics review: adds `TtpClient::remove_server`; no `Tunnel` / `TunnelNetwork` trait, wire, codec, publish, or `TtpServer` behavior change observed.
- Document logic review: no contradiction found in proposal/design/testing for the lifecycle behavior.
- Implementation logic review: no lifecycle correctness defect found in reviewed code; final acceptance blocked by missing mechanical evidence, not by code behavior.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed after all approvals.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): not fully recorded; checker reports design.md in the combined uncommitted worktree.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not a bugfix; new behavior/change request.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version <version> --module <module>`: `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` passed.
- `uv run --active python ./harness/scripts/admission-check.py --version <version> --module <module> --change-id <change_id> --evidence-file harness/evidence/admission/<task-id>.md`: passed for `ttp_client_connection_lifecycle` and wrote `harness/evidence/admission/20260625-ttp-client-connection-lifecycle.p2p-frame.stamp.json`.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage <stage>`: proposal, design, and testing scope checks passed with isolation; implementation scope check is unresolved because combined uncommitted design diff is reported as a violation.
- Unit test command through `harness/scripts/test-run.py`: `python3 ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260625T042003Z-p2p-frame-unit.json`.
- DV test command through `harness/scripts/test-run.py`: p2p-frame DV is disabled by `testplan.yaml`.
- Integration test command through `harness/scripts/test-run.py`: not completed; `all all` attempts were interrupted while waiting on `cargo test -p cyfs-p2p`.
- Module all command through `harness/scripts/test-run.py <module> all`: not separately run; unit evidence passed, DV disabled, integration/all blocked by whole-workspace run issue.
- Project all command through `harness/scripts/test-run.py all all`: attempted twice; both interrupted with exit 130 while waiting in `cargo test -p cyfs-p2p`; no fresh artifact.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): missing for this review; older `test-results/test-runs/20260622T143945Z-all-all.json` is stale and not accepted as current evidence.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: `python3 ./harness/scripts/quality-check.py` passed because `harness/quality-gates.yaml` declares `gates: []`.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not applicable; no quality gates configured.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not run because the unified `all all` entry itself did not complete.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: pending at report creation; should pass structurally for `needs changes`.
- Targeted migration search, when applicable: not required; no old public symbol, codec, or wire-format migration.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: Code and p2p-frame unit evidence satisfy the reviewed lifecycle behavior, but acceptance cannot be marked accepted without a fresh passing `all all` artifact and a mechanically passing implementation stage-scope check in the final combined worktree.
- Supporting test evidence: `cargo test -p p2p-frame ttp_client -- --nocapture` passed 4/4; `python3 ./harness/scripts/test-run.py p2p-frame unit` passed 152/152 with artifact `test-results/test-runs/20260625T042003Z-p2p-frame-unit.json`.
- Residual risk: whole-workspace compatibility is not proven in this review because `all all` did not complete; implementation scope evidence must be rerun in an isolated stage diff or after committing/otherwise separating prior document-stage changes.

## Follow-Up Tasks
- Requirement task: none.
- Design task: none unless implementation scope isolation requires a document/process adjustment.
- Implementation task: rerun implementation `stage-scope-check.py --stage implementation --change-id ttp_client_connection_lifecycle` in a worktree where approved design/proposal changes are not part of the implementation diff, or after stage changes are committed separately.
- Testing task: rerun `python3 ./harness/scripts/test-run.py all all` to completion and cite the fresh all-all artifact.
- Testing return reason if coverage is incomplete: whole-workspace artifact missing; TTP unit coverage itself is adequate.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
