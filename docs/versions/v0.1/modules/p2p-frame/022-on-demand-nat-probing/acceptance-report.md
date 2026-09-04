# P2P Frame 022 Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-ODNP-001 | none | acceptance | `sn_service.rs:827-850`; online/result-failure DV | Closed: initial UDP work and a failed result report no longer gate or remove the active SN | none |
| F-ODNP-002 | none | acceptance | `sn.rs:853-965`; `nat_probe_scheduler.rs:120-182`; legacy capability DV | Closed: probe control is explicitly negotiated and an old or unsupported client receives no directive after demand or a periodic deadline | none |
| F-ODNP-003 | none | acceptance | `service.rs:996-1044,1112-1161,1518-1519`; maintenance test | Closed: authenticated report/query/call/control traffic and bounded maintenance preserve or invalidate the exact QUIC authority | none |
| F-ODNP-004 | none | acceptance | `nat_probe_scheduler.rs:16,328-372,428`; capacity test | Closed: global in-flight issuance is capped at 256 and released by completion, invalidation or timeout | none |
| F-ODNP-005 | none | acceptance | final 14-step artifact and independent second-round review | No remaining blocking requirement, design, implementation or testing finding | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: SN now owns NAT probe timing and validity. A capable client probes only when its authoritative SN tunnel is QUIC, on SN-selected events or demand, plus one completion-based two-hour periodic deadline; TCP and legacy clients fail closed.
- What was verified: all five change_ids, capability negotiation, exact tunnel/address generations, correlated directive/results, client online ordering, two-hour scheduling, backoff/single-flight, profile publication and invalidation, global capacity, mixed-version behavior and repository consumer compilation.
- Evidence used: launch-confirmed proposal, checked pipeline plan/state, production and test sources, admission and stage-scope evidence, the final task artifact, and an independent two-round falsification review.
- Blocking issues: none; all implementation and testing returns are closed.
- Next action: mark acceptance complete and close the auto-pipeline.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 022-on-demand-nat-probing
- change_id values reviewed: sn_nat_probe_trigger_policy, sn_nat_probe_directive_protocol, nat_profile_server_owned_validity, sn_nat_probe_quic_eligibility, sn_nat_probe_two_hour_schedule
- Review date: 2026-08-30
- In scope: SN scheduling, ReportSn capability/result and response directive tails, QUIC authority tracking, profile publication, client execution gate, timeout/backoff/capacity, TCP and legacy fallback, test and consumer closure.
- Out of scope: NAT classification semantics from task 020, UDP packet format, traversal selection, PN fallback, operating-system network-change listeners, persistence, cross-SN replication and public deployment.
- Task-relevant acceptance scope: the six admitted production paths, dedicated/inline 022 tests, packet and pipeline files, admission/scope artifacts and the final task run artifact.
- Out-of-scope checks not run: public Internet or CGNAT matrix, two wall-clock hours of waiting, broad quality gates, root `all all`, hosted deployment and unrelated dirty-worktree suites.

## Optional Diff / Status Evidence
- `git status --short` summary: the worktree contains unrelated and prior-task changes; task-local stage manifests define the reviewed boundary.
- `git diff --check` result: run on task-relevant production, test and packet paths during closeout; any failure blocks completion.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| sn_nat_probe_trigger_policy / P-ODNP-1 | proposal and plan trigger mappings | scheduler observes capable reports and authenticated control endpoints; query/call demand is asynchronous; service maintenance reconciles vanished authority | scheduler events/demand/multi-tunnel tests plus real QUIC, TCP and legacy flows pass | implemented |
| sn_nat_probe_directive_protocol / P-ODNP-2 | proposal and plan wire mappings | optional capability/result and directive tails bind version, SN, peer, registration/config generation, request and expiry; the client validates before UDP | codec malformed/legacy tests, client gate tests, real UDP completion and repository consumer closure pass | implemented |
| nat_profile_server_owned_validity / P-ODNP-3 | proposal and plan ownership mappings | scheduler generation controls publication; registration refresh cannot renew observation; invalidation publishes Unknown | peer-cache ownership, stale/late result, query/context, reconnect and complete SN regressions pass | implemented |
| sn_nat_probe_quic_eligibility / P-ODNP-4 | proposal and plan authority mappings | the exact authenticated tunnel transport and remote endpoint determine eligibility; TCP cannot override a live QUIC authority | TCP-zero, concurrent tunnel, lost-authority maintenance, client protocol gate and late-result cases pass | implemented |
| sn_nat_probe_two_hour_schedule / P-ODNP-5 | proposal and plan completion-deadline mapping | per-peer SN state owns one two-hour completion-based deadline with in-flight exclusion, timeout and failure backoff | fake-time boundary cases and the real force-due directive/UDP/completion/no-duplicate DV pass | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| event/demand scheduling / sn_nat_probe_trigger_policy | normal / boundary / negative / error / lifecycle / cross-module | scheduler matrix plus real report/query/call flows | first, stable, address/config change, demand/backoff, timeout, authority loss and multi-tunnel cases execute | adequate |
| directive/result protocol / sn_nat_probe_directive_protocol | normal / malformed / replay / compatibility / lifecycle | codec, client gate and end-to-end UDP cases | legacy base, unknown version, identity/generation/request/deadline, invalid endpoints and correlated completion execute | adequate |
| profile ownership / nat_profile_server_owned_validity | normal / stale / error / lifecycle / cross-module | peer cache and real query/context flows | refresh, explicit invalidation, Unknown, late result and two-hour reuse assertions execute | adequate |
| QUIC eligibility / sn_nat_probe_quic_eligibility | normal / negative / transition / concurrency / compatibility | scheduler, client gate and real TCP/legacy flows | QUIC, TCP, mixed tunnels, disappearance, old client and zero-directive paths execute | adequate |
| two-hour cadence / sn_nat_probe_two_hour_schedule | boundary / error / lifecycle / concurrency | injected-time scheduler plus real force-due DV | before/deadline/after completion, event reset, timeout, capacity and no immediate duplicate execute | adequate |

## Defect-Discovery Categories
| Category | Status | Concrete Evidence and Falsification Result |
|----------|--------|--------------------------------------------|
| requirement-and-behavior | pass | P-ODNP-1 through P-ODNP-5 consistently retain event/demand probes and exactly one two-hour periodic mechanism; no client TTL or TCP trigger remains. |
| logic-and-control-flow | pass | `observe_capable_report`, `observe_control`, `mark_demand`, issuance and completion paths were inspected for duplicate or missing transitions; fake-time and real flow cases falsify re-entry and deadline errors. |
| boundary-and-input | pass | Missing/unknown capability, malformed tails, expired/cross-identity directives, invalid endpoint count/protocol/IP/port and the exact deadline boundary fail closed. |
| state-and-data-integrity | pass | Registration, config and request generations bind results; authority or configuration changes invalidate old publication; late/replayed results cannot overwrite current state. |
| error-handling-and-recovery | pass | Probe/result-report failures do not gate online state; timeout releases in-flight state, backoff prevents immediate retry, Unknown invalidates publication and later eligible demand can recover. |
| resource-lifetime-and-cleanup | pass | One owned maintenance task uses the server lifecycle; per-peer in-flight state is removed on completion, timeout, eligibility loss and cache removal; no client periodic timer was added. |
| concurrency-and-ordering | pass | Initial ActiveSN publication precedes UDP work, result correlation is single-flight, concurrent QUIC tunnels do not flap authority, and TCP cannot replace a live QUIC authority. |
| interface-and-compatibility | pass | Additive wire decoding preserves legacy bases while public Rust struct additions are correctly classified migration-required; p2p-frame, cyfs-p2p and sn-miner all-target consumers compile. |
| security-and-capacity | pass | Authenticated peer/tunnel identity, SN-owned bounded endpoint sets, expiry/replay checks and a 256 global in-flight cap prevent arbitrary amplification or unbounded issuance. |
| test-adequacy | pass | The final artifact contains 14 successful contract/unit/DV/integration steps, including real periodic UDP, legacy capability, TCP zero execution, maintenance, online failure and full SN regressions. |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | triggers, issuance, completion and publication | scheduler/service/client call paths and state tests | No duplicate event/periodic issuance, client-owned timer or synchronous query wait remains. | none | pass |
| termination and progress | probe timeout, failure backoff and maintenance | scheduler deadlines, client execution and server task lifecycle | Every in-flight request expires or completes; capacity is released and later demand can progress. | none | pass |
| concurrency and synchronization | multi-tunnel authority and result ordering | authority normalization, request generations and concurrent tunnel tests | Exact-tunnel state is stable and stale results cannot win after generation change. | none | pass |
| resource lifetime and cleanup | server maintenance and per-peer issuance state | start/stop task ownership, timeout and removal transitions | Maintenance is server-owned and bounded; no unbounded task or retained slot was found. | none | pass |
| state and data integrity | profile cache, generations and registration refresh | scheduler transitions, peer manager and query publication | Only the current SN-owned generation is publishable; refresh cannot extend an observation. | none | pass |
| error handling and recovery | UDP/result loss, Unknown, timeout and old clients | client online path, scheduler recovery and mixed-version tests | Failure remains off the online path and old/unsupported clients stay usable without retry loops. | none | pass |
| interface boundary and compatibility | public structs, codecs and repository consumers | protocol tails, public API contract and compile closure | Wire compatibility is additive; source migration is explicit and repository consumers compile. | none | pass |
| security and capacity safety | directive trust boundary and amplification limits | endpoint validation, identity correlation, timeout and global cap | Input is fail-closed and work is bounded per directive and globally. | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-ODNP-1 | P-ODNP-1 | SN alone triggers first/event/demand probes and stable reports do not duplicate work | scheduler transitions and report/query/call flows | pass |
| AR-ODNP-2 | P-ODNP-2 | only a current correlated directive executes and its result can update the matching request | wire, gate, replay, malformed and real UDP evidence | pass |
| AR-ODNP-3 | P-ODNP-3 | profile remains usable between probes but is invalidated by current-generation lifecycle events | peer cache, invalidation and query/context evidence | pass |
| AR-ODNP-4 | P-ODNP-4 | only a capable authoritative QUIC registration can receive or publish probe state | QUIC/TCP/mixed/legacy/disconnect evidence | pass |
| AR-ODNP-5 | P-ODNP-5 | periodic work occurs once at completion plus two hours, resets after early completion and does not re-enter | fake-time boundaries and real periodic directive count | pass |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/022-on-demand-nat-probing/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/022-on-demand-nat-probing/pipeline/plan.md`
- `docs/versions/v0.1/modules/p2p-frame/022-on-demand-nat-probing/pipeline/state.json`
- `docs/versions/v0.1/modules/p2p-frame/022-on-demand-nat-probing/testplan.yaml`
- six admitted p2p-frame production paths and dedicated/inline 022 tests
- `docs/versions/v0.1/evidence/admission/20260830-on-demand-nat-probing.md`
- task-local stage-scope manifests and baselines
- `test-results/test-runs/20260829T180059Z-p2p-frame+022-on-demand-nat-probing-all.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Re-read the launch-confirmed proposal and current pipeline plan without adopting earlier conclusions.
2. Inspect scheduler, wire, service, client, peer cache, callers and tests directly.
3. Generate counterexamples for online gating, old clients, mixed tunnels, vanished authority, capacity, replay, timeout and periodic duplication.
4. Return each concrete defect to its owning stage, then re-review the corrected production and fresh tests independently.
5. Select acceptance only after all five change_ids, ten defect categories and current artifact bindings were verified.

## Consistency Summary
- Proposal authority check: the auto-pipeline launch binds the proposal and all five change_ids; corrected wording consistently treats the two-hour periodic deadline as the sole fixed-time trigger.
- Proposal vs design: plan mappings retain every event, demand, capability, authority, profile and periodic requirement without expanding task-020 NAT classification.
- Design vs testing implementation: the final testplan registers scheduler, client, codec, cache, real QUIC/TCP/legacy/periodic and full integration evidence.
- Design vs long-lived boundary doc: no crate boundary, runtime entry or unrelated module responsibility changed.
- Design vs implementation: SN scheduler and exact authenticated tunnel observations own decisions; the client only validates and executes directives.
- Test implementation vs test code vs results: all 14 named steps resolve and exit 0 in the final artifact.
- Test design adequacy: adequate for deterministic protocol, lifecycle, capacity and loopback runtime behavior; deployment NAT diversity remains an explicit residual risk.
- change_id traceability: all five change_ids map from proposal through plan, implementation, testplan, runnable artifact and acceptance tables.
- Acceptance criteria traceability: first online, stable zero duplicate, address/config/demand events, two-hour cadence, TCP/legacy zero directive and current result correlation all have executable evidence.
- Cross-module admission: production changes are confined to admitted p2p-frame paths; risk-triggered cyfs-p2p and sn-miner consumers compile.
- Public API / codec / runtime semantics review: public struct construction is migration-required while old wire bases remain decodable; no probe packet or traversal protocol changed.
- Document logic review: proposal, plan, state and testplan now agree; all returns and corrected evidence are recorded.
- Implementation logic review: the independent second-round reviewer found no remaining production blocker.
- Implementation correctness audit completeness and routing: every required category passes and no unresolved return remains.
- Document approval timing: auto-pipeline launch evidence replaces manual approval metadata; current hashes are bound in admission.
- Implementation task paths bound to design Scope Paths: final implementation scope passed for all six admitted paths.
- Bugfix red-green regression evidence: acceptance-discovered online, capability, maintenance and capacity defects each have direct regressions in the current artifact.

## Validation Evidence
- Existing schema result: `schema-check.py --version v0.1 --module p2p-frame --submodule 022-on-demand-nat-probing` passed after final admission inputs were fixed.
- Existing admission stamp: `docs/versions/v0.1/evidence/admission/20260830-on-demand-nat-probing.p2p-frame.022-on-demand-nat-probing.stamp.json` binds the current proposal/plan, six production paths and five change_ids.
- Existing stage-scope result: implementation passed for six task paths; testing passed for nine task paths against the implementation-admitted return-3 baseline.
- Existing pipeline-plan result: current plan passed before final acceptance; the complete-state check is run after this report and state are closed.
- Task-relevant test run artifact: `test-results/test-runs/20260829T180059Z-p2p-frame+022-on-demand-nat-probing-all.json` records 14/14 steps with exit code 0.
- Commands rerun because checker-owned inputs changed: implementation admission/scope and testing coverage/scope were rerun after private test seams and the two returned runtime cases were finalized.
- Direct package/module runtime suites, whole-project suites and root shortcuts: no broad root shortcut was run; the task artifact includes the full SN suite and all-target compile closure for p2p-frame, cyfs-p2p and sn-miner.
- Risk-triggered task-local contracts: public consumer compile, removed-symbol scan, three-crate repository compile closure and wire compatibility all pass.
- Scoped evidence input hash current: artifact field `evidence_input_sha256` is `f77b7229d0f3e1fa218bb3eb85768cef9af762b2782f54ab9a0f0c89c1b00a61`; artifact file SHA-256 is `4059cd0f0ae42d7f4422b1011ebe1f665467ac56d7c8635759c1422d3396423b`.
- Quality gates: not applicable; they are outside the auto-pipeline default for this task.
- Quality run artifact: none.
- Architecture doc check: not run because no architecture document changed.
- Acceptance report check after this report was created or modified: run during final closeout; any failure blocks completion.
- Targeted migration search: repository consumer closure and all-target compilation found no unhandled public struct construction.
- Test-run stability note: the immediately preceding artifact `20260829T175839Z` failed the final integration step during a transient `AddrInUse` bind in an existing late-response test; that test then passed alone and the complete unchanged-input rerun passed 14/14. Runner stdout was observed but is not embedded in the failed artifact, so the event remains a disclosed non-blocking test-environment fluctuation.

## Automated Test Exception
- Applies: no
- Reason: a current machine-written task artifact covers every enabled level and all change_ids.
- Owner: acceptance
- Risk: no automation waiver is used; public-network NAT diversity and real wall-clock duration remain environment-dependent.
- Acceptance impact: local acceptance is supported by deterministic state tests and real loopback protocol flows; environment claims are not inferred.
- Alternative evidence: independent source review supplements but does not replace the automated artifact.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: the implementation matches all five proposal items, every returned defect has a direct regression, all required defect-discovery categories pass, the final 14-step artifact is green and the independent reviewer reports no blocker.
- Supporting task-relevant test evidence: `test-results/test-runs/20260829T180059Z-p2p-frame+022-on-demand-nat-probing-all.json`, 14/14 successful steps with current input and artifact hashes recorded above.
- Residual risk: tests use loopback rather than real public/CGNAT diversity; the two-hour DV advances only scheduler due state instead of waiting wall-clock two hours; one preceding integration run had an unretained `AddrInUse` fluctuation before the complete rerun passed.

## Follow-Up Tasks
- Requirement task: none.
- User decision required for proposal issue: none.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable; current coverage is complete.
- Iteration count: 4
- Stop reason if more than 5 unsuccessful iterations: not applicable.
