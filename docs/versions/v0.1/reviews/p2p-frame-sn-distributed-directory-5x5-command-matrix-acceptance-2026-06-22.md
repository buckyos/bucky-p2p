# p2p-frame SN Distributed Directory 5x5 Command Matrix Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-SN-5X5-001 | high | acceptance | `test-results/test-runs/20260622T031821Z-all-all.json` | Final whole-project `test-run.py all all` exited 1 because the workspace-harness integration step ran `admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive` without the required `--evidence-file`; SN 5x5 and workspace Rust tests passed, but accepted conclusion requires a fresh passing all/all artifact. | accepted report requires fresh passing whole-project test artifact |

## Object and Scope
- Module: `p2p-frame`
- Version: `v0.1`
- change_id values reviewed: `sn_five_by_five_command_matrix`
- Review date: 2026-06-22
- In scope: approved proposal/design/testing for `sn-distributed-directory`, implementation in `p2p-frame/src/sn/service/service.rs`, testplan wiring, admission evidence, and command evidence for the 5 Owner SN / 5 Serving SN / 5 user peer command matrix.
- Out of scope: fixing the unrelated workspace-harness `endpoint_area_server_reflexive` admission-check command, creating a real multi-process SN DV harness, and committing files.

## Optional Diff / Status Evidence
- `git status --short` summary: tracked worktree changes remained in `harness/pipeline-plan.md` and the regenerated SN 5x5 admission stamp; historical untracked review/admission files were present and not used as acceptance authority.
- `git diff --stat` summary: pipeline-plan progress notes and stamp timestamp changed after rerunning admission; acceptance report added in this review.
- `git diff --name-status` summary: not separately used; `git status --short` and direct evidence paths were sufficient.
- `git diff --check` result: passed with no whitespace errors.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| 5 owner ids, 5 serving services, and 5 user peers with distinct serving bindings | `proposal.md` P-SN-DIST-CMD-MATRIX-1 and `design.md` Directly Mapped Change Items | `sn_five_by_five_command_matrix_covers_all_sn_commands` constructs the fixed topology in `p2p-frame/src/sn/service/service.rs` | `test-results/test-runs/20260622T025429Z-p2p-frame+sn-distributed-directory-all.json` passed | implemented |
| Peer-facing SN command coverage remains client-compatible | `proposal.md` Scope and Success Criteria; `design.md` Invariants to Preserve | test asserts peer-facing `PackageCmdCode` SN command range for report/query/call/called responses without adding client-visible commands | `cargo test -p p2p-frame sn_five_by_five_command_matrix -- --nocapture` passed through testplan | implemented |
| Inter-SN command coverage includes heartbeat, publish/query, detail, and relay | `proposal.md` command matrix definition; `design.md` Key Call Flows | test asserts `InterSnCommandCode` values and drives query detail plus relay call through registered serving services | module all artifact passed | implemented |
| Owner serving-facing directory publish/query covered | `proposal.md` owner serving-facing directory command requirement; `design.md` Testability | test publishes and queries routes through owner directory client seams and validates route serving id for each peer | module all artifact passed | implemented |
| Validator reject has no successful command side effect | `proposal.md` security trigger and P-SN-INTER-AUTH-1; `design.md` failure handling | test installs a reject validator and verifies remote detail query is denied | module all artifact passed | implemented |
| Whole-project acceptance evidence | `acceptance-review-rules.md` Required Harness Commands | workspace Rust tests and SN 5x5 tests passed, but workspace-harness admission-check command was malformed | `test-results/test-runs/20260622T031821Z-all-all.json` exit_code 1 | missing |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `sn_five_by_five_command_matrix` topology cardinality | normal / boundary / lifecycle | `testing.md` Direct Change Coverage and Case-Type Coverage rows require exactly 5 owners, 5 servings, 5 users, and one-to-one bindings | `sn-5x5-command-matrix-unit` in `testplan.yaml`; module all artifact passed | adequate |
| SN command family compatibility | normal / compatibility / lifecycle | `testing.md` Design Element Coverage maps all SN command families to enum and command-path assertions | targeted and module all artifacts passed | adequate |
| Validator and reject path | negative / error | `testing.md` records permission-denied reject and no-side-effect expectation | targeted and module all artifacts passed | adequate |
| Real multi-process TTP lifecycle | cross-module / lifecycle | `testing.md` records DV/integration gap with owner, risk, and acceptance impact | no automated DV/integration harness exists; unit command semantics are covered | adequate |
| Whole-project acceptance freshness | acceptance gate | acceptance rules require `test-run.py all all` to pass for accepted conclusion | all/all artifact exists but failed in unrelated workspace-harness step | gap |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-SN-5X5-TOPOLOGY | proposal/design/testing | exactly 5 Owner SN ids, 5 Serving SN services, and 5 user peers are exercised with distinct user-to-serving bindings | targeted test and module all artifact | pass |
| AR-SN-5X5-COMMANDS | proposal/design/testing | peer-facing, inter-SN, and owner serving-facing command families are covered without new client-visible commands | targeted test assertions and command enum checks | pass |
| AR-SN-5X5-REJECT | proposal/design/testing | reject validator denies command path and records no accepted side effect | targeted test and module all artifact | pass |
| AR-SN-5X5-WIRING | testing/testplan | the new test is reachable through unified `test-run.py p2p-frame/sn-distributed-directory all` | `test-results/test-runs/20260622T025429Z-p2p-frame+sn-distributed-directory-all.json` | pass |
| AR-SN-5X5-WHOLE-PROJECT | acceptance-review-rules | whole-project `test-run.py all all` must produce a fresh passing artifact before accepted conclusion | `test-results/test-runs/20260622T031821Z-all-all.json` | fail |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/testplan.yaml`
- `harness/evidence/admission/20260622-sn-5x5-command-matrix.md`
- `p2p-frame/src/sn/service/service.rs`
- `test-results/test-runs/20260622T025429Z-p2p-frame+sn-distributed-directory-all.json`
- `test-results/test-runs/20260622T031821Z-all-all.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed approved proposal command-matrix requirement and non-goals.
2. Reviewed approved design mapping for `sn_five_by_five_command_matrix`.
3. Reviewed testing document and `testplan.yaml` registration.
4. Reviewed implementation evidence in `p2p-frame/src/sn/service/service.rs`.
5. Ran schema, admission, module all, whole-project all/all, and quality commands.
6. Compared evidence against acceptance fail conditions and generated this report.

## Consistency Summary
- Proposal authority check: proposal is approved and explicitly requires 5 Owner SN, 5 Serving SN, 5 user peer command matrix coverage without adding client-visible commands.
- Proposal vs design: design maps `sn_five_by_five_command_matrix` directly to P-SN-DIST-CMD-MATRIX-1 and defines deterministic unit-level topology plus DV/integration gap.
- Design vs testing implementation: testing.md and testplan.yaml include the 5x5 unit step and map normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases.
- Design vs long-lived boundary doc: no long-lived boundary change was required for this test-only command matrix evidence.
- Design vs implementation: implementation uses test-only topology support in `p2p-frame/src/sn/service/service.rs` inside admitted `p2p-frame/src/sn/**` scope.
- Test implementation vs test code vs results: targeted test passed through module all artifact; workspace all/all also ran the test successfully before failing an unrelated harness admission step.
- Test design adequacy: adequate for deterministic command semantics; real multi-process lifecycle is explicitly recorded as a DV/integration gap.
- change_id traceability: proposal, design, admission evidence, testing coverage, and testplan all reference `sn_five_by_five_command_matrix`.
- Acceptance criteria traceability: SN 5x5 criteria are implemented and verified, but final acceptance cannot be marked accepted because all/all artifact failed.
- Cross-module admission: reviewed change only bears on `p2p-frame/sn-distributed-directory`; all/all failure is in workspace-harness evidence for `endpoint_area_server_reflexive`.
- Public API / codec / runtime semantics review: no new client-visible SN command or response shape was added for the 5x5 matrix.
- Document logic review: no contradiction found among proposal/design/testing for the reviewed change.
- Implementation logic review: no correctness issue found in the deterministic 5x5 test implementation; it covers route publish/query, detail query, relay call, enum checks, and reject path.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed for the direct submodule packet.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): implementation scope check passed earlier for `sn_five_by_five_command_matrix`.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not applicable; this is new coverage, not a bugfix.

## Required Command Evidence
- `schema-check.py`: `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory` passed.
- `admission-check.py`: `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_five_by_five_command_matrix --evidence-file harness/evidence/admission/20260622-sn-5x5-command-matrix.md` passed.
- `stage-scope-check.py`: implementation and testing stage-scope checks passed earlier for `sn_five_by_five_command_matrix`; acceptance scope cannot complete with accepted conclusion while all/all evidence is failing.
- Unit test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory unit` passed; latest unit artifact `test-results/test-runs/20260621T164755Z-p2p-frame+sn-distributed-directory-unit.json`.
- DV test command through `harness/scripts/test-run.py`: `p2p-frame/sn-distributed-directory all` reported no dv tests with the recorded disabled reason in `testplan.yaml`.
- Integration test command through `harness/scripts/test-run.py`: `p2p-frame/sn-distributed-directory all` reported no integration tests with the recorded disabled reason in `testplan.yaml`.
- Module all command through `harness/scripts/test-run.py <module> all`: `uv run --active python ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory all` passed; artifact `test-results/test-runs/20260622T025429Z-p2p-frame+sn-distributed-directory-all.json`.
- Project all command through `harness/scripts/test-run.py all all`: `uv run --active python ./harness/scripts/test-run.py all all` failed; artifact `test-results/test-runs/20260622T031821Z-all-all.json` has `exit_code: 1`.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): `test-results/test-runs/20260622T031821Z-all-all.json`.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed with no configured gates because `harness/quality-gates.yaml` declares `gates: []`.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not required; no gates configured and the script does not write an artifact for an empty gate list.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not run after `test-run.py all all` failed; rerun after the workspace-harness admission command is fixed.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: `uv run --active python ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-sn-distributed-directory-5x5-command-matrix-acceptance-2026-06-22.md` passed.
- Targeted migration search, when applicable: not applicable; no public API, codec, or wire-format migration for this test-only matrix.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: SN 5x5 command matrix implementation and module-level evidence are consistent and passing, but final acceptance is blocked by a fresh whole-project `all all` artifact with exit code 1.
- Supporting test evidence: `test-results/test-runs/20260622T025429Z-p2p-frame+sn-distributed-directory-all.json` passed and includes the 5x5 command matrix step.
- Residual risk: real multi-process TTP lifecycle remains a recorded DV/integration gap, and the unrelated workspace-harness all/all failure must be fixed before this pipeline can be accepted.

## Follow-Up Tasks
- Requirement task: none for `sn_five_by_five_command_matrix`.
- Design task: none for deterministic command matrix coverage; future DV design may be needed for real multi-process lifecycle.
- Implementation task: fix or supply proper evidence for the workspace-harness `endpoint_area_server_reflexive` admission-check command so `test-run.py all all` can pass.
- Testing task: rerun `uv run --active python ./harness/scripts/test-run.py all all` and rerun acceptance after the harness command is corrected.
- Testing return reason if coverage is incomplete: not incomplete for SN 5x5 unit command matrix; final all/all evidence is failing outside this change_id.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
