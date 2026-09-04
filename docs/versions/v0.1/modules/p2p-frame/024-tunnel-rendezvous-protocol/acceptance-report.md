# P2P Frame 024 Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-TRP-001 | none | implementation | `rendezvous_state.rs` begin/cache/terminal and five state tests | Closed: in-flight same-digest requests originally conflicted; they now share one bounded result channel and never start a second target action | none |
| F-TRP-002 | none | implementation | `sn.rs` endpoint validator, TunnelManager base filtering and protocol negative cases | Closed: LAN-area/private endpoints originally passed the common validator; only server-reflexive non-LAN IPv4 endpoints now pass | none |
| F-TRP-003 | none | implementation | response validation, SN relay timeout and both TunnelManager action boundaries | Closed: local timeouts originally renewed the attempt budget; all stages now consume the original absolute deadline | none |
| F-TRP-004 | none | implementation | `RendezvousKey` request/terminal conversion and terminal negative test | Closed: serving-SN identity is now part of state correlation | none |
| F-TRP-005 | none | implementation | `attach_rendezvous_task`, gated target task and collision DV | Closed: target task attach/start is atomic with owner validation, so a displaced attempt cannot return an armed success | none |
| F-TRP-006 | none | acceptance | current proposal, plan, sources, tests and final 10-step artifact | No blocking requirement, design, implementation or testing defect remains after the recorded automatic returns | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: the independent SN tunnel rendezvous protocol now carries A-owned action endpoints and an optional request for B-owned predicted endpoints, arms B before responding, and lets TunnelManager perform exactly one planned connector action with bounded cleanup and serial legacy fallback.
- What was verified: all seven change_ids, exact request/response business bodies, four operations, same-socket prediction, same-SN and inter-SN dispatch, endpoint ownership, idempotency, deadline/collision/terminal behavior, real TunnelManager publication boundaries and mixed-version fallback behavior.
- Evidence used: launch-confirmed proposal, checked pipeline plan/state, current production and test sources, admission/scope evidence, and `test-results/test-runs/20260831T080843Z-p2p-frame+024-tunnel-rendezvous-protocol-all.json`.
- Blocking issues: none; eight recorded returns are closed and no issue repeated.
- Next action: close the auto-pipeline while retaining the public double-symmetric-NAT run as an explicit environment gap.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 024-tunnel-rendezvous-protocol
- change_id values reviewed: sn_tunnel_rendezvous_wire_contract, sn_tunnel_rendezvous_action_modes, sn_tunnel_rendezvous_endpoint_ownership, quic_rendezvous_socket_binding, sn_tunnel_rendezvous_lifecycle, sn_tunnel_rendezvous_security, tunnel_manager_rendezvous_integration
- Review date: 2026-08-31
- In scope: independent rendezvous wire family, request/response/terminal lifecycle, actual QUIC listener prediction, same/cross-SN relay, four target actions, TunnelManager integration, security/capacity limits and legacy/PN fallback.
- Out of scope: exchanging NAT type on wire, deleting SnCall, changing PN/TLS/public Tunnel APIs, TURN/UPnP/NAT-PMP, persistent attempt storage and guaranteeing traversal through every NAT.
- Task-relevant acceptance scope: thirteen admitted production paths, task-local tests/testplan, pipeline artifacts, admission/scope evidence and the final machine-written task run.
- Out-of-scope checks not run: two public carrier symmetric NATs, deployed multi-SN owner-directory infrastructure, broad quality gates, unrelated workspace suites and root `all all`.

## Optional Diff / Status Evidence
- `git status --short` summary: the shared worktree contains prior-task and unrelated changes; task manifests, not the whole dirty tree, define this review.
- `git diff --check` result: all task production/test paths edited during the acceptance returns passed.
- Note: diff/status output was used only for discovery, not as acceptance proof.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| P-STRP-1 / sn_tunnel_rendezvous_wire_contract | proposal exact bodies and independent family; plan exported interfaces | `common.rs` independent `0x28..=0x2b`; `sn.rs` versioned envelope/request/response/terminal; client/SN/inter-SN consumers | five protocol cases, real same-SN QA, inter-SN dispatcher and compile closure | implemented |
| P-STRP-2 / sn_tunnel_rendezvous_action_modes | proposal four-mode table; plan deterministic mapping | operation enum plus `RendezvousPlan` mapping and TunnelManager caller/target branches | five plan cases cover all four operations, one connector and symmetric/symmetric ownership | implemented |
| P-STRP-3 / sn_tunnel_rendezvous_endpoint_ownership | proposal owner-generated direction and strict list invariant | common server-reflexive non-LAN IPv4 validator; A/B call sites only consume owner-produced lists | protocol boundary cases, listener source-port DV and same-SN third-party rejection | implemented |
| P-STRP-4 / quic_rendezvous_socket_binding | proposal actual-socket/generation contract | listener-owned PNAT waiters and binding generation; QUIC network prediction implementation | two listener DVs prove observed bound socket, token correlation and close invalidation | implemented |
| P-STRP-5 / sn_tunnel_rendezvous_lifecycle | proposal ordering/idempotency/collision/terminal requirements | bounded SN state, in-flight response sharing, absolute deadline, gated action attach, owners/waiters and terminal convergence | five state tests, same-SN QA and expanded collision DV | implemented |
| P-STRP-6 / sn_tunnel_rendezvous_security | proposal authentication, ownership, anti-replay and capacity model | authenticated command/inter-SN boundaries; digest/SN correlation; 8 endpoints, 8 duplicate waiters, 8 pair, 32 rate and 256 total limits | malformed/digest/endpoint/deadline/replay/rate/capacity/third-party negatives | implemented |
| P-STRP-7 / tunnel_manager_rendezvous_integration | proposal real handshake publication and serial fallback | `open_rendezvous_tunnel` delegates success only to existing direct/incoming registration paths; failure cancels then invokes legacy path once | plan/collision/fallback/predicted-connect tests plus same-SN control flow | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| wire contract / sn_tunnel_rendezvous_wire_contract | normal / boundary / negative / error / compatibility / lifecycle / cross-module | exact codec, shape, version, correlation and failure cases | protocol unit, same-SN DV, inter-SN integration and repository compile steps pass | adequate |
| action modes / sn_tunnel_rendezvous_action_modes | normal / boundary / negative / error / compatibility / lifecycle / cross-module | operation/body domains and full NAT-plan matrix | five plan cases plus fallback integration pass | adequate |
| endpoint ownership / sn_tunnel_rendezvous_endpoint_ownership | normal / boundary / negative / error / compatibility / lifecycle / cross-module | owner direction, area/IP/list bounds and source-port observation | protocol negatives, real UDP listener DV and same-SN ownership rejection pass | adequate |
| socket binding / quic_rendezvous_socket_binding | normal / boundary / negative / error / compatibility / lifecycle / cross-module | real bound socket, generation, waiter correlation and close | two listener DVs plus predicted-connect integration pass | adequate |
| lifecycle / sn_tunnel_rendezvous_lifecycle | normal / boundary / negative / error / compatibility / lifecycle / cross-module | in-flight/cached idempotency, terminal wakeup, deadline, collision and fallback | five state cases, real same-SN QA, collision DV and fallback pass | adequate |
| security / sn_tunnel_rendezvous_security | normal / boundary / negative / error / compatibility / lifecycle / cross-module | trust-boundary malformed, third-party, replay, digest and capacity cases | protocol/state/same-SN/inter-SN steps pass | adequate |
| integration / tunnel_manager_rendezvous_integration | normal / boundary / negative / error / compatibility / lifecycle / cross-module | one connector, publication through existing register/publish, collision and serial fallback | plan, same-SN, collision, predicted candidate, fallback and compile steps pass | adequate |

## Defect-Discovery Categories
| Category | Status | Concrete Evidence and Falsification Result |
|----------|--------|--------------------------------------------|
| requirement-and-behavior | pass | Proposal P-STRP-1..7 was reread against current call paths. NAT type remains outside wire; exact business bodies and the B-before-response ordering are preserved. Message-level version/action/fixed IPv4 endpoint validation yields typed Unsupported/Invalid results and old commands remain untouched. |
| logic-and-control-flow | pass | All four plan outputs were traced through caller and target branches. Each plan has one connector; prediction true/false selects the correct owner list; response is not treated as Connected; failure enters one serial legacy path. |
| boundary-and-input | pass | Full-slice codec, version, operation/body relation, transport, deadline, list size, duplicates, server-reflexive area, non-LAN IPv4, port, digest and response-list invariants were inspected and have runnable negatives. |
| state-and-data-integrity | pass | State key binds SN/A/B/tunnel/attempt plus digest; same digest is New/InFlight/Cached without duplicate action, changed digest conflicts, terminal tombstones state, and first TunnelManager publication remains authoritative. |
| error-handling-and-recovery | pass | Prediction/relay/action/timeout/collision failures are typed, Cancel is best effort, local owners independently clean up, and rendezvous failure precedes a single legacy/PN fallback. |
| resource-lifetime-and-cleanup | pass | PNAT token waiters wake on close; duplicate waiters are capped; task/start/cancel handles are owned by attempt state; terminal, timeout, collision and peer removal drain or abort temporary resources. |
| concurrency-and-ordering | pass | The review found and closed command-server reentrancy, in-flight duplicate and target attach races. SN-to-B QA runs in a distinct awaited task, response sharing prevents duplicate target actions, and attach plus start release is atomic under the owner lock. |
| interface-and-compatibility | pass | New command ids and callback/prediction interfaces are additive; TunnelNetwork has a default unsupported method; SnCall/Called and PN wire remain unchanged; unknown/old participation fails before serial fallback. |
| security-and-capacity | pass | Authenticated tunnel identity and cert binding precede action; SN validates A endpoint IP ownership; target validates initiator cert; lists/state/rate/duplicate waiters are bounded and logs omit payload/cert/full endpoint lists. |
| test-adequacy | pass | The final artifact runs 10 task-specific unit/DV/integration steps. It exercises real in-process SN/client QA and real listener UDP source ownership, with dispatcher-level cross-SN and mock transport publication/fallback; unavailable public carrier NAT evidence is explicitly residual. |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | NAT-plan mapping and A/B actions | plan mapper, request/response branches and terminal/fallback paths | Four modes preserve endpoint direction and one connector; no response-only success branch exists. | none | pass |
| termination and progress | request, relay, prediction, action and terminal | original deadline propagation, owner finish/cancel and state expiry | Every stage is bounded by the original deadline or owner cancellation; no retry loop renews it. | none | pass |
| concurrency and synchronization | QA reentrancy, duplicate requests and collision | separate SN-to-B task, oneshot waiters, owner lock and gated attach | No mutex guard crosses a new await; duplicate and attach races are closed and tested where deterministic. | none | pass |
| resource lifetime and cleanup | sockets, token waiters, attempt tasks and incoming waiters | listener close, owner abort/finish, terminal and peer removal | Close/cancel/timeout/collision paths release all task-local resources; retained SN state is bounded by deadline. | none | pass |
| state and data integrity | correlation, digest, cache and publication | `RendezvousKey`, response validator, owner map and register/publish calls | SN identity and all attempt ids correlate; cached results cannot overwrite terminal state; only a transport tunnel is published. | none | pass |
| error handling and recovery | typed failures and compatibility fallback | client/SN mappings, timeout branches and legacy call path | Failures remain distinguishable and cleanup happens before one legacy/PN attempt. | none | pass |
| interface boundary and compatibility | public traits and wire consumers | command ids, callback trait, default network method and compile closure | Additive interfaces preserve existing implementors and old SnCall/PN behavior. | none | pass |
| security and capacity safety | public command input and outbound UDP/connect work | cert/owner checks, endpoint policy and all fixed ceilings | Third-party/private/LAN targets fail before action; state and per-request work are bounded. | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-TRP-1 | P-STRP-1 | independent versioned family with exact two-field request and one-field response bodies | codecs, command ids and real QA consumers | pass |
| AR-TRP-2 | P-STRP-2 | four operations map every supported NAT plan to exactly one connector | full plan matrix and branch inspection | pass |
| AR-TRP-3 | P-STRP-3 | only endpoint owner predicts; true returns nonempty owner result and false returns empty | protocol/listener/same-SN evidence | pass |
| AR-TRP-4 | P-STRP-4 | prediction observations and traversal work share the listener socket generation | bound-socket and close DVs | pass |
| AR-TRP-5 | P-STRP-5 | waiter before request, action before response, idempotent duplicates and deterministic cleanup | state, same-SN and collision evidence | pass |
| AR-TRP-6 | P-STRP-6 | authenticated inputs cannot induce unbounded or third-party public actions | security negatives and capacity tests | pass |
| AR-TRP-7 | P-STRP-7 | only real tunnel registration is Connected; new failure cancels before one legacy/PN fallback | TunnelManager source, predicted-connect and fallback tests | pass |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/024-tunnel-rendezvous-protocol/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/024-tunnel-rendezvous-protocol/pipeline/plan.md`
- `docs/versions/v0.1/modules/p2p-frame/024-tunnel-rendezvous-protocol/pipeline/state.json`
- `docs/versions/v0.1/modules/p2p-frame/024-tunnel-rendezvous-protocol/testplan.yaml`
- thirteen admitted production paths and task-local unit/DV/integration tests
- `docs/versions/v0.1/evidence/admission/20260831-tunnel-rendezvous-protocol.md`
- admission stamp and proposal/design/implementation/testing stage-scope evidence
- `test-results/test-runs/20260831T080843Z-p2p-frame+024-tunnel-rendezvous-protocol-all.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Restart from proposal and plan without adopting prior completion claims.
2. Trace protocol, SN/client/inter-SN, socket and TunnelManager sources and construct failure hypotheses for ownership, idempotency, deadline, collision, capacity and compatibility.
3. Route and close implementation/test defects, then regenerate admission and machine-written test evidence after each material source change.
4. Inspect the final tests and artifact and select acceptance only after all categories and change_ids pass.

## Consistency Summary
- Proposal authority check: explicit “确认，自动完成” launches and binds this sibling proposal; no manual approval metadata is required.
- Proposal vs design: the checked plan preserves the independent family, exact bodies, four operations, socket owner, bounded state, relay, security and fallback boundaries.
- Design vs testing implementation: testplan registers unit, DV and integration steps for every change_id and required case type.
- Design vs long-lived boundary doc: changes remain within p2p-frame ownership and preserve public Tunnel/PN/TLS boundaries.
- Design vs implementation: protocol/client/SN/socket/plan/TunnelManager owners match the plan and all acceptance-return defects were corrected without expanding scope.
- Test implementation vs test code vs results: all 10 registered steps resolve and exit 0 in the final current-input artifact.
- Test design adequacy: adequate for deterministic protocol, state, source-port, control-flow and publication/fallback contracts; public carrier NAT success remains environment-dependent.
- change_id traceability: each of seven change_ids maps through proposal, plan, admitted production paths, testplan, runnable artifact and this report.
- Acceptance criteria traceability: exact bodies, four actions, double-symmetric owner prediction, action-before-response, actual socket, lifecycle, real-handshake success boundary, relay, fallback and abuse limits all have source/test evidence.
- Cross-module admission: all changes are within p2p-frame; inter-SN behavior is an internal consumer boundary.
- Public API / codec / runtime semantics review: additions are backward-compatible; existing command ids and SnCall/PN bytes are unchanged.
- Document logic review: proposal, plan, state and testplan are consistent with the executable current Harness schema explicitly confirmed by the user.
- Implementation logic review: an independent subagent reviewer was unavailable under the current no-delegation authorization; the acceptance owner restarted from primary sources and recorded concrete evidence per category.
- Implementation correctness audit completeness and routing: all required categories pass after eight recorded returns; each return reran its owning and dependent stages.
- Document approval timing: auto-pipeline launch evidence and current proposal/plan hashes are bound by the admission stamp.
- Implementation task paths bound to design Scope Paths: current implementation scope passes for all thirteen production paths and seven change_ids.
- Bugfix red-green regression evidence: acceptance-found defects have failing-run or source-counterexample records followed by targeted tests and final full-task green evidence.

## Validation Evidence
- Existing schema result: `schema-check.py --version v0.1 --module p2p-frame --submodule 024-tunnel-rendezvous-protocol` passed on current packet inputs.
- Existing admission stamp: `docs/versions/v0.1/evidence/admission/20260831-tunnel-rendezvous-protocol.p2p-frame.024-tunnel-rendezvous-protocol.stamp.json` binds the current plan and thirteen production paths.
- Existing stage-scope result: implementation passed for 13 paths; testing passed for 16 task paths against its post-implementation baseline.
- Existing pipeline-plan result: current plan/state passed before report creation; complete-state validation runs after acceptance closes.
- Task-relevant test run artifact: `test-results/test-runs/20260831T080843Z-p2p-frame+024-tunnel-rendezvous-protocol-all.json`, 10/10 steps with exit code 0.
- Commands rerun because checker-owned inputs changed: admission/scope, testing coverage/scope and the unified task runner were rerun after idempotency, endpoint, deadline, correlation, attach and waiter-limit fixes.
- Direct package/module runtime suites, whole-project suites and root shortcuts: not run; the artifact contains task-selected tests and p2p-frame all-target x509 compile closure only.
- Risk-triggered task-local contract kinds and assertions: contract mode is disabled by the current backward-compatible API/build declaration; repository compile closure remains an explicit integration step.
- Scoped evidence input hash current: artifact `evidence_input_sha256` is `4c561097b3d22c72c507050cb6183b5b250dcb13f2cd336bdde2476294c5206b`; artifact SHA-256 is `a811ca3f7e168c3a6ff98922b21d621e3e7cef4b8aed95e13326e2757ecb5d10`.
- Quality gates: not applicable; the user did not explicitly request a broad quality run.
- Quality run artifact: none.
- Architecture doc check: not run because no architecture document changed.
- Acceptance report check after this report was created or modified: run during closeout; any failure blocks completion.
- Targeted migration search: command/trait consumers were inspected and the all-target compile step passed; no removed symbol exists.

## Automated Test Exception
- Applies: no
- Reason: a current machine-written task artifact covers every enabled level and all seven change_ids.
- Owner: acceptance
- Risk: no automation waiver is used; public carrier NAT behavior and deployed multi-SN infrastructure remain environment-dependent.
- Acceptance impact: local acceptance proves deterministic code/control/socket behavior but does not claim a real double-symmetric carrier NAT success rate.
- Alternative evidence: direct source falsification and in-process runtime evidence supplement rather than replace the artifact.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: all seven proposal outcomes are implemented within admitted scope, every discovered defect was routed and closed, all required audit categories pass, and the final 10-step task artifact is green.
- Supporting task-relevant test evidence: `test-results/test-runs/20260831T080843Z-p2p-frame+024-tunnel-rendezvous-protocol-all.json`, 10/10 successful steps.
- Residual risk: no real two-public-symmetric-NAT traversal or deployed two-SN owner-directory run was available; cross-SN evidence is dispatcher-level and transport success uses in-process/mock networks, so carrier prediction hit rate and deployment routing remain unproven environment evidence.

## Follow-Up Tasks
- Requirement task: none.
- User decision required for proposal issue: none.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable; deterministic task coverage is complete and only environment evidence remains.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable; no issue repeated more than once.
