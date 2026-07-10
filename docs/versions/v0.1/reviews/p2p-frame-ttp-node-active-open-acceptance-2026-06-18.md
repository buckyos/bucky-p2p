# p2p-frame TTP Node Active Open Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-001 | high | implementation/testing | `stage-scope-check.py --stage implementation ... --change-id ttp_node_active_open --ignore-untracked` and `stage-scope-check.py --stage testing ... --ignore-untracked` | The delivered code and tests satisfy the mapped behavior, but the required stage scope gates cannot mechanically isolate this task because the shared worktree contains many unrelated tracked modifications outside the admitted implementation and testing scopes. | implementation diff was not bound to admitted design Scope Paths; single-stage scope checks failed |

## Object and Scope
- Module: `p2p-frame`
- Version: `v0.1`
- change_id values reviewed: `ttp_node_active_open`
- Review date: 2026-06-18
- In scope: `TtpNode` production behavior, shared active-open helper reuse, `TtpServer` lookup-only compatibility, TTP unit tests, testing metadata, admission evidence, pipeline record.
- Out of scope: unrelated existing tracked modifications in SN, QUIC listener, harness scaffolding, scripts, and downstream binaries.

## Optional Diff / Status Evidence
- `git status --short` summary: not used as acceptance authority; stage-scope output listed unrelated tracked modifications.
- `git diff --stat` summary: scoped task diff covers `proposal.md`, `design.md`, `testing.md`, `testplan.yaml`, `harness/pipeline-plan.md`, `p2p-frame/src/ttp/client.rs`, `p2p-frame/src/ttp/server.rs`, and `p2p-frame/src/ttp/tests.rs`.
- `git diff --name-status` summary: scoped task files are modified; no delete/rename in scoped evidence.
- `git diff --check` result: passed for scoped task paths.
- Note: diff/status output is discovery evidence only; the blocking finding is the required mechanical stage-scope gate failure.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| `TtpNode` exposes the same listener/connector surface as `TtpServer` | `proposal.md` / `design.md` change_id `ttp_node_active_open` | `p2p-frame/src/ttp/node.rs` defines `TtpNode`, `TtpNodeRef`, `TtpPortListener`, and `TtpConnector` impls | `cargo test -p p2p-frame ttp -- --nocapture` passed; unified unit artifact `test-results/test-runs/20260618T084730Z-p2p-frame-unit.json` | implemented |
| `open_stream` and `open_control_stream` actively create missing tunnels | `design.md` Directly Mapped Change Items | `TtpNode::get_or_create_tunnel` calls shared `get_or_create_tunnel_for(...)` from `client.rs`, then opens stream/control stream | `node_open_stream_and_control_stream_create_missing_tunnel` passed | implemented |
| Existing tunnel reuse and id-only target matching | `design.md` active-open helper reuse and target matching boundary | shared `find_existing_tunnel_in(...)` retains connected tunnels and uses `match_target(...)` | `node_reuses_existing_tunnel_for_id_target_and_datagram` passed | implemented |
| `open_datagram` follows the first-version active-open design | `design.md` datagram boundary | `TtpNode::open_datagram(...)` uses the same active-open helper | `node_reuses_existing_tunnel_for_id_target_and_datagram` passed | implemented |
| `TtpServer` remains lookup-only | proposal non-regression boundary and design invariants | `TtpServer::open_*` still calls `get_existing_tunnel(...)`, not active-open helper | `server_open_stream_requires_existing_incoming_tunnel` passed | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `ttp_node_active_open` | normal / boundary / negative / error / compatibility / lifecycle / cross-module | `testing.md` Direct Change Coverage row `V-TTP-NODE-ACTIVE-OPEN-UNIT`; Case-Type Coverage rows for all required case types | `cargo test -p p2p-frame ttp -- --nocapture` passed; `python3 ./harness/scripts/test-run.py p2p-frame unit` passed with artifact `test-results/test-runs/20260618T084730Z-p2p-frame-unit.json` | adequate |
| stage scope isolation | compatibility / lifecycle | `harness/pipeline-plan.md` Return Records `SCOPE-WORKTREE-MIXED-1` | implementation and testing `stage-scope-check.py` failed due unrelated tracked modifications | gap |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-001 | proposal/design | `TtpNode::open_stream(...)` creates, attaches, caches, and opens a tunnel when none exists | code review plus unit test proving create count and opened stream vport | pass |
| AR-002 | proposal/design | `TtpNode::open_control_stream(...)` reuses a matching existing tunnel and opens a control stream | code review plus unit test proving no second create and opened control vport | pass |
| AR-003 | proposal/design | `TtpServer` missing-tunnel open behavior remains lookup-only and returns `NotFound` | code review plus existing compatibility unit test | pass |
| AR-004 | testing rules | Testing metadata maps `ttp_node_active_open` to runnable unified test entry | `testing-coverage-check.py --change-id ttp_node_active_open` | pass |
| AR-005 | implementation/testing scope gates | implementation and testing diffs are mechanically isolated to allowed scopes | `stage-scope-check.py` for implementation and testing | fail |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `harness/evidence/admission/20260618-ttp-node-active-open.md`
- `harness/pipeline-plan.md`
- `p2p-frame/src/ttp/client.rs`
- `p2p-frame/src/ttp/server.rs`
- `p2p-frame/src/ttp/tests.rs`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed approved proposal item `ttp_node_active_open`.
2. Reviewed approved design mapping for `TtpNode`, active-open helper reuse, datagram boundary, and `TtpServer` compatibility.
3. Reviewed implementation in `client.rs` and `server.rs`.
4. Reviewed tests in `ttp/tests.rs`.
5. Reviewed testing metadata and `testplan.yaml`.
6. Reviewed command evidence and stage-scope failures.
7. Produced needs-changes conclusion because required scope gates failed.

## Consistency Summary
- Proposal authority check: approved proposal directly covers `TtpNode` active-open behavior and `TtpServer` compatibility boundary.
- Proposal vs design: consistent; design explicitly maps `open_stream`, `open_control_stream`, and first-version `open_datagram` to active-open helper reuse.
- Design vs testing implementation: consistent; testing metadata and unit tests exercise create, reuse, error, and compatibility cases.
- Design vs long-lived boundary doc: no long-lived module boundary change was required for this local TTP API addition.
- Design vs implementation: consistent for reviewed behavior; `TtpNode` uses shared active-open helper while `TtpServer` remains lookup-only.
- Test implementation vs test code vs results: consistent; targeted TTP and unified unit tests passed.
- Test design adequacy: adequate for behavior, but final acceptance has a scope-gate gap due unrelated tracked modifications.
- change_id traceability: `ttp_node_active_open` appears in proposal, design, testing coverage, and `testplan.yaml` unit step.
- Acceptance criteria traceability: all behavior criteria have code and unit evidence; mechanical stage-scope criterion failed.
- Cross-module admission: no cross-module code evidence is required for this change; unrelated cross-module dirty files caused scope-gate failure.
- Public API / codec / runtime semantics review: no trait, codec, or wire-format change; public addition is `TtpNode` / `TtpNodeRef`.
- Document logic review: no contradiction found in proposal/design/testing for the reviewed behavior.
- Implementation logic review: no correctness issue found in `TtpNode` active-open path; shared helper preserves cache cleanup and target matching.
- Document approval timing (approved_content_sha256 verified by schema-check): passed via `schema-check.py`.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): failed due unrelated tracked modifications.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not applicable; this is a feature addition.

## Required Command Evidence
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`: passed.
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id ttp_node_active_open --evidence-file harness/evidence/admission/20260618-ttp-node-active-open.md`: passed and wrote `harness/evidence/admission/20260618-ttp-node-active-open.p2p-frame.stamp.json`.
- `python3 ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --change-id ttp_node_active_open --ignore-untracked` and `python3 ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --ignore-untracked`: failed due unrelated tracked modifications.
- Unit test command through `harness/scripts/test-run.py`: `python3 ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260618T084730Z-p2p-frame-unit.json`.
- DV test command through `harness/scripts/test-run.py`: not run; p2p-frame DV is disabled in `testplan.yaml`.
- Integration test command through `harness/scripts/test-run.py`: not run because acceptance returned after required stage-scope gates failed.
- Module all command through `harness/scripts/test-run.py <module> all`: not run because acceptance returned after required stage-scope gates failed.
- Project all command through `harness/scripts/test-run.py all all`: not run because acceptance returned after required stage-scope gates failed.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): not produced; conclusion is `needs changes`.
- Quality gates `python3 ./harness/scripts/quality-check.py`: passed with no gates configured in `harness/quality-gates.yaml`.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not applicable; gates list is explicitly empty.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not run because acceptance returned after required stage-scope gates failed.
- Acceptance report check `python3 ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-ttp-node-active-open-acceptance-2026-06-18.md`: passed.
- Targeted migration search, when applicable: not applicable; no migration or removed symbol.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: implementation and testing behavior are complete and tested, but mandatory stage-scope gates failed in the shared dirty worktree.
- Supporting test evidence: `cargo check -p p2p-frame`, `cargo test -p p2p-frame ttp -- --nocapture`, and `python3 ./harness/scripts/test-run.py p2p-frame unit` all passed.
- Residual risk: final acceptance must be rerun from an isolated diff or clean worktree so stage-scope gates can verify only this task's changes.

## Follow-Up Tasks
- Requirement task: none identified.
- Design task: none identified.
- Implementation task: isolate this task's production diff from unrelated tracked modifications and rerun implementation stage scope check.
- Testing task: isolate this task's testing/doc diff from unrelated tracked modifications and rerun testing stage scope check.
- Testing return reason if coverage is incomplete: coverage is complete for behavior; return reason is mechanical scope isolation failure.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
