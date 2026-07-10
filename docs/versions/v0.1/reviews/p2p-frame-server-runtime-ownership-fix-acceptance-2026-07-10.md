# p2p-frame Server Runtime Ownership Fix Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-001 | low | workspace harness | `python3 ./harness/scripts/harness-self-check.py` reports that `test-run.sh` lacks literal `uv run` / `--active`; `./test-run.sh all all` itself passed and wrote a fresh artifact | Pre-existing harness self-check text expectation is out of sync with the working root wrapper; it is outside the approved runtime/readiness scope and does not invalidate the executed root test entry | none |

## Object and Scope
- Module: p2p-frame / `server-runtime-ownership-fix`, with cyfs-p2p, cyfs-p2p-test and sn-miner consumers.
- Version: v0.1.
- change_id values reviewed: `server_runtime_required_api_alignment`, `process_runtime_single_owner`, `cyfs_config_runtime_injection`, `runtime_x509_test_registration`, `sn_miner_role_readiness_evidence`.
- Review date: 2026-07-10.
- In scope: compile-time runtime contract alignment, caller-supplied CYFS helper, one-runtime combined assembly, x509 test registration, owner/serving readiness and cleanup evidence.
- Out of scope: runtime worker implementation, TCP/QUIC/SN protocol behavior, TLS/identity semantics, endpoint classification, distributed-directory query/call completion, and unrelated workspace-harness changes.

## Optional Diff / Status Evidence
- `git status --short` summary: task changes are the sibling packet/review/evidence files, four production paths, p2p SN tests, and sn-miner real-process tests; numerous pre-existing workspace-harness and untracked evidence/review files remain unrelated.
- `git diff --stat` summary: tracked task production/test paths contain small runtime plumbing/readiness additions; the existing dirty workspace also contains unrelated workspace-harness changes.
- `git diff --name-status` summary: task paths are `cyfs-p2p/src/stack_builder.rs`, `cyfs-p2p-test/src/main.rs`, `cyfs-p2p/examples/cyfs_perf.rs`, `sn-miner-rust/src/main.rs`, `p2p-frame/src/sn/tests.rs`, `sn-miner-rust/tests/real_process.rs`, the sibling packet, admission evidence, pipeline plan and this report.
- `git diff --check` result: passed.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| Compile-time-required SN runtime contract replaces stale missing-runtime error claim | approved sibling proposal/design `server_runtime_required_api_alignment` | `SnServiceConfig::new` requires `ServerRuntime`; `create_sn_service` consumes it and retains `P2pResult` | feature-enabled runtime unit step in `test-results/test-runs/20260710T074336Z-p2p-frame+server-runtime-ownership-fix-all.json` | implemented |
| Combined startup owns one runtime | approved design combined startup flow / `process_runtime_single_owner` | `cyfs-p2p-test::all_in_one` has one `ServerRuntime::start`, clones to SN and passes the same handle to CYFS config | shared-runtime test plus all-target compile in sibling artifact; whole-project artifact `test-results/test-runs/20260710T074910Z-all-all.json` | implemented |
| CYFS config helper accepts caller-owned runtime without hidden start/panic | approved helper interface / `cyfs_config_runtime_injection` | helper signature accepts `ServerRuntime`; direct callers migrated; cyfs_perf maps start failure to `AppResult` | integration compile step in sibling and whole-project artifacts; targeted source search has no helper runtime start | implemented |
| x509-gated runtime tests are actually executed | approved testing/testplan / `runtime_x509_test_registration` | sibling testplan invokes Cargo with `--features x509 external_server_runtime` | two tests executed and passed in both cited artifacts; no zero-test result | implemented |
| Owner/serving readiness is marker plus real listener reachability with cleanup | approved readiness flow / `sn_miner_role_readiness_evidence` | markers are flushed only after role start; test guard captures output, polls, connects, kills and waits | five real-process cases pass in both cited artifacts, covering owner two listeners, serving listener, early exit, marker mismatch and probe failure | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| Required/shared runtime contract and API alignment | normal / boundary / negative / error / compatibility / lifecycle / cross-module | testing Direct Change Coverage, Case-Type Coverage and shared-runtime unit rows | two feature-enabled cases plus all-target compile in `test-results/test-runs/20260710T074336Z-p2p-frame+server-runtime-ownership-fix-all.json` | adequate |
| Hidden runtime creation and caller error ownership | normal / negative / error / compatibility / cross-module | external interface and integration rows; deterministic OS thread failure limitation recorded | source search, cyfs_perf fallible mapping, all callers compile, whole-project artifact passes | adequate |
| Feature registration must reject zero-test evidence | normal / boundary / negative / error | testplan requires x509 and testing Regression Focus records previous zero-test defect | unit step reports exactly two executed tests | adequate |
| Owner/serving runtime readiness and cleanup | normal / boundary / negative / error / compatibility / lifecycle / cross-module | DV rows cover main/lifecycle/failure; unit rows cover waiter branches | five real-process cases execute real binaries and loopback connects | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| A-RUNTIME-1 | proposal/design/code | SN runtime is compile-time required and new evidence contains no missing-runtime `InvalidParam` claim | approved sibling docs, constructor code and x509 unit output | pass |
| A-RUNTIME-2 | proposal/design/code | combined startup contains one runtime creation and passes clones to SN/stack | source inspection and shared-runtime unit | pass |
| A-RUNTIME-3 | proposal/design/code | CYFS helper contains no runtime start/default overload/expect and all callers supply a handle | source search and all-target compile | pass |
| A-RUNTIME-4 | proposal/testing/testplan | canonical sibling unit entry enables x509 and executes real matching tests | machine artifact step and test output count | pass |
| A-RUNTIME-5 | proposal/design/tests | owner and serving readiness requires post-start marker plus required TCP listener probes; failures diagnose and children are reaped | five-case real-process result and code inspection | pass |
| A-RUNTIME-6 | harness rules | approved docs, admissions, stage scopes, testing coverage, quality and whole-project evidence pass | checker output, stamps and fresh artifacts | pass |

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
- Proposal authority check: the user-approved sibling proposal defines all five requested outcomes and supersedes only stale runtime/evidence wording from the older packet.
- Proposal vs design: design directly maps all five change ids, preserves proposal non-goals, records the breaking/migration-required helper boundary, lifecycle, failures and exact Scope Paths.
- Design vs testing implementation: post-implementation testing derives shared-runtime, feature, caller compile and readiness state/failure cases from the approved design.
- Design vs long-lived boundary doc: existing module ownership remains p2p-frame core -> cyfs-p2p adapter -> startup binaries; no new long-lived production submodule or protocol boundary was introduced.
- Design vs implementation: helper signature, caller creation, combined clone flow, marker ordering and flush behavior match the approved design.
- Test implementation vs test code vs results: sibling testplan reaches the changed tests; artifacts record two x509 unit cases, five real-process cases and relevant all-target compilation with exit code 0.
- Test design adequacy: normal, boundary, negative, error, compatibility, lifecycle and cross-module cases are covered at the lowest practical levels; the only unit gap is nondeterministic stdout flush failure and has explicit rationale.
- change_id traceability: all five ids appear in approved proposal, design, testing, testplan and this report; production ids are bound by admission stamps.
- Acceptance criteria traceability: A-RUNTIME-1 through A-RUNTIME-6 map each proposal outcome to code and fresh runnable evidence.
- Cross-module admission: the sibling p2p-frame stamp binds CYFS/cyfs-p2p-test/sn-miner cross-module Scope Paths; sn-miner additionally passed its own owner/serving admission; cyfs-p2p-test is explicitly document-exempt.
- Public API / codec / runtime semantics review: only the approved Rust helper signature changes; wire, codec, TLS and endpoint semantics are unchanged; runtime creation ownership moves outward.
- Document logic review: new sibling docs consistently describe compile-time-required runtime and explicitly supersede the old missing-runtime error claim; no internal contradiction found.
- Implementation logic review: no hidden CYFS helper start remains; combined mode has one creation; readiness is emitted after successful start and tests require external reachability.
- Document approval timing (approved_content_sha256 verified by schema-check): proposal user approval and auto-pipeline design/testing hashes all revalidated successfully.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): main sibling and independent sn-miner implementation scope checks passed using isolated temporary indexes because the shared worktree contains unrelated changes.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: no pre-fix artifact was produced because the new required helper call/test and readiness marker tests cannot compile/run unchanged against the old signature/absent marker in the dirty shared tree; the testing document records this concrete infeasibility, while the pre-fix review captured zero-test and liveness-only behavior.

## Required Command Evidence
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule server-runtime-ownership-fix`: passed after proposal/design/testing approvals.
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule server-runtime-ownership-fix --change-id ... --evidence-file harness/evidence/admission/20260710-server-runtime-ownership-fix.md`: passed; plus independent sn-miner admission passed.
- `python3 ./harness/scripts/stage-scope-check.py --stage <stage>`: proposal, design, implementation and testing checks passed using isolated temporary indexes containing only task paths.
- Unit test command through `harness/scripts/test-run.py`: sibling all artifact unit step enabled x509 and passed 2 tests.
- DV test command through `harness/scripts/test-run.py`: sibling all artifact DV step passed 5 real-process tests.
- Integration test command through `harness/scripts/test-run.py`: sibling all artifact integration step passed relevant all-target compilation.
- Module all command through `harness/scripts/test-run.py <module> all`: `python3 ./harness/scripts/test-run.py p2p-frame/server-runtime-ownership-fix all` passed; artifact `test-results/test-runs/20260710T074336Z-p2p-frame+server-runtime-ownership-fix-all.json`.
- Project all command through `harness/scripts/test-run.py all all`: root wrapper invoked the unified `all all` route and passed.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): `test-results/test-runs/20260710T074910Z-all-all.json`, fresh, requested `all all`, exit code 0.
- Quality gates `python3 ./harness/scripts/quality-check.py`: passed; `harness/quality-gates.yaml` declares an explicitly empty gate list.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not applicable because quality gates are explicitly empty.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): `./test-run.sh all all` passed and produced the whole-project artifact above.
- Acceptance report check `python3 ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-server-runtime-ownership-fix-acceptance-2026-07-10.md`: passed.
- Targeted migration search, when applicable: `rg -n 'ServerRuntime::start|ServerRuntimeConfig' cyfs-p2p/src/stack_builder.rs` returned no matches; all `create_cyfs_p2p_config` callers pass runtime.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: all five approved outcomes are implemented, directly mapped, admitted, exercised through feature-enabled/unit/DV/integration paths, and pass a fresh whole-project run without changing excluded protocol semantics.
- Supporting test evidence: `test-results/test-runs/20260710T074336Z-p2p-frame+server-runtime-ownership-fix-all.json` and `test-results/test-runs/20260710T074910Z-all-all.json`.
- Residual risk: out-of-tree one-argument CYFS helper callers require migration; loopback port reservation has a small bind race; stdout flush failure is not deterministically injectable; the unrelated harness-self-check text mismatch remains.

## Follow-Up Tasks
- Requirement task: none.
- Design task: none.
- Implementation task: none.
- Testing task: optional workspace-harness follow-up for F-001; not part of this accepted runtime correction.
- Testing return reason if coverage is incomplete: not applicable; required coverage is complete with one explicitly justified non-injectable branch.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
