# TCP Reverse Data First-Claim Handoff Acceptance Report

## Findings

| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | current proposal, plan, docs, code, tests, scope records, and latest 17-step artifact | no blocking finding recorded | |

## Prior Finding Closure

| Prior ID | Status | Closure Evidence |
|----------|--------|------------------|
| F-TRDFCH-001 | closed | The composed test constructs a real signed-x509 B TCP control tunnel, closes B's direct listener before the successful attempt, exercises PN/TTP target selection plus reverse TCP, returns `ProxyOpenResp::Success`, and transfers bytes both ways. |
| F-TRDFCH-002 | closed | Actual TCP/TLS paths drive `PendingOpenRequestGuard`, the pending map, staged entry, and registered data entry: caller-abort, tunnel-close, and new-requester/old-creator snapshots converge from 1/1/1 to 0/0/0. Old-requester/new-creator uses the production control decoder with maximum command 11 sampled only after the actual frame header arrives; both mixed-version directions fail closed on real tunnels. |
| F-TRDFCH-003 | closed | RAII ownership, remove-before-retire cancellation, tunnel-close cleanup, late-response rejection, and the success tombstone close the cancellation and staged-entry leak. |
| F-TRDFCH-004 | closed | The module doc, authoritative TCP protocol doc, plan, and code consistently specify command 12, requester first claim, late rejection, cleanup, and coordinated rollout. |
| F-TRDFCH-005 | closed | The canonical testing manifest contains all seven task-owned testing paths. The original machine baseline covers the test-only `network.rs` production-path change, the owning testing stage-scope result passed, and `pn_server.rs` has no task diff after test registration moved under the existing traffic-manager test module. |

## Result Summary

- Overall result: accepted
- Plain-language outcome: The documented repair, production state machine, real lifecycle and compatibility paths, composed PN/TTP topology, current test artifact, and complete scoped evidence agree; no blocking correctness or evidence defect remains.
- What was verified: launch-confirmed proposal; plan and state; authoritative module and protocol docs; admission and all stage scopes; protocol and tunnel implementation; test registrations and dedicated tests; TTP and PN consumers; testplan; latest artifact; and closure of F-TRDFCH-001 through F-TRDFCH-005.
- Evidence used: current task primary sources and existing owning-stage machine evidence.
- Blocking issues: none.
- Next action: none; the accepted task is ready for handoff with coordinated deployment noted as residual operational risk.

## Object and Scope

- Module: p2p-frame
- Version: v0.1
- Task name: 012-tcp-reverse-data-first-claim
- change_id values reviewed: tcp_reverse_data_first_claim_handoff
- Review date: 2026-08-26
- In scope: P-TRDFCH-1; module and TCP protocol docs; task proposal, plan, and state; admission and stage-scope evidence; protocol and tunnel runtime; TCP test wiring and dedicated tests; TTP server and tests; PN server behavior, existing test wiring, and dedicated composed test; testplan; latest artifact.
- Out of scope: unrelated dirty-worktree changes, root and broad suites, quality gates, and unrelated architectures or transports.
- Task-relevant acceptance scope: the sole change_id `tcp_reverse_data_first_claim_handoff` and its p2p-frame reverse-data handoff behavior and evidence chain.
- Out-of-scope checks not run: unchanged owning-stage schema, admission, pipeline, scope, and testing commands; quality gates; broad package, workspace, and root commands.

## Optional Diff / Status Evidence

- `git status --short` summary: used only to locate task-owned documentation, implementation, test registration, dedicated test, and evidence paths while excluding unrelated worktree changes.
- `git diff --stat` summary: discovery only; not used as an acceptance criterion.
- `git diff --name-status` summary: confirmed that `pn_server.rs` no longer has a task diff and that the remaining testing paths match the seven-path canonical manifest.
- `git diff --check` result: passing result reused from the owning implementation and testing stages.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage

| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| race-free registration barrier and requester first claim | proposal, plan, and TCP protocol design | command 12 correlation and exact entry claim | state unit tests, reverse DV, and composed integration | implemented |
| timeout, cancellation, late response, and close cleanup | plan lifecycle mapping | guard, mutex-owned pending and staged retirement, and success tombstone | abort, close, and old-creator real lifecycle snapshots | implemented |
| actual error fidelity without synthetic terminal Conflict | proposal and plan | bounded response mapping and conflict-only retry loop | wire error unit plus listener and connect-failure DV | implemented |
| safe rolling upgrade | plan, module doc, and protocol design | bounded decoder, explicit command-12 rejection, and response timeout | both real-tunnel mixed-version directions | implemented |
| PN/TTP consumer recovery without transport bypass | proposal boundary and plan topology | unchanged consumer use of `Tunnel::open_stream` | composed signed A-to-PN/TTP-to-B reverse stream | implemented |
| current testing scope | Harness stage-scope rules | existing test-module wiring in network and traffic-manager files | seven-path manifest, passing owning scope result, and latest artifact | implemented |

## Test Design Adequacy

| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| response, correlation, ownership, and error mapping | normal, boundary, negative, error | four unit registrations invoke production seams and codec | 4 unit steps pass | adequate |
| direct failure to reverse success, reuse, and terminal errors | normal, error, lifecycle | real TCP/TLS fixture | 3 core DV steps pass | adequate |
| abort, close, and timeout ownership | lifecycle, concurrency, capacity | post-registration control gate and production snapshot | 3 lifecycle DV steps pass and state returns to zero | adequate |
| rolling upgrade | compatibility, negative | real tunnels with bounded legacy policies | 2 mixed-version DV steps pass | adequate |
| TTP and PN consumer composition | cross-module | TTP consumer tests and one forced-reverse signed topology | 3 integration steps pass | adequate |
| evidence governance | scope, currentness | current testplan, manifest, baseline, and artifact | seven-path testing scope passes and the 17-step artifact is current | adequate |

## Implementation Correctness Audit

| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | command-12 correlation, exact staged consumption, and lease-1 ownership | plan, source, unit tests, and DV | both event orders complete once; mismatch, duplicate, and terminal input fail closed; only genuine Conflict retries | none | pass |
| termination and progress | connection, response, and claim waits plus retry loop | source and DV | waits and retries are bounded; no spin, livelock, or starvation was found | none | pass |
| concurrency and synchronization | arrival-response reorder and success-cancel or close races | mutex transitions, guard, tombstone, and gated DV | map and entry ownership changes atomically; late delivery cannot resurrect retired state | none | pass |
| resource lifetime and cleanup | waiter, staged and registered entries, sockets, and tasks | close, cancellation, timeout source paths and snapshots | success consumes the exact entry; failure, cancellation, timeout, and close retire it without accumulating task-owned state | none | pass |
| state and data integrity | request, connection, and Arc correlation plus first-claim lease state | correlation code, unit tests, and DV | identity and owner invariants hold; a completed request is consumed once | none | pass |
| error handling and recovery | direct and reverse setup, protocol, listener, and claim failures | mapping, open loop, unit tests, and DV | concrete error codes survive; `claim retries exhausted` remains only for real bounded conflicts | none | pass |
| interface boundary and compatibility | internal command 12 and wire migration | plan, docs, bounded decoder, contract checks, and DV | no public API or build-surface change; unsupported peers reject or time out before business bytes | none | pass |
| security and capacity safety | TLS identity, listener checks, command bounds, and pending state | source, contracts, DV, and composed test | identity and PN validation remain unchanged; header and command bounds are enforced; lifecycle cleanup prevents unbounded request state | none | pass |

## Generated Acceptance Rules

| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-TRDFCH-1 | P-TRDFCH-1 | reverse fallback completes one valid first claim or returns the actual setup, timeout, or protocol error | source, unit tests, and DV | pass |
| AR-TRDFCH-2 | registration-barrier design | creator registration happens before requester claim through matching request and connection response | source, state tests, and DV | pass |
| AR-TRDFCH-3 | lifecycle boundary | timeout, cancellation, late response, and close leave no request-owned waiter, entry, or socket | owner-path snapshots | pass |
| AR-TRDFCH-4 | error-fidelity requirement | real setup, protocol, and claim categories remain distinguishable | unit tests and DV | pass |
| AR-TRDFCH-5 | compatibility plan | mixed versions fail before business-payload ambiguity | real bounded peer paths | pass |
| AR-TRDFCH-6 | system success criterion | one forced-reverse A-to-PN/TTP-to-B stream succeeds and transfers bytes | composed integration | pass |
| AR-TRDFCH-7 | evidence-chain rules | task tests and evidence are unified-entry reachable, current, and completely scoped | manifest, baseline, scope result, and artifact | pass |

## Inputs

- launch-confirmed `proposal.md`
- current `pipeline/plan.md` and completed `pipeline/state.json`
- admission evidence and stamp plus proposal, design, implementation, testing, and acceptance stage-scope records
- `docs/modules/p2p-frame.md` and `p2p-frame/docs/tcp_tunnel_protocol_design.md`
- `protocol.rs`, `tunnel.rs`, `network.rs`, TCP dedicated tests, TTP server and tests, PN server behavior, existing PN test registration, and PN composed test
- current `testplan.yaml`
- `test-results/test-runs/20260826T043936Z-p2p-frame+012-tcp-reverse-data-first-claim-all.json`
- `harness/rules/acceptance-review-rules.md` and test-design rules

## Review Order

1. Verified proposal and auto-pipeline authority, task binding, and change_id scope.
2. Compared the pipeline design mapping with authoritative module and TCP protocol documentation.
3. Reviewed admission and stage-scope evidence before using implementation or test artifacts.
4. Audited the production protocol and tunnel logic across all eight correctness categories.
5. Reviewed TCP, TTP, and PN test design and executable evidence, including lifecycle, mixed-version, and composed paths.
6. Verified artifact freshness, executed step content, and complete testing scope.
7. Rechecked prior findings and recorded the findings-first conclusion.

## Consistency Summary

- Proposal authority check: valid through the recorded auto-pipeline launch statement `确认，自动完成`; draft approval metadata is permitted by that mode.
- Proposal vs design: consistent; the plan maps the approved outcome, ownership, lifecycle, compatibility, and PN composition without narrowing or expansion.
- Design vs testing implementation: consistent; the promised seams and topologies are exercised by registered task tests.
- Design vs long-lived boundary doc: consistent after synchronizing the internal wire response, requester first claim, cleanup, and rollout contract.
- Design vs implementation: consistent; source follows the mapped protocol, state ownership, cleanup, and error behavior.
- Test implementation vs test code vs results: consistent; registered commands map to their source cases and all 17 artifact steps executed successfully.
- Test design adequacy: adequate for relevant normal, boundary, negative, error, compatibility, lifecycle, concurrency, capacity, and cross-module cases.
- change_id traceability: complete across proposal, plan, admission, code, testplan, state, and artifact.
- Acceptance criteria traceability: complete through unit, DV, integration, contract, scope, and documentation evidence.
- Cross-module admission: one p2p-frame packet is correct because TTP and PN are same-crate consumers, and the composed behavior has explicit evidence.
- Public API / codec / runtime semantics review: no public API or build-surface change; the internal command-12 migration is explicitly designed and tested in both mixed-version directions.
- Document logic review: no contradiction, impossible state, or unsupported narrowing was found.
- Implementation logic review: no blocking correctness defect was found.
- Implementation correctness audit completeness and routing: all eight required categories were reviewed and pass; no return is required.
- Document approval timing (approved_content_sha256 verified by schema-check): current auto-pipeline launch, schema, admission, and plan evidence are reused from their unchanged owning stages.
- Implementation task paths bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ... --changed-paths-file ...`): existing passing implementation scope covers the authoritative protocol doc, `protocol.rs`, and `tunnel.rs`.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: the supplied pre-fix PN log and exact source trace establish the red defect; automated pre-fix replay was infeasible without reverting shared production code and topology, while green tests deterministically force the same reverse branch and assert success and error fidelity.

## Validation Evidence

- Existing schema result (cite the owning-stage result; do not rerun unchanged input): current passing v0.1 p2p-frame schema evidence reused from the owning stage.
- Existing admission stamp (cite the stamp; do not run `--verify-only` during acceptance unless an admission-owned input changed): the current 20260826 TCP reverse-data admission stamp binds proposal hash `0a4557...`, plan hash `caeebb...`, target module, change_id, and admitted paths; reused without replay.
- Existing stage-scope result (cite the owning-stage result; do not rerun an unchanged manifest/scope): the proposal result was refreshed after removing the completed task from the unfinished index; design and implementation results remain current; the testing manifest has seven paths and its owning result passed with `.harness/baselines/012-tcp-reverse-data-first-claim/manifest.json`.
- Existing pipeline-plan result, when applicable (cite the latest result for current plan/state inputs): completion-required validation passes for plan hash `caeebb5375633c409ff2230aec5febe0aa17f320e4c94fce590abe1d4850118c` and the completed state.
- Task-relevant test run artifact(s) (reuse when implementation/tests/testplan/registration are unchanged): `test-results/test-runs/20260826T043936Z-p2p-frame+012-tcp-reverse-data-first-claim-all.json`; exact task and `all` level; 17 steps comprising 3 contract, 4 unit, 7 DV, and 3 integration steps; every step has non-empty sources and exit 0; overall exit 0.
- Commands rerun because checker-owned inputs changed after their previous pass (or `none` with evidence): proposal scope was refreshed after unfinished-index cleanup; the acceptance report checker, acceptance scope checker, and completion-required pipeline-plan checker were run for their changed closeout inputs. No implementation or task tests were rerun.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; risk-triggered compile-only consumer closure appears only inside the task artifact.
- Risk-triggered task-local contract kinds and assertions, when applicable: external-positive/new-path-compiles, removed-symbol-scan/no-unallowlisted-old-symbol-references, and repository-compile-closure/repository-consumers-compile all pass in the artifact.
- Scoped evidence input hash current, when risk-triggered: `7ec32b9d5d8ae06b6155194a5c106ba9f54d2b7c6719a630c3a96aeca96666da`.
- Quality gates: not applicable; quality gates were not run because the user did not request them.
- Explicitly requested quality run artifact, if any: none; no quality run was requested.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: not rerun; no workspace-level architecture boundary changed, and the relevant long-lived module and protocol docs were reviewed directly.
- Acceptance report check after this report was created or modified: passed for this accepted report.
- Targeted migration search, only when applicable to the reviewed task: the canonical removed-symbol-scan contract step passed in the task artifact; no ad hoc search is claimed as acceptance evidence.

## Automated Test Exception

- Applies: no
- Reason: a current task-local automated artifact exists and covers all required test levels and contract checks.
- Owner: testing
- Risk: none requiring an automated-test exception.
- Acceptance impact: none.
- Alternative evidence: not needed.

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: all approved behaviors, lifecycle and compatibility paths, cross-module evidence, all eight correctness categories, and the complete current evidence chain pass; F-TRDFCH-001 through F-TRDFCH-005 are closed and no new counterexample was found.
- Supporting task-relevant test evidence: the current 17-step artifact and complete stage-scope evidence, with evidence hash `7ec32b9d5d8ae06b6155194a5c106ba9f54d2b7c6719a630c3a96aeca96666da`.
- Residual risk: coordinated deployment remains required because old and new reverse-fallback peers intentionally fail closed instead of interoperating; this behavior is documented, bounded, and accepted.

## Follow-Up Tasks

- Requirement task: none.
- User decision required for proposal issue: no.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable; coverage is complete.
- Iteration count: 4
- Stop reason if more than 5 unsuccessful iterations: not applicable.
