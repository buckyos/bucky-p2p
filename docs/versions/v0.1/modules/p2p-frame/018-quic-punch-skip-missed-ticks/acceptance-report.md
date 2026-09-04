# QUIC Punch Missed-Tick Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-QPSM-000 | none | acceptance | proposal P-QPSM-1, pipeline next-offset design, admitted listener delta, dedicated delayed/overflow tests, final task artifact, and category-by-category falsification review | no blocking requirement, design, implementation, or testing defect found | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: delayed QUIC UDP punch scheduling now sends at most the current due packet and jumps directly to a future eligible 50ms grid point instead of replaying every historical tick.
- What was verified: exact five-second recovery arithmetic, normal active/reverse grid preservation, deadline and overflow termination, unchanged listener owner and close paths, unchanged non-punch QUIC success/failure behavior, and absence of a zero-wait catch-up loop.
- Evidence used: current proposal and plan, `listener.rs:575-582` and `listener.rs:872-890`, dedicated tests at `listener/tests.rs:75-146`, compile evidence, admission/scope evidence, and `test-results/test-runs/20260827T095542Z-p2p-frame+018-quic-punch-skip-missed-ticks-all.json`.
- Blocking issues: no blocking issues remain after direct logic, boundary, lifecycle, compatibility, capacity, and test-adequacy review.
- Next action: record accepted pipeline state, validate the final report and complete pipeline, then close unfinished-task bookkeeping.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 018-quic-punch-skip-missed-ticks
- change_id values reviewed: quic_punch_skip_missed_ticks
- Review date: 2026-08-27
- In scope: the per-candidate `run_udp_punch_burst` next-offset transition after a send, attempt-relative 50ms grid behavior, delayed recovery, deadline and duration arithmetic, and task-local regression coverage.
- Out of scope: cadence constants, active/reverse first offsets, connect timeout/retry, Quinn PTO, owner cancellation, candidate selection, punch payload, source socket, wire protocol, SN/PN/tunnel publication, and broad workspace behavior.
- Task-relevant acceptance scope: `p2p-frame/src/networks/quic/listener.rs`, the dedicated `listener/tests.rs` additions, packet proposal/plan/state/testplan, admission/scope evidence, and the final 018 task artifact.
- Out-of-scope checks not run: package-wide runtime suites, workspace-wide tests, Internet NAT environments, quality gates, deployment, and unrelated dirty-worktree tasks were not run or reclassified as 018 evidence; an extra read-only package formatting check was run separately and is recorded below.

## Optional Diff / Status Evidence
- `git status --short` summary: the worktree contains pre-existing 017 QUIC runtime/test changes and many untracked Harness artifacts; 018 preserved that shared state through selected-file baselines and task-specific manifests.
- `git diff --stat` summary: repository-wide diff is intentionally not used as the 018 scope because it includes the pre-existing 017 delivery; the implementation baseline isolates the new helper/call-site delta in `listener.rs`.
- `git diff --name-status` summary: task-owned production delta is `p2p-frame/src/networks/quic/listener.rs`; task-owned test delta is the added missed-tick cases in `p2p-frame/src/networks/quic/listener/tests.rs`.
- `git diff --check` result: passed for the task production and test files after the final edits.
- Package formatting observation: `cargo fmt -p p2p-frame -- --check` returned non-zero on pre-existing unrelated dirty paths, including control-stream, TCP, PN/SN and the baseline-existing `listener.rs:322`; its output did not identify the 018 helper/call-site or dedicated missed-tick tests, so no unrelated formatting was applied.
- Note: diff/status evidence was used only to locate task evidence and preserve unrelated work, not as the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| quic_punch_skip_missed_ticks / P-QPSM-1 | user-launch-confirmed `proposal.md`; validated `pipeline/plan.md` scope binding | `run_udp_punch_burst` calls `udp_punch_next_offset` after one send at `listener.rs:575-582`; helper keeps a future regular tick or jumps arithmetically to the first grid point at or after elapsed plus one interval at `listener.rs:872-890` | tests at `listener/tests.rs:75-146`; passing task artifact `test-results/test-runs/20260827T095542Z-p2p-frame+018-quic-punch-skip-missed-ticks-all.json` | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| quic_punch_skip_missed_ticks: normal phase, delayed burst prevention, deadline/overflow, lifecycle, capacity, and unchanged consumers | normal, boundary, negative, error, compatibility, lifecycle, cross-module | testplan unit covers both algorithm arms and all meaningful failure returns; DV maps the five-second recovery sequence; integration retains real QUIC TunnelNetwork success and unreachable-listener failure semantics; state records all seven case types with no gap | final task artifact executed 3 focused missed-tick tests plus 2 real QUIC integration tests with exit code 0 | adequate |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | next-offset branch, interval ceiling, loop update, and deadline comparison | `listener.rs:575-582`, `listener.rs:872-890`, exact 50ms/5000ms/5001ms expectations | `regular_next > elapsed` preserves normal phase; overdue flow computes a ceiling interval count in one operation; 50ms current offset at 5s becomes 5050ms rather than 100ms, so no historical zero-wait loop remains; `<= max_duration` preserves an on-deadline tick and rejects later ticks | none | pass |
| termination and progress | large delay, deadline, and arithmetic failure | helper `Option` returns and loop `let Some ... else return`; deadline/overflow tests | every helper path either yields a strictly greater offset or `None`; no unchanged offset or catch-up iteration can cause non-progress, and deadline/overflow terminates without panic | none | pass |
| concurrency and synchronization | runtime pause/wake, send completion, listener close, and owner drop ordering | post-send `started_at.elapsed()` observation; existing `close_notify` select and closed checks around send; unchanged 017 owner composition in `network.rs` | offset state remains future-local with one owner and no shared mutation; elapsed is sampled only after the current send completes; close/cancel paths remain structured and cannot create a detached catch-up task | none | pass |
| resource lifetime and cleanup | timer, UDP socket reference, punch future, and connect attempt lifetime | `run_udp_punch_burst` loop and existing listener close handling; 017 owner code consumed unchanged | helper allocates no resource or task; failure/None returns drop the same future/socket references as before; success, error, timeout, close, and cancellation ownership boundaries are unchanged | none | pass |
| state and data integrity | `next_offset`, grid phase, index, and per-candidate isolation | helper inputs/return, active/reverse offset assertions, loop assignment at `listener.rs:581-582` | each candidate keeps its own attempt-relative offset; normal active 250ms/reverse 0ms phase is preserved; skipped history is not counted as sent and the send index increments once per actual send | none | pass |
| error handling and recovery | checked duration arithmetic, interval-count conversion, deadline, and UDP send failure | checked adds/subtract/multiply/conversion in helper; `listener/tests.rs:120-146`; unchanged best-effort send branch at `listener.rs:551-573` | overflow or excessive interval count returns `None` and stops safely; deadline rejects the next packet; one UDP send error remains logged and does not change ownership or retry semantics | none | pass |
| interface boundary and compatibility | crate-private listener helper, `run_udp_punch_burst` caller, non-punch QUIC consumers, and protocol boundaries | pipeline interface table; unchanged method signature and `network.rs`; two real QUIC integration commands | no public API, wire, candidate, payload, source-port, retry, or connect-owner change; existing listener-based success and unreachable-listener failure both pass | none | pass |
| security and capacity safety | UDP amplification after scheduler pause and arithmetic work for large elapsed values | direct jump formula, five-second test, deadline cap, u32 conversion, and proposal capacity boundary | a five-second delay performs constant-size arithmetic and schedules one future tick instead of approximately 101 immediate sends; work and packet count do not scale with the number of missed ticks, while existing per-candidate deadline remains the hard lifetime cap | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-QPSM-1 | proposal P-QPSM-1 and user instruction | a five-second delayed reverse schedule must not advance from 50ms to 100ms and replay history; it must jump to a future eligible grid point | direct helper inspection plus 5000ms and 5001ms assertions | pass |
| AR-QPSM-2 | proposal normal-cadence and intent boundaries | normal future tick remains on the original 50ms grid and active/reverse starts remain 250ms/0ms | normal branch and intent-offset assertions | pass |
| AR-QPSM-3 | proposal deadline and overflow boundaries | next offset beyond deadline or unsafe duration/count arithmetic stops without send, panic, or wraparound | `Option` failure path plus deadline/Duration/u32-count tests | pass |
| AR-QPSM-4 | proposal non-goals and plan interface decision | owner, close, payload, candidate, retry, connect, public API, and non-punch consumer behavior remain unchanged | final diff inspection and QUIC success/failure integration steps | pass |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/018-quic-punch-skip-missed-ticks/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/018-quic-punch-skip-missed-ticks/pipeline/plan.md`
- `docs/versions/v0.1/modules/p2p-frame/018-quic-punch-skip-missed-ticks/pipeline/state.json`
- `docs/versions/v0.1/modules/p2p-frame/018-quic-punch-skip-missed-ticks/testplan.yaml`
- `p2p-frame/src/networks/quic/listener.rs`
- `p2p-frame/src/networks/quic/listener/tests.rs`
- `p2p-frame/src/networks/quic/network.rs`
- `test-results/test-runs/20260827T095542Z-p2p-frame+018-quic-punch-skip-missed-ticks-all.json`
- `docs/modules/p2p-frame.md`
- `harness/rules/acceptance-review-rules.md`

## Consistency Summary
- Proposal authority check: explicit user statement “确认，自动完成” is recorded verbatim in the plan and binds proposal P-QPSM-1 without separate approval metadata.
- Proposal vs design: the plan preserves the attempt-relative grid, requires one arithmetic jump over overdue ticks, keeps a recovery interval and deadline/overflow stop, and retains every proposal non-goal.
- Design vs testing implementation: dedicated tests cover the plan's normal, overdue, deadline and overflow transitions; task integration covers the unchanged consumer boundary.
- Design vs long-lived boundary doc: the change remains inside `src/networks/quic/**`, preserves listener-source punch ownership and 50ms cadence, and narrows burst risk consistently with `docs/modules/p2p-frame.md`.
- Design vs implementation: implementation is exactly the one-file Scope Path and uses the designed current-offset/elapsed/deadline inputs with checked arithmetic.
- Test implementation vs test code vs results: testplan step ids map to the dedicated tests and existing integration cases; the final artifact records all commands, registrations, change_id, evidence inputs, and zero exit codes.
- Test design adequacy: deterministic private scheduling logic is tested at unit/DV with exact boundary outputs; unchanged external QUIC consumer semantics have success and failure integration evidence; no relevant case-type gap remains.
- change_id traceability: `quic_punch_skip_missed_ticks` maps P-QPSM-1 -> plan scope binding -> admission quote -> helper/call site -> unit/DV/integration steps -> state evidence -> this report.
- Acceptance criteria traceability: delayed history is skipped, normal intent/grid behavior is preserved, deadline and overflow stop safely, excluded owner/protocol boundaries are unchanged, and runnable task evidence exists.
- Cross-module admission: not required because the packet and only production target are p2p-frame; integration is compatibility evidence, not an admitted neighboring implementation.
- Public API / codec / runtime semantics review: no public/codec/wire change; runtime semantics change only from catch-up replay to bounded missed-tick skipping.
- Document logic review: proposal, plan, testplan, state, admission and scope evidence use the same change_id and do not narrow or expand the user-selected behavior.
- Implementation logic review: arithmetic was checked for exact-boundary, off-grid, five-second, deadline, Duration and interval-count counterexamples; no zero/progress, wraparound, or repeated-send counterexample remains.
- Implementation correctness audit completeness and routing: all eight required correctness categories were reviewed with concrete current evidence and passed; requirement and test adequacy were reviewed separately above; no upstream return is required.
- Document approval timing (approved_content_sha256 verified by schema-check): auto-pipeline launch binds the draft proposal without separate approval metadata; current schema check passed after testplan creation.
- Implementation task paths bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ... --changed-paths-file ...`): passed for the exact listener production file, admission evidence/stamp, and task state.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: pre-fix replay is source-bound because the shared dirty listener file contains completed 017 work; the old `next_offset += 50ms` behavior would yield 100ms after a 50ms current offset at five seconds, while the new regression requires 5050ms. Reverting that shared baseline solely for red execution would overwrite unrelated work; the dedicated test is green on the admitted fix.

## Validation Evidence
- Existing schema result (cite the owning-stage result; do not rerun unchanged input): `schema-check.py --version v0.1 --module p2p-frame --submodule 018-quic-punch-skip-missed-ticks` passed after final testplan creation.
- Existing admission stamp (cite the stamp; do not run `--verify-only` during acceptance unless an admission-owned input changed): `docs/versions/v0.1/evidence/admission/20260827-quic-punch-skip-missed-ticks.p2p-frame.018-quic-punch-skip-missed-ticks.stamp.json` was written and verified against proposal/plan hashes and exact listener Scope Path.
- Existing stage-scope result (cite the owning-stage result; do not rerun an unchanged manifest/scope): proposal, design, implementation, and testing stage-scope checks passed; implementation covered 4 task paths and testing covered 4 task paths.
- Existing pipeline-plan result, when applicable: current plan/state validation passed after testing evidence and task status were recorded.
- Task-relevant test run artifact(s) (reuse when implementation/tests/testplan/registration are unchanged): `test-results/test-runs/20260827T095542Z-p2p-frame+018-quic-punch-skip-missed-ticks-all.json` records 3 successful deduplicated commands from 4 unit/DV/integration registrations and covers `quic_punch_skip_missed_ticks`.
- Commands rerun because checker-owned inputs changed after their previous pass (or `none` with evidence): the task `all` command reran after removing an unreachable helper branch, adding the elapsed-overflow assertion, and adding the integration consumer path to evidence inputs; an additional read-only `cargo fmt -p p2p-frame -- --check` later failed only on pre-existing unrelated dirty formatting and did not change files or task evidence.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; this single-task auto-pipeline used only `p2p-frame/018-quic-punch-skip-missed-ticks all` plus the implementation compile check.
- Risk-triggered task-local contract kinds and assertions, when applicable: not applicable because no public API, migration, crate-root export, build surface, or documentation example changed.
- Scoped evidence input hash current, when risk-triggered: no risk-triggered contract hash is required; the task artifact nevertheless records current evidence input hash `70e2cbc42161032c263bcc5260f0f556906cf1951a9d2c6b5f40fb236ea0a8d5`.
- Quality gates: not applicable; the user did not explicitly request quality gates and the task flow did not run them.
- Explicitly requested quality run artifact, if any: none because no quality run was requested.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: not applicable because no workspace/crate boundary or architecture contract changed.
- Acceptance report check after this report was created or modified: this report is validated immediately after writing; any failure blocks accepted state.
- Targeted migration search, only when applicable to the reviewed task: not applicable because no symbol, caller, codec, dependency, build, or migration surface changed.

## Automated Test Exception
- Applies: no
- Reason: a successful automated task-level `all` artifact exists with non-empty unit/DV and integration commands for the only change_id.
- Owner: acceptance
- Risk: no residual risk from missing automated execution evidence; real Internet NAT diversity remains outside the deterministic scheduler regression.
- Acceptance impact: automated task evidence is present and required.
- Alternative evidence: not needed because the current artifact records focused scheduler and real QUIC consumer commands.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: the delivered helper removes historical zero-wait catch-up, preserves normal active/reverse grid behavior, stops safely at deadline or arithmetic limits, changes no excluded owner/protocol boundary, and is supported by runnable task-specific unit/DV/integration evidence; the fresh defect search found no blocking counterexample.
- Supporting task-relevant test evidence: `test-results/test-runs/20260827T095542Z-p2p-frame+018-quic-punch-skip-missed-ticks-all.json`.
- Residual risk: deterministic tests do not emulate every OS scheduler and Internet NAT environment, but the burst-causing state transition is pure arithmetic and is directly covered for exact/off-grid five-second delay plus numeric limits.

## Follow-Up Tasks
- Requirement task: none required.
- User decision required for proposal issue: none required.
- Design task: none required.
- Implementation task: none required.
- Testing task: none required.
- Testing return reason if coverage is incomplete: coverage is complete; no return is required.
- Iteration count: 0
- Stop reason if more than 5 unsuccessful iterations: not applicable because acceptance passed without a return iteration.
