# p2p-frame SN Client Protocol Priority Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-001 | high | acceptance | `test-results/test-runs/20260703T091032Z-all-all.json` | Required whole-project `test-run.py all all` artifact has `exit_code: 1`; all visible cargo/testplan commands completed before the final workspace-harness step, but that step invokes `admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive` without `--evidence-file`. This is outside `sn_client_protocol_priority`, but it blocks an accepted conclusion under repository acceptance rules. | accepted conclusion requires fresh passing whole-project test run artifact |
| F-002 | high | acceptance | `stage-scope-check.py --stage implementation` and `stage-scope-check.py --stage testing` | Single-stage scope checks fail because this auto-pipeline turn intentionally contains proposal, design, implementation, testing, and pipeline-plan changes in one uncommitted worktree. The implementation production path itself is admitted by design scope, but the mechanical checker has no per-stage isolation mode for the current combined diff. | implementation/testing scope check missing or failing blocks accepted conclusion |

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- change_id values reviewed: `sn_client_protocol_priority`
- Review date: 2026-07-03
- In scope: SN client QUIC-first/TCP-fallback candidate ordering, QUIC success stop condition, TCP fallback condition, active SN deduplication, testing metadata, admission evidence, and pipeline evidence.
- Out of scope: pre-existing untracked reviews/evidence, unrelated workspace-harness all-all admission command for `endpoint_area_server_reflexive`, and unrelated module changes not touched by this task.

## Optional Diff / Status Evidence
- `git status --short` summary: reviewed; tracked task files are proposal/design/testing/testplan/pipeline plus `p2p-frame/src/sn/client/sn_service.rs`; many unrelated pre-existing untracked files are present.
- `git diff --stat` summary: 6 tracked files, 210 insertions, 90 deletions.
- `git diff --name-status` summary: modified proposal/design/testing/testplan/pipeline and `p2p-frame/src/sn/client/sn_service.rs`.
- `git diff --check` result: passed.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| SN client tries QUIC before TCP for the same SN | `proposal.md` `P-SN-CLIENT-PROTOCOL-PRIORITY-1`; `design.md` Directly Mapped row | `p2p-frame/src/sn/client/sn_service.rs` sorts listener entries with `sort_sn_client_listener_entries(...)` and loops per SN before protocol candidates | `test-results/test-runs/20260703T090130Z-p2p-frame-unit.json`; `sn_client_listener_entries_are_quic_first_then_tcp` | implemented |
| TCP fallback happens only after QUIC candidate failure | approved design `sn_client_protocol_priority` | `ping_proc` attempts candidates in sorted order and continues after failed classified tunnel/report; TCP candidates appear after QUIC entries | `cargo test -p p2p-frame sn_client` inside `test-results/test-runs/20260703T090130Z-p2p-frame-unit.json`; code inspection | implemented |
| QUIC report success stops TCP attempts for that SN | approved proposal/design stop condition | `ping_proc` sets `sn_reported = true` after successful report and breaks listener/endpoint/protocol loops for that SN | unit helper coverage plus code inspection; full p2p-frame unit artifact `test-results/test-runs/20260703T090130Z-p2p-frame-unit.json` | implemented |
| Active SN list avoids duplicate successful records for same SN | approved design active SN deduplication | successful path checks existing `active_sn_list` by `sn_peer_id` before push | code inspection; full p2p-frame unit artifact `test-results/test-runs/20260703T090130Z-p2p-frame-unit.json` | implemented |
| TTP target matching and `connect_server(...)` semantics unchanged | proposal non-goals and design rollback notes | production changes stay in `sn/client/sn_service.rs`; no `ttp` code changed | admission scope paths and diff evidence | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `sn_client_protocol_priority` | normal / boundary / negative / error / compatibility / lifecycle / cross-module | `testing.md` Direct Change Coverage and Case-Type Coverage map all case types to `V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT`; `testplan.yaml` adds targeted `p2p-frame-sn-client-protocol-priority` step | `test-results/test-runs/20260703T090130Z-p2p-frame-unit.json` and `cargo test -p p2p-frame sn_client` | adequate |
| Existing TCP/unspecified listener classification compatibility | boundary / compatibility | Code inspection confirms the original inline classification branch remains in `ping_proc`; no helper extraction or new behavior is retained | unchanged inline branch in `p2p-frame/src/sn/client/sn_service.rs` | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-01 | proposal/design | Same-SN candidates are attempted QUIC before TCP | code inspection and unit test | pass |
| AR-02 | design/code | A successful QUIC report stops same-SN TCP fallback | code inspection | pass |
| AR-03 | design/code | Failed QUIC tunnel/report can continue to TCP fallback | code inspection | pass |
| AR-04 | proposal non-goals | No SN wire, control-stream signaling, TCP/QUIC wire, TTP matching, or `connect_server(...)` semantic change | diff/admission evidence | pass |
| AR-05 | acceptance gate | Fresh whole-project `test-run.py all all` artifact passes | `test-results/test-runs/20260703T091032Z-all-all.json` | fail |
| AR-06 | acceptance gate | Implementation/testing scope checks pass or are mechanically isolated | stage-scope-check outputs | fail |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `harness/evidence/admission/20260703-sn-client-protocol-priority.md`
- `p2p-frame/src/sn/client/sn_service.rs`
- `harness/pipeline-plan.md`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed approved proposal requirement and non-goals.
2. Reviewed approved design mapping, state flow, rollback notes, and scope paths.
3. Generated acceptance rules from proposal, design, implementation, and tests.
4. Reviewed implementation in `sn_service.rs`.
5. Reviewed test design coverage for normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases.
6. Reviewed runnable test artifacts and harness command results.
7. Reviewed document approval hashes through `schema-check.py`.
8. Used diff/status output only to locate evidence.
9. Produced a needs-changes conclusion because repository acceptance gates still fail.

## Consistency Summary
- Proposal authority check: proposal is approved and directly covers QUIC-first/TCP-fallback behavior under `sn_client_protocol_priority`.
- Proposal vs design: design preserves proposal boundaries and maps the change to `p2p-frame/src/sn/client/sn_service.rs`.
- Design vs testing implementation: testing metadata and unit tests cover protocol ordering and compatibility helpers; code inspection covers success-stop and deduplication because the changed async loop is not isolated behind a fake command tunnel interface.
- Design vs long-lived boundary doc: no long-lived module boundary conflict found; behavior stays inside SN client.
- Design vs implementation: implementation follows the designed ordered per-SN candidate loop and does not modify TTP internals.
- Test implementation vs test code vs results: new tests are reachable through `test-run.py p2p-frame unit` and passed.
- Test design adequacy: adequate for the reviewed change; final acceptance is blocked by global all-all and stage-scope mechanics.
- change_id traceability: `sn_client_protocol_priority` appears in proposal, design, testing.md, testplan.yaml, admission evidence, and pipeline plan.
- Acceptance criteria traceability: all SN-client criteria have code and test or code-review evidence.
- Cross-module admission: evidence-bearing production change is confined to p2p-frame SN client and admission passed for the p2p-frame change_id.
- Public API / codec / runtime semantics review: no public API, codec, or wire change.
- Document logic review: no contradiction found in p2p-frame proposal/design/testing for this change.
- Implementation logic review: no SN client correctness defect found in the reviewed path.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed after proposal, design, and testing approvals.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): failed on combined auto-pipeline diff containing prior-stage documents and pipeline plan.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not applicable; this is a behavior change, not a bugfix.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`: passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority --evidence-file harness/evidence/admission/20260703-sn-client-protocol-priority.md`: passed and wrote `harness/evidence/admission/20260703-sn-client-protocol-priority.p2p-frame.stamp.json`.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority --ignore-untracked`: failed because the combined auto-pipeline diff also includes proposal/design/testing/testplan/pipeline files.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --ignore-untracked`: failed because the combined auto-pipeline diff also includes proposal/design/pipeline and production code.
- Unit test command through `harness/scripts/test-run.py`: `python3 ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260703T090130Z-p2p-frame-unit.json`.
- DV test command through `harness/scripts/test-run.py`: p2p-frame DV is disabled by `testplan.yaml`; `p2p-frame unit` and all-all evidence record no runnable p2p-frame DV.
- Integration test command through `harness/scripts/test-run.py`: included in `python3 ./harness/scripts/test-run.py all all`; artifact `test-results/test-runs/20260703T091032Z-all-all.json` failed at final workspace-harness admission step after cargo/integration work passed.
- Module all command through `harness/scripts/test-run.py <module> all`: not separately rerun; p2p-frame unit and all-all evidence include the new `p2p-frame-sn-client-protocol-priority` step.
- Project all command through `harness/scripts/test-run.py all all`: `python3 ./harness/scripts/test-run.py all all` failed in workspace-harness admission step; artifact `test-results/test-runs/20260703T091032Z-all-all.json`.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): `test-results/test-runs/20260703T091032Z-all-all.json` exists but has `exit_code: 1`.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed with no gates configured because `harness/quality-gates.yaml` declares `gates: []`.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not required; quality gates are explicitly empty.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not rerun after `test-run.py all all` failed on the same unified suite path.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: pending before final report validation.
- Targeted migration search, when applicable: not applicable; no public symbol migration.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: SN-client implementation and targeted testing evidence are complete, but repository acceptance requires a fresh passing all-all artifact and passing implementation/testing stage-scope evidence. Both are currently blocked by mechanics outside the SN client production change.
- Supporting test evidence: `test-results/test-runs/20260703T090130Z-p2p-frame-unit.json`; targeted rerun `cargo test -p p2p-frame sn_client`; failed global artifact `test-results/test-runs/20260703T091032Z-all-all.json`.
- Residual risk: final accepted state cannot be mechanically recorded until the existing all-all workspace-harness admission command is fixed or supplied an evidence file, and stage scope is rerun against isolated stage diffs or the checker gains an auto-pipeline isolation mode.

## Follow-Up Tasks
- Requirement task: none for the SN client protocol priority requirement.
- Design task: none for the SN client protocol priority design.
- Implementation task: none for the SN client protocol priority implementation.
- Testing task: none for the SN client protocol priority tests.
- Testing return reason if coverage is incomplete: not applicable; coverage is adequate for the reviewed change.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
