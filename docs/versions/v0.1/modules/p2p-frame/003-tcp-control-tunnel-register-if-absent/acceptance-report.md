# TCP Control Tunnel Post-Accept Registry Commit Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | approved proposal, validated pipeline plan, admitted implementation, post-implementation tests, successful task run, and resolved testing return | no unresolved blocking finding remains; the pending-decision coverage gap found during the first acceptance pass was supplemented and rerun successfully | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: 入站 TCP control tunnel 不再在上层处理前覆盖 registry。validator、acceptance subscriber 与 `TunnelManager` 明确接受后，listener 才提交该 tunnel；duplicate、reject、error、无 subscriber 或关闭 subscriber 都保持旧映射不变。
- What was verified: duplicate control 在 `TunnelManager` 判定 pending 期间原 tunnel data connection 仍可路由；duplicate 最终被拒绝/关闭后原 tunnel 仍可继续打开 data connection；首次 accepted tunnel 正常提交；legacy listener callback 与 stream round trip 保持兼容。
- Evidence used: proposal/plan 直接映射、admission stamp、各阶段 scope 结果、源码控制流审计、专用 unit/DV/integration 测试、compile-only consumer closure 和最终任务级 `all` artifact。
- Blocking issues: none; `TCTPAC-PENDING-COVERAGE-1` 已通过 testing return 补充并关闭。
- Next action: 自动流水线可以完成并从 unfinished-task index 移除此任务。

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 003-tcp-control-tunnel-register-if-absent
- change_id values reviewed: tcp_control_tunnel_commit_after_accept
- Review date: 2026-07-13
- In scope: acceptance-aware `TunnelNetwork` adapter；`NetManager` validator/subscriber acceptance 传播；`TunnelManager` duplicate/reverse 结果；TCP listener post-accept registry commit；TCP network legacy compatibility adapter；专用测试与任务 evidence。
- Out of scope: TCP wire frame、TLS 身份、tunnel key、公共 `Tunnel` trait、`TunnelManager` winner/publish 规则、registry cleanup 周期及其他协议的行为变化。
- Task-relevant acceptance scope: proposal P-TCTPAC-1、pipeline binding `tcp_control_tunnel_commit_after_accept`、五个 admitted production paths、task testplan 与最终成功 run artifact。
- Out-of-scope checks not run: module/package runtime suites、`all all`、root shortcut、quality gates 和无关脏工作区检查。

## Optional Diff / Status Evidence
- `git status --short` summary: 工作区存在用户其他未完成改动；审计只使用本任务 manifests 和绑定 evidence inputs。
- `git diff --stat` summary: 未作为通过条件；定向审计覆盖五个 production paths、两个专用测试文件和任务 packet。
- `git diff --name-status` summary: 未作为通过条件；任务路径以 stage-scope manifests 为准。
- `git diff --check` result: task production/test/packet paths 无 whitespace error。
- Note: diff/status 仅用于定位证据，不是验收标准。

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| 仅在上层明确接受后提交 TCP registry | proposal P-TCTPAC-1; plan State Ownership and Failure Flows | `TcpTunnelListener::on_control_connection` 不再 register；`deliver_incoming` 返回 acceptance；`handle_accepted_stream` 仅在 `Accepted` 且 tunnel 存活时 register | acceptance callback unit；真实首次 control 后 stream/data 成功 | implemented |
| validator/subscriber/manager rejection 不替换旧映射 | proposal Scope and Success Criteria | `NetManager::dispatch_tunnel`/`publish_tunnel` 返回 `Rejected` 并关闭新 tunnel；`TunnelManager` duplicate/error 返回 rejected | unit 覆盖 validator reject/error、missing subscriber、explicit reject、legacy closed；DV 覆盖 manager duplicate | implemented |
| upper decision pending 期间旧映射继续可达 | proposal Scope pending lookup requirement; plan pending data flow | registry 在 callback future 返回前不写新 tunnel，旧 weak mapping 不变 | DV 持有真实 `network-register-<remote>` locker，duplicate pending 时原 tunnel stream 成功 | implemented |
| duplicate 被关闭后旧 tunnel 后续 data connection 不出现错误覆盖导致的 TunnelNotFound | proposal Success Criteria | rejected duplicate 不执行 listener register；旧 registry entry 未清除 | 同一 DV 释放 manager locker 后确认 duplicate 不 publish，并再次从原 tunnel 成功打开 stream | implemented |
| acceptance 与 subscriber liveness 分离 | proposal Requirement Review; plan Exported Interfaces | 新 `IncomingTunnelAcceptanceSubscriber` 与旧 bool `IncomingTunnelSubscriber` 并存，handler mode 独立 | unit 覆盖 explicit accepted/rejected 与 legacy false/removal，compile closure 覆盖 repository consumers | implemented |
| wire/TLS/key/Tunnel trait/其他协议保持 | proposal non-goals | 改动只涉及 callback/dispatch/commit 时序；旧 listener constructor 和 default trait adapter 保留 | all-targets compile-only closure；legacy TCP x509 stream round trip | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| explicit acceptance dispatch | normal / negative / error | dedicated NetManager tests cover Accepted, Rejected, validator deny/error, missing subscriber, legacy false and duplicate registration | `tcp-post-accept-dispatch-unit` passed 3 tests | adequate |
| pending/rejected duplicate registry ordering | boundary / lifecycle / concurrency | real TLS/TCP pair, fixed tunnel/candidate ids, actual TunnelManager locker holds duplicate pending, original streams run before and after rejection | `tcp-duplicate-control-original-data-route-dv` passed | adequate after testing return |
| legacy adapter compatibility | compatibility / integration | existing direct `TunnelNetwork::listen` callback and real stream round trip retained | `tcp-legacy-listen-stream-integration` passed | adequate |
| repository/API consumer closure | contract / cross-module | additive/defaulted public interfaces plus unchanged legacy subscriber/listen contracts | external-positive and repository-compile-closure assertions shared one successful all-targets compile command | adequate |
| bugfix red-green contract | error / lifecycle | pre-fix executable artifact infeasible because tests are post-implementation; proposal records unconditional pre-register path and DV deliberately reproduces the same duplicate key/control/data ordering that would overwrite the old mapping before the fix | final task artifact has four successful executed commands and the DV asserts both pending and post-rejection data routing | adequate |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | validator -> subscriber -> manager -> listener commit branches | network/net_manager/tunnel_manager/listener/network adapter source plus unit/DV | `Accepted` is the only commit branch; all explicit rejection/error/no-subscriber branches return `Rejected`; legacy true/false behavior remains intentional; no fallthrough or inverted branch found | none | pass |
| termination and progress | async callback wait, manager locker, subscription dispatch | callback futures, lock acquisition/release, DV bounded timeouts | no registry lock is held across async work; NetManager drops subscription RwLock before callback; manager locker is externally releasable and existing; no new loop, retry, spin or unbounded wait introduced | none | pass |
| concurrency and synchronization | duplicate control arrival, old/new mapping visibility, registry mutex | listener commit order, manager per-remote locker, registry `Mutex<HashMap>`, pending/rejected DV | old mapping is not touched while upper decision is pending; manager serializes same-remote candidate registration; accepted commit is one mutex-protected insert; DV directly covers pending and post-rejection ordering | none | pass |
| resource lifetime and cleanup | rejected tunnels, listener callbacks, weak registry entries, subscriptions | NetManager close paths, manager duplicate close, listener live check, existing cleanup | rejected/no-subscriber tunnels are closed; duplicate close is idempotently tolerated; legacy closed subscribers are removed; registry retains weak refs and existing lookup/cleanup removes closed entries; no task/socket leak found in focused runtime test cleanup | none | pass |
| state and data integrity | TunnelManager candidate winner and TCP committed mapping | plan state owner, manager register result, listener registry write | registry no longer speculates about winner; only upper accepted tunnel can replace the key; rejected duplicate cannot overwrite/remove old entry; accepted closed tunnel is not committed | none | pass |
| error handling and recovery | validator error, callback error result, missing/closed subscriber, duplicate, listener close | NetManager dispatch/publish, manager result mapping, listener delivery | each failure maps to `Rejected` and closes the new tunnel where owned; old registry state remains usable; incoming accept errors are logged and do not commit | none | pass |
| interface boundary and compatibility | additive public listener method/result types, legacy callback/subscriber APIs, crate-private constructor | plan interfaces, source consumers, all-targets compile, legacy integration | `TunnelNetwork::listen_with_acceptance` has a default adapter; old `listen`, `IncomingTunnelSubscriber`, and `TcpTunnelListener::new` behavior remain available; no wire, codec, TLS or `Tunnel` trait migration | none | pass |
| security and capacity safety | validator trust boundary, callback allocation, registry size/cleanup | validator-before-commit order, existing bounded maps/cleanup and test channels | validator reject/error cannot reach registry commit; no sensitive logging or new unbounded queue/task/collection policy was introduced; cleanup interval unchanged | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-TCTPAC-1 | proposal P-TCTPAC-1 | only an explicitly upper-accepted TCP tunnel is committed to the registry | source audit plus acceptance unit/DV | pass |
| AR-TCTPAC-2 | proposal pending/rejection success criteria | old mapping remains routable while duplicate decision is pending and after duplicate rejection | real locked-pending duplicate DV with two successful original-tunnel streams | pass |
| AR-TCTPAC-3 | plan Exported Interfaces | acceptance result is distinct from legacy subscriber liveness without breaking existing consumers | unit branches, all-targets compile closure, legacy integration | pass |
| AR-TCTPAC-4 | proposal non-goals | no TCP wire/TLS/key/Tunnel trait/manager winner behavior change | admitted path review and focused compatibility evidence | pass |

## Inputs
- approved `proposal.md`
- launch-confirmed `pipeline/plan.md` and mutable `pipeline/state.json`
- `testplan.yaml`
- long-lived `docs/modules/p2p-frame.md` and applicable architecture constraints
- five admitted production files under `p2p-frame/src/networks/**` and `p2p-frame/src/tunnel/tunnel_manager.rs`
- dedicated NetManager and real TCP regression test files
- task run artifacts under `test-results/test-runs/`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed approved proposal, change mapping, non-goals and trigger requirements.
2. Reviewed validated pipeline design, interfaces, state ownership, failure flows and Scope Paths.
3. Reviewed admission stamp and production implementation control/concurrency flow.
4. Reviewed initial post-implementation tests and returned the missing pending-decision execution case to testing.
5. Reviewed the supplemented locked-pending DV, final task artifact, testing coverage and scope evidence.
6. Completed all eight implementation correctness categories and generated acceptance rules.

## Consistency Summary
- Proposal authority check: approved proposal hash remains the authoritative behavior baseline.
- Proposal vs design: pipeline plan maps P-TCTPAC-1 directly and preserves the user-selected post-accept replacement strategy.
- Design vs testing implementation: unit/DV/integration plus compile closure cover acceptance outcomes, pending/rejected ordering and compatibility.
- Design vs long-lived boundary doc: changes remain inside p2p-frame TCP listener/registry and tunnel orchestration responsibilities.
- Design vs implementation: five production paths, acceptance callback, subscriber mode, manager outcome and commit order match the plan.
- Test implementation vs test code vs results: final testplan commands exactly match four successful steps in the final artifact.
- Test design adequacy: adequate after `TCTPAC-PENDING-COVERAGE-1` added direct pending-decision execution.
- change_id traceability: proposal, plan, admission, testplan, state and artifact all use `tcp_control_tunnel_commit_after_accept`.
- Acceptance criteria traceability: every proposal outcome and non-goal maps to source plus automated or review evidence above.
- Cross-module admission: only p2p-frame bears implementation/test evidence; no second project module is changed.
- Public API / codec / runtime semantics review: API changes are additive/defaulted; legacy surfaces remain; wire/codec/TLS semantics are unchanged.
- Document logic review: no remaining contradiction, impossible state or unsupported assumption found.
- Implementation logic review: no unresolved correctness defect found after testing supplement and compatibility constructor correction.
- Implementation correctness audit completeness and routing: all eight categories are present and pass; the sole acceptance finding was routed to testing and resolved.
- Document approval timing (approved_content_sha256 verified by schema-check): passed for the user-approved proposal before downstream execution.
- Implementation task paths bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ... --changed-paths-file ...`): passed for five production files plus evidence/state companions.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: concrete pre-fix-run infeasibility is recorded; the exact pre-fix unconditional insertion is documented and the final green DV executes the reported duplicate/control/data sequence.

## Validation Evidence
- Existing schema result: `schema-check: passed` for v0.1/p2p-frame/003 after the final testplan content.
- Existing admission stamp: `docs/versions/v0.1/evidence/admission/20260713-tcp-control-tunnel-register-if-absent.p2p-frame.003-tcp-control-tunnel-register-if-absent.stamp.json`.
- Existing stage-scope results: proposal passed (2 paths), design passed (2 paths), implementation passed (8 paths), testing passed (10 paths; baseline `6b01da174d9ccfc27b5592981dc3f7d29be5bdb3`).
- Existing pipeline-plan result: `pipeline-plan-check: passed` after final testing completion; final acceptance state/report binding is checked after this report write.
- Task-relevant test run artifact: `test-results/test-runs/20260713T135130Z-p2p-frame+003-tcp-control-tunnel-register-if-absent-all.json`, exit code 0, four non-empty successful commands.
- Commands rerun because checker-owned inputs changed: design/admission after compatibility correction; implementation scope after constructor compatibility; testing coverage/scope and task test after pending-case supplement. No unchanged package/module runtime suite was replayed.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; compile-only all-target consumer closure ran inside the task artifact.
- Risk-triggered task-local contract kinds and assertions: `external-positive` / `new-path-compiles`; `repository-compile-closure` / `repository-consumers-compile`.
- Scoped evidence input hash current: `f4aaa08f0d8e8c2a9a5264fd8676d094b5554871d1362a1169f4d714574ac8bc` in the final artifact.
- Quality gates: not requested; not applicable to automatic single-task acceptance.
- Architecture doc check: not rerun; task does not change architecture documents or crate boundaries.
- Acceptance report check after this report was created or modified: pending this report's checker invocation.
- Targeted migration search: not required; no removed symbol or breaking/migration-required public API remains.

## Automated Test Exception
- Applies: no
- Reason: a successful automated task-level all artifact exists.
- Owner: acceptance
- Risk: none from missing automation.
- Acceptance impact: automated evidence is available and required.
- Alternative evidence: not needed because the task run executed four successful commands.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: the approved post-accept registry strategy is implemented with explicit upper-layer outcomes, admitted scope, direct pending/rejected duplicate coverage, compatibility evidence and no unresolved correctness finding.
- Supporting task-relevant test evidence: `test-results/test-runs/20260713T135130Z-p2p-frame+003-tcp-control-tunnel-register-if-absent-all.json`.
- Residual risk: finite runtime tests cannot enumerate every scheduler interleaving, but the mutex/locker control-flow proof plus a deliberately held pending duplicate and post-rejection data regressions cover the reported overwrite mechanism.

## Follow-Up Tasks
- Requirement task: none.
- User decision required for proposal issue: none.
- Design task: none.
- Implementation task: none.
- Testing task: none; `TCTPAC-PENDING-COVERAGE-1` resolved.
- Testing return reason if coverage is incomplete: not applicable; coverage is complete.
- Iteration count: 2
- Stop reason if more than 5 unsuccessful iterations: not applicable; the acceptance testing gap resolved in one return.
