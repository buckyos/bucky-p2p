# p2p-frame Stack Server Runtime External Required Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | Proposal, design, admission, implementation, testing, quality, and full-run evidence reviewed | no blocking finding recorded | none |

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- change_id values reviewed: stack_server_runtime_external_required
- Review date: 2026-07-07
- In scope: stack-level `ServerRuntime` ownership in `P2pConfig`, `create_p2p_env`, direct production caller migration, related test updates, and required harness evidence.
- Out of scope: TCP/QUIC wire protocol changes, SN command behavior changes, endpoint classification changes, unrelated runtime policy changes, and unrelated untracked workspace files.

## Optional Diff / Status Evidence
- `git status --short` summary: tracked changes are limited to `cyfs-p2p/src/stack_builder.rs`, p2p-frame proposal/design/testing/testplan docs, `harness/pipeline-plan.md`, `p2p-frame/src/sn/tests.rs`, `p2p-frame/src/stack.rs`, and `p2p-frame/tests/sn_command_matrix/five_by_five_command_matrix_tests.rs`; many pre-existing untracked review/evidence files are present and excluded from this acceptance judgment.
- `git diff --stat` summary: 10 tracked files changed, 156 insertions, 125 deletions.
- `git diff --name-status` summary: modified tracked paths are `cyfs-p2p/src/stack_builder.rs`, `docs/versions/v0.1/modules/p2p-frame/design.md`, `docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md`, `docs/versions/v0.1/modules/p2p-frame/proposal.md`, `docs/versions/v0.1/modules/p2p-frame/testing.md`, `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`, `harness/pipeline-plan.md`, `p2p-frame/src/sn/tests.rs`, `p2p-frame/src/stack.rs`, and `p2p-frame/tests/sn_command_matrix/five_by_five_command_matrix_tests.rs`.
- `git diff --check` result: passed with no whitespace errors.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| Stack-level `ServerRuntime` is externally supplied and stored as `ServerRuntime`, not `Option<ServerRuntime>` | `proposal.md` change_id `stack_server_runtime_external_required`; `design.md` stack runtime constructor/API rows | `p2p-frame/src/stack.rs:131` stores `server_runtime: ServerRuntime`; `p2p-frame/src/stack.rs:135-140` requires `server_runtime: ServerRuntime` in `P2pConfig::new` | `uv run --active python ./harness/scripts/test-run.py p2p-frame all` passed; artifact `test-results/test-runs/20260707T084837Z-p2p-frame-all.json` | implemented |
| `P2pConfig::new` and `create_p2p_env` must not create a default runtime fallback | `proposal.md` no-stack-fallback requirement; `design.md` `create_p2p_env` runtime behavior | `p2p-frame/src/stack.rs:162-163` assigns the caller supplied runtime; `p2p-frame/src/stack.rs:332` clones from config; `rg -n "ServerRuntime::start" p2p-frame/src/stack.rs` returned no matches | `cargo check -p p2p-frame --lib`, `cargo check -p cyfs-p2p --lib`, and `cargo check -p cyfs-p2p-test` passed before acceptance | implemented |
| Direct production caller supplies the runtime outside p2p-frame | `design.md` direct caller migration scope; admission evidence `20260707-stack-runtime-external-required.md` | `cyfs-p2p/src/stack_builder.rs:271-273` starts `ServerRuntime` in the caller and passes it to `P2pConfig::new` | `uv run --active python ./harness/scripts/test-run.py all all` passed; artifact `test-results/test-runs/20260707T083940Z-all-all.json`; root shortcut `./test-run.sh all all` passed; artifact `test-results/test-runs/20260707T084639Z-all-all.json` | implemented |
| SN test paths and 5x5 command matrix use explicit runtime injection | `testing.md` coverage rows for `stack_server_runtime_external_required`; `testplan.yaml` unit/integration/DV change_id rows | `p2p-frame/src/sn/tests.rs` and `p2p-frame/tests/sn_command_matrix/five_by_five_command_matrix_tests.rs` pass `test_server_runtime()` into `P2pConfig::new` and `SnServiceConfig::new` | `uv run --active python ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260707T082217Z-p2p-frame-unit.json`; `p2p-frame all` artifact passed | implemented |
| Harness admission and approved document chain remain valid | Approved `proposal.md`, approved `design.md`, admission evidence, and pipeline plan | `harness/evidence/admission/20260707-stack-runtime-external-required.md` binds proposal hash `99703f9bd69b266f2680a6d70cebd9f3dd5ad624801fede3fc056f3204c93f7a` and design hash `ce89a0dbf78489f8ebdedf98acfed899240cef406fed5a6fbcdbbce42abb4ce5` | `schema-check.py`, `admission-check.py`, testing coverage check, quality check, module all, and project all evidence all passed | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `stack_server_runtime_external_required` constructor and environment creation behavior | normal / compatibility / lifecycle / cross-module | `testing.md` records constructor-level runtime coverage and no stack fallback search; `testplan.yaml` lists the change_id under unit, integration, and DV coverage | `test-results/test-runs/20260707T084837Z-p2p-frame-all.json` and `test-results/test-runs/20260707T084639Z-all-all.json` passed | adequate |
| Removed default runtime fallback in stack layer | negative / error / compatibility | Negative evidence is compile/API level: callers cannot construct `P2pConfig` without `ServerRuntime`; targeted search confirms no `ServerRuntime::start` in `stack.rs` | `rg -n "ServerRuntime::start" p2p-frame/src/stack.rs` returned no matches; `cargo check` for p2p-frame, cyfs-p2p, and cyfs-p2p-test passed | adequate |
| Direct caller migration preserves runtime startup responsibility outside p2p-frame | normal / cross-module / lifecycle | `design.md` scopes `cyfs-p2p/src/stack_builder.rs`; testing docs include caller migration and compatibility coverage | `cyfs-p2p/src/stack_builder.rs:271-273`; full `all all` and root shortcut artifacts passed | adequate |
| SN service and command matrix compatibility after runtime injection API change | normal / integration / cross-module | `testing.md` and `testplan.yaml` include SN service and 5x5 matrix paths touched by constructor changes | `p2p-frame all` ran x509 SN client, SN directory, and 5x5 command matrix segments successfully | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| A-STACK-RUNTIME-1 | proposal/design/code | `p2p-frame/src/stack.rs` contains no default `ServerRuntime::start` fallback | Code search for `ServerRuntime::start` in `p2p-frame/src/stack.rs` returns no matches | pass |
| A-STACK-RUNTIME-2 | design/code | `P2pConfig::new` requires a `ServerRuntime` argument and stores `server_runtime: ServerRuntime` | `p2p-frame/src/stack.rs:131` and `p2p-frame/src/stack.rs:135-140` | pass |
| A-STACK-RUNTIME-3 | design/code | `create_p2p_env` uses the supplied config runtime only | `p2p-frame/src/stack.rs:332-340` clones and passes the config runtime to network creation | pass |
| A-STACK-RUNTIME-4 | design/code/tests | Direct caller starts runtime outside p2p-frame and passes it into `P2pConfig::new` | `cyfs-p2p/src/stack_builder.rs:271-273`; module and project test artifacts passed | pass |
| A-STACK-RUNTIME-5 | harness/testing | Admission, scope, quality, and runnable tests pass for the reviewed change_id | Admission evidence stamp, stage-scope results in pipeline plan, quality-check passed, module/all and all/all artifacts passed | pass |

## Inputs
- `proposal.md`
- `design.md`
- test implementation and optional `testing.md`
- `testplan.yaml` for completed testing work, or a versioned local exception with reason, owner, risk, and acceptance impact
- optional `acceptance.md`
- long-lived module doc
- implementation
- test code
- test results
- optional git diff/status evidence
- `harness/rules/acceptance-review-rules.md`

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
- Proposal authority check: approved proposal defines external stack `ServerRuntime` ownership and no stack fallback; reviewed implementation follows that authority.
- Proposal vs design: design expands the proposal into required constructor injection, config storage, `create_p2p_env` behavior, and direct caller migration without contradicting the proposal.
- Design vs testing implementation: testing artifacts cover constructor-level runtime injection, no stack fallback search, SN service caller updates, and 5x5 command matrix paths.
- Design vs long-lived boundary doc: no long-lived p2p-frame boundary expansion was required; crate boundary remains p2p-frame owning stack wiring while runtime creation is caller supplied.
- Design vs implementation: implementation matches the design: `P2pConfig` stores `ServerRuntime`, constructor requires it, `create_p2p_env` clones it, and `cyfs-p2p` creates it externally.
- Test implementation vs test code vs results: updated test call sites compile and pass through unit, module-all, project-all, and root shortcut artifacts.
- Test design adequacy: normal, compatibility, lifecycle, cross-module, and compile/API negative coverage are adequate for a constructor-level runtime ownership migration.
- change_id traceability: `stack_server_runtime_external_required` appears in proposal, design, admission evidence, testing docs, testplan, pipeline plan, and acceptance report.
- Acceptance criteria traceability: generated acceptance rules A-STACK-RUNTIME-1 through A-STACK-RUNTIME-5 map directly to proposal/design requirements and runnable evidence.
- Cross-module admission: admission evidence covers `p2p-frame/src/stack.rs`, `cyfs-p2p/src/stack_builder.rs`, and required test paths; admission-check passed.
- Public API / codec / runtime semantics review: public constructor signature changes as approved; codec, wire protocol, TLS, and endpoint semantics are unchanged by the diff.
- Document logic review: approved proposal/design/testing chain is internally consistent after auto-pipeline updates.
- Implementation logic review: no hidden stack fallback remains; caller-supplied runtime is cloned where needed and passed to TCP/QUIC network creation.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed after approved document metadata updates.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): stage-scope implementation result is recorded as passed in `harness/pipeline-plan.md`.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not applicable; this is an approved API/runtime ownership migration rather than a red-green bugfix.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`: passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id stack_server_runtime_external_required --evidence-file harness/evidence/admission/20260707-stack-runtime-external-required.md`: passed; stamp `harness/evidence/admission/20260707-stack-runtime-external-required.p2p-frame.stamp.json`.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage <stage>`: proposal, design, implementation, and testing stage-scope checks passed as recorded in `harness/pipeline-plan.md`.
- Unit test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260707T082217Z-p2p-frame-unit.json`.
- DV test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py p2p-frame all` reported no p2p-frame DV tests and completed successfully; artifact `test-results/test-runs/20260707T084837Z-p2p-frame-all.json`.
- Integration test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py p2p-frame all` passed integration/workspace segments; artifact `test-results/test-runs/20260707T084837Z-p2p-frame-all.json`.
- Module all command through `harness/scripts/test-run.py <module> all`: `uv run --active python ./harness/scripts/test-run.py p2p-frame all` passed; artifact `test-results/test-runs/20260707T084837Z-p2p-frame-all.json`.
- Project all command through `harness/scripts/test-run.py all all`: `uv run --active python ./harness/scripts/test-run.py all all` passed; artifact `test-results/test-runs/20260707T083940Z-all-all.json`.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): `test-results/test-runs/20260707T084639Z-all-all.json` from `./test-run.sh all all`, and `test-results/test-runs/20260707T083940Z-all-all.json` from direct `test-run.py all all`.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed; repository quality gates are explicitly empty.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not applicable because `harness/quality-gates.yaml` declares an explicitly empty gates list.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): `./test-run.sh all all` passed; artifact `test-results/test-runs/20260707T084639Z-all-all.json`.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: passed for `docs/versions/v0.1/reviews/p2p-frame-stack-server-runtime-external-required-acceptance-2026-07-07.md`.
- Targeted migration search, when applicable: `rg -n "ServerRuntime::start" p2p-frame/src/stack.rs` returned no matches.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: implementation, tests, documentation, and harness evidence satisfy the approved external stack `ServerRuntime` ownership requirement without introducing a stack-level fallback or unrelated protocol behavior changes.
- Supporting test evidence: `test-results/test-runs/20260707T084837Z-p2p-frame-all.json`, `test-results/test-runs/20260707T083940Z-all-all.json`, and `test-results/test-runs/20260707T084639Z-all-all.json`.
- Residual risk: downstream out-of-tree callers must update to the new `P2pConfig::new(..., ServerRuntime)` signature.

## Follow-Up Tasks
- Requirement task: none.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable; coverage is complete for this change.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
