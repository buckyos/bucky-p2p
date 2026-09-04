# P2P Frame 020 Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-NTAT-001 | none | acceptance | `service.rs:1422-1456`; `sn-miner-rust/src/main.rs:248-274,523-563` | Closed: probe advertisement is derived only from identity static-WAN evidence and the public IP override API is absent | none |
| F-NTAT-002 | none | acceptance | `tunnel_manager.rs:387-399`; `tunnel_manager_tests.rs:159-228` | Closed: callee handling is awaited and incoming success or owner drop cancels PunchOnly | none |
| F-NTAT-003 | none | acceptance | `tunnel_manager.rs:885-911`; `tunnel_manager_tests.rs:112-157` | Closed: prediction accepts only non-LAN IPv4 QUIC ServerReflexive bases | none |
| F-NTAT-004 | none | acceptance | `tunnel_manager_tests.rs:301-443`; fresh task artifact | Closed: a predicted endpoint succeeds and all predicted misses converge to PN | none |
| F-NTAT-005 | none | acceptance | `listener/tests.rs:120-235`; fresh deadline and cadence steps | Closed: deadline, duration overflow and listener close terminate punching | none |
| F-NTAT-006 | none | acceptance | common/v0 NAT wire tests; fresh task artifact | Closed: ReportSn request, SnCalled context and legacy/malformed/unknown tails are fail-closed and SnCallResp is unchanged | none |
| F-NTAT-007 | none | acceptance | `service.rs:85-220,538-620,2609-2705`; `sn-distributed-directory/design.md:51,108-110,225-226` | Closed: production keeps the TTP adapter and the distributed test uses an explicit test-only constructor/trait fake with no global-registry fallback | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: The delivered model expresses only observed Unknown/NonSymmetricLike/SymmetricLike behavior, first-attempt planning uses the current SnQueryResp profile, both parties execute one ordered plan, and punch/connect/fallback ownership is bounded and compatible.
- What was verified: all five change_ids, identity-derived probe endpoints, profile ownership and expiry, local and distributed first-query flow, no remote cache, no SnCallResp gate, additive wire compatibility, ordered NN/NS/SN/SS actions, bounded prediction hit/miss, direction waiters, PN fallback, PunchOnly cancellation and the explicit Inter-SN test seam.
- Evidence used: current proposal/plan/state/testplan, implementation and test sources, admission/scope evidence, neighboring distributed-directory contract, independent source review, and `test-results/test-runs/20260829T102716Z-p2p-frame+020-nat-type-aware-traversal-all.json`.
- Blocking issues: none; all four acceptance iterations' findings are closed.
- Next action: record accepted completion in pipeline state and close task bookkeeping.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 020-nat-type-aware-traversal
- change_id values reviewed: sn_nat_probe_ports, nat_type_peer_cache_and_exchange, nat_type_aware_strategy_selection, symmetric_port_prediction, nat_aware_connect_flow
- Review date: 2026-08-29
- In scope: NAT mapping observations, SN probes and profile exchange, one-shot lookup, distributed detail response, ordered tunnel plans, candidate prediction, PunchOnly lifecycle, direction waiters, PN fallback, sn-miner configuration, compatibility and task evidence.
- Out of scope: traditional four-class NAT claims, Internet-wide NAT diversity, prediction hit-rate measurement, TURN/UPnP, unrelated dirty-worktree changes, broad quality gates and root-wide suites.
- Task-relevant acceptance scope: admitted p2p-frame/sn-miner implementation paths, dedicated 020 tests, task packet, admission/stage evidence and current task artifact.
- Out-of-scope checks not run: public-network deployment, quality gates, root `all all`, unrelated module suites and hosted validation.

## Optional Diff / Status Evidence
- `git status --short` summary: the worktree contains unrelated pre-existing and historical task files; explicit 020 stage manifests define ownership.
- `git diff --check` result: passed for the task-relevant implementation, inline tests, dedicated tests and testplan.
- Targeted static search: service/client/directory contain no `InterSnRegistry::global` call; the unused legacy registry type is not a communication fallback.
- Note: diff/status/search output supports discovery and boundary checks but is not a substitute for behavior review.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| sn_nat_probe_ports / P-NTAT-1 | proposal and plan NAT probe mappings | fixed 32-byte token protocol, same-socket probes, bounded endpoints/rate and identity-derived advertisement | model/config/packet/real loopback probe tests and repository compile closure pass | implemented |
| nat_type_peer_cache_and_exchange / P-NTAT-2 | proposal query/context and ownership rules | CachedPeerInfo owns server profile, ActiveSN owns per-SN local profile, DeviceFinder passes one-shot query result, SnCall/SnCalled freeze the same context, distributed response uses explicit Inter-SN client abstraction | first local query, explicit-fake distributed final query, per-SN isolation, expiry and wire tests pass | implemented |
| nat_type_aware_strategy_selection / P-NTAT-3 | ordered matrix in proposal and plan | pure selector returns paired caller/callee actions for NN/NS/SN/SS plus Public/Unknown/unpredictable branches | selector tests cover ordering and fallback branches; TunnelManager action regressions pass | implemented |
| symmetric_port_prediction / P-NTAT-4 | bounded ServerReflexive prediction rule | fresh hints, delta/parity, deduplication, eight-candidate cap and non-LAN IPv4 QUIC ServerReflexive filter | model bounds, invalid-base rejection, predicted success and all-miss-to-PN tests pass | implemented |
| nat_aware_connect_flow / P-NTAT-5 | owner/action/waiter/fallback rules | rendezvous and action are concurrent, direction waiter registers first, callee is awaited, PunchOnly is an owned future, action errors reach PN | no-SnCallResp gate, direction, duplicate, PN, incoming, owner-drop, deadline, overflow, listener-close and regressions pass | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| probe configuration and observation / sn_nat_probe_ports | normal / boundary / negative / security / cross-module | unit, DV and contract steps bind model, service, binary and protocol files | 0/1/2/3+, malformed/token/source/timeout, same-socket reflectors and three-crate compile steps pass | adequate |
| transient profile exchange / nat_type_peer_cache_and_exchange | normal / expiry / compatibility / cross-module | local/distributed query, per-SN, expiry and additive-tail steps are registered | cold local query, explicit-fake distributed final response, no-cache assertions, ReportSn/SnCalled tails and SN suite pass | adequate |
| ordered selector / nat_type_aware_strategy_selection | normal / boundary / negative / runtime | selector enumerates four ordered combinations plus Public/Unknown/unpredictable states | pure selector and 56-test TunnelManager filter execute the planned branches | adequate |
| prediction / symmetric_port_prediction | boundary / negative / error / fallback | model and TunnelManager steps bind hint math, base eligibility and PN convergence | current-base application, cap/parity/IP/TTL, invalid bases, hit and all-miss-to-PN execute | adequate |
| lifecycle / nat_aware_connect_flow | lifecycle / concurrency / error / compatibility | waiter, call/action, punch-owner, listener and fallback cases are explicit | incoming/owner-drop/deadline/overflow/listener-close, wrong direction, duplicate cleanup, no-call gate and PN all execute | adequate |

## Defect-Discovery Categories
| Category | Status | Concrete Evidence and Falsification Result |
|----------|--------|--------------------------------------------|
| requirement-and-behavior | pass | The implementation uses only Unknown/NonSymmetricLike/SymmetricLike observations, derives remote profile from current SnQueryResp, leaves SnCallResp unchanged, and preserves the proposal's connector matrix and non-goals. |
| logic-and-control-flow | pass | Ordered selector actions, query-before-plan flow, call/action concurrency, candidate filtering and unified PN fallback were inspected and executed without an inconsistent branch. |
| boundary-and-input | pass | Zero/one/duplicate ports, malformed/unknown tails, profile expiry, IP changes, overflow, candidate cap, endpoint area/IP/protocol and direction mismatches fail closed. |
| state-and-data-integrity | pass | CachedPeerInfo, per-ActiveSN local state and one-shot PeerLookupInfo have distinct owners; distributed tests prove the querying cache remains empty and no remote map or replicated profile state exists. |
| error-handling-and-recovery | pass | Unknown/mixed/expired data returns to legacy timing; probe failures become Unknown; predicted/action failures reach PN; missing Inter-SN transport does not use a singleton shortcut. |
| resource-lifetime-and-cleanup | pass | Callee actions are awaited; waiter guards remove registrations; PunchOnly stops on incoming success, owner drop, deadline, duration overflow and listener close; duplicate tunnels are closed or deduplicated. |
| concurrency-and-ordering | pass | Waiters are direction-keyed and registered before action/call, action completion cancels the call owner, first success cleans competitors, and no detached punch task remains. |
| interface-and-compatibility | pass | Wire fields are additive, legacy bases decode, unknown/truncated tails fail closed, SnCallResp layout is unchanged, TunnelNetwork has a default method, and the private Inter-SN trait preserves production TTP query/relay dispatch. |
| security-and-capacity | pass | Probe packets are fixed-size/token/source validated, response rate and endpoint count are bounded, prediction is capped, configuration defaults off, and advertised targets remain tied to identity evidence. |
| test-adequacy | pass | The current 20-step artifact covers all five change_ids with unit, DV, integration and contract checks; the reviewer inspected assertions and independently matched its evidence hash. |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | probe/profile, selector, candidate and tunnel actions | model, SN service/client, selector and TunnelManager paths | No incorrect branch, traditional-NAT overclaim, call-response gate or unconditional reverse action remains. | none | pass |
| termination and progress | probe timeout, call/action race, PunchOnly and PN | timeout/cancellation functions and current tests | Every planned wait has a deadline/owner; action success can precede SnCallResp; failures converge to legacy or PN. | none | pass |
| concurrency and synchronization | per-SN state, waiter keys, competing candidates and callbacks | ActiveSN updates, registration guard, incoming matching, on_sn_called and regressions | State ownership and direction keys prevent cross-SN/profile mixing, lost waiters and both-sides-wait behavior. | none | pass |
| resource lifetime and cleanup | sockets, reflectors, listener punch futures, registrations and tunnels | reflector/listener owners, callee callback and duplicate tests | Owned futures and handles terminate on all required events; no detached punch or leaked waiter was found. | none | pass |
| state and data integrity | SN peer cache, one-shot remote snapshot and distributed response | PeerManager, DeviceFinder, context and explicit-fake distributed test | TTL is enforced, snapshots are immutable per plan, querying cache stays empty, and no persistence/replication path was added. | none | pass |
| error handling and recovery | malformed input, Unknown, missing hint, connect miss and transport absence | codecs, selector, open_nat_aware_tunnel and service query | Invalid data fails closed; predicted misses and action errors reach PN; missing transport is logged/skipped without hidden singleton fallback. | none | pass |
| interface boundary and compatibility | public model, wire, network trait, SN service client seam | exports/codecs/default trait/constructors and consumers | Additions retain legacy decode and implementer compatibility; production constructors still accept concrete TtpInterSnClientRef and the fake constructor is test-only. | none | pass |
| security and capacity safety | public UDP probe/spray and parsing budgets | packet validation, limiter, endpoint/candidate caps and defaults | No unbounded amplification, arbitrary advertised IP override or unsafe parsing path was found. | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-NTAT-1 | P-NTAT-1 | ports-only configuration, identity-derived advertised endpoints and truthful same-IP stable/changed/Unknown observation | config/source/probe tests and first report/query flow | pass |
| AR-NTAT-2 | P-NTAT-2 | current SnQueryResp drives a one-shot context, per-SN/distributed/expiry semantics hold, SnCallResp is not a source, and no remote/singleton cache exists | local/distributed/per-SN/expiry/wire/static-search evidence | pass |
| AR-NTAT-3 | P-NTAT-3 | both parties derive the same ordered NN/NS/SN/SS plan with exactly one connector | selector and TunnelManager action evidence | pass |
| AR-NTAT-4 | P-NTAT-4 | only fresh eligible ServerReflexive bases are predicted, candidates are bounded, a hit connects and all misses reach PN | model, invalid-base, hit and miss tests | pass |
| AR-NTAT-5 | P-NTAT-5 | rendezvous does not gate action, no detached handler exists, PunchOnly terminates on every owner event and PN closes failures | concurrency, lifecycle, duplicate and fallback tests | pass |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/020-nat-type-aware-traversal/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/020-nat-type-aware-traversal/pipeline/plan.md`
- `docs/versions/v0.1/modules/p2p-frame/020-nat-type-aware-traversal/pipeline/state.json`
- `docs/versions/v0.1/modules/p2p-frame/020-nat-type-aware-traversal/testplan.yaml`
- admitted p2p-frame and sn-miner implementation paths
- dedicated and inline 020 tests plus testing baselines
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md`
- `test-results/test-runs/20260829T102716Z-p2p-frame+020-nat-type-aware-traversal-all.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Re-read the launch-confirmed proposal, neighboring distributed-directory boundary and current plan without adopting prior conclusions.
2. Inspect implementation, callers, codecs, owners, tests, registration and the fresh artifact directly.
3. Generate observation, identity, expiry, ordering, prediction, fallback, lifecycle, registry, compatibility and capacity counterexamples.
4. Record all five change_ids and all ten defect-discovery categories.
5. Select the accepted conclusion only after independent hash verification and falsification review.

## Consistency Summary
- Proposal authority check: the auto-pipeline plan records the user's launch and binds the current proposal hash and all five change_ids.
- Proposal vs design: plan mappings preserve truthful observation scope, one-shot SnQueryResp ownership, ordered connector actions, bounded prediction and owned PunchOnly lifecycle.
- Design vs neighboring contract: the private client trait retains production TTP behavior and permits only explicit test constructor injection; no service/client/directory global registry fallback exists.
- Design vs testing implementation: current testplan registers every required normal, boundary, negative, error, lifecycle, compatibility and cross-module case.
- Design vs implementation: identity advertisement, profile owners, selector, prediction filter, callee callback, waiter direction and PN fallback match their mappings.
- Test implementation vs results: all named tests resolve and the fresh task artifact contains 20 successful steps with no disabled level.
- Test design adequacy: adequate for locally reproducible protocol, owner, lifecycle and fallback behavior; public-network distribution remains an explicit environment residual risk.
- change_id traceability: all five change_ids map from proposal through plan, implementation, testplan, runnable steps and accepted review rows.
- Acceptance criteria traceability: first SnQueryResp timing, no SnCallResp gate, local/distributed ownership, four ordered combinations, bounded prediction and every PunchOnly termination event have evidence.
- Cross-module admission: p2p-frame and sn-miner implementation paths are bound by the current admission evidence and implementation scope.
- Public API / codec / runtime semantics review: exports and additive codecs compile for repository consumers; legacy layouts/default trait behavior remain compatible; no public Tunnel API or PN protocol changed.
- Document logic review: proposal, plan, testplan, state and implementation agree; return records accurately preserve the repaired iterations.
- Implementation logic review: independent review found no remaining control-flow, ownership, state, recovery, interface or capacity defect.
- Implementation correctness audit completeness and routing: all required audit categories pass and no upstream return remains.
- Bugfix red-green regression evidence: each acceptance-discovered implementation defect has a direct boundary/lifecycle regression in the current task suite.

## Validation Evidence
- Existing schema result (cite the owning-stage result; do not rerun unchanged input): the current v0.1/p2p-frame schema passed after the final task testplan and pipeline inputs were present; no schema-owned input changed during final acceptance.
- Existing admission stamp (cite the stamp; do not run `--verify-only` during acceptance unless an admission-owned input changed): `docs/versions/v0.1/evidence/admission/20260829-nat-type-aware-traversal.p2p-frame.020-nat-type-aware-traversal.stamp.json` binds the current proposal, plan, both target modules and all five change_ids; no admission-owned input changed during final acceptance.
- Existing stage-scope result (cite the owning-stage result; do not rerun an unchanged manifest/scope): implementation passed for 21 paths and final testing passed for 20 paths using the current return-4 baseline.
- Existing pipeline-plan result, when applicable: the plan/state passed immediately before final acceptance with D/I/T complete and A running.
- Task-relevant test run artifact(s) (reuse when implementation/tests/testplan/registration are unchanged): `test-results/test-runs/20260829T102716Z-p2p-frame+020-nat-type-aware-traversal-all.json` records 20 successful steps and all five change_ids.
- Commands rerun because checker-owned inputs changed after their previous pass (or `none` with evidence): none; implementation, tests, testplan and registration are unchanged since the fresh artifact, whose evidence hash the independent reviewer recomputed as `8ff23ba38875fa635acb2c7805637b0cf41d59b636d118326bd11fd99cdc5b35`, exactly matching the artifact.
- Direct package/module runtime suites, whole-project suites and root shortcuts: not run; task-scoped unit/DV/integration plus compile/consumer closure are the acceptance evidence.
- Risk-triggered task-local contract kinds and assertions, when applicable: the task artifact contains the required public-API, removed-symbol, repository compile-closure and protocol compatibility contract assertions for the admitted surface.
- Scoped evidence input hash current, when risk-triggered: the independent acceptance review recomputed the artifact binding as `8ff23ba38875fa635acb2c7805637b0cf41d59b636d118326bd11fd99cdc5b35`; it matches the recorded current input hash.
- Quality gates: not applicable; the user did not explicitly request a quality run.
- Explicitly requested quality run artifact, if any: not applicable because no quality run was requested.
- Architecture doc check: not run because 020 did not change architecture documents.
- Acceptance report check after this report was created or modified: run during final closeout; any failure blocks completion.
- Targeted migration/static search: no remote_nat_profiles/traditional four-class symbol or service/client/directory global registry fallback was found; SnCallResp has no NAT field.

## Automated Test Exception
- Applies: no
- Reason: a current machine-written task artifact covers every enabled level and change_id.
- Owner: acceptance
- Risk: no automation waiver is used; only public-network NAT diversity remains environment-dependent.
- Acceptance impact: the local task is accepted; the environment residual risk is explicitly disclosed and does not substitute for a failed local check.
- Alternative evidence: direct source and static review supplement but do not replace the automated artifact.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: all proposal behaviors and compatibility/lifecycle boundaries are implemented and evidenced, every acceptance return is closed, all ten defect categories pass, and no blocking finding remains.
- Supporting task-relevant test evidence: `test-results/test-runs/20260829T102716Z-p2p-frame+020-nat-type-aware-traversal-all.json`, 20/20 successful steps with a reviewer-matched evidence hash.
- Residual risk: public Internet NAT mapping diversity and real predicted-port hit rates were not measured; loopback and controlled tests prove protocol, ownership, ordering, bounds and fallback rather than deployment reachability.

## Follow-Up Tasks
- Requirement task: none.
- User decision required for proposal issue: none.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable; current coverage is complete.
- Iteration count: 4
- Stop reason if more than 5 unsuccessful iterations: not applicable.
