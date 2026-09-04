# PN Traffic Asynchronous Cleanup Task Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | launch-confirmed proposal, validated pipeline plan/state, admission stamp, production source, dedicated tests, testplan, stage manifests, machine task artifact, module boundary doc, and relevant architecture docs | no blocking requirement, design, implementation, testing, evidence-consistency, or logic finding remains | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: the PN traffic retention cleanup no longer creates a dedicated operating-system thread or performs a blocking condition-variable wait; exactly one Tokio runtime task per started traffic manager now performs the same retained-user cleanup while yielding during idle waits and large expiry batches.
- What was verified: runtime-independent construction, one idempotently started task, async timer/Notify wakeups, lost-wakeup avoidance, earlier-deadline preemption, fixed 64-item batches and executor yielding, reconnect/stale-generation safety, task ownership, synchronous shutdown/drop, start failure-state handling, extreme `Duration`, and preservation of task 008 observation, delta, retention, limit-policy, and late-session behavior.
- Evidence used: proposal and plan hashes, current admission stamp, passing proposal/design/implementation/testing stage-scope evidence, current testplan, dedicated 26-test traffic-manager source, successful 10-case async DV selection, and the successful machine-written task artifact.
- Blocking issues: none.
- Next action: the parent orchestrator can mark acceptance complete, finalize pipeline state, and close the unfinished task record.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 009-pn-traffic-async-cleanup-task
- change_id values reviewed: pn_traffic_async_cleanup_task
- Review date: 2026-07-15
- In scope: `PnTrafficManager` cleanup task construction/start, notifier and timer loop, cleanup batching, manager shared-state/handle ownership, reconnect/release wake sites, `PnServer::start`, synchronous shutdown/stop/drop, the task-local testplan, dedicated traffic tests, and task evidence.
- Out of scope: public retention semantics, snapshot/accounting fields and ordering, PN wire/codec/bridge behavior, persistence, per-user timers/tasks, new dependencies or runtime features, neighboring modules, and unrelated workspace changes.
- Task-relevant acceptance scope: packet 009, `p2p-frame/src/pn/service/pn_server.rs`, `p2p-frame/src/pn/service/pn_server/tests/traffic_manager_tests.rs`, p2p-frame long-lived/architecture boundaries, admission/stage evidence, testplan, pipeline state, and the final task artifact.
- Out-of-scope checks not run: direct package/module runtime suites, workspace-wide/root runtime suites, quality gates, unrelated harness checks, and neighboring-module tests.

## Optional Diff / Status Evidence
- `git status --short` summary: the admitted PN production file is modified and task 009 packet/evidence plus the dedicated test file are present; unrelated worktree changes were excluded.
- `git diff --stat` summary: discovery confirms the production correction remains in the plan's single admitted source path and testing remains in the dedicated nested test path.
- `git diff --name-status` summary: no task implementation path outside `p2p-frame/src/pn/service/pn_server.rs` was used.
- `git diff --check` result: passed for the task-specific production, test, packet, admission, and stage-evidence paths reviewed before report creation.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| replace the cleanup OS thread, `Condvar`, and blocking waits with one runtime task | proposal P-PN-TRAFFIC-ASYNC-CLEANUP-1; plan exported interfaces and rejected alternatives | `Notify` shared wakeup, empty construction-time handle, `Executor::spawn_with_handle` in `start_cleanup_task`, no thread/Condvar/wait API in production | source contract test, runtime-independent construction test, idempotent-start DV, all-target compile step | implemented |
| async wait, prompt earlier-deadline/reconnect/shutdown wake, and no lost notification | proposal scope/risks; plan notification ownership and future-deadline failure flow | notification future is created before `cleanup_action`; bounded `runtime::sleep` is selected against `Notify`; every release/cancellation/shutdown state change uses `notify_one`; the loop always rechecks locked authoritative state | current-thread progress/no-access expiry, earlier-deadline preemption, reconnect/stale-generation, and shutdown DV | implemented |
| no synchronous manager guard across await and bounded executor fairness | proposal lock/yield constraints; plan bounded cleanup state | `cleanup_action` contains the only cleanup state guard and returns before await; it removes at most `PN_TRAFFIC_CLEANUP_BATCH_SIZE == 64`, returning `Yield` while due work remains | direct 129-deadline branch assertion plus current-thread task progress and eventual-drain DV | implemented |
| task does not retain the manager and stop/drop cannot permit later mutation | proposal lifetime/shutdown requirements; plan task/handle ownership and synchronous-stop failure flow | task future owns only `Arc<PnTrafficManagerShared>`; manager handle is taken after shutdown flag/state clear under handle-before-state lock order; retained Notify permit wakes the detached task and every cleanup mutation starts behind the shutdown check | weak-manager destruction DV, shutdown/late-session DV, handle-taken/extreme-duration DV, manager-before-session-drop regression | implemented |
| startup idempotency and error recovery | proposal task cardinality; plan server-start failure flow | cleanup handle mutex serializes one spawn; repeated calls return success; `PnServer::start` resets `started` and propagates any `spawn_with_handle` error | one-handle idempotency DV and compile contract; active Tokio executor cannot inject the wrapper's defensive error return, with gap recorded in testplan | implemented |
| preserve task 008 retention behavior and full `Duration` handling | proposal preservation boundary; plan state ownership and extreme-deadline failure flow | active/idle counts, ordered deadlines, generation/deadline identity, retained limit policy, checked/saturated absolute deadline, one-hour sleep cap, zero default, future-only setter, shared entries/baselines, and late unregistered sessions remain intact | full dedicated traffic-manager filter plus max-duration, stale-generation, future-only setter, expiry/policy, shutdown, iterator/delta, and limit regressions | implemented |
| preserve public/wire/build/neighbor boundaries | proposal non-goals; plan API/build and consumer closure | public setter/snapshot/iterator signatures and PN transport/accounting paths are unchanged; no Cargo, executor, runtime-feature, codec, or neighboring-module file changed | p2p-frame all-target compile contract and scoped source/document review | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| pn_traffic_async_cleanup_task construction and single-task startup | normal, negative, compatibility, lifecycle | plain synchronous construction verifies no runtime-bound spawn; current-thread DV starts twice and compares the single stored handle; source contract excludes thread/Condvar | contract and unit/DV steps in the final artifact | adequate |
| asynchronous wait/progress and wake ordering | normal, error, lifecycle, concurrency | current-thread tests prove executor progress while cleanup waits, automatic expiry without observation, a 50ms newly inserted deadline preempts an existing 800ms wait, and shutdown notifies/clears | 10-case `traffic_async_cleanup_dv_` filtered step, exit code 0 | adequate |
| batching, capacity, and termination | boundary, negative, lifecycle | direct action test creates 129 due users, confirms exactly 64 are processed before `Yield`, then starts the real task and proves another task runs plus all remaining work drains | unit/DV task steps, exit code 0 | adequate |
| reconnect, stale work, and retained task 008 state | normal, boundary, negative, compatibility, lifecycle | real-task stale-generation/reconnect, future-only setter, max duration, retained policy, zero/default, iterator visibility, deltas, active counts, shutdown and orphan-session tests | full 26-test traffic-manager unit filter plus targeted async DV, both exit code 0 | adequate |
| spawn failure defense | error | source branch propagates executor failure and resets `PnServer.started`; testplan records that active Tokio `spawn_with_handle` always returns `Ok` after `tokio::spawn`, so no injectable error seam exists | compile contract and direct implementation audit; structured testplan gap is concrete and non-blocking | adequate |
| cross-module/runtime backend compatibility | cross-module not applicable with reason | task changes only a private p2p-frame manager/server lifecycle; current enabled/default backend is `runtime-tokio`; no public, wire, feature, dependency, or neighboring consumer changed | p2p-frame all-target compile contract; integration level disabled with task-specific rationale | adequate |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | startup, action selection, deadline validation, batching, server error branch | plan state/failure tables; `start_cleanup_task`, `run_cleanup_task`, `cleanup_action`, `PnServer::start`; branch and DV tests | action selection correctly exits on shutdown, removes only due exact-generation idle entries, waits for the capped next deadline, and yields only when additional due work remains; spawn errors reset `started`; no off-by-one or wrong deletion branch found | none | pass |
| termination and progress | empty/future/due schedules, 64-item batches, extreme deadlines, shutdown | async loop, one-hour cap, Notify/select, yield path; current-thread, 129-item, no-access, max-duration and shutdown DV | empty schedules suspend on Notify, future schedules select a bounded timer, each due pass removes at most 64 items then yields, stale work is consumed, and shutdown exits; no hot spin, worker blocking, starvation, or unbounded retry found | none | pass |
| concurrency and synchronization | start/shutdown races, notification timing, session/deadline races, lock/await boundaries | handle/state access order, notified-before-inspect loop, state mutex scopes, release/reconnect notifications, stale-generation DV | start and shutdown consistently acquire cleanup-handle before shared state; cleanup acquires only shared state; no lock is held across await; retained/coalesced Notify permits plus authoritative recheck prevent lost earlier-deadline/shutdown wakeups; exact identity checks prevent stale deletion | none | pass |
| resource lifetime and cleanup | task handle/future, manager/shared ownership, timers, user/deadline/limit state, stop/drop | manager/shared type graph, shutdown/drop code, weak-manager and shutdown tests | one task and one notifier exist per started manager, with no per-user future/channel; task owns shared state but not the manager; shutdown clears all manager-owned state, takes the handle, notifies, and prevents post-stop mutation before detached physical completion | none | pass |
| state and data integrity | active/idle transitions, generation/deadline identity, deltas, limit policy, shutdown | release/acquire/cleanup implementation and full task 007/008 regression tests | reconnect cancels the exact deadline and reuses entry/baseline; stale generations cannot remove current state; expiry removes only live statistics while retained limits remain; shutdown prevents registration and clears policy/configuration | none | pass |
| error handling and recovery | spawn error, extreme input, stale/missing deadlines, shutdown races, invariant failures | server start rollback, saturated deadline helper, capped sleep, stale tests, testplan gap notes | reachable spawn errors propagate and roll back `started`; `Duration::MAX` is converted to a representable deadline and safe bounded sleep; stale/missing work is ignored; mutex poisoning and generation exhaustion remain explicit fail-fast internal invariant paths | none | pass |
| interface boundary and compatibility | public synchronous stop/setter/snapshot APIs, runtime construction, wire/build behavior | proposal/plan API tables, source signatures, compile artifact, module/architecture docs | synchronous construction remains runtime-independent and stop remains synchronous; public retention/snapshot semantics, PN codec/wire/accounting, crate exports, dependencies, and runtime features do not change | none | pass |
| security and capacity safety | retained-user/deadline/task growth, executor monopolization, remote/trust surfaces | ordered-set and batch constants, task ownership, proposal risks, source/tests | task count is fixed at one per manager, deadline memory remains one item per idle user, each lock pass is capped at 64, and no unbounded queue or per-user task is added; no remote interface, permission, identity, secret, unsafe, parsing, or path boundary changes | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-PN-ASYNC-1 | proposal goal/scope and plan exported interfaces | no PN traffic cleanup OS thread, Condvar, or blocking wait remains; one runtime task starts idempotently from the async server boundary | production source, source contract, construction/start DV, compile result | pass |
| AR-PN-ASYNC-2 | proposal wakeup/lost-wakeup constraints and plan notification flow | cleanup waits asynchronously and newly earlier deadlines, reconnect cancellation, or shutdown interrupt stale waits and cause an authoritative recheck | Notify-before-inspect/select source plus earlier-deadline, reconnect, current-thread and shutdown DV | pass |
| AR-PN-ASYNC-3 | proposal lock/progress constraints and plan bounded-batch ownership | no synchronous state guard crosses await and no due set can monopolize the executor without a yield | action/task separation, 64-item constant, 129-item branch/yield/eventual-progress DV | pass |
| AR-PN-ASYNC-4 | proposal ownership/stop/drop constraints and plan handle lifecycle | future does not retain manager; synchronous stop clears state, makes later mutation impossible, wakes task, and takes handle without blocking join | manager/shared ownership review, weak-manager and shutdown/late-session DV | pass |
| AR-PN-ASYNC-5 | proposal task 008 preservation and extreme-duration criteria | retention, visibility, deltas, reconnect, generation, policy, zero/default, future-only updates, no-access expiry, shutdown, and full `Duration` remain correct | source comparison and complete dedicated unit/DV regression evidence | pass |
| AR-PN-ASYNC-6 | proposal non-goals and plan API/build closure | no per-user task/channel, dependency/runtime-feature/executor abstraction, public API, PN wire/accounting, or neighbor-module change occurs | scoped implementation review, module/architecture review, compile contract | pass |

## Inputs
- launch-confirmed `proposal.md`
- final `pipeline/plan.md` and current mutable `pipeline/state.json`
- `testplan.yaml`
- `docs/modules/p2p-frame.md`
- `docs/architecture/principles.md`
- `docs/architecture/workspace-constraints.md`
- `docs/architecture/validation-model.md`
- `p2p-frame/src/pn/service/pn_server.rs`
- `p2p-frame/src/pn/service/pn_server/tests/traffic_manager_tests.rs`
- admission evidence and generated stamp
- proposal/design/implementation/testing stage-scope manifests
- `test-results/test-runs/20260715T083511Z-p2p-frame+009-pn-traffic-async-cleanup-task-all.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed the user-launch-confirmed goal, full task 008 preservation boundary, non-goals, risks, and acceptance conditions.
2. Reviewed plan interfaces, ownership graph, notification/timer algorithm, lock order, failure paths, fixed-batch fairness, runtime compatibility, rejected alternatives, and the exact Scope Path.
3. Generated the acceptance rules above before judging source and tests.
4. Inspected production startup, cleanup control flow, progress, synchronization, resource lifetime, state integrity, error recovery, compatibility, and capacity behavior.
5. Reviewed the post-implementation testplan and dedicated tests across normal, boundary, negative, error, compatibility, lifecycle/concurrency, and cross-module applicability.
6. Matched the current testplan and change ID to the non-empty contract/unit/DV steps in the machine-written artifact.
7. Reused existing schema, admission, stage-scope, pipeline-plan, testing-coverage, and task-run results without replay because their owned inputs were unchanged.
8. Completed all eight correctness categories and found no issue requiring proposal, design, implementation, or testing return.

## Consistency Summary
- Proposal authority check: the pipeline plan records the verbatim user statement `确认，启动自动流水线`, which launch-confirms packet 009 under the auto-pipeline metadata exception.
- Proposal vs design: the plan directly maps async startup, Notify/timer recheck, fixed batch/yield, handle/shared ownership, lock ordering, shutdown, error recovery, runtime independence, full-duration behavior, and every preserved task 008 invariant.
- Design vs testing implementation: dedicated tests derive from every cleanup action, task lifecycle, notifier ordering, current-thread progress, ownership, shutdown, deadline-generation race, parameter boundary, and preserved behavior named by the plan.
- Design vs long-lived boundary doc: the only production path remains in `src/pn/service/**`, which `docs/modules/p2p-frame.md` assigns to relay-side PN server behavior; the current Tokio runtime is the workspace-priority backend and no crate/dependency boundary moved.
- Design vs implementation: empty synchronous handle construction, one executor-spawned shared-only future, notified-before-inspect loop, bounded sleep/select, 64-item action, yield, handle-before-state shutdown, state clear, notification, and start rollback match the plan.
- Test implementation vs test code vs results: testplan registers one all-target compile contract, the complete dedicated traffic-manager unit filter, and the targeted async DV filter; the artifact records all three as non-empty successful steps with exit code zero.
- Test design adequacy: private branch/state behavior is exercised at unit level, actual Tokio current-thread scheduling and timer/Notify lifecycle at DV, and cross-module integration is reasonably disabled because no public/wire/build/neighbor consumer changed.
- change_id traceability: proposal, plan, admission, stamp, state, testplan, manifests, artifact, and this report consistently use `pn_traffic_async_cleanup_task`.
- Acceptance criteria traceability: every visible outcome, correctness constraint, evidence obligation, and explicit non-goal maps to implementation and automated/direct-review evidence above.
- Cross-module admission: p2p-frame is the only evidence-bearing implementation module; no neighboring implementation path or new cross-crate interface exists.
- Public API / codec / runtime semantics review: public PN traffic APIs and codec/wire semantics are unchanged; only the private cleanup execution model changes, using the existing default/enabled Tokio runtime and executor abstraction.
- Document logic review: no contradiction, silent scope widening/narrowing, impossible required state, unsupported alternative, or stale bound hash was found.
- Implementation logic review: task cardinality, lock ordering, permit/recheck behavior, capped timers, batch/yield progress, exact stale validation, shared-only ownership, and synchronous shutdown form one consistent lifecycle.
- Implementation correctness audit completeness and routing: all eight mandatory categories pass; no return is required.
- Document approval timing (approved_content_sha256 verified by schema-check): proposal approval metadata is intentionally omitted for this explicit auto-pipeline; current proposal hash `e8a699fa...41d5` and plan hash `291ddd71...06b` exactly match the admission evidence/stamp and state plan hash.
- Implementation task paths bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ... --changed-paths-file ...`): the current implementation stage evidence passed for the single admitted production Scope Path plus admission/state evidence.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: this is a proposal-defined execution-model correction; the immutable task 008 baseline explicitly contained `std::thread`/`Condvar`, so the task 009 negative source contract would fail before the correction and passes now, while runtime behavior is covered post-implementation as required by the workflow.

## Validation Evidence
- Existing schema result (cite the owning-stage result; do not rerun unchanged input): `schema-check: passed` for v0.1/p2p-frame/009 with the launch-confirmed proposal and final plan inputs.
- Existing admission stamp (cite the stamp; do not run `--verify-only` during acceptance unless an admission-owned input changed): `docs/versions/v0.1/evidence/admission/20260715-pn-traffic-async-cleanup-task.p2p-frame.009-pn-traffic-async-cleanup-task.stamp.json`, binding proposal hash `e8a699fa...41d5`, plan hash `291ddd71...06b`, change ID `pn_traffic_async_cleanup_task`, target p2p-frame, and `p2p-frame/src/pn/service/pn_server.rs`.
- Existing stage-scope result (cite the owning-stage result; do not rerun an unchanged manifest/scope): proposal passed with 2 paths, design with 2 paths, implementation with 4 paths, and testing with 4 paths; current pipeline exit conditions record stage scope complete.
- Existing pipeline-plan result, when applicable (cite the latest result for current plan/state inputs): `pipeline-plan-check: passed` for plan hash `291ddd710dc4925cebb2209493681923f9b88bbf9c92aa55ced4831266b3606b` after task completion evidence and the A-1 scheduling wave were recorded.
- Task-relevant test run artifact(s) (reuse when implementation/tests/testplan/registration are unchanged): `test-results/test-runs/20260715T083511Z-p2p-frame+009-pn-traffic-async-cleanup-task-all.json`, requested module/task and level `all`, matching testplan/change ID, exit code 0, and three non-empty successful contract/unit/DV steps.
- Commands rerun because checker-owned inputs changed after their previous pass (or `none` with evidence): none during acceptance; proposal, plan, admission, production, tests, testplan, and registered command inputs were unchanged from the current passing evidence.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; risk-triggered compile-only consumer closure appears only inside the task artifact.
- Risk-triggered task-local contract kinds and assertions, when applicable: `external-positive/new-path-compiles` passed via `cargo test -p p2p-frame --no-run --all-targets`; runtime scheduling risk is covered by the real current-thread unit/DV steps.
- Scoped evidence input hash current, when risk-triggered: the current artifact records `9044001a06034e53609fe383fe197cac967cbc1cb81e41d47f131e7e389372af` over the declared final proposal, plan, testplan, production, and test inputs.
- Quality gates: not applicable; no authorization was given for the separate broad quality gate.
- Explicitly requested quality run artifact, if any: none; no quality run was requested.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: not rerun; the three architecture documents were directly reviewed, their owned files were unchanged, and no crate, protocol, dependency, build, or deployment boundary changed.
- Acceptance report check after this report was created or modified: run immediately after this report write.
- Targeted migration search, only when applicable to the reviewed task: scoped production review found no `Condvar`, `std::thread`, or blocking wait in the cleanup implementation; no public symbol migration or removed-symbol contract applies.

## Automated Test Exception
- Applies: no
- Reason: a current successful task-level artifact contains a compile contract, the full dedicated traffic-manager unit filter, and a targeted real-runtime async cleanup DV filter.
- Owner: acceptance
- Risk: no residual risk from missing automated task execution evidence.
- Acceptance impact: automated evidence is present, current, non-empty, and satisfies the mandatory task conditions.
- Alternative evidence: not needed because the task artifact is available and successful.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: the admitted implementation replaces the dedicated cleanup thread/blocking wait with one bounded, wakeable runtime task and preserves the full task 008 contract; independent concurrency, lifetime, progress, boundary, and evidence review found no remaining defect.
- Supporting task-relevant test evidence: `test-results/test-runs/20260715T083511Z-p2p-frame+009-pn-traffic-async-cleanup-task-all.json`.
- Residual risk: synchronous shutdown intentionally does not await detached physical task completion, relying on the locked shutdown flag plus retained Notify permit to prevent mutation and drive exit; only the currently enabled Tokio backend is compiled; timing DV uses bounded tolerances; `Duration::MAX` causes hourly rechecks; mutex poisoning and generation exhaustion remain internal fail-fast extremes.

## Follow-Up Tasks
- Requirement task: none required.
- User decision required for proposal issue: none required.
- Design task: none required.
- Implementation task: none required.
- Testing task: none required.
- Testing return reason if coverage is incomplete: coverage is complete; no return is required.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable; acceptance passed on the first audit and the pipeline has no return records.
