# P2P Frame 029 Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-PN-READY-FINAL | none | acceptance | current proposal/plan/state/testplan, observer and cache sources, TCP/PN callers, dedicated tests, stage receipts, and content-bound task artifact | No blocking requirement, design, implementation, or testing finding was discovered | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: the test-only PN control-tunnel readiness observer now detects a matching currently available cached tunnel without pruning a normal Connecting entry; production TTP cache, TCP state, PN open, and reverse first-claim behavior remain unchanged.
- What was verified: no observer retain/remove/insert/attach/open; lock-local cache scan; Connecting preservation and later Connected visibility; target matching and availability reuse; callback completion after attach/remember; unchanged single-request PN/TTP/TCP path; deterministic unit, exact PN DV, and 12-case concurrent DV evidence.
- Evidence used: current primary sources, task packet, admission/stage receipts, selected-file baseline, test code, and `test-results/test-runs/20260901T082521Z-p2p-frame+029-stabilize-pn-control-tunnel-readiness-all.json`.
- Blocking issues: none.
- Next action: mark automatic acceptance complete and close task 029.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 029-stabilize-pn-control-tunnel-readiness
- change_id values reviewed: pn_cache_readiness_observer_non_destructive
- Review date: 2026-09-01
- In scope: test-only readiness observer, shared cache availability/target matching definitions, incoming attach/remember completion, passive TCP promotion, unchanged PN target open, deterministic observer regression, exact reverse-TCP DV, and concurrent DV.
- Out of scope: production cache-policy changes, TCP/PN protocol changes, deadline changes, request retry, broad workspace validation, public NAT, and deployed multi-host scheduling.
- Task-relevant acceptance scope: `ttp/client.rs`, `ttp/server.rs`, `ttp/tests.rs`, `networks/tcp/tunnel.rs`, `pn/service/pn_server.rs`, PN reverse regression tests, task artifacts, and machine run evidence.
- Out-of-scope checks not run: quality gates, package-wide or workspace-wide suites, root shortcuts, network/deployment checks, and `cyfs-p2p-test`.

## Optional Diff / Status Evidence
- The shared worktree contains unrelated pre-existing changes; task manifests and the task-specific baseline were used to isolate task 029.
- The selected-file baseline proves task 029 changed only the body of the pre-existing `#[cfg(test)]` observer in `ttp/server.rs`; `git diff` was used only as a discovery aid.
- No broad diff or status result is treated as correctness proof.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| P-PN-READY-1 / pn_cache_readiness_observer_non_destructive | proposal item P-PN-READY-1, risks, and plan state/failure mappings | `TtpServer::has_cached_tunnel_for_test` holds the cache mutex and performs only bucket lookup plus `any`; it calls the existing `is_tunnel_available` and `match_target` helpers and does not call destructive production lookup | deterministic regression proves absent false, Connecting false without deletion, Connected endpoint mismatch false, and the same cached tunnel later true; exact PN and concurrent PN DVs also pass | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| non-destructive observer / pn_cache_readiness_observer_non_destructive | normal / boundary / negative / lifecycle / state integrity | V-PN-READY-OBSERVER-UNIT covers absent bucket, cached Connecting, Connected transition, endpoint mismatch, and matching ready state after callback completion | unit step exits zero in the task artifact | adequate |
| unchanged real PN/TTP/TCP behavior / pn_cache_readiness_observer_non_destructive | normal / error visibility / lifecycle / concurrency / cross-submodule | exact PN case plus three rounds of four complete concurrent topologies; each waits before one request, closes the direct target listener, and verifies reverse TCP bytes both ways | both DV steps exit zero in the task artifact | adequate |
| compatibility and production preservation | compatibility / interface boundary | observer remains crate-private `#[cfg(test)]`; production helper, lookup, attach, TCP promotion, and PN open paths are unchanged | current evidence-input binding covers every named production owner and test consumer | adequate |

## Defect-Discovery Categories
| Category | Status | Concrete Evidence and Falsification Result |
|----------|--------|--------------------------------------------|
| requirement-and-behavior | pass | The sole change id is fully covered: Connecting observation is false and non-destructive, the same entry becomes observable after Connected, and no production wait/retry guarantee is introduced. |
| logic-and-control-flow | pass | The observer uses `get(remote_id) -> values().any(available && target_match) -> false`; it contains no call to `find_existing_tunnel`, `retain`, mutation, attach, or stream open. |
| boundary-and-input | pass | The regression covers an absent identity bucket, a matching unavailable entry, a mismatched endpoint, and a matching available entry; the PN default-endpoint identity-only form is exercised by the exact/stress DVs. |
| state-and-data-integrity | pass | The cache mutex is held for the whole scan, the Connecting entry is not removed, and later observation of the same `Arc<FakeTunnel>` as Connected proves preservation rather than replacement or reinsertion. |
| error-handling-and-recovery | pass | Observer false has no side effect or error remapping; setup timeout remains before the sole request, while production cache/open/claim/transport errors retain their existing PN response path. |
| resource-lifetime-and-cleanup | pass | The observer allocates no tunnel, stream, task, timer, or handle and holds one synchronous mutex only for a finite scan; existing PN DV teardown remains unchanged. |
| concurrency-and-ordering | pass | Attach then remember completes inside the awaited subscriber callback before the fixture completion signal; observer scanning is lock-local. A later close can still race with the real open, but that unchanged TOCTOU remains visible as a production error rather than being retried. |
| interface-and-compatibility | pass | The existing signature and `#[cfg(test)] pub(crate)` visibility are unchanged; no facade, public API, codec, wire, build, production lookup, TCP, or PN semantic change was found. |
| security-and-capacity | pass | No trust/input surface changes; scan work is bounded by the existing per-peer cache, adds no retry or storage, and runs only in test configuration. |
| test-adequacy | pass | The unit test exposes the original destructive-read defect deterministically and exercises predicate branches; exact and 12-case concurrent real-TCP DVs prevent a mock-only pass. All are task-runner registered, content-bound, successful, and do not use `cyfs-p2p-test`. |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | observer predicate and production caller path | observer source, availability/match helpers, production lookup and PN single-open | Predicate reports true only for one matching available entry and performs no destructive or active operation. | none | pass |
| termination and progress | mutex scan, setup deadline, PN open and stress rounds | finite cache scan, existing 5-second setup bound, PN open timeout, fixed DV rounds | No unbounded wait, sleep extension, or retry was introduced. | none | pass |
| concurrency and synchronization | cache lock, tunnel state visibility, attach/remember completion and request ordering | observer lock scope, `Tunnel::state`, NetManager subscriber await, TTP callback, completion oneshot, concurrent DV | No observer-induced lost entry or lock-order change; completion ack is after attach and remember. | none | pass |
| resource lifetime and cleanup | cache guard and test fixture channels/tasks | observer body, FakeNetwork completion sender, existing PN teardown | Cache guard is lexical and finite; completion channel does not outlive callback completion; no task-owned production resource was added. | none | pass |
| state and data integrity | Connecting -> Connected entry preservation | selected-file baseline, same-object FakeTunnel state transition, production cache helpers | Observer does not mutate the cache and reuses the canonical state/match semantics. | none | pass |
| error handling and recovery | absent/mismatch/unavailable false, callback failure, real PN open errors | boolean observer, completion receiver, TTP attach branch, PN error propagation | False is side-effect free; callback cancellation fails the test; real request errors remain visible and unretried. | none | pass |
| interface boundary and compatibility | cfg(test) seam and production cache/TCP/PN boundaries | cfg gating, unchanged signature, current production sources, testplan API impact | No production-visible API, wire, codec, build, or caller migration exists. | none | pass |
| security and capacity safety | cache scan cost, lock duration, task-only concurrency | bounded HashMap scan, no await while locked, fixed test concurrency/timeouts | No new production capacity or security surface was introduced. | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-PN-READY-1 | proposal P-PN-READY-1 | observing a matching Connecting tunnel returns false without removal, and the same entry returns true after Connected | direct source inspection plus deterministic same-object lifecycle regression | pass |
| AR-PN-READY-2 | proposal boundaries and risks | readiness reuses canonical availability and target matching but does not reuse destructive lookup | calls to `is_tunnel_available`/`match_target`; absence of retain/remove/insert/attach/open | pass |
| AR-PN-READY-3 | proposal success criteria | exact PN reverse path and existing 12-case concurrent path remain successful without retry/deadline changes | two task-scoped DV steps and current successful artifact | pass |
| AR-PN-READY-4 | proposal non-goals | production cache, TCP promotion, PN open/error, reverse first-claim, wire, and timeout behavior remain unchanged | baseline/diff isolation, production-owner inspection, admission and scope receipts | pass |

## Inputs
- `proposal.md`, automatic `pipeline/plan.md`, `pipeline/state.json`, and `testplan.yaml`
- current admission evidence and stage-scope receipts
- `p2p-frame/src/ttp/client.rs`, `p2p-frame/src/ttp/server.rs`, and `p2p-frame/src/ttp/tests.rs`
- `p2p-frame/src/networks/tcp/tunnel.rs`
- `p2p-frame/src/pn/service/pn_server.rs`
- `p2p-frame/src/pn/service/pn_server/tests/reverse_tcp_proxy_tests.rs`
- `test-results/test-runs/20260901T082521Z-p2p-frame+029-stabilize-pn-control-tunnel-readiness-all.json`
- `harness/rules/acceptance-review-rules.md` and the applicable `cyfs-p2p-test` acceptance ban

## Review Order
1. The independent reviewer started from the current proposal and treated task state/testing conclusions as untrusted.
2. The observer and cache helpers were inspected for destructive operations, stale-state handling, lock order, TOCTOU, matching, and availability errors.
3. Attach/remember completion, TCP promotion, PN single-open/error flow, regression code, and artifact content binding were inspected before selecting the result.
4. Document consistency and lifecycle readiness were checked only after all ten defect categories and eight correctness categories were completed.

## Consistency Summary
- Proposal authority check: automatic launch is recorded verbatim as `确认，自动完成`; draft approval metadata is valid for this launch mode.
- Proposal vs design: the plan preserves production cache pruning and TCP/PN semantics while assigning only the existing test observer to testing.
- Design vs testing implementation: the observer is a lock-local non-mutating scan that reuses the exact helper semantics named by the plan.
- Design vs long-lived boundary doc: no public or long-lived module boundary changed.
- Design vs implementation: production files are inspection-only for task 029; the selected-file baseline isolates the observer body change inside the pre-existing cfg(test) item.
- Test implementation vs test code vs results: all three registered commands match current tests and exit zero.
- Test design adequacy: central mutation/state transition, target mismatch, real PN flow, and concurrent scheduling risks are runnable.
- change_id traceability: `pn_cache_readiness_observer_non_destructive` is bound through proposal, plan, admission, testplan, state evidence, run artifact, and this report.
- Acceptance criteria traceability: deterministic Connecting preservation plus exact and concurrent PN results directly cover every success criterion.
- Cross-module admission: not applicable; packet and target module are p2p-frame, with no neighboring-crate interface change.
- Public API / codec / runtime semantics review: no public, codec, wire, build, or production runtime behavior changed.
- Document logic review: proposal, plan, state, testplan, current code, and artifact agree on a test-only non-destructive observer.
- Implementation logic review: observer performs no retain/remove/insert/attach/open; production lookup remains destructive by design and PN remains single-open.
- Implementation correctness audit completeness and routing: all eight categories pass; no upstream return is required.
- Document approval timing: current proposal and plan hashes match the admission stamp.
- Implementation task paths bound to design Scope Paths: current implementation receipts enumerate all four production owners; task 029 reports no production modification.
- Bugfix red-green regression evidence: state records deterministic pre-fix exit 101 caused by observer removal; the current artifact passes the corrected exact unit and both PN DVs.

## Validation Evidence
- Existing schema result: current auto-pipeline packet inputs were accepted before downstream execution; unchanged during this acceptance review.
- Existing admission stamp: `docs/versions/v0.1/evidence/admission/20260901-stabilize-pn-control-tunnel-readiness.p2p-frame.029-stabilize-pn-control-tunnel-readiness.stamp.json`; bound proposal hash `8332e9d461dfbf5ae4cef6f089f27e376393f89e82bf8a18ca0557d0ff0bf8a2` and plan hash `0eeb22e319d1feaa929f3caf330d16ef741a087c86468c0d9148436f1374a86a` match current files.
- Existing stage-scope result: implementation and 10-path testing manifests are recorded as passed in current state; the selected-file baseline hash is `886ebc0942aaf47ddf2d9ff9f402df8108cb6ecf116c9be32f489d3fa2a5d74a`.
- Existing pipeline-plan result: current plan hash matches state and admission; downstream tasks are complete before A-1.
- Task-relevant test run artifact(s): `test-results/test-runs/20260901T082521Z-p2p-frame+029-stabilize-pn-control-tunnel-readiness-all.json`; 3/3 registered steps exit zero.
- Commands rerun because checker-owned inputs changed after their previous pass: none before report creation; only report and acceptance-scope checkers are run for acceptance-owned outputs.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run.
- Risk-triggered task-local contract kinds and assertions: disabled with a concrete no-API/build/wire reason in testplan.
- Scoped evidence input hash current: independently recomputed as `ad47da1a82c2e409ea0a6c6a0ffe04f5003f86e163eb84151a2434e80aac1dee`, equal to the artifact.
- Quality gates: not applicable; not requested and not run.
- Explicitly requested quality run artifact: none.
- Architecture doc check: not applicable because architecture documents and boundaries are unchanged.
- Acceptance report check after this report was created or modified: run during this acceptance task; failure blocks acceptance completion.
- Targeted migration search: not applicable because no public symbol migration occurred.

## Automated Test Exception
- Applies: no
- Reason: the task-scoped runner directly executes the deterministic unit regression and both required PN DV cases.
- Owner: acceptance
- Risk: local single-process loopback does not represent deployed multi-host or public-NAT timing.
- Acceptance impact: accepted only for the proposal's test-observer scope; no deployed-environment production claim is made.
- Alternative evidence: direct source and call-chain falsification supplements the machine artifact.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: the existing test-only observer is now non-destructive, preserves Connecting entries until real TCP promotion, reuses canonical availability/match semantics, and leaves production behavior unchanged; deterministic, exact PN, and concurrent PN evidence are current and successful.
- Supporting task-relevant test evidence: `test-results/test-runs/20260901T082521Z-p2p-frame+029-stabilize-pn-control-tunnel-readiness-all.json`, 3/3 steps successful with current evidence-input hash.
- Residual risk: the boolean readiness result cannot eliminate the unchanged close-after-observation TOCTOU before production open; that error remains intentionally visible. Evidence is local loopback/single-process and does not establish public-NAT or deployed multi-host behavior.

## Follow-Up Tasks
- Requirement task: none.
- User decision required for proposal issue: none.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
