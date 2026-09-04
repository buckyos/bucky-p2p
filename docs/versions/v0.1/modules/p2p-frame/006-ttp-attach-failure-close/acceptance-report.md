# TTP Attach Failure Tunnel Close Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | approved proposal, validated pipeline plan, admitted runtime change, dedicated fail-closed tests, and successful task run | no unresolved blocking finding was identified | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: `TtpRuntime::attach_tunnel(...)` now closes the affected tunnel whenever stream, control-stream, or datagram listener registration returns a real error, while preserving the original error and leaving successful or unsupported capabilities open.
- What was verified: all three ordered failure positions route through one cleanup helper, close is attempted exactly once, close failure is diagnostic only, `NotSupport` remains non-fatal, pointer-identity attachment behavior is unchanged, and retry remains based on creating a new tunnel instance.
- Evidence used: approved proposal and launch-confirmed pipeline mappings, admission stamp, stage-scope results, source review, dedicated branch/lifecycle tests, testplan coverage, task-start red source, and the successful task-level `all` artifact.
- Blocking issues: no blocking issue or acceptance return was required.
- Next action: mark the pipeline complete and remove task 006 from the unfinished-task index.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 006-ttp-attach-failure-close
- change_id values reviewed: ttp_attach_failure_closes_tunnel
- Review date: 2026-07-14
- In scope: `p2p-frame/src/ttp/runtime.rs` attach error handling, `p2p-frame/src/ttp/tests.rs` fault-injection fixture/cases, task-local plan/state/testplan, admission evidence, and task-run artifact.
- Out of scope: listener rollback APIs, same-instance retry, attach concurrency state machines, `attached_tunnels` redesign, transport close implementation changes, wire behavior, tunnel selection/caching/publish changes, and broad workspace validation.
- Task-relevant acceptance scope: proposal P-TAFC-1, pipeline binding `ttp_attach_failure_closes_tunnel`, the admitted production file, dedicated TTP test filter, and matching task-level run artifact.
- Out-of-scope checks not run: direct package/module runtime suites, `all all`, root shortcuts, quality gates, unrelated dirty-worktree tests, and transport-specific broad suites.

## Optional Diff / Status Evidence
- `git status --short` summary: the repository contains unrelated user work; acceptance used only the task manifests and bound evidence paths.
- `git diff --stat` summary: discovery showed one admitted production file and one dedicated existing test file changed; documentation/evidence paths are governed separately by task manifests.
- `git diff --name-status` summary: not used as a pass condition; exact production and testing paths come from the admitted binding and stage manifests.
- `git diff --check` result: no whitespace errors were reported for `p2p-frame/src/ttp/runtime.rs` or `p2p-frame/src/ttp/tests.rs`.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| every real stream/control/datagram registration failure closes the affected tunnel exactly once | proposal P-TAFC-1; plan Failure Flows | each non-`NotSupport` match arm calls the single `close_failed_attach` helper, which invokes `Tunnel::close()` once | ordered fault-injection cases cover first, second, and third registration positions and assert close count one | implemented |
| the original listener-registration error remains caller-visible even when close fails | proposal Scope and Requirement Review; plan cleanup failure flow | helper logs `close_err` and returns the owned `attach_err` unchanged | close-failure case injects `ConnectionAborted` attach error plus `OutOfLimit` close error and observes the original code/message | implemented |
| successful and `NotSupport` capabilities do not close the tunnel | proposal P-TAFC-1 and non-goals; plan optional capability flow | existing success and guarded `NotSupport` arms remain unchanged and bypass the helper | success asserts all callbacks registered and close count zero; all-three-unsupported case asserts success and close count zero | implemented |
| failed instance is terminal while retry, identity deduplication, callers, APIs, and transports remain unchanged | proposal boundaries; plan State Ownership/API impact | `try_mark_attached`, callers, trait signatures, transport implementations, caches, and publish paths have no task diff | scoped source review plus task compilation/test execution; integration is explicitly disabled with task-specific unchanged-boundary reasoning | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `ttp_attach_failure_closes_tunnel` registration ordering | boundary / error / lifecycle | table-driven failure injection observes zero, one, and two earlier successful registrations before the failing listener | `ttp-attach-failure-unit` passed for stream, control-stream, and datagram errors | adequate |
| cleanup and error precedence | negative / error / recovery | fake tunnel independently injects listener and close errors; assertions require one close attempt and unchanged attach code/message | dedicated close-failure test passed in the task artifact | adequate |
| success and optional capability compatibility | normal / compatibility | one full success and one all-`NotSupport` attach exercise every unchanged match arm without close | dedicated success/unsupported test passed | adequate |
| runtime and cross-module depth decision | lifecycle / cross-module | the real crate-private runtime coordinator and Tunnel boundary are exercised at unit level; DV/integration are disabled with owner, risk, and acceptance-impact reasons because no socket, public interface, cache, or caller contract changed | task-level all executed the enabled unit step; disabled levels do not hide an uncovered changed interface | adequate |
| bugfix red-green obligation | regression / error | task-start commit `a5a5d098...f9a` has all three real error arms returning directly without `close`; delivered source routes the same arms through cleanup | green task artifact executes all three positions plus cleanup-error and non-error behavior | adequate |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | three sequential listener match expressions and one cleanup helper | production diff, plan failure flows, ordered branch tests | only the final non-`NotSupport` arm in each match calls the helper; success and guarded unsupported arms cannot fall through to close; one helper call produces one close attempt; no missing branch or unintended fallthrough found | none | pass |
| termination and progress | synchronous compensation after an awaited registration error | helper source and existing attach sequence | the change adds no loop, recursion, wait, retry, spawn, or blocking operation; `close()` is the existing synchronous trait operation and the original async function returns immediately afterward | none | pass |
| concurrency and synchronization | existing `attached_tunnels` mutex and callback registration ordering | proposal non-goals, plan State Ownership, unchanged `try_mark_attached` | no lock or shared-state operation was added; cleanup occurs after listener calls return and outside the attached mutex; same-instance concurrency semantics are explicitly unchanged and not silently widened | none | pass |
| resource lifetime and cleanup | tunnel callbacks/control resources after zero or partial registration | helper, concrete TCP/QUIC/PN close behavior inspected during design, close-count tests | every real attach error now requests whole-tunnel cleanup once; cleanup failure is visible in logs and cannot be rolled back further through the current trait; no duplicate close or new retained resource is introduced | none | pass |
| state and data integrity | attached identity marker and partial listener state | plan lifecycle, unchanged marker code, ordered partial-registration assertions | a failed instance remains terminal as approved; no partially attached tunnel is reported as a successful return from the failing call, and later retry uses a new pointer identity; no cache or persistent state mutation was added | none | pass |
| error handling and recovery | listener error, `NotSupport`, and close error precedence | proposal, plan, helper, exact-code/message tests | causative listener error is never swallowed or replaced; unsupported capability stays success; cleanup error is logged with tunnel context; no retry storm, fallback, or recursive failure path exists | none | pass |
| interface boundary and compatibility | crate-private attach behavior plus existing Tunnel methods/callers | plan interface table, production/test diff, unchanged client/node/server and trait paths | signatures, public exports, wire formats, caller error values, and transport contracts remain unchanged; only the approved internal cleanup side effect is added | none | pass |
| security and capacity safety | error-path resource retention and diagnostic logging | helper log fields, bounded state, proposal security trigger decision | closing partial tunnels reduces resource-retention risk; the change adds no queue, allocation growth, unsafe code, secret data, authorization boundary, or attacker-controlled retry amplification; log context uses existing IDs/endpoints and error diagnostics | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-TAFC-1 | proposal P-TAFC-1 | stream, control-stream, and datagram real registration errors each cause exactly one close attempt | three-position source mapping and close-count cases | pass |
| AR-TAFC-2 | proposal error precedence | `attach_tunnel` returns the original registration error even if cleanup fails | helper source plus distinct attach/close error test | pass |
| AR-TAFC-3 | proposal compatibility/non-goals | success and `NotSupport` do not close; APIs, marker logic, transports, callers, and retry model remain unchanged | positive/unsupported test plus scoped diff review | pass |
| AR-TAFC-4 | proposal partial-attachment goal | failures after earlier registrations cannot leave an open tunnel accepted as fully attached | ordered partial-registration evidence plus close invocation | pass |
| AR-TAFC-5 | bugfix evidence obligation | task-start source demonstrates missing close and delivered task test demonstrates fail-closed behavior | bound task-start commit, production diff, and successful task artifact | pass |

## Inputs
- approved `proposal.md`
- launch-confirmed `pipeline/plan.md` and mutable `pipeline/state.json`
- `testplan.yaml`
- `docs/modules/p2p-frame.md` TTP/tunnel runtime boundary
- `p2p-frame/src/ttp/runtime.rs`
- `p2p-frame/src/ttp/tests.rs`
- admission evidence and generated stamp
- task run artifact under `test-results/test-runs/`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed the approved fail-closed requirement, success criteria, constraints, and non-goals.
2. Reviewed the launch-confirmed design mappings for ownership, interface compatibility, partial failure, cleanup error precedence, and exact Scope Path.
3. Generated the acceptance rules above from the proposal and plan before judging implementation evidence.
4. Reviewed only the admitted production diff and task testing diff for branch selection, cleanup count, error identity, and preserved unsupported behavior.
5. Reviewed test design across normal, boundary, negative, error, compatibility, lifecycle, concurrency/retry, and cross-module relevance.
6. Reused the successful task artifact and existing schema, admission, stage-scope, pipeline-plan, and coverage results without replaying unchanged checks.
7. Completed all eight implementation correctness categories and found no defect requiring return routing.

## Consistency Summary
- Proposal authority check: the explicitly user-approved proposal has a current content hash and remains the acceptance authority.
- Proposal vs design: pipeline plan directly maps P-TAFC-1 to centralized cleanup, original-error precedence, `NotSupport` compatibility, terminal same-instance behavior, and one concrete production path without narrowing or expansion.
- Design vs testing implementation: tests derive from every registration failure flow, both helper outcomes, optional capability behavior, partial state ordering, and unchanged interface boundaries.
- Design vs long-lived boundary doc: the work remains inside `src/ttp/**`, which `docs/modules/p2p-frame.md` assigns to the p2p-frame core runtime/tunnel responsibility; no long-lived boundary update is required.
- Design vs implementation: all three mapped failure arms use the planned one helper; the helper performs exactly the planned close/log/return ordering.
- Test implementation vs test code vs results: testplan command exactly matches the one successful executed step in the machine-written task artifact and selects the three new `ttp_attach_failure_` tests.
- Test design adequacy: unit coverage reaches every changed branch at the lowest effective boundary; DV/integration disablement is specific to the absence of socket, public, cache, or caller changes and records owner/risk/acceptance impact.
- change_id traceability: proposal, plan, admission, state, testplan, run artifact, and this report all use `ttp_attach_failure_closes_tunnel`.
- Acceptance criteria traceability: every required behavior and explicit non-goal has implementation plus test or scoped source-review evidence.
- Cross-module admission: only p2p-frame contains production/test evidence; no neighboring project implementation or exported interface changed, so no second packet is required.
- Public API / codec / runtime semantics review: public APIs, codecs, wire behavior, transport listener/close implementations, caller-visible attach error values, and build surface remain unchanged; the only runtime change is approved fail-closed compensation.
- Document logic review: no contradiction, impossible state, unsupported assumption, or silent scope change was found.
- Implementation logic review: each mutually exclusive match selects cleanup only for real errors; helper error handling borrows diagnostic values and returns the original owned error after one close call.
- Implementation correctness audit completeness and routing: all eight required categories are present and pass; no return to proposal, design, implementation, or testing is required.
- Document approval timing (approved_content_sha256 verified by schema-check): proposal approval hash `60f03cb4...8787db6` was recorded from the explicit 2026-07-14 user approval, and schema-check passed after pipeline/testplan inputs were finalized.
- Implementation task paths bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ... --changed-paths-file ...`): passed for runtime.rs plus the bound task admission evidence/stamp and mutable pipeline state.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: task-start commit `a5a5d098...f9a` contains three direct `Err(err) => return Err(err)` branches with no close; the delivered source routes those exact branches through cleanup and the task artifact passes all green cases.

## Validation Evidence
- Existing schema result (cite the owning-stage result; do not rerun unchanged input): `schema-check: passed` for v0.1/p2p-frame/006 after proposal approval, pipeline plan, and testplan inputs were finalized.
- Existing admission stamp (cite the stamp; do not run `--verify-only` during acceptance unless an admission-owned input changed): `docs/versions/v0.1/evidence/admission/20260714-ttp-attach-failure-close.p2p-frame.006-ttp-attach-failure-close.stamp.json`.
- Existing stage-scope result (cite the owning-stage result; do not rerun an unchanged manifest/scope): proposal passed with 2 paths, design passed with 2 paths, implementation passed with 4 paths, and testing passed with 4 paths.
- Existing pipeline-plan result, when applicable (cite the latest result for current plan/state inputs): `pipeline-plan-check: passed` after testing completion; final complete mode is reserved for final acceptance state/report binding.
- Task-relevant test run artifact(s) (reuse when implementation/tests/testplan/registration are unchanged): `test-results/test-runs/20260714T100306Z-p2p-frame+006-ttp-attach-failure-close-all.json`, exit code 0, one non-empty successful unit step covering `ttp_attach_failure_closes_tunnel`.
- Commands rerun because checker-owned inputs changed after their previous pass (or `none` with evidence): schema reran after testplan creation; pipeline-plan reran after mutable state transitions; testing coverage/scope ran after test metadata and artifact binding; admission and task tests were not replayed during acceptance.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; this single-task acceptance used only `p2p-frame/006-ttp-attach-failure-close all`.
- Risk-triggered task-local contract kinds and assertions, when applicable: no breaking or migration-required API, crate-root export, build-surface, or documentation-example trigger; the crate-private runtime behavior is covered by the dedicated task step.
- Scoped evidence input hash current, when risk-triggered: task artifact records `cf4376f09a5d8481680e80ac5125b8ec0c46a1270485a6cbeec4064fb89687bf` over the declared plan, proposal, runtime, tests, and testplan inputs.
- Quality gates: not applicable to automatic single-task acceptance; no quality-gate run was requested.
- Explicitly requested quality run artifact, if any: no quality run was requested.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: not run because this task changes no workspace/crate boundary, public architecture contract, or architecture document.
- Acceptance report check after this report was created or modified: this report is the checker-owned input and is validated immediately after the acceptance write.
- Targeted migration search, only when applicable to the reviewed task: no symbol or caller migration exists; scoped source review confirms unchanged signatures and call sites.

## Automated Test Exception
- Applies: no
- Reason: a successful automated task-level all artifact exists with a non-empty unit step covering every changed production branch.
- Owner: acceptance
- Risk: no residual risk from missing automated execution evidence.
- Acceptance impact: automated evidence is present and required.
- Alternative evidence: not needed because the task run artifact records a successful executed step.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: the approved fail-closed lifecycle is implemented inside the admitted boundary, the original error and unsupported capability behavior remain compatible, task-scoped regression evidence passes, and the evidence audit found no correctness or consistency defect.
- Supporting task-relevant test evidence: `test-results/test-runs/20260714T100306Z-p2p-frame+006-ttp-attach-failure-close-all.json`.
- Residual risk: future `Tunnel` implementations that keep the trait's default no-op `close()` would not provide physical cleanup, and an already executing callback may finish while close begins; neither behavior is introduced by this task, supported production tunnel implementations override close, and changing those contracts is outside the approved scope.

## Follow-Up Tasks
- Requirement task: none required.
- User decision required for proposal issue: none required.
- Design task: none required.
- Implementation task: none required.
- Testing task: none required.
- Testing return reason if coverage is incomplete: coverage is complete; no return is required.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable because acceptance passed on the first audit.
