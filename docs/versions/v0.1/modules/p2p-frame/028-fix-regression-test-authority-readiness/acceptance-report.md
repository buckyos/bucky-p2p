# P2P Frame 028 Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-RTR-FINAL | none | acceptance | current proposal/plan/state/testplan, production call paths, corrected tests, stage receipts and content-bound unified artifact | No blocking requirement, design, implementation or testing finding remains; first-review finding F-RTR-TEST-1 is closed by the task-scoped 12-case parallel stress DV | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: the stale SN authority fixture and PN cache-readiness race are corrected without widening production NAT, TTP, PN or TCP behavior.
- What was verified: leased remote-detail-by-value aggregation with a cold querying cache; cfg(test)-only TTP cache observation; source attach and cache readiness before one proxy request; real reverse TCP first claim; bidirectional bytes; four-way repeated scheduling pressure.
- Evidence used: current proposal/plan/state/testplan, admitted production sources, both test files, cfg(test) observer, red SN artifact, final three-step all artifact and stage-scope receipts.
- Blocking issues: none.
- Next action: close the automatic pipeline and remove task 028 from the unfinished-task index.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 028-fix-regression-test-authority-readiness
- change_id values reviewed: distributed_nat_profile_authority_fixture, pn_reverse_tcp_cache_ready_synchronization
- Review date: 2026-09-01
- In scope: two named regressions, scheduler-owned NAT publication, cold distributed detail aggregation, TTP cache readiness, unchanged PN/TCP first-claim and error behavior, task-local parallel evidence.
- Out of scope: production protocol or scheduler changes, TTP cache-policy changes, PN relay changes, broad workspace quality, public NAT and deployed multi-host validation.
- Task-relevant acceptance scope: admitted production owners, one cfg(test)-only observer, two dedicated test files, task metadata and machine run artifacts.
- Out-of-scope checks not run: package-wide runtime suite, root `all all`, quality gates, public NAT and multi-host tests.

## Optional Diff / Status Evidence
- The shared worktree contains unrelated pre-existing changes; explicit task manifests define this review boundary.
- `git diff --check` passed for task code, tests, packet and evidence paths.
- Diff/status output was used only for discovery, not as correctness proof.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| P-RTR-1 / distributed_nat_profile_authority_fixture | proposal P-RTR-1 and risks | `StaticDetailInterSnClient` validates leased serving-SN/peer identity; unchanged `query_remote_details` and `handle_query_sn` copy remote detail by value without a peer-cache write | red unit artifact shows prior `None != Some(profile)`; final all artifact passes profile, endpoint, peer-info and before/after cold-cache assertions | implemented |
| P-RTR-2 / pn_reverse_tcp_cache_ready_synchronization | proposal P-RTR-2 and success criteria | cfg(test)-only observer delegates to real lookup; every case waits after source attach and before one request; production target factory, PN bridge and TCP first-claim remain unchanged | exact DV plus three rounds of four concurrent complete topologies pass; direct B data listener is closed and both byte directions are asserted | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| cold distributed detail / distributed_nat_profile_authority_fixture | normal / boundary / lifecycle / authority integrity | V-RTR-SN-COLD-DETAIL, owner lease, strict fixture identity and cold-cache assertions | red unit plus final task-scoped all artifact | adequate |
| cache-ready reverse fallback / pn_reverse_tcp_cache_ready_synchronization | normal / boundary / negative / error / lifecycle / concurrency / cross-submodule | V-RTR-PN-CACHE-READY-STRESS, single-case DV and 3x4 full-topology stress step | all artifact runs exact DV and 12 concurrent complete cases, all successful | adequate |
| compatibility and neighboring crates | compatibility / cross-module | no public API, wire, build or neighboring-crate change; integration disabled with owner/reason/risk | production cargo check and current content-bound artifact | adequate |

## Defect-Discovery Categories
| Category | Status | Concrete Evidence and Falsification Result |
|----------|--------|--------------------------------------------|
| requirement-and-behavior | pass | Both change ids match proposal boundaries; the first-review parallel-pressure gap is closed by 12 complete concurrent cases through the task runner. |
| logic-and-control-flow | pass | SN retains owner lease -> remote detail -> final response; PN readiness precedes construction/write of the sole request and no target retry/fallback remains. |
| boundary-and-input | pass | SN fixture rejects unexpected serving SN/peer; TTP target matches production PN default-endpoint identity shape; all readiness/open/I/O waits are bounded. |
| state-and-data-integrity | pass | Querying peer cache is absent before and after; each stress case owns independent identities, networks, cache and channels; the observer creates no tunnel/stream state. |
| error-handling-and-recovery | pass | Readiness failure occurs before a request; later cache/open/claim/transport errors remain on the unchanged production path and are not reinterpreted. |
| resource-lifetime-and-cleanup | pass | Every successful case shuts down stream writers and stops PN; independent listeners/networks are case-local and released after each fixed-size round. |
| concurrency-and-ordering | pass | A four-worker runtime runs three rounds of four complete topologies; each preserves source attach -> cache readiness -> one request while cases overlap with isolated real TCP ports. |
| interface-and-compatibility | pass | Observer is `#[cfg(test)] pub(crate)` with no facade/re-export; production API, build, wire, NAT, TTP, PN and TCP semantics remain unchanged. |
| security-and-capacity | pass | No authorization/input surface changes; stress concurrency and rounds are fixed, channels bounded and waits finite, with no production resource amplification. |
| test-adequacy | pass | Unit, exact DV and complete-topology parallel stress are all registered in the task-local testplan and successful artifact; no `cyfs-p2p-test` evidence is used. |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | SN lease/aggregation and PN readiness/request | service call chain, observer, helper and exact/stress cases | Authority and ordering are correct; every case sends one request. | none | pass |
| termination and progress | readiness, target handshake, bridge I/O and stress | finite timeouts and fixed 3x4 rounds | No unbounded wait or retry was added. | none | pass |
| concurrency and synchronization | attach/cache/request happens-before under overlap | TTP attach/remember order and 12-case stress | Complete isolated topologies overlap while retaining case-local ordering. | none | pass |
| resource lifetime and cleanup | listeners, tunnels, streams, server and callbacks | case shutdown paths and existing production drops | No task-introduced leak or shared listener collision was found. | none | pass |
| state and data integrity | scheduler authority, cold cache and TTP lookup | scheduler/service paths, strict fixture and observer | No production authority widening, cache fabrication or cross-case state sharing. | none | pass |
| error handling and recovery | cache miss, claim, timeout and transport failures | production `?` propagation, response mapping and test order | Real errors remain visible and are not converted into readiness retry. | none | pass |
| interface boundary and compatibility | crate/public/wire/build surfaces | cfg(test) gating, testplan API impact and production cargo check | No consumer migration or production interface change. | none | pass |
| security and capacity safety | identities, channels, work bounds and ports | real TLS identities, bounded channels/timeouts, dynamic TCP ports | Finite test-only pressure introduces no production attack or capacity surface. | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-RTR-1 | proposal P-RTR-1 | remote profile reaches final response without granting querying-cache publication authority | red-green exact unit, owner lease, strict remote identity and before/after cold-cache assertions | pass |
| AR-RTR-2 | proposal P-RTR-2 | accepted B tunnel is cache-ready before exactly one request and real reverse fallback transfers both directions | observer/request source order plus exact real-TCP DV | pass |
| AR-RTR-3 | proposal P-RTR-2 success evidence | corrected PN case stays stable under task-scoped parallel scheduling pressure | three rounds of four concurrent complete topologies and successful all artifact | pass |
| AR-RTR-4 | proposal non-goals | no production NAT, TTP, PN, TCP, API or wire behavior changes | admission/source audit, cfg(test) gating, production check and implementation scopes | pass |

## Inputs
- current task proposal, pipeline plan/state and testplan
- admitted SN, TTP, PN and TCP production sources
- `p2p-frame/tests/unit/sn_tests/service/distributed_nat_profile_tests.rs`
- `p2p-frame/src/pn/service/pn_server/tests/reverse_tcp_proxy_tests.rs`
- cfg(test)-only observer in `p2p-frame/src/ttp/server.rs`
- admission and stage-scope receipts
- red `20260901T041604Z-...-unit.json` and final green `20260901T044650Z-...-all.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. An independent reviewer started from the proposal and ignored prior completion claims.
2. The first review found missing parallel-pressure evidence and returned Testing.
3. Re-acceptance inspected each stress branch as a complete topology, verified isolation and current artifact content binding, then completed all defect categories before selecting the result.

## Consistency Summary
- Proposal authority check: explicit `确认，自动完成` launch is recorded verbatim in the task plan.
- Proposal vs design: corrected plan preserves scheduler authority and cache-ready-before-one-request error fidelity.
- Design vs testing implementation: tests follow the mapped authority, readiness, no-retry and real reverse-path boundaries.
- Design vs long-lived boundary doc: no long-lived module boundary update was required.
- Design vs implementation: cfg(test)-only observer is inside admitted `ttp/server.rs`; production configuration remains unchanged.
- Test implementation vs test code vs results: three registered steps match current sources and all exit zero.
- Test design adequacy: normal, boundary, lifecycle, ordering and applicable concurrency/cross-submodule risks are runnable; no neighboring crate contract changed.
- change_id traceability: both change ids map through proposal, plan, admission, testplan, state validation, artifact and this report.
- Acceptance criteria traceability: red/green SN, cold cache, readiness, sole request, real reverse fallback, two-way bytes and parallel stability have direct evidence.
- Cross-module admission: not applicable; packet and target module are p2p-frame and neighboring crate interfaces are unchanged.
- Public API / codec / runtime semantics review: no public, wire, codec, build or production runtime change.
- Document logic review: proposal, plan, state and testplan now agree on parallel-pressure evidence.
- Implementation logic review: remote detail does not publish local authority; observer does not open a stream; PN/TCP errors remain real.
- Implementation correctness audit completeness and routing: every category passes and no return remains.
- Document approval timing: proposal and plan hashes remain bound by the current admission stamp.
- Implementation task paths bound to design Scope Paths: initial and returned implementation scope checks passed, including the cfg(test) observer in `ttp/server.rs`.
- Bugfix red-green regression evidence: SN has machine red/green evidence; PN's user-reported broad-suite red was schedule-sensitive, while deterministic ordering plus 12-case parallel green provide the corrected evidence.

## Validation Evidence
- Existing schema result: current packet schema passed after final testplan change.
- Existing admission stamp: `docs/versions/v0.1/evidence/admission/20260901-fix-regression-test-authority-readiness.p2p-frame.028-fix-regression-test-authority-readiness.stamp.json`.
- Existing stage-scope result: implementation return and final 15-path testing scope passed.
- Existing pipeline-plan result: current plan/state passed before final acceptance merge.
- Task-relevant test run artifact(s): `test-results/test-runs/20260901T044650Z-p2p-frame+028-fix-regression-test-authority-readiness-all.json`; three steps exit zero, including 12 complete concurrent cases.
- Commands rerun because checker-owned inputs changed: schema, testing coverage, unified all, testing scope and pipeline plan were rerun after the stress case/testplan return.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run.
- Risk-triggered task-local contract kinds and assertions: disabled because no API/build/wire/documentation impact exists.
- Scoped evidence input hash current: `4a48de75ae30d0949589d07e4731033c87d17a04b9dfbf58c2f4792ab499ffcb`.
- Quality gates: not required; the user did not request a quality run.
- Architecture doc check: not run because architecture documents and boundaries were unchanged.
- Acceptance report check after this report was created or modified: run during final closeout; failure blocks completion.
- Targeted migration search: not applicable because no public symbol migration occurred.

## Automated Test Exception
- Applies: no
- Reason: the task-scoped runner directly executes all required unit, exact DV and parallel-stress evidence.
- Owner: acceptance
- Risk: local single-process loopback does not reproduce deployed multi-host, public NAT or a complete wide-suite environment.
- Acceptance impact: the evidence supports the scoped scheduling fix without claiming excluded deployment validation.
- Alternative evidence: direct source falsification supplements, rather than replaces, the task artifact.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: both regressions are corrected at their actual authority/readiness boundaries, production behavior remains unchanged, and the final task-local artifact passes the exact cases plus twelve complete cases under four-way parallel pressure.
- Supporting task-relevant test evidence: `test-results/test-runs/20260901T044650Z-p2p-frame+028-fix-regression-test-authority-readiness-all.json`, 3/3 registered steps successful.
- Residual risk: evidence is local loopback and single-process; public NAT, multi-host and complete workspace scheduling remain outside the approved scope.

## Follow-Up Tasks
- Requirement task: none.
- User decision required for proposal issue: none.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable; F-RTR-TEST-1 is closed.
- Iteration count: 2
- Stop reason if more than 5 unsuccessful iterations: not applicable.
