# p2p-frame SN Client QA Correlation Fix Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | completed second-iteration review evidence | no blocking finding remains after `ACCEPTANCE-TEST-DEPTH-001` added constructible exit-path coverage | none |

## Object and Scope
- Module: p2p-frame / `sn-client-qa-correlation-fix`; technical dependency evidence includes the local `callback-result 0.2.4` patch consumed by `sfo-cmd-server`.
- Version: v0.1.
- change_id values reviewed: `sn_client_qa_transactions`, `sn_client_qa_pending_cleanup`, `sn_client_qa_migration_boundary`.
- Review date: 2026-07-10.
- In scope: ReportSn/SnCall/SnQuery QA framing, business response validation, removal of SN-owned waiters/response handlers, callback cancellation cleanup, SnCalled preservation, fallback/state integrity, and coordinated deployment boundaries.
- Out of scope: migrating SnCalled/inter-SN/owner commands, body codec/schema changes, endpoint classification rules, capability negotiation, mixed-version fallback, generic command-runtime concurrency redesign, and unrelated shared-worktree changes.

## Optional Diff / Status Evidence
- `git status --short` summary: task evidence is isolated to the sibling packet, two SN production files, root callback patch/lock binding, one existing SN test file, the dedicated callback test, admission evidence, pipeline plan, and this report; the shared worktree also contains unrelated pre-existing changes.
- `git diff --stat` summary: reviewed implementation replaces application waiters with three direct QA calls, returns three typed service responses, and adds a small callback future drop guard.
- `git diff --name-status` summary: production/build paths match the admission stamp; testing paths match the sibling testing scope.
- `git diff --check` result: passed for the reviewed tracked production/test paths.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| Report/Call/Query use command QA without SN header-sequence state | approved proposal P-SN-CLIENT-QA-1 and design Overall Approach | three `send_by_specify_tunnel_with_resp` calls; service handlers return `Some(CmdBody)`; no legacy response handlers | new/new workflow and targeted migration search; sibling artifact `test-results/test-runs/20260710T141828Z-p2p-frame+sn-client-qa-correlation-fix-all.json` | implemented |
| Report A late cannot complete B or pollute endpoints | proposal High-Level Outcomes and design Report QA/concurrency | QA runtime owns `(tunnel, command, header seq)` callback; client accepts endpoint only after body validation | delayed A overlaps pending B; active list contains only B endpoint | implemented |
| Response business identity is verified | proposal Scope and design body validation | Report checks seq/SN id, Call checks seq/SN id, Query checks seq | wrong fields and malformed bodies for all three flows are rejected without active-state pollution | implemented |
| SN-owned waiter state and completion handlers are removed | proposal P-SN-CLIENT-QA-2 and design state ownership | `cur_report_future`, `recv_future`, `SnResp`, and three completion handlers absent | targeted source search plus compile and all QA cases | implemented |
| QA callback lifecycle cleans complete/timeout/cancel/drop | design QA cancellation cleanup and Data and State | vendored `ResultFuture` owns private drop cleanup; ready path disarms after normal removal | normal, timeout, late result, and 4096 unique cancellation retention tests pass | implemented |
| Exit paths do not corrupt active state | proposal Success Criteria and design failure transitions | timeout keeps healthy active SN; non-timeout command failure removes stale connection; mismatches never return a result | Report/Call/Query timeout, closed tunnel, malformed body, mismatch and late-response cases pass | implemented |
| SnCalled remains standalone asynchronous behavior | proposal non-goal and design invariant | only SnCalled/SnCalledResp handlers/sends remain standalone | new/new workflow receives call response and independent SnCalled | implemented |
| Migration is coordinated with no legacy fallback | proposal P-SN-CLIENT-QA-3 and design mixed-version decision | response enum/body codecs remain, but new client/service neither emit nor handle legacy response commands | new/new passes; testing records old/new and new/old as unsupported manual matrix with owner/risk/impact | implemented |
| Exported waiter-state removals are declared source breaks | revised design Interfaces and Dependencies | fields/type are removed; no workspace consumers found | workspace/all-target compilation passes; out-of-tree migration remains documented manual risk | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| Three QA transactions and body trust / `sn_client_qa_transactions` | normal / boundary / negative / error / lifecycle / cross-module | testing Direct Change, Case-Type, Unit/DV/Integration rows | new/new, mismatch/malformed, timeout and closed-tunnel steps in sibling artifact | adequate |
| Pending cleanup and late correlation / `sn_client_qa_pending_cleanup` | boundary / negative / error / lifecycle / concurrency / cross-module | callback state transitions, Report A/B and exit-path tables | three callback cases, delayed A/B, Call/Query timeout and command-tunnel failure | adequate |
| Coordinated framing / `sn_client_qa_migration_boundary` | normal / negative / compatibility / lifecycle / cross-module | explicit compatibility/manual rows and trigger coverage | new/new automated; old/new and new/old intentionally unsupported with release-engineering owner/risk/acceptance impact | adequate |
| Changed-code branch depth | unit branches plus explicit gaps | testing Unit Tests records each executed branch and the remaining encode/half-frame/multiple-SN reasons | constructible response decode, timeout and send failure branches execute after acceptance return | adequate |
| Whole module and neighbor contracts | DV main/failure/config plus integration success/failure | testing DV/Integration tables and testplan | loopback QUIC/TCP Report fallback, public Call/Query errors, dependency compile, SnCalled | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| A-SN-QA-1 | proposal/design/code | Report/Call/Query complete only through the matching command QA transaction; SN code owns no QA header key | source search, client/service code, new/new test | pass |
| A-SN-QA-2 | proposal/design/tests | Report A timeout followed by B cannot consume A endpoint or identity | delayed overlap regression with exact active endpoint assertion | pass |
| A-SN-QA-3 | proposal/design/tests | wrong business seq/SN identity and malformed bodies never produce accepted results or active-state pollution | malicious handler tests for all three responses | pass |
| A-SN-QA-4 | proposal/design/dependency/tests | completion, timeout, cancellation, tunnel failure and late result leave no reusable/unbounded pending state | callback lifecycle/allocation tests plus timeout/closed-tunnel tests | pass |
| A-SN-QA-5 | proposal/design/tests | SnCalled semantics and Report endpoint/fallback behavior remain intact | new/new SnCalled workflow and QUIC/TCP Report fallback | pass |
| A-SN-QA-6 | proposal/design/testing | no silent legacy fallback or mixed-version compatibility claim; source breaks have migration notes | migration search, interface table and manual compatibility rows | pass |
| A-SN-QA-7 | harness rules | approvals, admission, stage scopes, testing coverage, quality, root shortcut and fresh artifacts pass | hashes, stamp, checker results and machine artifacts | pass |

## Inputs
- `proposal.md`
- `design.md`
- test implementation and optional `testing.md`
- `testplan.yaml` for completed testing work, or a versioned local exception with reason, owner, risk, and acceptance impact
- optional `acceptance.md`
- long-lived module doc
- implementation
- test code
- test results
- optional git diff/status evidence
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Review approved requirements and acceptance boundaries
2. Review design against proposal
3. Generate or finalize acceptance rules and expected results from proposal, design, implementation, and test implementation
4. Review implementation against proposal and design
5. Review whether test design reasonably covers proposal/design/code behavior, including normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases where applicable
6. Review tests and results against generated test evidence, optional `testing.md`, and required completed-testing `testplan.yaml` or its versioned exception
7. Review document and implementation logic for contradictions, invalid assumptions, impossible states, and correctness defects
8. Use diff/status output only when helpful to locate evidence
9. Produce conclusion

## Consistency Summary
- Proposal authority check: the user-approved sibling proposal directly defines all three change ids, QA ownership, body validation, cleanup, SnCalled exclusion, and coordinated migration; no chat-only behavior is used as acceptance authority.
- Proposal vs design: design maps every proposal item to concrete client/service/dependency paths, failure transitions, state ownership, compatibility classification, and rollback; revised design explicitly records the exported waiter-state source break.
- Design vs testing implementation: post-implementation testing derives body domains, callback/report transitions, timeout/send/decode failures, concurrency, invariants, and compatibility from the approved design.
- Design vs long-lived boundary doc: `docs/modules/p2p-frame.md` keeps SN low-frequency commands on the Tunnel control stream; QA changes correlation/framing within that boundary and does not alter endpoint classification or other module ownership.
- Design vs implementation: direct QA calls, typed handler response bodies, validation fields, removed application waiters, preserved SnCalled, local callback patch and lock binding match design.
- Test implementation vs test code vs results: the sibling testplan reaches all changed tests; the fresh sibling artifact records eight passing steps, and the two fresh project/root artifacts include those steps.
- Test design adequacy: initial acceptance depth finding `ACCEPTANCE-TEST-DEPTH-001` returned constructible gaps to testing; Call/Query timeout, closed-tunnel send failure, and all malformed response decodes now execute. Remaining encode/half-frame/multiple-active rows name owner/risk/acceptance impact and do not hide an approved core behavior.
- change_id traceability: all three ids appear in approved proposal, design, testing, testplan, admission stamp, tests and this report.
- Acceptance criteria traceability: A-SN-QA-1 through A-SN-QA-7 map every approved outcome/non-goal to implementation and evidence.
- Cross-module admission: p2p-frame sibling admission directly includes the required `third-party/callback-result/**` technical dependency path; no other workspace business module is changed or required as acceptance authority.
- Public API / codec / runtime semantics review: public method and body-codec signatures stay unchanged; removal of publicly re-exported waiter fields/enum is an approved source break; Report/Call/Query wire framing is migration-required; SnCalled and identity/TLS semantics stay unchanged.
- Document logic review: proposal/design/testing agree that SN does not manage tunnel/header sequence but still validates business fields; no contradiction or impossible state remains.
- Implementation logic review: QA runtime key includes tunnel/command/header seq; callback drop removal is race-safe because normal Ready removes then disarms and cancellation removes the current id before field destruction; no legacy handler can compete.
- Document approval timing (approved_content_sha256 verified by schema-check): proposal user approval, revised design approval, and post-return testing approval hashes all pass schema validation.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): isolated nine-path check passed for all three ids; testing isolated four-path scope check also passed.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: a failing pre-fix Report artifact is unavailable because testing began after both sides migrated framing and reproducing it would require reconstructing an old client plus standalone-response server baseline. Testing records this concrete infeasibility. The dependency-layer retained-allocation test is a direct red-green seam against released callback behavior, and the Report A/B test asserts the exact historical pollution outcome on the delivered code.

## Required Command Evidence
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-client-qa-correlation-fix`: passed after revised proposal/design/testing approvals.
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-client-qa-correlation-fix --change-id sn_client_qa_transactions --change-id sn_client_qa_pending_cleanup --change-id sn_client_qa_migration_boundary --evidence-file harness/evidence/admission/20260710-sn-client-qa-correlation-fix.md`: passed in final acceptance and refreshed the content-bound stamp.
- `python3 ./harness/scripts/stage-scope-check.py --stage <stage>`: proposal, revised design, implementation and both testing iterations passed against isolated temporary indexes; acceptance scope is checked for this report.
- Unit test command through `harness/scripts/test-run.py`: sibling all artifact passed callback lifecycle/bounded retention and Report/Call/Query mismatch/malformed steps.
- DV test command through `harness/scripts/test-run.py`: sibling all artifact passed delayed Report A/B, Call/Query timeout and closed-tunnel steps.
- Integration test command through `harness/scripts/test-run.py`: sibling all artifact passed new/new Report/Query/Call/SnCalled and all-target compilation.
- Module all command through `harness/scripts/test-run.py <module> all`: `python3 ./harness/scripts/test-run.py p2p-frame/sn-client-qa-correlation-fix all` passed; artifact `test-results/test-runs/20260710T141828Z-p2p-frame+sn-client-qa-correlation-fix-all.json`.
- Project all command through `harness/scripts/test-run.py all all`: passed; artifact `test-results/test-runs/20260710T142242Z-all-all.json` records 46 steps and exit code 0.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): `test-results/test-runs/20260710T142903Z-all-all.json`, generated by the root shortcut, requested `all all`, 46 steps, exit code 0.
- Quality gates `python3 ./harness/scripts/quality-check.py`: passed; `harness/quality-gates.yaml` declares an explicitly empty gate list.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not applicable because no quality gates are configured.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): `./test-run.sh all all` passed and generated `test-results/test-runs/20260710T142903Z-all-all.json`.
- Acceptance report check `python3 ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-sn-client-qa-correlation-fix-acceptance-2026-07-10.md`: passed.
- Targeted migration search, when applicable: no `cur_report_future`, `recv_future`, `SnResp`, or three legacy completion handlers remain; no ReportSnResp/SnCallResp/SnQueryResp command handlers/sends remain in client/service; exactly three `send_by_specify_tunnel_with_resp` sites remain and standalone SnCalled/Resp sites are preserved.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: the three approved QA/correlation/cleanup/migration outcomes are implemented inside admitted paths, the initial test-depth finding was corrected, all constructible security/lifecycle/error/concurrency paths pass through the unified entrypoint, and no blocking document, design, implementation, or evidence inconsistency remains.
- Supporting test evidence: `test-results/test-runs/20260710T141828Z-p2p-frame+sn-client-qa-correlation-fix-all.json`, `test-results/test-runs/20260710T142242Z-all-all.json`, and root-shortcut artifact `test-results/test-runs/20260710T142903Z-all-all.json`.
- Residual risk: old/new and new/old Report/Call/Query are intentionally unsupported; out-of-tree users of removed waiter-state exports require source migration; the local callback patch must be carried until upstream release includes equivalent cleanup; structurally infallible request encoding, mid-frame reader failure, and multiple-active-SN fallback sub-branches remain source-reviewed explicit gaps.

## Follow-Up Tasks
- Requirement task: none.
- Design task: none.
- Implementation task: none.
- Testing task: none blocking; optional future fault-injection seam may cover half-frame reader and multiple-active-SN fallback branches.
- Testing return reason if coverage is incomplete: not applicable after `ACCEPTANCE-TEST-DEPTH-001`; remaining gaps are explicit, non-core and non-injectable without expanding production seams/topology.
- Iteration count: 2
- Stop reason if more than 5 unsuccessful iterations: not applicable.
