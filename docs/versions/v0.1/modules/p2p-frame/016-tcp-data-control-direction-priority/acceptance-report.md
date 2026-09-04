# TCP Data Connection Control-Direction Priority Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-TDCDP-000 | none | acceptance | proposal, pipeline plan, admitted source diff, dedicated TCP tests, successful task artifact, and category audit | no blocking requirement, design, implementation, testing, compatibility, lifecycle, or logic finding was identified | none |

## Result Summary
- Overall result: accepted.
- Plain-language outcome: TCP channel creation now follows the established control-connection direction first. Active tunnels create locally first; Passive tunnels ask the control initiator to create first; either form falls back to the opposite direction only after preferred failure.
- What was verified: existing data-entry reuse remains ahead of creation, stream and datagram consumers reach the shared policy, both preferred paths and both fallback paths work, dual failures preserve ordered direction-labelled causes, and current reverse registration/first-claim behavior is unchanged.
- Evidence used: launch-confirmed proposal and pipeline mappings, admission stamp, stage-scope results, direct source review, dedicated signed loopback TCP/TLS cases, testplan/state coverage, and the successful task-level `all` artifact.
- Blocking issues: none.
- Next action: mark the auto-pipeline complete and remove task 016 from the unfinished-task index.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 016-tcp-data-control-direction-priority
- change_id values reviewed: tcp_data_control_direction_priority
- Review date: 2026-08-27
- In scope: `TcpTunnel::open_channel` new-connection selection, its local/peer creation order and combined errors, test-only ownership/idle observation, the dedicated control-direction tests, and task-local plan/state/testplan/evidence.
- Out of scope: wire-frame changes, TLS/identity behavior, listener authorization, request-correlation redesign, first-claim redesign, simultaneous bidirectional racing, retry/timeout tuning, PN/TTP workarounds, public APIs, and broad workspace validation.
- Task-relevant acceptance scope: proposal item P-TDCDP-1, the pipeline binding for `tcp_data_control_direction_priority`, admitted `tunnel.rs`, testing-only `network.rs` include wiring and dedicated test file, and the matching task-level artifact.
- Out-of-scope checks not run: package/module runtime suites, `all all`, root shortcuts, quality gates, architecture audits, unrelated dirty-worktree tests, and downstream broad suites.

## Optional Diff / Status Evidence
- `git status --short` summary: the repository already contains unrelated untracked Harness/task material; acceptance used only this task's explicit manifests and bound evidence.
- `git diff --stat` summary: discovery showed one admitted production file plus one existing test include file changed; the new dedicated test file is untracked and therefore not represented by ordinary diff stat.
- `git diff --name-status` summary: not used as a pass condition; production and testing scope came from admission and stage manifests.
- `git diff --check` result: no whitespace errors were reported for the task production and test-wiring paths; the dedicated test file was reviewed directly and compiled by the task command.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| reuse a claimable data entry before establishing another physical connection | proposal Scope and P-TDCDP-1; plan existing-connection flow | `find_claimable_entry` remains the first branch in `open_channel`; the direction helper is called only from the `None` arm | Active and Passive tests logically release the first stream, wait for one idle entry on both peers, then open a second channel while the data-entry count remains one | implemented |
| Active control initiator prefers local-created data, then peer-created fallback | proposal P-TDCDP-1; plan Active failure flow | Active and Proxy forms call `create_data_connection(None)` first and call `request_remote_data_connection` only from its error arm | preferred-success owner snapshot records Active local creation; a closed Passive listener forces peer-created fallback and transfers bytes successfully | implemented |
| Passive control acceptor prefers creation by the peer control initiator, then local-created fallback | proposal P-TDCDP-1; plan Passive failure flow | Passive calls `request_remote_data_connection` first and calls local `create_data_connection(None)` only from its error arm | preferred-success snapshots for stream and datagram record Active peer creation; closing the Passive listener forces Passive local-created fallback | implemented |
| preserve ordered diagnostics and actual terminal error when both directions fail | proposal Scope/Success Criteria; plan failure flows | each branch constructs one `P2pError` using the fallback error code and formats preferred then fallback direction causes | both-listeners-closed case verifies `ConnectFailed` and exact preferred-before-fallback labels for Active and Passive | implemented |
| preserve wire, registration barrier, claim ownership, retries, TLS, public API, and consumers | proposal non-goals; plan API/state/interface sections | protocol/listener/public trait files are outside production scope; helper only reorders calls to existing mechanisms and returns the same `DataConnEntry` to unchanged `claim_entry` | real TLS control/data flows, reverse peer creation, business bytes, reuse, stream, and datagram cases pass; contract checks are disabled with a concrete no-interface-change reason | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| Active and Passive preferred-direction selection | normal / boundary / compatibility | dedicated in-module tests observe production entry ownership while both listeners remain available | Active-local and Passive-peer stream cases plus Passive-peer datagram case passed | adequate |
| preferred success short-circuits fallback and existing Idle entry is reused | normal / lifecycle / ordering | snapshots require exactly one data entry, explicit creator count, and convergence to one Idle entry before a second open | both Active and Passive reuse cases passed without a second data entry | adequate |
| opposite-direction recovery | negative / error / lifecycle | separate real fixtures close the preferred target listener for Active and Passive opens | combined fallback case passed for local-to-peer and peer-to-local order | adequate |
| both directions unavailable | negative / error / termination | both listener sets are closed and each open is bounded by ten seconds | both Active and Passive return `ConnectFailed` with ordered direction-labelled causes | adequate |
| stream/datagram and cross-module depth | compatibility / cross-module | stream and datagram consume the same private `open_channel`; integration is disabled with owner, risk, and acceptance-impact reasoning because no wire/public/neighbor contract changed | one task-scoped command runs five real signed TCP/TLS cases; cross-module is correctly not applicable | adequate |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | pool reuse branch, TunnelForm match, preferred success, fallback error arms, and conflict retry loop | admitted diff, constructors, plan flow tables, five focused cases | the helper is reached only when no claimable entry exists; Active and Proxy select local then peer, Passive selects peer then local; success returns immediately and failure alone enters fallback; the unchanged claim loop still retries only conflicts | none | pass |
| termination and progress | two sequential asynchronous creation attempts and outer bounded conflict loop | helper source, existing connector/request timeouts, four-attempt claim loop, dual-failure tests | no loop or task was added; each creation mechanism retains its existing timeout/terminal behavior, fallback occurs at most once per selected entry creation, and the outer claim loop remains bounded at four iterations | none | pass |
| concurrency and synchronization | immutable form read, existing pool lock boundaries, peer request guard, and possible concurrent opens | `TcpTunnel` construction, state accessors, pending request ownership, unchanged claim arbitration | the change adds no lock, shared field, waiter, or check-then-insert sequence; form is immutable, each existing mechanism owns its current registration/cleanup, and simultaneous claim handling remains in the unchanged arbitration path | none | pass |
| resource lifetime and cleanup | failed local connection, failed peer request, successful entry handoff, logical stream release, and tunnel close | existing create/request methods, pending guard, entry retirement, reuse fixture, fallback and dual-failure cases | no connection or request owner is duplicated by the helper; preferred failure completes before fallback starts, successful entries transfer to unchanged claim ownership, and tests show logical release returns one entry to Idle without creating extras | none | pass |
| state and data integrity | `data_conns`, pending requests, first-claim state, lease reuse, and creator ownership | plan State Ownership, unchanged state methods, test-only production snapshots | the helper owns no new state and cannot register entries itself; each returned entry was registered by its existing mechanism, creator identity agrees on both peers, and a reused entry remains the single registered entry | none | pass |
| error handling and recovery | preferred connect/setup failure, fallback failure, and later claim conflict | both helper error arms, proposal diagnostics, dual-failure and fallback cases | first failure is retained as ordered context, second failure supplies the caller-visible code as before, successful fallback is not contaminated by the first error, and claim conflicts remain separately handled by the unchanged retry branch | none | pass |
| interface boundary and compatibility | internal call order, TCP frames, public Tunnel methods, TTP/PN/downstream callers | plan interface/API impact, production diff, protocol and trait scope, stream/datagram tests | no signature, export, frame, identifier, TLS validation, vport, or consumer contract changed; Active keeps prior preferred order while Passive receives the approved behavior change through the same public result contract | none | pass |
| security and capacity safety | authenticated connection mechanisms, attempt count, queues, logs, and resource amplification | proposal trigger matrix, existing TLS/request paths, helper source, bounded tests | both paths still use existing authenticated TCP/TLS and correlated control messages; the helper adds no queue, allocation, task fanout, unsafe code, credential logging, or parallel dial amplification, and retains one sequential fallback | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-TDCDP-1 | proposal P-TDCDP-1 | Active first attempts local-created data; Passive first attempts peer-created data | direct control-flow mapping plus production owner observations with both listeners available | pass |
| AR-TDCDP-2 | proposal fallback requirement | either form attempts the opposite direction only after preferred failure | selectively closed-listener success cases and helper error-arm review | pass |
| AR-TDCDP-3 | proposal reuse invariant | a claimable Idle entry remains ahead of any direction policy or new dial | `Some` branch review plus one-entry Idle/reopen evidence for both forms | pass |
| AR-TDCDP-4 | proposal diagnostics requirement | dual failure preserves both ordered direction causes and returns the fallback code | both-listeners-closed assertions for Active and Passive | pass |
| AR-TDCDP-5 | proposal compatibility/non-goals | no frame, first-claim, TLS, public API, PN/TTP, or connection-race change enters delivery | exact production scope, unchanged mechanism calls, stream/datagram real-network evidence, and no contract-migration trigger | pass |

## Inputs
- user-launch-confirmed `proposal.md`
- launch-confirmed `pipeline/plan.md` and mutable `pipeline/state.json`
- `testplan.yaml`
- `docs/modules/p2p-frame.md` TCP boundary
- `p2p-frame/src/networks/tcp/tunnel.rs`
- `p2p-frame/src/networks/tcp/network.rs`
- `p2p-frame/src/networks/tcp/tests/control_direction_priority_tests.rs`
- admission evidence and generated stamp
- task run artifact under `test-results/test-runs/`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed the launch-confirmed requirement, success criteria, risks, constraints, and explicit non-goals.
2. Reviewed pipeline mappings for Active/Passive ownership, exact call order, state ownership, failure propagation, compatibility, and one-file production scope.
3. Generated the acceptance rules above from proposal and plan before selecting a conclusion.
4. Reviewed the admitted implementation directly, including its placement relative to reuse and conflict retry, and traced both existing connection mechanisms through registration and cleanup.
5. Reviewed test design across normal, boundary, negative, error, compatibility, lifecycle, ordering, and cross-module relevance.
6. Reused the current successful task artifact and existing schema, admission, stage-scope, plan, and coverage results without replaying unchanged checks.
7. Completed all eight implementation correctness categories and found no defect requiring return routing.

## Consistency Summary
- Proposal authority check: explicit auto-pipeline launch statement “确认，自动完成任务” confirms the bound proposal; auto-pipeline rules intentionally do not require proposal approval metadata.
- Proposal vs design: the plan maps P-TDCDP-1 to Active local-to-peer, Passive peer-to-local, reuse-first behavior, sequential fallback, direction-labelled errors, and exactly one production Scope Path without narrowing or expansion.
- Design vs testing implementation: tests derive from both form branches, both success arms, both fallback arms, reuse, stream/datagram consumption, dual failure, and compatibility non-goals.
- Design vs long-lived boundary doc: all production/test behavior remains within `src/networks/tcp/**`, which `docs/modules/p2p-frame.md` assigns to p2p-frame TCP control/data ownership; no module-boundary update is needed.
- Design vs implementation: the delivered helper exactly implements the planned order and calls only the existing local/peer mechanisms; `open_channel` keeps pool reuse and claim handling outside the helper.
- Test implementation vs test code vs results: testplan unit and DV steps declare identical argv; the runner correctly deduplicated them, preserved both registration sources, executed five tests, and recorded exit code zero.
- Test design adequacy: every changed branch is reached in the crate-local test target and the same real TCP/TLS execution supplies the required single-module DV; integration disablement is justified by unchanged public/wire/neighbor boundaries.
- change_id traceability: proposal, plan, admission, state, testplan, artifact, manifests, and this report consistently use `tcp_data_control_direction_priority`.
- Acceptance criteria traceability: every required direction, fallback, reuse, diagnostic, stream/datagram, and explicit non-goal has implementation plus runnable or direct boundary evidence.
- Cross-module admission: only p2p-frame contains production/test evidence; no neighboring project implementation or exported contract changed, so no globals packet or second admission is required.
- Public API / codec / runtime semantics review: public API and codecs are unchanged; the approved runtime semantic change is limited to Passive attempt order while Active preserves its former order.
- Document logic review: no contradiction, impossible state, unsupported assumption, or silent scope change was found.
- Implementation logic review: each mutually exclusive form branch starts one preferred future, awaits it, starts fallback only on `Err`, and returns the existing registered entry to unchanged claim logic.
- Implementation correctness audit completeness and routing: all eight required categories are present and pass; no return to proposal, design, implementation, or testing is required.
- Document approval timing (approved_content_sha256 verified by schema-check): auto-pipeline launch confirmation replaces proposal approval metadata for this packet; schema-check passed with the recorded launch and current plan/testplan structure.
- Implementation task paths bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ... --changed-paths-file ...`): passed for `tunnel.rs`, admission evidence/stamp, and mutable pipeline state against `tcp_data_control_direction_priority`.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: this is a behavior-order change rather than a defect packet; the pre-change admitted source is the source-bound red baseline because it unconditionally called local creation first, and the task artifact supplies green Active/Passive owner evidence without reverting shared production state.

## Validation Evidence
- Existing schema result (cite the owning-stage result; do not rerun unchanged input): `schema-check: passed` for v0.1/p2p-frame/016 after pipeline launch and task testplan creation.
- Existing admission stamp (cite the stamp; do not run `--verify-only` during acceptance unless an admission-owned input changed): `docs/versions/v0.1/evidence/admission/20260827-tcp-data-control-direction-priority.p2p-frame.016-tcp-data-control-direction-priority.stamp.json`.
- Existing stage-scope result (cite the owning-stage result; do not rerun an unchanged manifest/scope): proposal passed with 2 paths, design passed with 2 paths, implementation passed with 4 paths, and testing passed with 6 paths using its captured baseline.
- Existing pipeline-plan result, when applicable (cite the latest result for current plan/state inputs): `pipeline-plan-check: passed` after A-1 entered running state; final complete mode follows report/state binding.
- Task-relevant test run artifact(s) (reuse when implementation/tests/testplan/registration are unchanged): `test-results/test-runs/20260827T052035Z-p2p-frame+016-tcp-data-control-direction-priority-all.json`, exit code 0, one non-empty deduplicated command with unit and DV sources, five passed tests, and `tcp_data_control_direction_priority` coverage.
- Commands rerun because checker-owned inputs changed after their previous pass (or `none` with evidence): schema reran after testplan creation; plan checking reran after mutable state transitions; testing coverage and scope reran after test metadata/artifact changes; the task test reran after adding datagram coverage; admission was not replayed because its proposal/plan/scope binding remained unchanged.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; this single-task flow used only `p2p-frame/016-tcp-data-control-direction-priority all`.
- Risk-triggered task-local contract kinds and assertions, when applicable: no breaking/migration-required API, crate-root export, build-surface, documentation-example, or wire-format change requires contract steps; runtime direction semantics are covered by the task unit/DV command.
- Scoped evidence input hash current, when risk-triggered: the current artifact records `bf0ff47bd51159acb6210261fe4c4de8e604ed2c7b8a65fd1f5b55ee1171fe74` over proposal, plan, testplan, production, include wiring, and dedicated test inputs.
- Quality gates: not applicable to automatic single-task acceptance; the user did not request a quality-gate run.
- Quality request status: not requested, so no quality-run artifact exists or is cited.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: not run because no workspace/crate boundary or architecture contract changed.
- Acceptance report check after this report was created or modified: this report is validated immediately after writing; any structural failure blocks final state completion.
- Targeted migration search, only when applicable to the reviewed task: not applicable because no symbol, frame, caller, dependency, or build migration exists; exact Scope Path and source review establish the behavior-only change.

## Automated Test Exception
- Applies: no
- Reason: a successful automated task-level `all` artifact exists with one non-empty executed command covering both registered unit and DV steps.
- Owner: acceptance
- Risk: no residual risk from missing automated execution evidence.
- Acceptance impact: automated evidence is present and required.
- Alternative evidence: not needed because the task artifact records five passing real TCP/TLS cases.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: the control-direction-first behavior is implemented inside the admitted boundary, both direction orders and fallbacks are correct, reuse and existing safety mechanisms remain intact, task-scoped evidence passes, and the independent correctness audit found no blocking defect.
- Supporting task-relevant test evidence: `test-results/test-runs/20260827T052035Z-p2p-frame+016-tcp-data-control-direction-priority-all.json`.
- Residual risk: sequential fallback retains the sum of two bounded attempt latencies when both directions are unavailable; this is the approved tradeoff for avoiding connection racing and preserving existing timeout/resource semantics.

## Follow-Up Tasks
- Requirement task: none required.
- User decision required for proposal issue: none required.
- Design task: none required.
- Implementation task: none required.
- Testing task: none required.
- Testing return reason if coverage is incomplete: coverage is complete; no return is required.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable because acceptance passed on the first audit.
