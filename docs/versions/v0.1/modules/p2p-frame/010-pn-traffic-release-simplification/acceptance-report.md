# PN Traffic Release Simplification Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | proposal/plan hashes, admitted source, dedicated tests, stage evidence, and task-run artifact reviewed | no blocking finding recorded | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: The generation namespace and platform-edge `Instant` search were removed without weakening reachable session, reconnect, expiry, cancellation, or shutdown behavior. Retention is now clamped to 30 days and an unexpected bounded `checked_add(None)` fails closed by immediately removing the just-idle user.
- What was verified: both change IDs, the reduced single-mutex state machine, 30-day normalization and public documentation, migration evidence, production code, dedicated tests, stage evidence, and the task-scoped machine artifact.
- Evidence used: proposal `sha256:3b1aa058a507e3dafbd5b136e7453b269e3eb59fcd187f7aa47592557bca14b3`; plan `sha256:7cb5f1455eac8db3c201e3205df0bc92568567196b3f1ce8490e007db1acf070`; admission stamp; stage manifests/results; `test-results/test-runs/20260715T143615Z-p2p-frame+010-pn-traffic-release-simplification-all.json` with scoped evidence hash `e92ab3cfe4fb16a131e0951822ab07a016a793dcdd16666509a5719760feb807`.
- Blocking issues: no blocking issue or acceptance return was required.
- Next action: none; the parent finalized pipeline state and the report/completion checks passed.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 010-pn-traffic-release-simplification
- change_id values reviewed: `pn_traffic_release_state_simplification`, `pn_traffic_retention_finite_bound`
- Review date: 2026-07-15
- In scope: `p2p-frame/src/pn/service/pn_server.rs`, dedicated `traffic_manager_tests.rs`, bound packet artifacts, p2p-frame/architecture boundaries, admission/stage evidence, and the task-run artifact.
- Out of scope: traffic accounting semantics, wire/bridge behavior, neighboring-crate behavior, broad module/workspace suites, and quality gates.
- Task-relevant acceptance scope: private PN traffic user lifecycle simplification plus the source-compatible, migration-required retention clamp.
- Out-of-scope checks not run: package/module runtime suites, whole-workspace suites, root shortcuts, quality gates, and unchanged schema/admission/stage/pipeline/testing/test commands.

## Optional Diff / Status Evidence
- `git status --short` summary: targeted discovery showed the admitted production file, dedicated test file, and task packet in the stacked dirty worktree.
- `git diff --stat` summary: not run; manifests and bound artifacts defined the task scope.
- `git diff --name-status` summary: not run; not needed for the acceptance standard.
- `git diff --check` result: not run in acceptance.
- Note: diff/status output was used only as a discovery aid.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| `pn_traffic_release_state_simplification` | proposal P-PN-TRAFFIC-RELEASE-SIMPLE-1; plan state ownership/failure flows and scope binding | `PnTrafficUserState` and cleanup deadlines contain no generation; acquire cancels the exact deadline; release retains acquired-entry identity and active count; cleanup pops, validates, and removes under one mutex | unit/DV coverage for final participant, same-user participation, reconnect reuse/cancellation, expiry, batching, cancellation/drop, and shutdown; artifact exits 0 | implemented |
| `pn_traffic_retention_finite_bound` | proposal P-PN-TRAFFIC-RETENTION-BOUND-1; plan 30-day compatibility and fallback mapping | `PN_TRAFFIC_RETENTION_MAX = 2_592_000s`; setter uses `min`; public doc states clamp; one `checked_add` returns `None` to immediate removal; no platform-limit search remains | exact-max and `Duration::MAX` normalization, one-hour wait cap, future-transition behavior, compile closure, and artifact exits 0 | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| Reduced lifecycle / `pn_traffic_release_state_simplification` | normal, boundary, negative, error, compatibility, lifecycle; cross-module not applicable | testplan and state map production-only reconnect/expiry races, multi-session and same-user counts, shutdown, retained visibility/identity, and explicit invariant-only gaps; obsolete stale-generation/replacement fixtures are absent | unit and focused current-thread DV steps in the task artifact pass | adequate |
| Bounded retention / `pn_traffic_retention_finite_bound` | zero, ordinary, exact maximum, above maximum, error, compatibility, lifecycle | testplan covers clamp and future-idle semantics; migration-required external-positive, removed-symbol-scan, and repository-compile-closure contract kinds are present with required assertions | three contract sources plus unit/DV steps pass; evidence-input hash is current | adequate |
| Supported-target `checked_add(None)` branch | error boundary | explicit gap names owner, risk, acceptance impact, and supported Windows/MSVC plus Linux rationale; 30 days is representable on those targets and the branch is non-panicking/immediate-release | indirect source inspection plus exact-bound automated coverage | adequate; nonblocking supported-target gap |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | acquire/release/expiry and normalization branches | source lines for exact cancellation, final-count transition, exact deadline comparison, clamp, and fallback; dedicated tests | Conditions preserve final-participant semantics; same source/target counts once; zero removes immediately; above-max clamps; no generation/platform search remains | none | pass |
| termination and progress | cleanup loop and waits | notification-before-action pattern, 64-item batch, yield, one-hour wait cap, task lifecycle tests | No lock crosses an await, no hot spin for future deadlines, and due batches yield while making bounded progress | none | pass |
| concurrency and synchronization | release/reconnect/cleanup/shutdown interleavings | one manager-state mutex, `Arc::ptr_eq`, exact deadline set, Notify sequencing, reconnect/expiry DV | Reconnect removes authoritative old work under the mutex; cleanup selects, validates, and removes under the same lock, so no authoritative stale work survives an unlock or await | none | pass |
| resource lifetime and cleanup | guard, entry, timer/task, shutdown | RAII guard, weak manager reference, handle take/notify, shutdown clearing, manager-drop/session-drop tests | Final guard releases both distinct participants once; shutdown clears state and late guards are no-ops; the cleanup task does not retain the manager | none | pass |
| state and data integrity | users, counts, idle deadline set, limits/baselines | state ownership table, source invariants, reconnect/expiry/limit tests | One owner and one exact deadline per reachable idle transition are preserved; fabricated stale/replacement recovery is deliberately unsupported, while the batch test constructs consistent due work only | none | pass |
| error handling and recovery | deadline construction, shutdown, timer timing | checked-add fallback, saturating wait calculation, early/late wake handling, shutdown paths | Unexpected `checked_add(None)` removes immediately instead of panicking/wrapping; early/late wakeups re-read authoritative state | none | pass |
| interface boundary and compatibility | public retention setter and snapshot APIs | plan API impact/migration closure, setter doc/signature, contract steps | Signature and ordinary behavior remain compatible; callers requesting more than 30 days must migrate by accepting clamp or requesting at most 30 days | none | pass |
| security and capacity safety | internal state/task growth and extreme duration input | bounded duration, one cleanup task, ordered set, batch/wait caps | No new trust boundary; extreme input cannot create platform-edge search or unbounded timer work; task count remains one per manager | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-1 | proposal/simple-state item | no generation namespace or fabricated-stale guarantee; reachable races remain safe | reduced source state plus lifecycle tests | pass |
| AR-2 | plan concurrency model | no authoritative cleanup decision crosses unlock/await | source lock/action audit and reconnect DV | pass |
| AR-3 | proposal/retention-bound item | values above 30 days clamp; zero/ordinary/exact max remain defined | constant/setter/docs and boundary tests | pass |
| AR-4 | plan failure flow | bounded `checked_add(None)` causes immediate release without panic/search | source fallback and supported-target gap record | pass |
| AR-5 | migration-required contract | required contract kinds/assertions and current scoped evidence closure exist | task artifact contract sources and evidence hash | pass |
| AR-6 | acceptance rules | all task steps succeed and evidence remains traceable to both change IDs | machine artifact and admission/stage evidence | pass |

## Inputs
- `proposal.md` (`3b1aa058...14b3`) and launch evidence
- `pipeline/plan.md` (`7cb5f145...f070`) plus pre-acceptance `pipeline/state.json`
- `testplan.yaml`, implementation, dedicated test code, and task-run artifact
- admission evidence/stamp and proposal/design/implementation/testing stage manifests/results
- `docs/modules/p2p-frame.md` and the three required workspace architecture documents
- `harness/rules/acceptance-review-rules.md`, acceptance task/auto-pipeline/test-design rules, and the acceptance-report template

## Review Order
1. Bound proposal and both change IDs.
2. Validated pipeline design/state/concurrency/failure mappings.
3. Admission and stage scope evidence.
4. Production implementation and dedicated tests, including removed fabricated cases.
5. Testplan, migration contracts, task artifact, and scoped evidence hash.
6. Long-lived p2p-frame and architecture boundaries.
7. Eight-category correctness audit and conclusion.

## Consistency Summary
- Proposal authority check: launch-confirmed proposal hash matches admission stamp and remains authoritative.
- Proposal vs design: plan maps both proposal items without narrowing or expansion.
- Design vs testing implementation: testplan/state cover reduced reachable transitions and bounded retention, with concrete supported-target/invariant-only gaps.
- Design vs long-lived boundary doc: changes remain in `p2p-frame/src/pn/service/**`; crate and neighbor boundaries are unchanged.
- Design vs implementation: exact-deadline state, 30-day clamp, immediate-release fallback, and serial scope match the plan.
- Test implementation vs test code vs results: registered contract/unit/DV commands match testplan and all artifact steps exit 0.
- Test design adequacy: adequate at the lowest effective levels; integration is explicitly disabled because the lifecycle state is private, while repository consumer compile closure covers the migration surface.
- change_id traceability: both IDs appear in proposal, plan bindings, admission stamp, testplan, state, and run artifact.
- Acceptance criteria traceability: all success criteria map to AR-1 through AR-6.
- Cross-module admission: only p2p-frame bears implementation evidence; neighboring production callers were not found and repository compile closure passed.
- Public API / codec / runtime semantics review: setter is migration-required only above 30 days; no codec/wire/signature change.
- Document logic review: no ambiguity, contradiction, or unsupported narrowing found.
- Implementation logic review: no reachable stale-deletion, overflow, progress, cleanup, or shutdown defect found.
- Implementation correctness audit completeness and routing: all eight categories reviewed and passed; no return route required.
- Document approval timing (approved_content_sha256 verified by schema-check): auto-pipeline launch binds draft proposal; existing schema pass and admission stamp bind current proposal/plan hashes.
- Implementation task paths bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ... --changed-paths-file ...`): passed for admitted `pn_server.rs` scope according to owning-stage result and manifest.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not applicable; this approved task is a simplification/refactor plus intentional bounded semantic migration, not a defect correction.

## Validation Evidence
- Existing schema result (cite the owning-stage result; do not rerun unchanged input): passed before implementation for v0.1/p2p-frame/010 with current proposal/plan bindings.
- Existing admission stamp (cite the stamp; do not run `--verify-only` during acceptance unless an admission-owned input changed): `docs/versions/v0.1/evidence/admission/20260715-pn-traffic-release-simplification.p2p-frame.010-pn-traffic-release-simplification.stamp.json`; both change IDs, proposal `3b1aa058...14b3`, plan `7cb5f145...f070`, scope `pn_server.rs`.
- Existing stage-scope result (cite the owning-stage result; do not rerun an unchanged manifest/scope): proposal, design, implementation, and testing owning-stage checks passed using the four `010-pn-traffic-release-simplification*.paths` manifests and sidecars.
- Existing pipeline-plan result, when applicable (cite the latest result for current plan/state inputs): final `--require-complete` validation passed; plan hash `7cb5f145...f070` matches recorded `plan_sha256`.
- Existing testing-coverage result: passed for both change IDs and the current testplan before the task run.
- Task-relevant test run artifact: `test-results/test-runs/20260715T143615Z-p2p-frame+010-pn-traffic-release-simplification-all.json`; exact task scope/all, both change IDs, three required contract sources, unit and DV steps, all exit 0; current scoped evidence-input hash `e92ab3cfe4fb16a131e0951822ab07a016a793dcdd16666509a5719760feb807`.
- Commands rerun because checker-owned inputs changed after their previous pass: none; acceptance reused unchanged owning-stage evidence.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; the compile-only consumer closure is contained in the task artifact.
- Risk-triggered task-local contract kinds and assertions, when applicable: `external-positive/new-path-compiles`, `removed-symbol-scan/no-unallowlisted-old-symbol-references`, and `repository-compile-closure/repository-consumers-compile`; all pass.
- Scoped evidence input hash current, when risk-triggered: yes, `e92ab3cf...b807` for proposal, testplan, production source, and dedicated tests.
- Quality gates: not applicable; no authorization was given for the separate broad quality gate.
- Explicitly requested quality run artifact, if any: none.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: existing architecture result reused; required documents were inspected and no boundary mismatch found.
- Acceptance report check after this report was created or modified: passed after shared pipeline state was finalized.
- Targeted migration search, only when applicable to the reviewed task: canonical removed-symbol scan in the task artifact passed; no ad hoc search is claimed as acceptance evidence.

## Automated Test Exception
- Applies: no
- Reason: automated task-scoped evidence exists and passed.
- Owner: testing stage
- Risk: none from absence of automation; the supported-target fallback gap is separately recorded.
- Acceptance impact: none.
- Alternative evidence: not needed because the task artifact contains executed steps.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: The delivered code matches the launch-confirmed proposal and validated plan, preserves reachable lifecycle safety with a smaller exact-deadline state model, implements the documented 30-day clamp and fail-closed fallback, and has current task-scoped migration/unit/DV evidence.
- Supporting task-relevant test evidence: `test-results/test-runs/20260715T143615Z-p2p-frame+010-pn-traffic-release-simplification-all.json` (exit 0; evidence hash `e92ab3cf...b807`).
- Residual risk: external callers requesting retention above 30 days now observe clamping; on a future unsupported target where a 30-day `Instant::checked_add` fails, the user is released immediately. Both are explicit, bounded, and nonblocking.

## Follow-Up Tasks
- Requirement task: none
- User decision required for proposal issue: none
- Design task: none
- Implementation task: none
- Testing task: none
- Testing return reason if coverage is incomplete: none; the supported-target `checked_add(None)` gap is concrete and nonblocking.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable
