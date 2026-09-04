# QUIC NAT Punch Connect-Lifetime Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-QNPL-000 | none | acceptance | proposal, pipeline plan, admitted QUIC source, dedicated ownership/lifecycle tests, current task artifact, and fresh defect-discovery audit | no blocking requirement, design, implementation, testing, lifecycle, compatibility, concurrency, or capacity defect remains; the initial one-sided delayed test gap was returned to testing and closed by current two-sided evidence | none |

## Result Summary
- Overall result: accepted.
- Plain-language outcome: eligible active and reverse QUIC attempts now keep sending the existing short UDP punch packets for the full candidate connect timeout instead of stopping at one second, while Quinn continues one pending `Connecting` and its own PTO recovery.
- What was verified: 250ms/0ms first offsets and 50ms cadence remain unchanged; the one-second outer early-error retry window remains separate; success, final error, timeout/deadline, owner cancellation, worker-task drop, and listener close converge; source socket, candidate policy, best-effort send behavior, and non-punch TunnelNetwork results remain compatible.
- Evidence used: launch-confirmed proposal and pipeline mappings, admission stamp, direct source and caller review, implementation/testing scope results, task testplan/state, two-sided delayed lifecycle coverage, real listener/source-port coverage, real QUIC success/failure integration, and the latest successful task-level `all` artifact.
- Blocking issues: none.
- Next action: mark the auto-pipeline complete and remove task 017 from the unfinished-task index.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 017-quic-nat-traversal-improvement
- change_id values reviewed: quic_nat_punch_connect_lifetime, quic_nat_punch_owner_cancellation
- Review date: 2026-08-27
- In scope: listener-source punch scheduling, the per-candidate `open_or_connect` owner, worker-runtime connect cancellation, deadline/close transitions, existing early-error retry compatibility, dedicated tests, and task-local plan/state/testplan/evidence.
- Out of scope: SN wire or endpoint freshness, candidate classification changes, the 300ms reverse hedge, multiple ports/SNs, NAT type prediction, IPv6 punch, UPnP/NAT-PMP, Quinn PTO tuning, public API changes, or broad workspace validation.
- Task-relevant acceptance scope: proposal items P-QNPL-1/P-QNPL-2, both pipeline scope bindings, admitted `listener.rs`/`network.rs`, dedicated listener and ownership tests, and the latest task-level artifact.
- Out-of-scope checks not run: package-wide or workspace-wide suites, `all all`, quality gates, architecture audits, unrelated dirty-worktree tests, and real Internet NAT-lab interoperability.

## Optional Diff / Status Evidence
- `git status --short` summary: the repository contains unrelated untracked Harness/task material; acceptance used only this task's explicit manifests and bound evidence.
- `git diff --stat` summary: task discovery shows the two admitted QUIC production files, one existing dedicated listener test file, and one new dedicated ownership test file; untracked task artifacts are not fully represented by ordinary diff stat.
- `git diff --name-status` summary: not used as a pass condition; production and testing boundaries come from admission and stage manifests.
- `git diff --check` result: no whitespace errors were reported for the task's production and test paths.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| extend eligible punch through the corresponding connect deadline | proposal P-QNPL-1; plan connect-lifetime binding | `QuicTunnelListener::run_udp_punch_burst` receives the `open_or_connect` start time and calculated timeout, schedules until `max_duration`, and no longer applies the one-second constant | schedule test proves active 250ms..3s and reverse 0ms..3s at exact 50ms intervals; delayed active/reverse counters remain live beyond one second | implemented |
| retain one Quinn connect flow and existing PTO/early-error behavior | proposal P-QNPL-1 and explicit non-goals; plan connect state/failure flow | `open_or_connect` constructs one connect future, races it with punch, and awaits the same pinned future if punch ends first; the existing loop recreates only after an actual early terminal error, using the unchanged 50ms/one-second helper | single-connect counters remain one both when punch ends first and when connect completes after 1100ms; existing early-error boundary tests pass | implemented |
| stop punch on success, final error, timeout/deadline, owner cancellation, and listener close | proposal P-QNPL-2; plan ownership transitions | `connect_with_owned_udp_punch` drops punch on connect result; structured future drop owns both futures; `AbortOnDropTask` aborts the worker Quinn task; listener waits select on `close_notify` and checks `closed` before sends | success/error/drop tests, two-sided delayed success, owner-abort test, worker-task abort test, and real listener-close wake test all pass | implemented |
| preserve source port, candidate policy, payload filtering, and best-effort errors | proposal Scope/Success Criteria; plan compatibility and UDP failure flows | listener continues cloning its registered `punch_socket`; policy and private payload magic are unchanged; send errors are traced and ignored | source-port, eligibility matrix, payload/magic, receive filtering discovery, and send-failure tests pass in the filtered suite | implemented |
| preserve non-punch consumer behavior and boundaries | proposal non-goals; plan exported interfaces/API impact | public signatures, SN/TunnelManager/PN paths, timeout defaults, candidate selection, and QUIC/TLS codecs remain outside production diff | real signed listener-based QUIC success and unreachable no-listener failure integration cases pass | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| full connect-lifetime active/reverse schedule | normal / boundary / lifecycle | the existing schedule test now covers both first offsets, exact cadence, short boundary deadlines, and a three-second deadline beyond the old cap | filtered task step passes schedule plus concurrent active/reverse counters beyond one second | adequate |
| one connect flow while punch is pending or finishes first | normal / ordering / compatibility | dedicated generic owner tests count connect future entry and exercise both `tokio::select!` branches | connect count is one after 1100ms delayed success and one after punch-first completion | adequate |
| all owner terminal paths | negative / error / lifecycle / concurrency | dedicated tests inject success, final error, caller abort, worker abort, missing sender, rejected candidate, and listener close | all focused lifecycle tests pass; task artifact records exit code zero | adequate |
| source socket and best-effort I/O | boundary / error / compatibility | real sfo-reuseport socket tests compare source ports and invoke a closed sender | source-port and send-failure cases pass without changing the connect result contract | adequate |
| consumer and neighbor compatibility | compatibility / cross-module | integration steps use unchanged TunnelNetwork success and failure entrypoints with real QUIC/TLS | both integration commands pass; no public or wire migration trigger exists | adequate |

## Defect Discovery Audit
| Category | Concrete Failure Hypotheses | Evidence Inspected | Result | Owning Stage |
|----------|-----------------------------|--------------------|--------|--------------|
| requirement-and-behavior | punch still stops at one second; cadence accidentally drives Quinn recreation; deadline or offsets drift from the approved values | proposal P-QNPL-1/P-QNPL-2, plan invariants, listener scheduling loop, network connect loop, task tests | pass: punch max duration is the calculated connect timeout, cadence is confined to the listener future, and early Quinn recreation remains conditional on a completed error | none |
| logic-and-control-flow | off-by-one skips first/last planned punch; punch-first select loses the connect future; immediate policy return changes non-punch results | `run_udp_punch_burst` comparisons/checked addition, both select arms, candidate-policy branch, schedule and punch-first tests | pass: starts are inclusive, deadline is a hard upper bound, the same pinned connect future is awaited after punch completion, and rejected punch work falls through to normal connect | none |
| boundary-and-input | timeout shorter than 250ms, zero/huge duration, non-QUIC/private/IPv6/zero-port candidates, absent listener socket | offset/deadline guards, checked duration addition, eligibility helper and matrix, missing-sender test | pass: active may produce zero sends when its approved first offset is outside the deadline, reverse handles zero, arithmetic is checked, and unsupported candidates/sockets return without punch | none |
| state-and-data-integrity | multiple connect states or duplicate tunnel publication appear; punch state survives into a later candidate | one local connect future construction, sequential early-error loop, `finish_connect` placement, no new shared collection, single-connect tests | pass: one candidate owner contains one pending connect future at a time, finish/publish remains after the helper, and no punch registry or cross-attempt state was introduced | none |
| error-handling-and-recovery | UDP send failure aborts QUIC; early error window is extended to the full punch deadline; final Quinn errors are swallowed | best-effort send arm, `udp_punch_retry_delay_after_error`, error return/log path, existing boundary tests | pass: send errors remain trace-only, early terminal retry still ends at one second, and the final connect error propagates unchanged | none |
| resource-lifetime-and-cleanup | detached punch or worker-runtime Quinn task survives success/cancel/close; long timeout preallocates an unbounded schedule | absence of `Executor::spawn_ok`, structured owner helper, `AbortOnDropTask`, streaming offset loop, drop/close tests | pass: no punch task is spawned, owner drop aborts the worker task, listener close wakes the future, and the schedule uses constant memory | none |
| concurrency-and-ordering | listener close notification is lost; close/send race causes recurring late sends; connect and punch deadline race deadlocks | notified-before-check pattern, atomic close flag, wait/send selects, owner select, close/abort tests | pass: notify/check ordering closes the lost-wakeup window, all waits and sends remain cancellable, and either deadline winner completes without circular waiting; an already-polled UDP send may race close but no subsequent scheduled send can begin | none |
| interface-and-compatibility | public API, QUIC/SN wire, source socket, candidate eligibility, or client-endpoint fallback changes | exact production scope, imports/signatures, payload/policy helpers, source-port test, real success/failure integration | pass: only crate-private lifetime composition changed; public/wire/candidate/source-port contracts remain intact | none |
| security-and-capacity | full timeout causes unbounded memory/task growth, secret logging, unauthenticated success, or traffic after tunnel establishment | streaming loop, bounded payload constants, logs, TLS `Connecting` result boundary, cancellation path | pass: work is bounded by configured timeout and candidate count, payload stays 5..30 bytes, no secrets are logged, and punch cannot establish or outlive the authenticated connection owner | none |
| test-adequacy | tests merely inspect helpers, miss two-sided overlap, fail to exercise real listener close/source socket, or omit consumer failure semantics | test source, initial return record, final testplan/state, latest artifact commands and assertions | pass: the initial one-sided gap was corrected; current evidence combines two-sided delayed counters, owner branch/future-drop tests, real listener/source I/O, and real QUIC consumer success/failure | none |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | deadline comparisons, offset progression, policy exits, connect/punch select arms, and early-error loop | listener/network source plus schedule, punch-first, delayed-connect, and compatibility tests | active/reverse offsets are preserved, deadline is not capped at one second, connect result stops punch, punch completion retains the same connect future, and 50ms Quinn recreation occurs only after the unchanged actual-error branch | none | pass |
| termination and progress | long punch loop, connect timeout, punch-first continuation, and checked arithmetic | streaming loop, timeout calculation, `checked_add`, structured select, deadline/close tests | every path is bounded by connect timeout, close, or owner completion; no schedule vector or detached loop grows with timeout; arithmetic overflow terminates | none | pass |
| concurrency and synchronization | close notification ordering, future cancellation, worker-runtime join task, and select races | atomic close flag, Notify creation/check/select sequence, `AbortOnDropTask`, owner guard, concurrent tests | no lock is held across await; close cannot leave future waits asleep; caller cancellation drops punch and aborts worker connect; no deadlock or persistent late-send loop was found | none | pass |
| resource lifetime and cleanup | listener socket clone, punch future, Quinn Connecting worker, timers, and payload | owner nesting, close path, abort guard, test drop flags, real listener close | resources remain within the candidate owner; success/error/drop/close release futures and timers; no detached punch handle or registry remains | none | pass |
| state and data integrity | candidate-specific deadline, single connect state, tunnel finish/publish, and early retry | `open_or_connect`, helper call, `finish_connect`, unchanged tunnel caller | deadline/start values are shared by connect and punch, no state crosses candidates, and tunnel construction still occurs once after authenticated connect success | none | pass |
| error handling and recovery | UDP send errors, final connect errors, timeout classification, missing sender, and early retry | trace/error branches plus focused error/negative tests | punch remains best-effort, final error propagates, unavailable punch work does not alter connect, and existing retry classifications are preserved | none | pass |
| interface boundary and compatibility | crate-private listener/network interface, source port, public TunnelNetwork, QUIC/TLS and SN wire | plan API/interface mapping, exact diff, source-port and integration evidence | no public signature/export, wire format, candidate policy, timeout default, or neighboring module changed | none | pass |
| security and capacity safety | public UDP amplification, timeout scale, payload confidentiality, and authentication boundary | proposal risk limits, constants, streaming loop, cancellation, TLS completion path | traffic grows linearly only until configured connect timeout, uses existing small random private payloads and candidate policy, logs no payload/secret, and success still requires QUIC/TLS | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-QNPL-1 | proposal P-QNPL-1 | eligible active/reverse punch continues beyond one second up to the supplied connect deadline with unchanged offsets/cadence | direct scheduling flow plus three-second offset and delayed two-sided evidence | pass |
| AR-QNPL-2 | proposal single-Connecting invariant | punch cadence never creates Quinn state; one connect future remains pending and punch-first completion awaits that same future | source flow and connect-entry counters across both select orderings | pass |
| AR-QNPL-3 | proposal outer-retry compatibility | early completed errors retain the current one-second window and 50ms delay | unchanged helper/control branch and boundary assertions | pass |
| AR-QNPL-4 | proposal P-QNPL-2 | success, final error, timeout/deadline, caller drop, worker drop, and listener close stop later punch work | structured owner source and deterministic terminal-path tests | pass |
| AR-QNPL-5 | proposal compatibility/non-goals | source port, candidate policy, best-effort I/O, payload filtering, public/wire behavior, and non-punch consumers remain unchanged | source/policy/send tests and real QUIC success/failure integration | pass |

## Inputs
- user-launch-confirmed `proposal.md`
- launch-confirmed `pipeline/plan.md` and mutable `pipeline/state.json`
- `testplan.yaml`
- `docs/modules/p2p-frame.md` QUIC boundary
- `p2p-frame/src/networks/quic/listener.rs`
- `p2p-frame/src/networks/quic/listener/tests.rs`
- `p2p-frame/src/networks/quic/network.rs`
- `p2p-frame/src/networks/quic/network/punch_owner_tests.rs`
- admission evidence and generated stamp
- current task run artifact under `test-results/test-runs/`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Because the active execution policy did not authorize an independent sub-agent, the root reviewer started a fresh falsification pass and did not use the implementation summary as a correctness conclusion.
2. Reviewed the launch-confirmed requirements, success criteria, risks, constraints, and non-goals.
3. Reviewed the pipeline ownership/deadline mapping and generated the acceptance rules above before selecting a conclusion.
4. Traced the current source from candidate intent through `open_or_connect`, the listener punch future, worker-runtime Quinn connect, close, final tunnel construction, and existing caller boundaries.
5. Tested failure hypotheses for one-second capping, timer-driven Quinn recreation, lost close wakeups, owner-drop leaks, early-error retry drift, source-port drift, unchecked duration growth, and non-punch compatibility.
6. Returned the initial one-sided delayed-evidence weakness to testing, inspected the two-sided replacement assertions, and reused only the regenerated current task artifact.
7. Completed both the current ten-category defect-discovery audit and the repository checker's eight-category correctness audit; no blocking finding remains.

## Consistency Summary
- Proposal authority check: explicit user statement “确认，自动完成” launches the auto-pipeline and binds the proposal without separate approval metadata.
- Proposal vs design: the plan maps P-QNPL-1/P-QNPL-2 to full-deadline scheduling, structured ownership, exact two-file scope, one-Connecting semantics, unchanged one-second early-error retry, and explicit non-goals without narrowing or expansion.
- Design vs testing implementation: tests derive from schedule parameters, both select orderings, every owner/close/error transition, source/candidate boundaries, and unchanged consumer success/failure semantics.
- Design vs long-lived boundary doc: all production/test behavior remains within `src/networks/quic/**`, which the p2p-frame module owns; no module-boundary update is required.
- Design vs implementation: listener owns source-socket scheduling and close, while `open_or_connect` owns the punch/connect future pair exactly as planned.
- Test implementation vs test code vs results: testplan unit/DV steps share one filtered command; the runner deduplicated it and executed 25 tests plus two distinct integration commands successfully.
- Test design adequacy: the returned one-sided evidence gap is closed by active/reverse concurrent counters beyond one second; real listener close/source-port and real non-punch QUIC consumer paths supplement the deterministic owner tests.
- change_id traceability: proposal, plan, admission, state, testplan, artifact, manifests, and this report consistently include both change IDs.
- Acceptance criteria traceability: every lifetime, one-Connecting, early-retry, cancellation, close, source, policy, best-effort, and compatibility criterion has concrete implementation and runnable evidence.
- Cross-module admission: only p2p-frame changes production/test behavior; no neighboring crate or exported contract changed, so no globals packet or second admission is required.
- Public API / codec / runtime semantics review: public API and codecs are unchanged; the approved runtime semantic change is only punch lifetime/ownership.
- Document logic review: no contradiction, impossible state, unsupported requirement inference, or silent scope expansion remains.
- Implementation logic review: one streaming punch loop and one connect future share start/timeout values; every owner terminal path drops the other future and the existing early-error loop remains error-driven.
- Implementation correctness audit completeness and routing: all required categories pass; the single testing return is closed and no return to proposal, design, implementation, or testing remains.
- Document approval timing (approved_content_sha256 verified by schema-check): auto-pipeline launch confirmation replaces proposal approval metadata for this packet; schema-check passed for the current launch/plan structure.
- Implementation task paths bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ... --changed-paths-file ...`): passed for both admitted QUIC production files, admission evidence/stamp, and mutable state against both change IDs.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: this is an optimization/lifecycle task; the admitted pre-change source is the source-bound red baseline because it capped scheduling at one second and used `Executor::spawn_ok`, while the current task artifact supplies green full-deadline and structured-owner evidence.

## Validation Evidence
- Existing schema result (cite the owning-stage result; do not rerun unchanged input): `schema-check: passed` for v0.1/p2p-frame/017 after pipeline launch.
- Existing admission stamp (cite the stamp; do not run `--verify-only` during acceptance unless an admission-owned input changed): `docs/versions/v0.1/evidence/admission/20260827-quic-nat-traversal-improvement.p2p-frame.017-quic-nat-traversal-improvement.stamp.json`.
- Existing stage-scope result (cite the owning-stage result; do not rerun an unchanged manifest/scope): proposal passed with 2 paths, design passed with 2 paths, implementation passed with 5 paths, and current testing passed with 8 paths using the captured implementation baseline.
- Existing pipeline-plan result, when applicable (cite the latest result for current plan/state inputs): `pipeline-plan-check: passed`; the plan hash remains bound in sibling state while execution evidence is state-owned.
- Task-relevant test run artifact(s) (reuse when implementation/tests/testplan/registration are unchanged): `test-results/test-runs/20260827T090958Z-p2p-frame+017-quic-nat-traversal-improvement-all.json`, exit code 0, three non-empty deduplicated commands, 25 focused tests plus two real QUIC integration tests, and both change IDs covered.
- Commands rerun because checker-owned inputs changed after their previous pass (or `none` with evidence): the task `all` command reran after the initial one-sided delayed case was replaced by two-sided active/reverse evidence; testing coverage and testing scope reran after the current artifact/state/manifest update; admission was not replayed because proposal/plan/scope binding did not change.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; this single-task flow used only `p2p-frame/017-quic-nat-traversal-improvement all` plus implementation-stage compile checks.
- Risk-triggered task-local contract kinds and assertions, when applicable: no breaking/migration-required API, crate-root export, build-surface, documentation-example, or wire-format change requires contract steps; runtime semantics are covered by unit/DV/integration task commands.
- Scoped evidence input hash current, when risk-triggered: the current artifact records `edc5124e7eb09951b6760437d033d28c8653968dfef741ee89692ec6ee3dd09c` over proposal, plan, testplan, production, and dedicated test inputs.
- Quality gates: not required for this task acceptance; no quality-gate run was requested.
- Explicitly requested quality run artifact, if any: none; no quality run was requested.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: not run because no workspace/crate boundary or architecture contract changed.
- Acceptance report check after this report was created or modified: this report is validated immediately after writing; any structural failure blocks final state completion.
- Targeted migration search, only when applicable to the reviewed task: not applicable because no symbol, public caller, frame, dependency, or build migration exists; exact scope and compiler-backed task commands establish behavior compatibility.

## Automated Test Exception
- Applies: no
- Reason: a successful automated task-level `all` artifact exists with non-empty unit/DV and integration commands for both change IDs.
- Owner: acceptance
- Risk: no residual risk from missing automated execution evidence; real Internet NAT diversity remains an environment validation concern rather than an automated-test exception.
- Acceptance impact: automated evidence is present and required.
- Alternative evidence: not needed because the current artifact records the focused lifecycle suite and both real QUIC consumer commands.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: full-connect-deadline punch scheduling and structured connect ownership are implemented inside the admitted boundary, the single-Connecting and early-error compatibility invariants hold, the returned two-sided evidence gap is closed, and the fresh defect-discovery audit found no blocking defect.
- Supporting task-relevant test evidence: `test-results/test-runs/20260827T090958Z-p2p-frame+017-quic-nat-traversal-improvement-all.json`.
- Residual risk: the hermetic suite cannot reproduce every carrier-grade/symmetric NAT or real SN timing environment; the change improves overlap for eligible existing candidates but intentionally does not solve stale candidates, socket-mapping mismatch, UDP blocking, or symmetric NAT.

## Follow-Up Tasks
- Requirement task: none required.
- User decision required for proposal issue: none required.
- Design task: none required.
- Implementation task: none required.
- Testing task: none required; F-QNPL-TEST-001 is closed by the current two-sided delayed case and regenerated artifact.
- Testing return reason if coverage is incomplete: coverage is complete; no return is required.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable because acceptance passed after the first testing return.
