# P2P Frame 026 Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-SSRP-FINAL | none | acceptance | independent review of proposal, revised plan, ten production paths, current tests, return fixes and final 12-step artifact | No blocking requirement, design, implementation, compatibility, security, lifecycle or testing defect remains | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: the new SN rendezvous family is reduced to flat request, notify and response messages containing only the fields required for routing, authentication, action and immediate prediction consumption.
- What was verified: exact wire fields and ids, strict version and trailing-byte rejection, authenticated identity derivation, same-SN and two-SN delivery, non-cacheable predicted endpoints, bounded state, local timeout cleanup, unique-token owner lifecycle, direction-aware collision handoff, registered-tunnel success and serial legacy/PN fallback.
- Evidence used: current proposal/plan/state/testplan, admitted production files, task test sources, stage manifests, acceptance return analysis and `test-results/test-runs/20260831T154432Z-p2p-frame+026-simplify-sn-rendezvous-protocol-all.json`.
- Blocking issues: none; F-SSRP-001 through F-SSRP-006 are closed by the current design, source and final artifact.
- Next action: close the auto-pipeline; public double-symmetric-NAT and deployed owner-directory routing remain environment evidence gaps.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 026-simplify-sn-rendezvous-protocol
- change_id values reviewed: simplify_sn_rendezvous_wire_contract
- Review date: 2026-08-31
- In scope: simplified SN rendezvous codecs, client/SN/inter-SN routing, state, prediction ownership, TunnelManager lifecycle and compatibility fallback.
- Out of scope: NAT classification/math, TLS design, PN wire changes, public Tunnel APIs, unrelated NAT probe or SN query work.
- Task-relevant acceptance scope: ten admitted production paths, task-local testing paths, pipeline artifacts, admission/scope evidence and final machine-written run.
- Out-of-scope checks not run: public multi-NAT deployment, deployed owner-directory fleet, broad quality gate, unrelated workspace runtime suites and root `all all`.

## Optional Diff / Status Evidence
- The shared worktree contains earlier task changes; task stage manifests, not the whole dirty tree, define this review boundary.
- Task production and testing paths pass `git diff --check`.
- Diff/status output was used only for path discovery and was not treated as acceptance proof.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| P-SSRP-1 / simplify_sn_rendezvous_wire_contract | proposal exact-field tables and revised plan mappings | flat request/notify/response, compact result, isolated ids/version, authenticated context, bounded state and token-bound owners | codec, removed API, state, same-SN, inter-SN, two-SN, lifecycle, fallback and compile steps | implemented |
| request-scoped prediction | proposal non-cacheable rule and plan State Ownership | live waiters receive current response; completed prediction requests retain only generic failure with empty endpoints | live-only state case plus current/expired/closed listener generation cases | implemented |
| one collision winner | proposal cleanup criteria and plan failure flows | stable peer ordering, completion handoff, unique local owner token on attach/complete/cancel | same-tuple replacement, stale guard/complete/attach, waiter cleanup and registered tunnel assertions | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| flat wire / simplify_sn_rendezvous_wire_contract | normal / boundary / negative / error / compatibility | exact fields, invalid enum/endpoints/results, trailing bytes, mixed version and removed symbols | unit and contract steps exit 0 | adequate |
| identity and routing / simplify_sn_rendezvous_wire_contract | security / cross-module / normal / negative | verified caller cert, third-party endpoint rejection, same-SN and two-SN target delivery | DV and integration steps exit 0 | adequate |
| lifecycle / simplify_sn_rendezvous_wire_contract | lifecycle / concurrency / error / compatibility | prediction live-only, future drop, local timeout, same-tuple handoff, stale token operations, real tunnel registration and fallback | unit, DV and integration steps exit 0 | adequate |

## Defect-Discovery Categories
| Category | Status | Concrete Evidence and Falsification Result |
|----------|--------|--------------------------------------------|
| requirement-and-behavior | pass | Wire structs match the proposal field tables and forbidden envelope/body/digest/deadline/terminal concepts are absent. |
| logic-and-control-flow | pass | Authenticated routing, response ordering and serial fallback follow the revised plan. |
| boundary-and-input | pass | Invalid ids, operation values, endpoint domains, response shapes, versions, lengths and trailing bytes fail closed. |
| state-and-data-integrity | pass | Duplicate keys include authenticated initiator plus seq/tunnel; conflicts reject; prediction vectors never enter completed state. |
| error-handling-and-recovery | pass | Wire failures are generic, local causes remain local, and legacy/PN fallback remains available. |
| resource-lifetime-and-cleanup | pass | RAII cancellation and local timeouts remove owner/waiter work without terminal wire commands. |
| concurrency-and-ordering | pass | Direction-aware peer ordering selects one winner; unique owner tokens reject stale attach, complete and cancel operations. |
| interface-and-compatibility | pass | New command ids/version are isolated; old layouts are rejected; legacy SnCall remains the rolling-upgrade path. |
| security-and-capacity | pass | Identities come from authenticated tunnels/verified certificates; endpoint ownership and bounded state limits remain enforced. |
| test-adequacy | pass | The final current-input artifact has 12 successful contract/unit/DV/integration steps and all-target x509 compile closure. |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | client, SN, inter-SN and TunnelManager | request/notify/response handlers and fallback paths | The target action completes before success response, and collision losers wait for the winner before fallback. | none | pass |
| termination and progress | state and local actions | 10-second SN bounds, connection timeout and expiry | No protocol terminal is required for bounded cleanup. | none | pass |
| concurrency and synchronization | duplicate and collision ownership | RendezvousState mutex, owner map, Arc token and completion channels | Stale same-tuple generations cannot attach, complete or cancel replacement owners. | none | pass |
| resource lifetime and cleanup | owners, tasks and waiters | registration Drop, abort, yielded cleanup and completion | Caller drop, timeout, displacement and tunnel success converge without detached work. | none | pass |
| state and data integrity | prediction and correlation | cache_response, request equality and response validation | Live prediction is delivered only to current waiters; completed replay is generic and empty. | none | pass |
| error handling and recovery | codec, transport and action failures | validation, generic response and fallback tests | All failure shapes remain deterministic and bounded. | none | pass |
| interface boundary and compatibility | public/internal SN protocol | command ids/version, strict decoders and consumer closure | Coordinated new components cannot decode old layouts as new; legacy fallback is unchanged. | none | pass |
| security and capacity safety | authenticated command hops and state ceilings | certificate derivation, ownership validation, pair/rate/total/waiter limits | Payload identity cannot redirect routing and no unbounded state is introduced. | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-SSRP-1 | proposal field tables | exactly three flat message shapes with only listed semantic fields | source inventory, codec round trips and removed-symbol scan | pass |
| AR-SSRP-2 | proposal trust boundary | caller/target identities derive from authenticated context and verified certs | same-SN, two-SN and abuse-negative evidence | pass |
| AR-SSRP-3 | proposal prediction rule | predicted endpoints are current, immediate and non-cacheable | listener generation and live-only replay tests | pass |
| AR-SSRP-4 | proposal lifecycle rule | timeout/drop/collision/tunnel success leave one owner and no stale task/waiter | RAII, token, stale attach/complete/drop and real registration tests | pass |
| AR-SSRP-5 | compatibility boundary | mixed versions fail closed into unchanged legacy/PN behavior | version negatives, removed API, compile closure and fallback test | pass |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/026-simplify-sn-rendezvous-protocol/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/026-simplify-sn-rendezvous-protocol/pipeline/plan.md`
- `docs/versions/v0.1/modules/p2p-frame/026-simplify-sn-rendezvous-protocol/pipeline/state.json`
- `docs/versions/v0.1/modules/p2p-frame/026-simplify-sn-rendezvous-protocol/testplan.yaml`
- ten admitted production paths and task-local test sources
- admission stamp and task stage-scope manifests
- `test-results/test-runs/20260831T154432Z-p2p-frame+026-simplify-sn-rendezvous-protocol-all.json`
- acceptance-review and acceptance-task rules

## Review Order
1. The independent reviewer restarted from proposal and revised plan rather than trusting prior acceptance metadata.
2. It traced production flows and returned prediction caching, cancellation, collision, stale owner and attach defects to their owning stages.
3. It re-reviewed each repair, current tests and final artifact; the parent only assembled the already-returned independent conclusions after the review report-writing turn stalled.

## Consistency Summary
- Proposal authority check: explicit `确认，自动完成` launched this auto-pipeline; manual approval metadata is not required.
- Proposal vs design: the revised plan preserves exact fields, non-cacheable prediction, unique owner lifecycle and legacy compatibility.
- Design vs implementation: all ten admitted paths implement their mapped responsibilities.
- Design vs testing: state coverage records all seven required case types and six design-element categories.
- Test implementation vs results: all 12 registered steps resolve and exit 0 in the final artifact.
- Test design adequacy: adequate for exact wire, identity abuse, prediction freshness, cleanup, collision, same/cross-SN and compatibility behavior.
- change_id traceability: the single change_id maps through proposal, plan, admission, testplan, state, artifacts and this report.
- Acceptance criteria traceability: exact fields, authenticated identity, prediction true/false, cleanup, fallback and task-local evidence all have direct source and runnable proof.
- Cross-module admission: all production changes remain within p2p-frame and are bound to the single admitted change_id.
- Public API/codec/runtime review: obsolete new APIs are removed, strict codecs are isolated by version and real success remains registered tunnel establishment.
- Document logic review: proposal, revised plan, state and testplan are mutually consistent.
- Implementation logic review: an independent acceptance reviewer traced current primary sources, returned six defects to owning stages and confirmed the final token/attach fixes before this report was assembled.
- Implementation correctness audit completeness and routing: all required categories pass after three acceptance iterations and six routed findings; no upstream return remains.
- Document approval timing: auto-pipeline launch and the revised plan hash are bound by the current admission stamp.
- Implementation task paths bound to design Scope Paths: implementation scope passed for all ten production paths and the single change_id.
- Bugfix red-green regression evidence: failed artifacts retain assertion/lifecycle discoveries and are followed by the current green artifact with prediction and stale-owner regressions.

## Validation Evidence
- Existing schema result: `schema-check.py --version v0.1 --module p2p-frame --submodule 026-simplify-sn-rendezvous-protocol` passed before implementation resumed.
- Existing admission stamp: the task stamp binds the revised plan and ten production paths for `simplify_sn_rendezvous_wire_contract`.
- Existing stage-scope result: implementation passed for 10 task paths; testing passed for 18 paths.
- Existing pipeline-plan result, when applicable: the current plan/state passed before acceptance closeout; complete-state validation follows final state update.
- Task-relevant test run artifact: `test-results/test-runs/20260831T154432Z-p2p-frame+026-simplify-sn-rendezvous-protocol-all.json`, 12/12 steps exit 0.
- Commands rerun because checker-owned inputs changed: the unified runner was rerun after prediction, cancellation, collision, token and stale-attach fixes; coverage and testing scope passed on the final inputs.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; the artifact contains task-selected tests and p2p-frame all-target x509 compile closure only.
- Risk-triggered task-local contract kinds and assertions, when applicable: external-positive, external-negative, removed-symbol-scan and repository-compile-closure steps all exit 0.
- Scoped evidence input hash current, when risk-triggered: the final artifact records the current testplan and all listed source/test evidence inputs.
- Quality gates: not required; broad quality execution is outside this task closeout.
- Explicitly requested quality run artifact, if any: none was requested.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: not run because no architecture document changed.
- Acceptance report check after this report was created or modified: run during closeout; failure blocks completion.
- Targeted migration search, only when applicable to the reviewed task: consumer-closure checker and all-target compile passed with every mapped repository caller migrated.

## Automated Test Exception
- Applies: no
- Reason: a current machine-written artifact covers every enabled level and the single change_id.
- Owner: acceptance
- Risk: public double-symmetric-NAT and deployed owner-directory paths were not available.
- Acceptance impact: deterministic source and in-process evidence supports acceptance without claiming deployed-environment validation.
- Alternative evidence: direct independent source falsification supplements, rather than replaces, the artifact.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: the exact minimum wire contract and revised local lifecycle are implemented within admitted scope; all six returned findings are closed and the final 12-step current-input artifact is green.
- Supporting task-relevant test evidence: `test-results/test-runs/20260831T154432Z-p2p-frame+026-simplify-sn-rendezvous-protocol-all.json`, 12/12 successful steps.
- Residual risk: no public double-symmetric-NAT deployment or deployed owner-directory fleet was exercised; the in-process two-SN test uses two real services and the real target command transport, so external routing remains unproven environment evidence only.

## Follow-Up Tasks
- Requirement task: none.
- User decision required for proposal issue: none.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable; task coverage is complete.
- Iteration count: 3
- Stop reason if more than 5 unsuccessful iterations: not applicable; acceptance completed in three iterations.
