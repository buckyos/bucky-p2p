---
module: p2p-frame
submodule: sn-client-qa-correlation-fix
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-07-10T22:23:22+08:00
approved_content_sha256: 27c2930083a5a8caab57e93fd3c6d4298831265f3d2bbe38ebb0cc8701e2de64
---

# SN Client QA Correlation Fix Testing

## Test Document Index
| Document | Topic | Scope |
|----------|-------|-------|
| `testing.md` | Derived QA correlation, identity, cleanup, lifecycle, and migration cases | all three change ids |
| `testplan.yaml` | Unified unit, DV, and integration commands | sibling task entry |

## Unified Test Entry
- Machine-readable plan: `docs/versions/v0.1/modules/p2p-frame/sn-client-qa-correlation-fix/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py p2p-frame/sn-client-qa-correlation-fix unit`
- DV: `python3 ./harness/scripts/test-run.py p2p-frame/sn-client-qa-correlation-fix dv`
- Integration: `python3 ./harness/scripts/test-run.py p2p-frame/sn-client-qa-correlation-fix integration`
- Module all: `python3 ./harness/scripts/test-run.py p2p-frame/sn-client-qa-correlation-fix all`
- Project all: `python3 ./harness/scripts/test-run.py all all`
- Registration: the sibling testplan is discovered as `p2p-frame/sn-client-qa-correlation-fix`; X509 is enabled for the SN network tests.

## Submodule Tests
| Submodule | Responsibility | Detailed Test Doc | Required Behaviors | Edge/Failure Cases | Test Type | Test Files | Status | Gap / Manual Reason |
|-----------|----------------|-------------------|--------------------|--------------------|-----------|------------|--------|---------------------|
| `sn_client` | QA request, business-body validation, fallback/result state | this file | Report/Call/Query success; no SN-owned pending map | late report; wrong seq/SN id; no active-state pollution | unit + DV | `p2p-frame/src/sn/tests.rs` | ready | lower-level transport/decode branch gaps are listed per branch below |
| `sn_service` | Return typed QA bodies while retaining SnCalled | this file | new/new Report/Call/Query and SnCalled success | handler validation failure remains non-success | DV | `p2p-frame/src/sn/tests.rs`, existing service unit tests | ready | mixed old/new processes are unsupported and not spawned |
| `cmd_qa_runtime` | Header correlation and callback cleanup | this file | complete, timeout, cancel/drop remove callback ids | 4096 unique drops remain bounded; late set_result is rejected | unit | `third-party/callback-result/tests/drop_cleanup.rs` | ready | private map is verified by retained-allocation bound plus public late-result behavior |

## Module-Level Tests
| Test Item | Covered Boundary | Entry | Expected Result | Test Type | Test File/Script | Status | Gap / Manual Reason |
|-----------|------------------|-------|-----------------|-----------|------------------|--------|---------------------|
| Callback lifecycle | command QA waiter dependency | `qa-callback-drop-cleanup` | complete/timeout/drop paths remove live waiter; unique cancellations do not grow retained state | unit | `third-party/callback-result/tests/drop_cleanup.rs` | ready | not-applicable: automated |
| Report A/B regression | two protocol tunnels and one SN client | `qa-report-late-correlation` | A times out, A late arrives while B is pending, only B endpoint becomes active | DV | `p2p-frame/src/sn/tests.rs` | ready | pre-fix run artifact unavailable; reason in Regression Focus |
| Body identity/codec rejection | SN client against malicious response handlers | `qa-body-mismatch` | wrong report seq/SN id, call SN id, query seq, and malformed bodies are rejected without active-SN pollution | unit | `p2p-frame/src/sn/tests.rs` | ready | loopback command runtime is real; malicious handlers are controlled test doubles |
| Call/Query exit paths | active SN clients and controlled timeout/closed-tunnel conditions | `qa-call-query-timeout`, `qa-call-query-closed-tunnel` | timeout preserves healthy active SN; closed tunnel fails and removes stale active SN | DV | `p2p-frame/src/sn/tests.rs` | ready | half-frame reader failure remains explicitly non-injectable below |
| New/new QA workflow | SN service and two clients | `qa-new-new-e2e` | Report online, Query, Call, and asynchronous SnCalled all succeed | integration | `p2p-frame/src/sn/tests.rs` | ready | not-applicable: automated |

## External Interface Tests
| Interface | Responsibility | Success Cases | Failure/Edge Cases | Test Type | Test Doc/File | Status | Gap / Manual Reason |
|-----------|----------------|---------------|--------------------|-----------|---------------|--------|---------------------|
| `SNClientService::call` | Return typed call response from active SN | new/new call and NotFound response | wrong response SN id returns `ConnectFailed` and leaves active SN intact | integration | `p2p-frame/src/sn/tests.rs` | ready | send/decode failure branches recorded below |
| `SNClientService::query` | Return typed query result | existing/missing peer queries | wrong response seq returns `ConnectFailed` and leaves active SN intact | integration | `p2p-frame/src/sn/tests.rs` | ready | send/decode failure branches recorded below |
| SN report loop | Register endpoints for matching SN only | normal online and late-A/B regression | wrong seq or SN id never creates active SN | DV | `p2p-frame/src/sn/tests.rs` | ready | not-applicable: automated |
| `callback_result::ResultFuture` | Await or cancel command callback | normal completion and timeout | dropped unique futures remain bounded; late result is `NoWaiter` | unit | `third-party/callback-result/tests/drop_cleanup.rs` | ready | not-applicable: automated |
| removed waiter-state exports | Source migration boundary | workspace compiles without the fields/enum | out-of-tree source users must migrate; no legacy alias exists | integration | Cargo compile plus source audit | manual | owner: downstream caller; reason: no out-of-tree consumer fixture; risk: declared source break; acceptance impact: must remain documented, not a blocker for coordinated release |

## Direct Change Coverage
| change_id | design_source | validation_id | testplan_level | testplan_step_id | Gap? | Gap / Manual Reason |
|-----------|---------------|---------------|----------------|------------------|------|---------------------|
| sn_client_qa_transactions | `design.md` Overall Approach, Key Call Flows, and body validation | VAL-QA-NEW-NEW | integration | qa-new-new-e2e | no | main step covers all real QA flows; body mismatch steps provide additional negative evidence |
| sn_client_qa_pending_cleanup | `design.md` QA cancellation cleanup, Data and State, and Report concurrency | VAL-QA-DROP-CLEANUP | unit | qa-callback-drop-cleanup | no | callback lifecycle is the direct step; late-report DV adds transaction-ordering evidence |
| sn_client_qa_migration_boundary | `design.md` mixed-version matrix | VAL-QA-FRAMING | integration | qa-new-new-e2e | yes | new/new is automated; owner: release engineering; old/new and new/old binaries are unavailable and intentionally unsupported; risk: coordinated deployment required; acceptance impact: accept only with explicit no-compatibility claim |

## Case-Type Coverage
| change_id | case_type | required | validation_id | level | status | gap_manual_reason |
|-----------|-----------|----------|---------------|-------|--------|-------------------|
| sn_client_qa_transactions | normal | yes | VAL-QA-NEW-NEW | integration | covered | three QA flows plus SnCalled succeed |
| sn_client_qa_transactions | boundary | yes | VAL-QA-BODY-MISMATCH | unit | covered | business seq and SN identity equality boundaries are exercised |
| sn_client_qa_transactions | negative | yes | VAL-QA-BODY-MISMATCH | unit | covered | wrong report seq/SN id, call SN id, and query seq are denied |
| sn_client_qa_transactions | error | yes | VAL-QA-REPORT-LATE | dv | covered | Report timeout progresses safely; Call/Query timeout and closed-tunnel failures are additional DV steps |
| sn_client_qa_transactions | compatibility | yes | VAL-QA-FRAMING | integration | manual | owner: release engineering; old/new binaries unavailable and combinations are explicitly unsupported; risk: mixed deployment failure; acceptance impact: coordinated upgrade note required |
| sn_client_qa_transactions | lifecycle | yes | VAL-QA-REPORT-LATE | dv | covered | pending A times out, is removed, late response is ignored, B completes |
| sn_client_qa_transactions | cross-module | yes | VAL-QA-NEW-NEW | integration | covered | p2p-frame SN code consumes sfo-cmd-server QA and callback-result |
| sn_client_qa_pending_cleanup | normal | yes | VAL-QA-DROP-CLEANUP | unit | covered | normal callback result is delivered then removed |
| sn_client_qa_pending_cleanup | boundary | yes | VAL-QA-DROP-CLEANUP | unit | covered | 4096 unique immediate cancellations retain less than 64 KiB |
| sn_client_qa_pending_cleanup | negative | yes | VAL-QA-DROP-CLEANUP | unit | covered | result after timeout/drop returns `NoWaiter` |
| sn_client_qa_pending_cleanup | error | yes | VAL-QA-DROP-CLEANUP | unit | covered | timeout and drop paths are distinct and covered |
| sn_client_qa_pending_cleanup | compatibility | yes | VAL-QA-DROP-CLEANUP | integration | covered | callback-result public type/version/API remain usable by sfo-cmd-server compile |
| sn_client_qa_pending_cleanup | lifecycle | yes | VAL-QA-DROP-CLEANUP | unit | covered | inserted -> completed/timed-out/canceled -> removed transitions execute |
| sn_client_qa_pending_cleanup | cross-module | yes | VAL-QA-REPORT-LATE | dv | covered | dependency cleanup permits next p2p-frame report operation |
| sn_client_qa_migration_boundary | normal | yes | VAL-QA-FRAMING | integration | covered | new client/new service succeeds |
| sn_client_qa_migration_boundary | boundary | yes | VAL-QA-REPORT-LATE | dv | covered | QA response framing separates adjacent transactions across tunnels |
| sn_client_qa_migration_boundary | negative | yes | VAL-QA-BODY-MISMATCH | unit | covered | malformed business identity is not accepted despite valid QA framing |
| sn_client_qa_migration_boundary | error | yes | VAL-QA-REPORT-LATE | dv | covered | timed-out old transaction does not corrupt the next one |
| sn_client_qa_migration_boundary | compatibility | yes | VAL-QA-FRAMING | integration | manual | owner: release engineering; no capability negotiation or old binaries; risk: mixed deployment fails; acceptance impact: no mixed-version compatibility claim |
| sn_client_qa_migration_boundary | lifecycle | yes | VAL-QA-REPORT-LATE | dv | covered | coordinated new/new client lifecycle reaches active state after timeout fallback |
| sn_client_qa_migration_boundary | cross-module | yes | VAL-QA-NEW-NEW | integration | covered | client/service framing and command runtime agree on request-command QA responses |

## Design Element Coverage
| element_type | design_source | derived_cases | level | status | gap_manual_reason |
|--------------|---------------|---------------|-------|--------|-------------------|
| parameter-domain | `design.md` Interfaces and Dependencies | Report correct/wrong seq and SN id; Call wrong SN id; Query wrong seq; known/missing query and call peer | unit | covered | malformed raw body is a per-branch gap below |
| state-transition | `design.md` Data and State | callback inserted -> completed/timeout/drop -> removed; report inactive -> timeout -> valid active | unit | covered | callback lifecycle is placed at the lowest level; report transition is additionally exercised by DV |
| failure-path | `design.md` Key Call Flows | Report timeout/late, body mismatch/malformed, Call/Query timeout, closed tunnel, mismatch/malformed | dv | covered | local encode and half-frame read injection gaps are recorded below |
| error-handling | changed code `ConnectFailed`, `InvalidData`, raw/read mappings | public mismatch, malformed decode, timeout/fallback, and closed-tunnel errors | unit | covered | response decode is unit-registered; timeout/send failures are additionally DV-tested; only non-injectable encode/partial-read branches remain explicit below |
| invariant | `design.md` Invariants to Preserve | body codecs compile unchanged; validated endpoint only; SnCalled remains async; no legacy response handlers | integration | covered | source audit complements the network cases |
| concurrency | `design.md` per-tunnel serialization and Report A/B ordering | delayed A arrives while B is pending on the next tunnel; 4096 cancellations | dv | covered | same-tunnel second request is serialized by dependency as designed; cancellation bound is additionally unit-tested |

## Validation Rationale
| Behavior or Risk | Validation Signal | Why This Is Sufficient | Gap / Manual Reason |
|------------------|-------------------|------------------------|---------------------|
| Report A endpoint cannot become B endpoint | test asserts delayed A endpoint is absent and B endpoint is the sole active endpoint | Recreates the state-pollution outcome with A late during B pending | pre-fix artifact unavailable as recorded below |
| QA body cannot lie or decode as another request/SN | malicious handlers return wrong identity and malformed QA bodies and public APIs reject them | Separates transport correlation from business validation and codec failure | partial body-reader I/O failure remains non-injectable |
| Timeout and command-tunnel failure cleanly exit | delayed Call/Query responses preserve healthy active SNs; cleared tunnels make both methods fail and remove stale active SNs | Exercises timeout versus non-timeout failure branches and their distinct connection-state rules | request encode and mid-frame read faults remain explicit gaps |
| Canceled QA waiters cannot grow without bound | counting allocator observes bounded retained bytes after 4096 unique drops of both future constructors | Old tombstone behavior retains thousands of map/notify allocations; patched path retains only small map capacity | allocator step runs single-threaded for determinism |
| SnCalled behavior remains asynchronous | full new/new call observes call response and independently receives SnCalled | Directly checks the excluded async command boundary | not-applicable: automated |
| Deployment boundary is honest | new/new passes; docs/source show no legacy response handlers or capability negotiation | Matches approved coordinated-upgrade decision | mixed binaries are manual and unsupported |

## Unit Tests
| Function or Unit | Branch or Condition | Covered Behavior | Test File | Status | Gap / Manual Reason |
|------------------|---------------------|------------------|-----------|--------|---------------------|
| `ResultFuture::poll/drop` | result ready | cleanup is disarmed after normal removal | `third-party/callback-result/tests/drop_cleanup.rs` | covered | normal completion case |
| `ResultFuture::drop` | future never polled / canceled | callback id and notify allocation are removed immediately | `third-party/callback-result/tests/drop_cleanup.rs` | covered | both timeout and non-timeout constructors in 4096 loop |
| `CallbackWaiter::create_timeout_result_future` | timeout | returns Timeout, removes id, rejects late result | `third-party/callback-result/tests/drop_cleanup.rs` | covered | not-applicable: automated |
| callback cached-result branch | cached result exists before future | unchanged dependency branch | no new case | gap | owner: upstream dependency; branch was not changed by cleanup patch; risk: none to cancellation fix; acceptance impact: existing behavior not claimed as new evidence |
| `SNClientService::report` | QA success and matching body | returns endpoints for matching SN | `p2p-frame/src/sn/tests.rs` | covered | new/new and late-B cases |
| `SNClientService::report` | body seq or SN id mismatch | returns InvalidData and active list remains empty | `p2p-frame/src/sn/tests.rs` | covered | both mismatch operands covered |
| `SNClientService::report` | timeout then next protocol candidate | A removed/ignored and B succeeds | `p2p-frame/src/sn/tests.rs` | covered | loopback real tunnel is required to reproduce ordering |
| `SNClientService::report` | malformed response decode | returns codec error and active list remains empty | `p2p-frame/src/sn/tests.rs` | covered | third report attempt returns invalid bytes |
| `SNClientService::report` | request encode or mid-frame response read failure | maps error without active insertion | no direct test | gap | owner: testing; derived request codec is structurally infallible for valid fields and network runtime does not expose a half-frame body seam; risk: error mapping only; acceptance impact: source review required |
| `SNClientService::report` | non-timeout send failure | removes failed connection | no direct test | gap | owner: testing; report candidate is not yet active and the background loop recreates missing tunnels before send; risk: stale candidate tunnel only; acceptance impact: callback cancellation test plus source review required |
| `SNClientService::call` | one active SN and matching response | returns Ok/NotFound typed body | `p2p-frame/src/sn/tests.rs` | covered | existing end-to-end cases |
| `SNClientService::call` | wrong response SN id | rejects body and preserves active SN | `p2p-frame/src/sn/tests.rs` | covered | malicious QA handler case |
| `SNClientService::call` | timeout | returns ConnectFailed and preserves healthy active SN | `p2p-frame/src/sn/tests.rs` | covered | delayed valid response exceeds call_timeout |
| `SNClientService::call` | closed tunnel / non-timeout send failure | returns ConnectFailed and removes stale active SN | `p2p-frame/src/sn/tests.rs` | covered | command client tunnels cleared before call |
| `SNClientService::call` | malformed response decode | returns ConnectFailed and preserves active SN | `p2p-frame/src/sn/tests.rs` | covered | handler returns invalid bytes |
| `SNClientService::call` | no active SN, request encode, mid-frame read, multiple-active fallback | final ConnectFailed / error mapping / next SN | partial direct coverage | gap | owner: testing; no-active follows closed-tunnel removal, while encode/half-frame/multiple-SN injection needs new seams/topology; risk: fallback diagnostics; acceptance impact: source review and do not claim those sub-branches executed |
| `SNClientService::query` | matching existing/missing response | returns typed query body | `p2p-frame/src/sn/tests.rs` | covered | existing and missing peer cases |
| `SNClientService::query` | wrong response seq | rejects body and preserves active SN | `p2p-frame/src/sn/tests.rs` | covered | malicious QA handler case |
| `SNClientService::query` | timeout | returns ConnectFailed and preserves healthy active SN | `p2p-frame/src/sn/tests.rs` | covered | delayed valid response exceeds call_timeout |
| `SNClientService::query` | closed tunnel / non-timeout send failure | returns ConnectFailed and removes stale active SN | `p2p-frame/src/sn/tests.rs` | covered | command client tunnels cleared before query |
| `SNClientService::query` | malformed response decode | returns ConnectFailed and preserves active SN | `p2p-frame/src/sn/tests.rs` | covered | handler returns invalid bytes |
| `SNClientService::query` | no active SN, request encode, mid-frame read, multiple-active fallback | final ConnectFailed / error mapping / next SN | partial direct coverage | gap | owner: testing; no-active follows closed-tunnel removal, while encode/half-frame/multiple-SN injection needs new seams/topology; risk: fallback diagnostics; acceptance impact: source review and do not claim those sub-branches executed |
| service QA handlers | business handler success | encode and return `Some(CmdBody)` | `p2p-frame/src/sn/tests.rs` | covered | new/new Report/Call/Query workflow |
| service QA handlers | decode/business/encode failure | command handler returns error/no success body | existing report validator tests plus compile/source | gap | owner: testing; validator failure is covered, malformed raw and encode failure are not injectable; risk: timeout diagnostics; acceptance impact: source review required |

## DV Tests
| Workflow | Kind | Entry | Expected Result | Test File or Script | Status | Gap / Manual Reason |
|----------|------|-------|-----------------|---------------------|--------|---------------------|
| Report QA late-response fallback | main | `qa-report-late-correlation` | A timeout, A late during B, B endpoint only | `p2p-frame/src/sn/tests.rs` | covered | not-applicable: automated |
| Report body trust failure | failure | `qa-body-mismatch` | wrong seq/SN never activates SN | `p2p-frame/src/sn/tests.rs` | covered | not-applicable: automated |
| Call/Query body trust failure | failure | `qa-body-mismatch` | public methods fail and active SN remains correct | `p2p-frame/src/sn/tests.rs` | covered | not-applicable: automated |
| Call/Query timeout | failure | `qa-call-query-timeout` | both requests time out, late bodies cannot complete later requests, active SN remains healthy | `p2p-frame/src/sn/tests.rs` | covered | not-applicable: automated |
| Call/Query closed tunnel | failure | `qa-call-query-closed-tunnel` | both requests fail non-timeout and stale active SN entries are removed | `p2p-frame/src/sn/tests.rs` | covered | not-applicable: automated |
| New/new SN module workflow | main | `qa-new-new-e2e` | online Report, Query, Call, and SnCalled pass | `p2p-frame/src/sn/tests.rs` | covered | also integration evidence because it crosses command dependency boundary |
| QUIC/TCP configuration variants | config | `qa-report-late-correlation` | candidate fallback uses two protocol tunnels without endpoint pollution | `p2p-frame/src/sn/tests.rs` | covered | not-applicable: automated |
| explicit shutdown/restart | lifecycle | no entry | no startup/shutdown implementation changed | none | not-applicable | design Data and State scopes transaction lifetime, not server lifecycle; transaction cancellation lifecycle is covered by callback and late-report cases |
| persisted recovery | persistence | no entry | p2p-frame SN QA has no persisted state | none | not-applicable | design Data and State lists only in-memory callback/business/active state |

## Integration Tests
| Contract or Flow | Modules Involved | Success Case | Failure Case | Test File | Status | Gap / Manual Reason |
|------------------|------------------|--------------|--------------|-----------|--------|---------------------|
| Report/Call/Query QA framing | p2p-frame client/service, sfo-cmd-server | new/new full workflow | mismatched body and delayed prior response rejected | `p2p-frame/src/sn/tests.rs` | covered | network test duplicates lower cases because header/tunnel correlation only exists end-to-end |
| `SNClientService::call/query` exported results | p2p-frame stack consumers and SN service | Ok/NotFound/query details | ConnectFailed on wrong identity without active-state removal | `p2p-frame/src/sn/tests.rs` | covered | direct transport send/decode failures remain explicit unit gaps |
| callback-result patch consumed by command runtime | callback-result, sfo-cmd-server, p2p-frame | workspace compile and QA tests | timeout/drop cleanup rejects late result | callback unit plus Cargo integration step | covered | not-applicable: automated |
| mixed-version framing | old/new p2p-frame endpoints | new/new passes | old/new and new/old intentionally unsupported | no old binary fixture | manual | owner: release engineering; reason: repository has no versioned old binary fixture; risk: request timeout in mixed deployment; acceptance impact: coordinated deployment is mandatory |
| removed public waiter-state fields/enum | out-of-tree Rust callers | workspace callers compile | old source import/field access would fail | source audit | manual | owner: downstream caller; no out-of-tree fixture; risk and migration are explicitly approved; acceptance impact: source-breaking release note required |

## Trigger Coverage
| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | QA request-command response framing and removed public waiter exports | positive/negative contract, compatibility, caller/callee review | new/new, body mismatch, compile/source review | owner: release engineering; old/new binaries absent; mixed operation unsupported; acceptance impact: coordinated release only | mixed deployment times out |
| data/schema | no | approved design preserves all Report/Call/Query body codecs and no persistence exists | source/compile review | protocol files unchanged; compile passes | none | none |
| security/privacy/permission | yes | response seq/SN id is an identity trust boundary | denied identity and malformed cases, logging review | all required mismatched body fields and malformed bodies rejected; logs include ids only | none | none |
| runtime/integration | yes | timeout, cancellation, ordering, fallback and resource lifetime changed | failure/timeout/concurrency/DV | late A/B, Report/Call/Query timeout, closed tunnel, malformed body, timeout/drop, bounded retention, new/new | encode/half-frame/multiple-active injection deferred; owner: testing; risk: error mapping/fallback only; acceptance impact: explicit per-branch gaps | rare error branches source-reviewed only |
| build/dependency/config/deployment | yes | root patch and lock bind local callback-result | reproducible compile, dependency risk, rollback | callback tests, p2p-frame compile, lock/source review | old/new deployment deferred; owner: release engineering; acceptance impact: coordinated deployment | local patch must be carried until upstream fix |
| ui/datamodel/workflow | no | no UI paths or presentation contracts in admitted paths | confirm no UI diff | scope/source review | none | none |
| harness/process | no | task consumes but does not change harness rules/scripts/schema | run existing checks | doc/schema/coverage/scope/unified commands | none | none |

## Regression Focus
- The original failure is represented by `sn_report_late_response_does_not_complete_next_tunnel_report`: Report A exceeds `call_timeout`, Report B starts on the next protocol tunnel, A arrives while B is pending, and the active endpoint must be B's endpoint only.
- A failing pre-fix artifact is unavailable because post-implementation testing began after both client and server wire framing had already migrated; applying the new QA test to only the old client would require reconstructing the old standalone server response handler and a second baseline workspace. The current regression test is structurally red if QA correlation or the report body checks are removed, and this concrete infeasibility is retained for acceptance rather than inventing a pre-fix run.
- Call and Query use the same QA runtime but retain separate body checks; the malicious-handler test covers Call SN identity and Query sequence independently.
- The callback allocation test is the dependency-layer red-green seam: the released implementation retains thousands of canceled map/notify entries, while the patched implementation remains below the bounded retained-allocation threshold.

Execution evidence:
- Post-return artifact `test-results/test-runs/20260710T141828Z-p2p-frame+sn-client-qa-correlation-fix-all.json` records all eight sibling steps with exit code 0, including the added timeout and closed-tunnel cases.
- Post-return artifact `test-results/test-runs/20260710T142242Z-all-all.json` records the required project-wide `all all` run with exit code 0 and includes all sibling QA steps.

## Definition of Done
- [x] Testing docs cover all three designed submodules.
- [x] `testplan.yaml` matches unit, DV, and integration surfaces.
- [x] Generated tests are reachable through the sibling unified entry.
- [x] All changed branches are covered or listed with per-branch gap reasons.
- [x] DV covers main, failure, configuration, and transaction lifecycle behaviors; non-applicable server restart/persistence are justified.
- [x] Integration covers consumed QA/public interfaces or records explicit mixed-version/out-of-tree gaps.
- [x] Every design element and case type maps to a case or concrete owner/risk/acceptance-impact gap.
- [x] All three change ids map to validation ids and testplan steps.
- [x] Required unified commands and machine-written artifacts pass after acceptance return `ACCEPTANCE-TEST-DEPTH-001`.

## Approval Record
- approver:
- approval_date:
- user_statement: ""
