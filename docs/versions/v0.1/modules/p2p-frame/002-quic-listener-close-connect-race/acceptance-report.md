# QUIC Listener Close/Connect Race Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | approved proposal, validated pipeline plan, admitted implementation, post-implementation tests, and successful task run | no unresolved blocking finding remains after the two recorded return routes | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: QUIC listener 关闭与主动建链并发时，worker endpoint 读取先在持有同一 state 锁期间判定 closed/empty，再进行首项或随机选择；空集合不再进入 `unwrap` 或 `random_range(0..0)`。
- What was verified: closed 优先映射为 `Interrupted`，open+empty 映射为 `ErrorState`，正常 worker 选择继续工作，关闭后调用方不会 panic，crate-private `bound_local` 错误不会在 fallible connect 路径中被吞掉。
- Evidence used: proposal/plan 直接映射、admission stamp、两阶段 scope 结果、源码审计、专用 unit/DV/integration 测试和成功的任务级 `all` 运行制品。
- Blocking issues: none; `QLCCR-ERR-PROP-1` 与 `QLCCR-ACTIVE-CONNECT-COVERAGE-1` 已分别经 implementation 和 testing return 修复。
- Next action: 任务可完成并从 unfinished-task index 移除。

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 002-quic-listener-close-connect-race
- change_id values reviewed: quic_listener_close_connect_race
- Review date: 2026-07-12
- In scope: `p2p-frame/src/networks/quic/listener.rs` 的 closed/empty endpoint guard、随机/首项选择、`bound_local` 错误；`p2p-frame/src/networks/quic/network.rs` 的直接调用方传播；专用测试和任务 evidence。
- Out of scope: QUIC wire/TLS/CID、worker 创建数量、TCP/PN/SN、关闭后重启和工作区宽范围维护验证。
- Task-relevant acceptance scope: proposal P-QLCCR-1、pipeline binding `quic_listener_close_connect_race`、两个 admitted production paths、task testplan 与最终成功 run artifact。
- Out-of-scope checks not run: package/module suite、`all all`、root shortcut、quality gates 和无关脏工作区检查。

## Optional Diff / Status Evidence
- `git status --short` summary: 工作区存在用户的其他未完成改动；本审计只使用任务 manifest 中的路径。
- `git diff --stat` summary: 未作为通过条件；定向源码 diff 显示 listener guard/返回类型、QUIC network 调用方和专用测试变更。
- `git diff --name-status` summary: 未作为通过条件；任务路径由 stage-scope manifests 定义。
- `git diff --check` result: task production/test paths passed with no whitespace errors.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| 同锁 closed/empty 检查后再选择 endpoint | proposal P-QLCCR-1; plan State Ownership and Failure Flows | `ensure_worker_endpoints_available` is called while the state read guard is held in `bound_local` and `connect_with_owner_runtime` | guard unit plus active-connect/close DV in final task run | implemented |
| closed 返回 Interrupted；open+empty 返回 ErrorState | proposal P-QLCCR-1 | closed-first guard branch and fallible `bound_local` | guard unit covers open/nonempty, open/empty, closed/nonempty, closed/empty; active connect directly covers open/empty and closed | implemented |
| 空集合不进入 first unwrap/random range | proposal Background, Scope, P-QLCCR-1 | guarded `state.endpoints[0]` and guarded `random_range(0..len)` | unit and active-connect DV complete without panic | implemented |
| 正常首项/随机 worker 选择保持 | proposal Requirement Review and Success Criteria | first index remains zero; active connect still selects random index over nonempty vector | real listener bound-local DV and real active-connect DV | implemented |
| crate-private caller compatibility | plan Exported Interfaces | `open_or_connect` caches fallible bound address; local-ep selection propagates error; listener info omits unavailable entries | focused TunnelNetwork listener info/close integration | implemented |
| wire/TLS/CID and worker creation remain unchanged | proposal non-goals | no changes outside listener access and direct network callers | targeted integration plus source diff review | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| quic_listener_close_connect_race | normal / boundary / negative / error | unit guard covers normal nonempty, empty, closed and closed+empty priority | `quic-listener-endpoint-guard-unit` passed | adequate |
| close/read synchronization | lifecycle / concurrency | real one-worker listener with eight concurrent readers and close | `quic-listener-close-race-dv` passed | adequate |
| active connect random selection versus close | lifecycle / concurrency / error | sixteen real `connect_with_owner_runtime` attempts use a started worker against a bounded no-response UDP peer while close races; before/after states assert ErrorState/Interrupted | `quic-listener-active-connect-close-race-dv` passed | adequate |
| TunnelNetwork compatibility | compatibility / cross-module | real network listener reports a bound address and disappears after close through unchanged trait methods | `quic-listener-network-integration` passed | adequate |
| bugfix red-green contract | error / lifecycle | pre-fix executable artifact is infeasible because tests are required post-implementation and the old code lacked a fallible seam; the exact pre-fix `first().unwrap()` and `random_range(0..0)` paths are recorded in proposal/testplan, with final green automated evidence | final task-level all artifact contains four successful executed steps | adequate |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | closed-first availability guard, first/random index selection, caller branching | listener/network source and unit/DV evidence | guard returns before all indexing; nonempty normal path selects index 0 or `0..len`; no off-by-one or fallthrough found | none | pass |
| termination and progress | lock duration, connect timeout, retry behavior | read-guard blocks, endpoint clone boundary, 50 ms DV timeout, unchanged connect loop | guard is constant time; state lock is released before runtime spawn/await; no new loop or unbounded wait introduced | none | pass |
| concurrency and synchronization | close atomic flag, state RwLock, endpoint clear/select interleavings | `close`, `bound_local`, `connect_with_owner_runtime`, reader and active-connect race tests | close marks closed before acquiring write lock; endpoint readers hold read lock across closed/empty/select; a reader winning first clones a valid endpoint before clear, while close winning first yields Interrupted; no check-then-act panic window remains | none | pass |
| resource lifetime and cleanup | cloned Quinn endpoint, worker runtime task, listener close | selected endpoint ownership, close loop, DV cleanup | selected clone remains owned through spawn; close still closes all registered endpoints/server; tests close listeners and blackhole socket is scope-owned | none | pass |
| state and data integrity | closed flag and endpoint vector | plan state ownership and source | closed has priority over empty diagnosis; endpoint vector is read/cleared only under its RwLock; no writer or collection mutation was added | none | pass |
| error handling and recovery | Interrupted, ErrorState, connect failure, caller propagation | helper, fallible bound_local, open_or_connect and local-ep selection | expected lifecycle/state errors are explicit; acceptance-returned swallowed error was fixed with `?`; infallible info snapshot intentionally omits unavailable listeners | none | pass |
| interface boundary and compatibility | crate-private bound_local migration; public TunnelNetwork methods | plan Exported Interfaces, all direct call sites, integration test | all direct callers migrated; no public trait, wire, codec, TLS or downstream API changed | none | pass |
| security and capacity safety | random index bounds, task/thread counts, buffers | guard and tests | empty upper bound cannot reach RNG; no unbounded allocation/task creation or security boundary change; DV concurrency is fixed at 16 connects/8 readers | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-QLCCR-1 | proposal P-QLCCR-1 | closed/empty is checked under the endpoint state lock before first/random selection | source audit plus unit/DV | pass |
| AR-QLCCR-2 | proposal Success Criteria | close/connect interleavings never panic and return lifecycle/state/normal connect errors | active-connect DV task step | pass |
| AR-QLCCR-3 | plan Exported Interfaces | fallible bound-local migration preserves public TunnelNetwork compatibility | direct caller audit plus integration step | pass |
| AR-QLCCR-4 | proposal non-goals | no QUIC wire, TLS, CID, worker creation, or adjacent transport behavior changes | path-bound diff and implementation scope stamp | pass |

## Inputs
- approved `proposal.md`
- launch-confirmed `pipeline/plan.md` and mutable `pipeline/state.json`
- `testplan.yaml`
- long-lived `docs/modules/p2p-frame.md` and applicable architecture constraints
- `p2p-frame/src/networks/quic/listener.rs`
- `p2p-frame/src/networks/quic/network.rs`
- `p2p-frame/src/networks/quic/listener/tests.rs`
- task run artifacts under `test-results/test-runs/`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed approved proposal and non-goals.
2. Reviewed pipeline design mapping and admitted Scope Paths.
3. Reviewed implementation and found/routed one caller error propagation defect.
4. Reviewed initial testing and found/routed one active-connect coverage gap.
5. Re-reviewed corrected implementation, generated tests, task testplan, and successful final evidence.
6. Completed all implementation correctness categories and generated acceptance rules.

## Consistency Summary
- Proposal authority check: approved proposal hash was validated by the latest schema result and remains the acceptance authority.
- Proposal vs design: pipeline plan directly maps P-QLCCR-1 without narrowing or expansion.
- Design vs testing implementation: final unit/DV/integration steps cover the planned state, failure, lifecycle, concurrency and compatibility surfaces.
- Design vs long-lived boundary doc: changes remain inside `p2p-frame/src/networks/quic/**` lifecycle responsibility.
- Design vs implementation: locked guard, error priority, crate-private migration and two Scope Paths match the plan.
- Test implementation vs test code vs results: testplan commands exactly match four executed successful steps in the final run artifact.
- Test design adequacy: adequate after the recorded testing return added direct active-connect/close coverage.
- change_id traceability: proposal, plan, admission, testplan, pipeline state and run artifact all use `quic_listener_close_connect_race`.
- Acceptance criteria traceability: every proposal outcome and non-goal has implementation and test/review evidence above.
- Cross-module admission: only p2p-frame bears implementation/test evidence; no second module admission is required.
- Public API / codec / runtime semantics review: public interfaces and wire/codec/TLS semantics are unchanged; crate-private callers were migrated.
- Document logic review: no contradiction, impossible required state or unsupported acceptance assumption remains.
- Implementation logic review: no remaining correctness defect found after the error-propagation return.
- Implementation correctness audit completeness and routing: all eight categories are present and pass; both prior findings were routed to and resolved by their owning stages.
- Document approval timing (approved_content_sha256 verified by schema-check): passed on 2026-07-12 after pipeline/test state changes.
- Implementation task paths bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ... --changed-paths-file ...`): passed for listener.rs, network.rs, admission evidence/stamp and pipeline state.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: concrete infeasibility reason is recorded for missing pre-fix executable red artifact; exact pre-fix panic operations are documented and four final automated steps provide green evidence.

## Validation Evidence
- Existing schema result (cite the owning-stage result; do not rerun unchanged input): `schema-check: passed` for v0.1/p2p-frame/002 on 2026-07-12 after final test/state inputs.
- Existing admission stamp (cite the stamp; do not run `--verify-only` during acceptance unless an admission-owned input changed): `docs/versions/v0.1/evidence/admission/20260712-quic-listener-close-connect-race.p2p-frame.002-quic-listener-close-connect-race.stamp.json`.
- Existing stage-scope result (cite the owning-stage result; do not rerun an unchanged manifest/scope): proposal passed (2 paths), design passed (2 paths), implementation passed (5 paths), testing passed after return (10 paths; baseline `b48b05b99d538e34126cd047aa3f977d47775a98`).
- Existing pipeline-plan result, when applicable (cite the latest result for current plan/state inputs): `pipeline-plan-check: passed` after testing completion; it will be rerun only for final acceptance state/report binding.
- Task-relevant test run artifact(s) (reuse when implementation/tests/testplan/registration are unchanged): `test-results/test-runs/20260712T122128Z-p2p-frame+002-quic-listener-close-connect-race-all.json`, exit code 0, four non-empty successful steps.
- Commands rerun because checker-owned inputs changed after their previous pass (or `none` with evidence): schema after final state/testplan changes; implementation scope after caller propagation fix; testing coverage/scope and task tests after each implementation/testing return.
- Package/module, whole-project, and root-shortcut tests: not run; forbidden for single-task acceptance.
- Quality gates: not applicable to this single-task acceptance because the user did not request them.
- Explicitly requested quality run artifact, if any: none requested.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: not rerun; task does not change architecture docs or crate boundaries.
- Acceptance report check after this report was created or modified: pending this report's checker invocation.
- Targeted migration search, only when applicable to the reviewed task: direct `bound_local` call-site search was performed during design/implementation; all QUIC call sites were migrated.

## Automated Test Exception
- Applies: no
- Reason: a successful automated task-level all artifact exists.
- Owner: acceptance
- Risk: none from missing automation.
- Acceptance impact: automated evidence is available and required.
- Alternative evidence: not needed because the task run executed four successful steps.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: proposal-defined panic prevention, error priority, normal selection compatibility and caller behavior are implemented, admitted, directly tested and free of unresolved correctness findings after two successful return routes.
- Supporting task-relevant test evidence: `test-results/test-runs/20260712T122128Z-p2p-frame+002-quic-listener-close-connect-race-all.json`.
- Residual risk: scheduling stress is finite and cannot enumerate every runtime interleaving, but the locked control-flow proof plus direct 16-connect/close and 8-reader/close regressions cover the reported race mechanism.

## Follow-Up Tasks
- Requirement task: none.
- User decision required for proposal issue: none.
- Design task: none.
- Implementation task: none; `QLCCR-ERR-PROP-1` resolved.
- Testing task: none; `QLCCR-ACTIVE-CONNECT-COVERAGE-1` resolved.
- Testing return reason if coverage is incomplete: not applicable; coverage is complete.
- Iteration count: 3
- Stop reason if more than 5 unsuccessful iterations: not applicable; no issue exceeded one return.
