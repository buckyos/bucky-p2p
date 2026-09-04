# Callback Result Replacement Waiter Cleanup Acceptance Report

## Findings

| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | proposal, pipeline design mapping, implementation, regression tests, scoped evidence, and current task artifact | no blocking finding recorded | none |

## Result Summary

- Overall result: accepted
- Plain-language outcome: Each keyed callback registration now carries a unique private identity, so completion or destruction of an older future cannot remove a newer waiter that reused the same callback ID. The focused replacement regressions, existing cleanup suite, and real p2p-frame consumer compile all pass.
- What was verified: auto-pipeline authority; proposal and plan coverage; admission and stage scope; exact-registration conditional cleanup; normal, timeout, cancellation, duplicate, cache, and replacement behavior; unchanged public API and consumer build surface; and all eight implementation-correctness categories.
- Evidence used: task packet primary sources and the existing owning-stage machine evidence listed below.
- Blocking issues: none.
- Next action: none; the accepted task is ready for handoff.

## Object and Scope

- Module: p2p-frame
- Version: v0.1
- Task name: 013-callback-result-replacement-waiter-cleanup
- change_id values reviewed: callback_result_replacement_waiter_cleanup
- Review date: 2026-08-26
- In scope: proposal item P-CRRWC-1; pipeline plan and state; admission and stage-scope evidence; `third-party/callback-result/src/lib.rs`; task-owned replacement tests; existing drop-cleanup regressions; testplan; and the current task artifact.
- Out of scope: p2p-frame SN, tunnel, and QA protocol behavior; `SingleCallbackWaiter`; callback-ID allocation; cache capacity policy; unrelated dirty-worktree changes; broad workspace suites; and quality gates.
- Task-relevant acceptance scope: the sole change_id `callback_result_replacement_waiter_cleanup` and the keyed `CallbackWaiter` registration lifecycle plus its p2p-frame compile consumer.
- Out-of-scope checks not run: unchanged schema, admission, implementation/testing scope, and task tests; unrelated package/workspace/root commands; and quality gates.

## Optional Diff / Status Evidence

- `git status --short` summary: used only to locate the task packet, evidence, vendored source, and regression test in a pre-existing dirty worktree; unrelated paths were excluded.
- `git diff --stat` summary: not required; the task-owned manifests define scope.
- `git diff --name-status` summary: not required; the task-owned manifests define scope.
- `git diff --check` result: passed after implementation and testing; no whitespace error was found.
- Note: diff/status output is discovery evidence only and is not the acceptance standard.

## Evidence Coverage

| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| cleanup removes only its exact keyed registration | proposal P-CRRWC-1 and plan State Ownership | private `CallbackRegistration.identity` and mutex-atomic `remove_registration_if_current` | old-future drop and ready replacement cases pass | implemented |
| normal and timeout constructors preserve replacement ownership | proposal scope and plan Failure Flows | both constructors allocate and capture a distinct identity | focused normal-drop, normal-ready, and timeout-drop cases pass | implemented |
| cancellation and timeout release owned state | proposal requirement and plan lifecycle | drop closure and awaited timeout converge on conditional cleanup | cancellation/timeout/replacement case plus prior bounded-retention suite pass | implemented |
| duplicate, cache, NoWaiter, and delivery semantics remain stable | proposal non-regression boundary | existing public branches and return types remain unchanged | duplicate and cached/live delivery case plus full crate suite pass | implemented |
| public API and consumer build remain compatible | proposal non-goals and plan API impact | no public signature, crate metadata, or dependency change | `cargo check -p p2p-frame --lib` passes in the task artifact | implemented |

## Test Design Adequacy

| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| old cleanup after same-ID replacement | normal, lifecycle, concurrency | deterministic drop and Ready sequences for an already-delivered old future | focused unit step executes both cases successfully | adequate |
| timeout and cancellation ownership | boundary, error, lifecycle | unpolled cancellation, actual timeout, and subsequent replacement delivery | focused unit step passes | adequate |
| duplicate and absent waiter errors | negative, error | live duplicate checks both constructors; post-delivery `NoWaiter` assertion | focused unit step passes | adequate |
| cache and live delivery compatibility | compatibility | FIFO cache consumption and live cached delivery retain old observable results | focused unit and full crate DV steps pass | adequate |
| dependency and real consumer closure | cross-module | unchanged package identity and public API compiled through p2p-frame | repository compile-closure step passes | adequate |
| bugfix red-green proof | regression | first two deterministic tests exercise the exact old key-only deletion sequence | pre-fix reproduction returned `Err(NoWaiter)`; both tests pass after the fix | adequate |

## Implementation Correctness Audit

| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | registration, result delivery, ready/drop cleanup, and replacement | plan, source, and focused tests | cleanup deletes only when callback ID and private identity both match; a mismatched replacement remains intact | none | pass |
| termination and progress | synchronous cleanup and timeout completion | source and timeout tests | cleanup is one mutex acquisition plus bounded map lookup/removal; timeout remains externally bounded and no loop or retry was added | none | pass |
| concurrency and synchronization | delivery/replacement/old-cleanup interleavings | state model, mutex critical sections, and deterministic ordering tests | identity comparison and removal occur under the same existing mutex; registration and delivery use that mutex, so no check-then-act window or new lock-order cycle exists | none | pass |
| resource lifetime and cleanup | notifier, registration identity, future cancellation, completion, and timeout | `Arc` capture ownership, drop closure, awaited cleanup, and regressions | every future retains its token until its terminal cleanup; owned state is removed on cancellation, timeout, or completion, while replacement state is not double-released | none | pass |
| state and data integrity | keyed map entry and cache queues | source transitions and replacement/cache tests | the map has one current owner per key; old identities cannot mutate successor entries; cache and exactly-once notifier consumption remain unchanged | none | pass |
| error handling and recovery | AlreadyExist, NoWaiter, Timeout, canceled notifier, and cached fallback | source branches and focused/full-suite results | existing errors remain classified as before; timeout/cancellation leave the key reusable and `set_result_with_cache` still falls back to cache | none | pass |
| interface boundary and compatibility | vendored crate public surface and workspace consumer | plan API table, source diff, Cargo metadata, and compile closure | only private state representation changed; public types, signatures, errors, version, dependency resolution, and caller code remain compatible | none | pass |
| security and capacity safety | per-registration memory and callback map retention | source and bounded-retention regression | one small `Arc<()>` token is bounded per live registration; cleanup prevents tombstones and adds no queue, task, scan, input parsing, or trust-boundary behavior | none | pass |

## Generated Acceptance Rules

| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-CRRWC-1 | P-CRRWC-1 | old ready/drop cleanup cannot remove a successfully registered same-ID replacement | source identity audit and deterministic replacement tests | pass |
| AR-CRRWC-2 | proposal constructor coverage | normal and timeout futures bind cleanup to their own registration | both constructor paths and focused cases | pass |
| AR-CRRWC-3 | proposal lifecycle boundary | pre-poll cancellation and timeout release only their owned entry | lifecycle source audit and tests | pass |
| AR-CRRWC-4 | proposal compatibility boundary | cache, duplicate, NoWaiter, Timeout, public API, and package identity remain unchanged | focused/full regressions and consumer compile | pass |
| AR-CRRWC-5 | Harness evidence boundary | all evidence is current, task-scoped, unified-entry reachable, and mapped to the change_id | stage manifests, testplan, state, and task artifact | pass |

## Inputs

- launch-confirmed `proposal.md`
- current `pipeline/plan.md` and `pipeline/state.json`
- admission evidence and stamp
- proposal, design, implementation, and testing stage-scope records
- `third-party/callback-result/src/lib.rs`
- `third-party/callback-result/tests/replacement_waiter.rs` and existing `drop_cleanup.rs`
- task `testplan.yaml`
- `test-results/test-runs/20260826T082122Z-p2p-frame+013-callback-result-replacement-waiter-cleanup-all.json`
- `docs/modules/p2p-frame.md` and the relevant architecture constraints
- `harness/rules/acceptance-review-rules.md` and test-design rules

## Review Order

1. Verified the proposal, exact auto-pipeline launch statement, task identity, and change_id boundary.
2. Compared the pipeline design mapping with the proposal and long-lived module boundary.
3. Reused the owning-stage schema, admission, plan, stage-scope, and task-run evidence after confirming their inputs remained unchanged.
4. Inspected the production state transitions and synchronization across all eight correctness categories.
5. Reviewed focused and existing tests against normal, boundary, negative, error, compatibility, lifecycle, concurrency, and cross-module risks.
6. Checked the task artifact identity, evidence hash, registered source steps, non-empty execution, and successful exits.
7. Recorded the findings-first conclusion and closeout actions.

## Consistency Summary

- Proposal authority check: valid under the verbatim auto-pipeline launch `确认，启动自动流水线`; draft manual approval metadata is permitted in this mode.
- Proposal vs design: consistent; the plan maps exact registration identity, mutex-atomic conditional removal, lifecycle transitions, compatibility, rejected alternatives, and one production Scope Path without narrowing or expansion.
- Design vs testing implementation: consistent; both constructors and all promised terminal paths are exercised after implementation.
- Design vs long-lived boundary doc: consistent; this private vendored dependency repair does not change the documented p2p-frame module boundary.
- Design vs implementation: consistent; every new registration creates an unshared identity and every cleanup compares it under the existing state mutex.
- Test implementation vs test code vs results: consistent; the task artifact records the focused test, full package regression, and deduplicated consumer compile, all with exit code 0.
- Test design adequacy: adequate for relevant normal, boundary, negative, error, compatibility, lifecycle, concurrency, capacity, and cross-module cases.
- change_id traceability: complete across proposal, plan, admission evidence/stamp, source scope, testplan, state, and artifact.
- Acceptance criteria traceability: complete through focused regression, full dependency DV, compile contract, and stage-scope evidence.
- Cross-module admission: one p2p-frame packet is correct because the changed vendored dependency is consumed by the same target-module closure and no second implementation module changes.
- Public API / codec / runtime semantics review: no API, codec, wire, version, or build-surface change; observable keyed-waiter behavior changes only by eliminating the documented invalid deletion.
- Document logic review: no contradiction, impossible state, stale assumption, or unsupported narrowing was found.
- Implementation logic review: `Arc::ptr_eq` cannot exhibit pointer ABA while the old future still retains its identity; registration and conditional removal serialize on one mutex; no blocking defect was found.
- Implementation correctness audit completeness and routing: all eight required categories were reviewed and pass; no return is required.
- Document approval timing (approved_content_sha256 verified by schema-check): auto-pipeline launch authority and the unchanged proposal/plan hashes are bound by the existing schema/admission evidence.
- Implementation task paths bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ... --changed-paths-file ...`): the existing passing implementation scope contains only the admitted source plus its required evidence/state files.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: the exact pre-fix sequence—deliver the old waiter, register a replacement, then drop the old future—returned `Err(NoWaiter)` under key-only cleanup; the first two focused tests encode the drop and Ready variants and now pass.

## Validation Evidence

- Existing schema result (cite the owning-stage result; do not rerun unchanged input): the v0.1 p2p-frame schema check passed after the final testplan was created; its owned inputs are unchanged.
- Existing admission stamp (cite the stamp; do not run `--verify-only` during acceptance unless an admission-owned input changed): `20260826-callback-result-replacement-waiter-cleanup.p2p-frame.013-callback-result-replacement-waiter-cleanup.stamp.json` binds proposal hash `6a00bd5b...`, plan hash `0128d67c...`, target module, change_id, and `third-party/callback-result/src/lib.rs`; reused without replay.
- Existing stage-scope result (cite the owning-stage result; do not rerun an unchanged manifest/scope): proposal, design, implementation, and testing scope checks passed for their task-specific manifests; unchanged implementation/testing manifests were not replayed.
- Existing pipeline-plan result, when applicable (cite the latest result for current plan/state inputs): validation passed for plan hash `0128d67cf659fdea8256eae2fb2bb3daaa9b72af7ba8ebebdddcb6bfecf48847` before acceptance; completion validation is a closeout action because state/report inputs change.
- Task-relevant test run artifact(s) (reuse when implementation/tests/testplan/registration are unchanged): `test-results/test-runs/20260826T082122Z-p2p-frame+013-callback-result-replacement-waiter-cleanup-all.json`; exact task and `all` level; three executed commands cover contract/integration, unit, and DV sources; all exits and overall exit are 0.
- Commands rerun because checker-owned inputs changed after their previous pass (or `none` with evidence): architecture-doc-check and `git diff --check` passed during the read-only audit; acceptance report, acceptance scope, refreshed proposal scope after index closeout, and completion-required pipeline checks are the only closeout checkers to run for their changed inputs.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; risk-triggered compile-only consumer closure appears only inside the task artifact.
- Risk-triggered task-local contract kinds and assertions, when applicable: repository-compile-closure with assertion repository-consumers-compile passed for p2p-frame.
- Scoped evidence input hash current, when risk-triggered: `ecf24c7f13c027a39cf3ff56019f12a327868d6bb0d0e7fc120177f4c1b2dc10`.
- Quality gates: not applicable; they were not run because the user did not explicitly request them.
- Explicitly requested quality run artifact, if any: none; no quality run was requested.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: passed for the four governed architecture files; no architecture file was changed by this task.
- Acceptance report check after this report was created or modified: passed for this accepted report.
- Targeted migration search, only when applicable to the reviewed task: not applicable; no symbol, API, codec, wire, or dependency migration occurred.

## Automated Test Exception

- Applies: no
- Reason: a current task-local automated artifact exists with successful executed unit, DV, integration, and contract evidence.
- Owner: testing
- Risk: none requiring an automated-test exception.
- Acceptance impact: none.
- Alternative evidence: not needed.

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: P-CRRWC-1 is fully mapped and implemented, the ownership and synchronization model is correct, all required regression and compatibility evidence passes, and no blocking document, logic, lifecycle, concurrency, or evidence defect was found.
- Supporting task-relevant test evidence: the current three-command task artifact with evidence hash `ecf24c7f13c027a39cf3ff56019f12a327868d6bb0d0e7fc120177f4c1b2dc10`, plus passing task stage-scope evidence.
- Residual risk: the fix depends on consumers continuing to use the repository's vendored crates.io patch; within that delivery boundary, remaining risk is low and covered by the consumer compile closure.

## Follow-Up Tasks

- Requirement task: none.
- User decision required for proposal issue: no.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable; coverage is complete.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
