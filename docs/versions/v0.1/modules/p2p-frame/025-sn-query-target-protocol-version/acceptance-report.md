# P2P Frame 025 Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-SQPV-001 | none | acceptance | independent review of proposal, plan, state, five production paths, task tests and final 10-step artifact | No blocking requirement, design, implementation, compatibility, security or testing defect was found | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: SN now records the authenticated client's self-reported application protocol baseline and `SnQueryResp` exposes the target version as `Some(version)` or unknown as `None`, including distributed SN queries.
- What was verified: shared producer constant, report identity binding, peer-cache lifecycle, local authority, fail-closed remote agreement, additive query/detail wire compatibility and public consumer migration.
- Evidence used: launch-confirmed proposal, checker-validated pipeline plan/state, current production and test sources, admission/scope evidence and `test-results/test-runs/20260831T101636Z-p2p-frame+025-sn-query-target-protocol-version-all.json`.
- Blocking issues: none.
- Next action: close the auto-pipeline; deployed mixed-version multi-SN behavior remains an environment evidence gap, not a deterministic code gap.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 025-sn-query-target-protocol-version
- change_id values reviewed: sn_protocol_version_registration, sn_query_target_protocol_version
- Review date: 2026-08-31
- In scope: SN application protocol constant and producers, authenticated ReportSn registration, peer cache, SnQueryResp/SnDetailResp extensions, inter-SN detail propagation and local/distributed query aggregation.
- Out of scope: semantic/build versions, persistent version storage, automatic feature negotiation, NAT-probe/rendezvous capability replacement and changes to endpoint/certificate/profile selection.
- Task-relevant acceptance scope: five admitted production paths, task-local testing changes/testplan, pipeline artifacts, admission/scope evidence and the final machine-written task run.
- Out-of-scope checks not run: deployed old/new SN fleet, public network traffic, broad quality gates, unrelated workspace suites and root `all all`.

## Optional Diff / Status Evidence
- `git status --short` summary: the shared worktree contains prior task changes; task stage manifests define this review boundary.
- `git diff --stat` summary: used only to locate task paths in a dirty worktree.
- `git diff --name-status` summary: the five implementation paths and task-local test/document paths match their manifests.
- `git diff --check` result: task production and testing paths pass.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| P-SQPV-1 / sn_protocol_version_registration | proposal registration requirement and plan state/trust-boundary mapping | `SN_PROTOCOL_VERSION`, three client producers, authenticated-id validation and `CachedPeerInfo.protocol_version` lifecycle | registration unit, report/query DV, external consumer and compile closure | implemented |
| P-SQPV-2 / sn_query_target_protocol_version | proposal query semantics and plan wire/distributed flow | SQPV/SDPV tails, response/detail fields, inter-SN mapping, local authority and all-lease remote aggregation | wire/malformed unit, five service DVs, inter-SN integration, external consumer and compile closure | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| authenticated registration / sn_protocol_version_registration | normal / boundary / negative / error / compatibility / lifecycle / cross-module | unknown, known-zero, replacement, non-report, removal, identity mismatch and report-to-query cases | unit, DV, external API and compile steps exit 0 | adequate |
| target query / sn_query_target_protocol_version | normal / boundary / negative / error / compatibility / lifecycle / cross-module | None/0/1 wire, malformed tail, local precedence, remote agreement/missing/error/old/conflict/local lease, inter-SN and detail preservation | unit, DV, integration and contract steps exit 0 | adequate |

## Defect-Discovery Categories
| Category | Status | Concrete Evidence and Falsification Result |
|----------|--------|--------------------------------------------|
| requirement-and-behavior | pass | Both proposal items were traced to current producers, report handling and final query response; feature-specific versions remain independent. |
| logic-and-control-flow | pass | Local cache is authoritative; remote results require every directory lease to succeed with the same known byte while detail aggregation remains best effort. |
| boundary-and-input | pass | `None`, `Some(0)`, `Some(1)`, absent tails, malformed tails, missing details, local leases and conflicting versions are exercised. |
| state-and-data-integrity | pass | Protocol version lives in the existing peer entry, replaces atomically with a valid report and disappears with peer removal; no side cache exists. |
| error-handling-and-recovery | pass | Remote transport/NotFound/old response failures yield unknown without converting SnQuery into an error or discarding successful detail data. |
| resource-lifetime-and-cleanup | pass | No new task, socket, timer or persistent allocation was introduced; peer removal owns version cleanup. |
| concurrency-and-ordering | pass | Registration fields update under the existing peer-map mutex, and remote consensus is monotonic fail-closed and independent of response order. |
| interface-and-compatibility | pass | SQPV/SDPV are independent append-only extensions after SQRP/SDRP; old readers ignore the tail, new readers decode old messages as unknown, and repository literals compile. |
| security-and-capacity | pass | Certificate/tunnel validation and optional claimed-id equality precede report-owned mutation; the authenticated peer id is always the cache and scheduler key; no unbounded collection was added. |
| test-adequacy | pass | Final artifact contains 10 successful task-scoped contract/unit/DV/integration steps, including all-target compile closure and current evidence-input binding. |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | report and query paths | `handle_report_sn`, `handle_query_sn`, `query_remote_details` and focused DVs | Authentication, local precedence and fail-closed all-participant agreement follow the planned branches. | none | pass |
| termination and progress | directory and inter-SN query loop | bounded serving-lease iteration and existing async calls | No new retry or wait loop exists; every lease is processed once. | none | pass |
| concurrency and synchronization | peer state and remote aggregation | peer-map mutex updates and function-local accumulator | Version replacement is atomic with peer update; aggregation state cannot recover after a failure/conflict. | none | pass |
| resource lifetime and cleanup | cached version and wire values | `remove_peer`, non-report construction and response-local fields | Version lifetime equals the existing peer entry and adds no independent resource. | none | pass |
| state and data integrity | authenticated identity and known/unknown values | validator ordering, cache key, `Option` fields and lifecycle tests | Claimed identity cannot redirect mutation; zero is preserved as known and unknown is never synthesized as zero. | none | pass |
| error handling and recovery | malformed wire and partial distributed failure | extension decoder, inter-SN mappings and query failure cases | Existing core/profile/detail data survives version absence or failure while target version fails closed. | none | pass |
| interface boundary and compatibility | public Rust structs and query/detail wire | exported interfaces, consumer closure, external test and repository compile | Source migration is explicit and both wire directions retain legacy compatibility. | none | pass |
| security and capacity safety | ReportSn trust boundary and serving leases | certificate/tunnel/claimed-id checks and existing deduplicated directory leases | Only authenticated self-report changes state; protocol bytes are opaque and do not authorize features or allocate new capacity. | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-SQPV-1 | P-SQPV-1 | all three SN client messages use one current baseline and only an authenticated report creates a known cached target version | producer scan, registration tests, identity-negative DV and removal test | pass |
| AR-SQPV-2 | P-SQPV-2 | query/detail responses distinguish known zero from unknown without changing legacy core/profile behavior | new/old/malformed codec tests and external public consumer | pass |
| AR-SQPV-3 | proposal distributed semantics | local registration wins; remote-only returns known only when every serving lease supplies one identical known value | consensus/local/detail-preservation DVs and inter-SN mapping test | pass |
| AR-SQPV-4 | plan migration closure | every repository public struct literal and update caller uses the new interface | consumer-closure checker, external-positive compile and all-target compile | pass |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/025-sn-query-target-protocol-version/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/025-sn-query-target-protocol-version/pipeline/plan.md`
- `docs/versions/v0.1/modules/p2p-frame/025-sn-query-target-protocol-version/pipeline/state.json`
- `docs/versions/v0.1/modules/p2p-frame/025-sn-query-target-protocol-version/testplan.yaml`
- five admitted production paths and task-local test sources
- `docs/versions/v0.1/evidence/admission/20260831-sn-query-target-protocol-version.md` and its stamp
- task stage-scope manifests and baseline manifest
- `test-results/test-runs/20260831T101636Z-p2p-frame+025-sn-query-target-protocol-version-all.json`
- `harness/rules/acceptance-review-rules.md` and `harness/rules/acceptance-task-rules.md`

## Review Order
1. The independent reviewer restarted from proposal and plan and did not treat pipeline metadata as implementation proof.
2. It traced the five production paths and attempted to falsify identity, known/unknown, local authority, remote agreement and compatibility invariants.
3. It inspected task tests, failed and successful run artifacts, admission and scope evidence before reporting no blocking finding.

## Consistency Summary
- Proposal authority check: explicit “确认，自动完成” launched and bound this sibling proposal; manual approval metadata is not required.
- Proposal vs design: the checked plan preserves both requirements, five production paths, optional wire compatibility and local/distributed precedence.
- Design vs testing implementation: testplan and state cover every change_id and all seven required case types.
- Design vs long-lived boundary doc: all behavior remains within p2p-frame SN ownership; no crate boundary or feature capability contract changed.
- Design vs implementation: constant, cache owner, authentication, wire and aggregation match the plan.
- Test implementation vs test code vs results: all 10 registered steps resolve and exit 0 in the final current-input artifact.
- Test design adequacy: adequate for deterministic wire, cache, authentication and distributed aggregation behavior.
- change_id traceability: both change_ids map through proposal, plan, admitted paths, testplan, runnable evidence and this report.
- Acceptance criteria traceability: known-zero/unknown, authentication, lifecycle, local authority, remote missing/error/conflict and compatibility all have direct source and runnable evidence.
- Cross-module admission: all production paths are within p2p-frame; inter-SN is an internal boundary.
- Public API / codec / runtime semantics review: public struct migration is explicit; wire additions are backward-compatible and do not alter endpoint/certificate/profile runtime selection.
- Document logic review: proposal, plan, state and testplan are mutually consistent under the launched auto-pipeline policy.
- Implementation logic review: an independent acceptance child reviewed primary sources and reported no blocking finding before this report was assembled.
- Implementation correctness audit completeness and routing: all required categories pass; no upstream return is required.
- Document approval timing: auto-pipeline launch and current document hashes are bound by the admission stamp.
- Implementation task paths bound to design Scope Paths: implementation scope passed for all five production paths and both change_ids.
- Bugfix red-green regression evidence: the identity mutation-order defect has a direct negative test; test-entry configuration failures are retained in two failed artifacts followed by the final green artifact.

## Validation Evidence
- Existing schema result: `schema-check.py --version v0.1 --module p2p-frame --submodule 025-sn-query-target-protocol-version` passed before implementation.
- Existing admission stamp: `docs/versions/v0.1/evidence/admission/20260831-sn-query-target-protocol-version.p2p-frame.025-sn-query-target-protocol-version.stamp.json` binds current proposal/plan and five production paths.
- Existing stage-scope result: proposal and design passed; implementation passed for 5 paths; testing passed for 15 paths against `.harness/baselines/025-sn-query-target-protocol-version-testing/manifest.json`.
- Existing pipeline-plan result, when applicable: current plan/state passed before report creation; complete-state validation follows acceptance closeout.
- Task-relevant test run artifact: `test-results/test-runs/20260831T101636Z-p2p-frame+025-sn-query-target-protocol-version-all.json`, 10/10 steps with exit code 0.
- Commands rerun because checker-owned inputs changed: unified runner was rerun after removing plan prose from removed-symbol evidence inputs and after adding required `x509` feature to task unit commands; final coverage and testing scope passed.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; the artifact contains task-selected tests and p2p-frame all-target x509 compile closure only.
- Risk-triggered task-local contract kinds and assertions, when applicable: external-positive/new-path-compiles, removed-symbol-scan/no-unallowlisted-old-symbol-references and repository-compile-closure/repository-consumers-compile all exit 0.
- Scoped evidence input hash current, when risk-triggered: artifact `evidence_input_sha256` is `e486032513e461da98f970bb257b171d00dc21193300289ae1886f195fde9942`; artifact SHA-256 is `f4d764e3eedbb56314174ba81dd1f72a0c6621416a737345453b8b82f27044c3`.
- Quality gates: not required; broad quality execution is out of scope for this task closeout.
- Explicitly requested quality run artifact, if any: none was requested.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: not run because no architecture document changed.
- Acceptance report check after this report was created or modified: run during closeout; failure blocks completion.
- Targeted migration search, only when applicable to the reviewed task: consumer-closure checker and all-target compile passed with every mapped repository caller migrated.

## Automated Test Exception
- Applies: no
- Reason: a current machine-written task artifact covers every enabled level and both change_ids.
- Owner: acceptance
- Risk: no automation waiver is used; a deployed mixed-version multi-SN fleet was not available.
- Acceptance impact: deterministic source and in-process evidence supports acceptance without claiming deployment validation.
- Alternative evidence: direct source falsification supplements rather than replaces the artifact.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: both proposal outcomes are implemented within admitted scope, every correctness category passes and the final 10-step task artifact is green on current inputs.
- Supporting task-relevant test evidence: `test-results/test-runs/20260831T101636Z-p2p-frame+025-sn-query-target-protocol-version-all.json`, 10/10 successful steps.
- Residual risk: no live old/new deployed multi-SN fleet was exercised; cross-SN evidence uses the real mapper/dispatcher with deterministic in-process peers, so deployment routing remains unproven environment evidence.

## Follow-Up Tasks
- Requirement task: none.
- User decision required for proposal issue: none.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable; task coverage is complete.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable; no acceptance issue was returned.
