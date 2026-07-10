# Pipeline Plan: Listenerless QUIC Client Endpoint

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- User launch confirmed: confirmed
- User launch statement: `确认，自动处理后续步骤`
- Per-stage user confirmation: not required
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): p2p-frame
- change_id values: sn_client_protocol_priority

## Acceptance Baseline
- Final acceptance uses approved proposal item `P-SN-CLIENT-PROTOCOL-PRIORITY-1` as authority.
- A local QUIC listener is optional for outbound SN command-tunnel establishment when the QUIC network/runtime is available.
- Listener-backed QUIC endpoints remain preferred and preserve their local endpoint and same-source UDP behavior.
- Without a matching listener, `QuicTunnelNetwork` owns and reuses an outbound-only client endpoint bound to an OS-assigned ephemeral port; it is not exposed as a listener and is not closed by `close_all_listener()`.
- QUIC failure still reaches the existing TCP fallback, while QUIC report success stops fallback and active-SN insertion remains deduplicated.
- No SN/QUIC wire, TLS identity, TTP target matching, maintained-target, incoming-listener, or UDP-punch contract changes.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-QUIC-CLIENT-ENDPOINT-1 | proposal | confirmed | Preserve the approved listener-optional QUIC-first/TCP-fallback outcome and non-goals | root p2p-frame `proposal.md` item `P-SN-CLIENT-PROTOCOL-PRIORITY-1` | root | user approval | approved proposal baseline | direct proposal mapping remains approved and content-bound |
| D-QUIC-CLIENT-ENDPOINT-1 | design | confirmed | Define endpoint ownership, IPv4/IPv6 selection, listener preference, lifecycle, errors, local metadata, Scope Paths, and regression seams | root p2p-frame `design.md` change `sn_client_protocol_priority` | root | P-QUIC-CLIENT-ENDPOINT-1 | approved design | design structure/scope/schema checks pass; user approval hash recorded |
| I-QUIC-CLIENT-ENDPOINT-1 | implementation | complete | Add the minimum network-owned listenerless QUIC client endpoint path without changing public traits or listener behavior | admitted production changes in `p2p-frame/src/networks/quic/network.rs` plus current admission evidence | root | D-QUIC-CLIENT-ENDPOINT-1 | production implementation | schema/admission pass before edits; focused compile and implementation scope checks pass |
| T-QUIC-CLIENT-ENDPOINT-1 | testing | complete | Replace the obsolete no-listener `NotFound` expectation and add real listenerless success/lifecycle regression coverage derived from delivered code | p2p-frame test code, testing artifacts/testplan, unified runner evidence | root | I-QUIC-CLIENT-ENDPOINT-1 | runnable regression tests and artifacts | red-green record exists; testing structure/coverage/scope checks and canonical p2p-frame run pass |
| A-QUIC-CLIENT-ENDPOINT-1 | acceptance | complete | Audit proposal/design/code/testing consistency, endpoint resource lifetime, concurrency, error propagation, listener isolation, and fallback boundaries | `docs/versions/v0.1/reviews/` | root | T-QUIC-CLIENT-ENDPOINT-1 | acceptance report | quality/current test evidence and acceptance-report checks pass with no blocking findings |

Merged implementation reason: the production correction is one file-level QUIC network responsibility. Endpoint cache ownership, listener selection, connection creation, and tunnel metadata must change atomically; splitting them would create an intermediate path that either still returns `NotFound` or drops the endpoint backing live connections.

## Design Mapping
| change_id | proposal_id | owner | design_coverage | scope_paths | interface_consumer | compatibility | failure_handling | state_owner | rejected_alternative |
|-----------|-------------|-------|-----------------|-------------|--------------------|---------------|------------------|-------------|----------------------|
| sn_client_protocol_priority | P-SN-CLIENT-PROTOCOL-PRIORITY-1 | `networks/quic` | Prefer matching listener endpoints; otherwise lazily create/reuse one outbound-only Quinn endpoint per IP version on wildcard port 0, reuse existing per-connection TLS/transport configuration, derive tunnel local metadata from Quinn local IP plus assigned port, keep client endpoints out of listener/incoming state, and retain them across `close_all_listener()` | `p2p-frame/src/networks/quic/network.rs` | `TtpClient` and SN command-tunnel active open through existing `TunnelNetwork::create_tunnel(...)` | backward-compatible | bind/create failure is returned and not cached; connect/handshake errors are returned to existing SN fallback; listener path remains first; listenerless path does not claim same-source UDP punch | `QuicTunnelNetwork` owns IPv4/IPv6 client endpoints until network drop | requiring a listener violates proposal; per-connect endpoint drop risks live connections; fake listeners corrupt listener/incoming semantics |

## File-Level Implementation Sequence
| sequence | task_id | file | responsibility | depends_on | status |
|----------|---------|------|----------------|------------|--------|
| 1 | I-QUIC-CLIENT-ENDPOINT-1 | `p2p-frame/src/networks/quic/network.rs` | Add IP-family client endpoint storage/creation, listenerless connect flow, local endpoint derivation, and preserve listener-first/close semantics | D-QUIC-CLIENT-ENDPOINT-1 | complete |

## Testing Coverage Plan
| change_id | level | behavior | planned_location | dependency | status |
|-----------|-------|----------|------------------|------------|--------|
| sn_client_protocol_priority | regression | Pre-fix no-listener active open returns `NotFound`; post-fix a client with no listener establishes a real QUIC tunnel to a listening server | p2p-frame QUIC tests | I-QUIC-CLIENT-ENDPOINT-1 | covered |
| sn_client_protocol_priority | unit/integration | Listenerless endpoint uses a nonzero ephemeral port, stays absent from `listener_infos()`, and remains usable after `close_all_listener()` | p2p-frame QUIC tests | I-QUIC-CLIENT-ENDPOINT-1 | covered |
| sn_client_protocol_priority | compatibility | Existing listener-backed active open still uses the matching listener endpoint and incoming acceptance remains unchanged | existing plus focused p2p-frame QUIC tests | I-QUIC-CLIENT-ENDPOINT-1 | covered |
| sn_client_protocol_priority | error | Unreachable remote returns a concrete connect error rather than listener `NotFound`, allowing SN TCP fallback | focused p2p-frame QUIC test | I-QUIC-CLIENT-ENDPOINT-1 | covered |

## Return Routing
- Proposal issue: stop and ask the user only if listener-optional outbound support or fallback/non-goal boundaries are ambiguous.
- Design issue: return to design for missing endpoint ownership, runtime, IP-family, concurrency, local metadata, error, or Scope Paths coverage.
- Implementation issue: return to implementation for endpoint drop/leak, duplicate lazy endpoints, listener-selection regression, hidden incoming listener behavior, or incorrect error propagation.
- Testing issue: return to testing for stale `NotFound` expectations, missing real connection/lifecycle coverage, absent red-green evidence, or unreachable unified entrypoints.
- Acceptance issue: route each non-requirement finding to its owning stage and record the iteration below.

## Exit Condition
- [x] proposal approved
- [x] design approved
- [x] implementation admission passed
- [x] implementation completed
- [x] testing completed
- [x] acceptance completed

## Evidence
- User approval and launch statement: `确认，自动处理后续步骤`
- Design approval hash: `2e4928976d5cd8fd3e899730fcffd54a415f46ca9ea7b7afddadc2e4103ca43f`.
- Design document structure and isolated one-path stage scope checks passed before approval.
- Implementation admission passed for `sn_client_protocol_priority`; stamp `harness/evidence/admission/20260710-listenerless-quic-client-endpoint.p2p-frame.stamp.json` binds the approved proposal/design hashes and QUIC network Scope Paths.
- `cargo check -p p2p-frame --lib` passed after the production change.
- Implementation stage scope passed against an isolated one-path worktree containing only `p2p-frame/src/networks/quic/network.rs`; unrelated shared-worktree changes were excluded without modifying the user's index.
- Pre-fix red run failed on the old `no listener found` `NotFound` branch as intended; artifact `test-results/test-runs/20260710T160009Z-p2p-frame+listenerless-quic-client-endpoint-red-unit.json`.
- Refreshed testing approval hash after the acceptance return: `86c176c61e27fe3ee6c0d0331c9d66d5744315be4b6f7b91e5e1375e07e44822`.
- Testing document structure, schema, direct change coverage, isolated two-path p2p-frame testing scope, refreshed one-path implementation scope, and isolated workspace-harness testplan scope checks passed.
- `python3 ./harness/scripts/test-run.py p2p-frame all` passed; artifact `test-results/test-runs/20260710T161547Z-p2p-frame-all.json` includes the two listenerless QUIC targeted tests and workspace integration.
- Return `TESTING-STALE-FIXTURE-001` was resolved by creating current evidence `harness/evidence/admission/20260711-test-run-p2p-frame-admission.md` and switching only the workspace-harness admission fixture path; `python3 ./harness/scripts/test-run.py workspace-harness all` passed with artifact `test-results/test-runs/20260710T162431Z-workspace-harness-all.json`.
- `python3 ./harness/scripts/test-run.py all all` passed after the fixture refresh; artifact `test-results/test-runs/20260710T162857Z-all-all.json`.
- Return `ACCEPTANCE-LISTENER-LOCAL-EP-001` was resolved by limiting routed-local-IP refinement to the listenerless client endpoint, retaining `listener.bound_local()` on the existing listener path, and adding an exact listener-backed tunnel metadata assertion; focused compile/tests plus refreshed isolated implementation/testing scopes passed.
- Refreshed post-return `python3 ./harness/scripts/test-run.py p2p-frame all` passed; artifact `test-results/test-runs/20260710T163817Z-p2p-frame-all.json` includes the listenerless target and listener-backed compatibility assertion.
- `python3 ./harness/scripts/quality-check.py` passed with an explicitly empty configured gate list.
- Final root shortcut `./test-run.sh all all` passed and wrote `test-results/test-runs/20260710T164336Z-all-all.json` with requested module/level `all all` and exit code 0.
- Acceptance report `docs/versions/v0.1/reviews/p2p-frame-listenerless-quic-client-endpoint-acceptance-2026-07-11.md` concluded accepted on iteration 2; acceptance report and isolated one-path acceptance scope checks passed.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
| TESTING-STALE-FIXTURE-001 | testing | T-QUIC-CLIENT-ENDPOINT-1 | The first project-wide run passed business tests but failed when workspace-harness replayed a July 4 admission fixture bound to the pre-amendment root design hash. | Generate a new date-valid fixture bound to the current approved design, update only the workspace-harness testplan fixture path, rerun its focused entry, then obtain a passing project-wide artifact. |
| ACCEPTANCE-LISTENER-LOCAL-EP-001 | implementation | I-QUIC-CLIENT-ENDPOINT-1 | Acceptance logic review found the shared tunnel-finalization helper applied Quinn's routed local IP to both the new client endpoint and the existing listener-backed path, which could violate the approved requirement to preserve listener-derived `local_ep`. | Apply routed-local-IP refinement only in the listenerless client-endpoint path, keep listener-backed finalization on `listener.bound_local()`, and rerun focused compile/test plus implementation scope evidence. |

## Prior Completed Plan Archive

# Pipeline Plan: SN Client QA Correlation Fix

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/sn-client-qa-correlation-fix/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "确认，自动处理后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): p2p-frame/sn-client-qa-correlation-fix
- change_id values: sn_client_qa_transactions, sn_client_qa_pending_cleanup, sn_client_qa_migration_boundary

## Acceptance Baseline
- Final acceptance uses the approved sibling proposal as authority.
- `ReportSn`, `SnCall`, and `SnQuery` use command QA request/response; SN client/service do not manage tunnel/header sequence or an application-level pending map.
- Report/Call validate response body `seq` and `sn_peer_id`; Query validates response body `seq`.
- `cur_report_future`, `ActiveSN.recv_future`, `SnResp`, and the three standalone response completion handlers are removed without changing request/response body codecs.
- `SnCalled` / `SnCalledResp` remains the existing server-initiated asynchronous command flow.
- Mixed-version Report/Call/Query is unsupported without capability negotiation or legacy fallback; supported new/new behavior must fail safely on mismatched, late, duplicate, canceled, or timed-out responses.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-SN-CLIENT-QA-1 | proposal | confirmed | Define the three QA transaction outcomes, application waiter removal, body validation, SnCalled boundary, migration boundary, and evidence requirements | `docs/versions/v0.1/modules/p2p-frame/sn-client-qa-correlation-fix/proposal.md` | root | user approval | approved proposal | proposal structure and scope checks pass; approval hash and launch evidence recorded |
| D-SN-CLIENT-QA-1 | design | confirmed | Define client/server QA interfaces and flow, body validation, state ownership, dependency waiter cleanup, failure transitions, migration matrix, concrete Scope Paths, and file implementation order | sibling packet `design.md` and required long-lived boundary sync only | root | P-SN-CLIENT-QA-1 | approved design | design structure, schema, design scope, and pipeline-plan checks pass; auto-pipeline approval hash recorded |
| I-SN-CLIENT-QA-1 | implementation | complete | Migrate Report/Call/Query to QA, remove SN application waiters/handlers, preserve SnCalled and business semantics, and resolve admitted dependency cleanup requirements | admitted production paths plus current admission evidence | root | D-SN-CLIENT-QA-1 | production implementation | schema/admission pass before edits; implementation scope check and relevant compile checks pass |
| T-SN-CLIENT-QA-1 | testing | complete | Generate post-implementation red-green and normal/boundary/negative/error/lifecycle/concurrency/compatibility coverage for the delivered QA implementation | sibling testing artifacts, testplan, and test files | root | I-SN-CLIENT-QA-1 | runnable testing implementation and artifacts | document/testing coverage checks pass; canonical task test entry produces a passing current-state artifact |
| A-SN-CLIENT-QA-1 | acceptance | complete | Audit proposal/design/code/testing consistency, response correlation, state integrity, pending cleanup, SnCalled preservation, migration boundary, and execution evidence | sibling `acceptance-report.md` or version review report | root | T-SN-CLIENT-QA-1 | acceptance report | relevant tests and quality evidence pass; acceptance-report-check passes; no blocking findings remain |

Merged implementation reason: all production edits are inside the same `sn/client` request API and `sn/service` handler boundary, and the three transactions share the same application waiter removal and QA response mechanism. Splitting them into separately admitted implementation children would temporarily leave duplicate response handlers or incompatible client/server framing; design Scope Paths and change ids still bind each responsibility explicitly.

## Return Routing
- Proposal issue: return to proposal only if the selected three commands, SnCalled exclusion, body validation, or coordinated-upgrade boundary must change.
- Design issue: return to design for incomplete QA interface/flow, response validation, dependency waiter cleanup, failure transitions, compatibility matrix, or Scope Paths.
- Implementation issue: return to implementation for retained application waiters, legacy response races, mismatched body acceptance, altered SnCalled semantics, or state pollution.
- Testing issue: return to testing for missing report red-green evidence, incomplete Report/Call/Query branches, missing cancellation/timeout/late response coverage, or unreachable unified entrypoints.
- Acceptance issue: return to the owning stage named by each finding and record the iteration below.

## Exit Condition
- [x] proposal approved
- [x] design approved
- [x] implementation admission passed
- [x] implementation completed
- [x] testing completed
- [x] acceptance completed

## Evidence
- User approval and launch statement: `确认，自动处理后续步骤`
- Proposal approval hash: `109ca1634c7f580f05911b1ba632055e069c9c8040489a174d7060a3a211e6d7`.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule sn-client-qa-correlation-fix --docs proposal` passed.
- Proposal stage scope check passed against a temporary index containing only the sibling proposal because the shared worktree already contains unrelated changes.
- Revised design approval hash: `f40190913d9f706461d4182b7b58515cf10abb2d0ff8fb555a82789a6486436d`.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule sn-client-qa-correlation-fix --docs design` passed.
- Design stage scope check passed against a temporary index containing only the sibling design because the shared worktree already contains unrelated changes.
- Return `DESIGN-API-001` was resolved by classifying the re-exported waiter-state removal as a breaking Rust source API change and documenting the coordinated caller migration path before refreshing admission.
- Upstream inspection confirmed released `callback-result 0.2.4`, its current upstream main, and `sfo-cmd-server 0.3.4` do not provide cancellation-before-first-poll callback-id cleanup; the approved design uses an API-compatible local patch.
- Refreshed implementation admission passed for all three change ids after `DESIGN-API-001`; stamp `harness/evidence/admission/20260710-sn-client-qa-correlation-fix.p2p-frame.sn-client-qa-correlation-fix.stamp.json` binds the revised approved document hashes and exact production/build Scope Paths.
- `cargo check -p p2p-frame --lib` passed with the vendored `callback-result 0.2.4` patch and the migrated SN client/service QA paths.
- Implementation stage scope passed against an isolated temporary index containing the nine admitted production/build/evidence paths and all three change ids; unrelated shared-worktree changes were excluded without modifying the user's index.
- Return `IMPLEMENTATION-TEST-COMPILE-001` was resolved by preserving the private handler identity/tunnel inputs while returning typed QA bodies; `cargo test -p p2p-frame --lib --no-run` and the refreshed nine-path implementation scope check passed.
- Testing approval hash: `02f5c89f999d8997dcd0b0afa1c8483b8c68fcaba4f2ba624e81492eabc968dd`.
- Testing doc structure, schema, direct change coverage, and isolated four-path testing stage scope checks passed.
- `python3 ./harness/scripts/test-run.py p2p-frame/sn-client-qa-correlation-fix all` passed; artifact `test-results/test-runs/20260710T135929Z-p2p-frame+sn-client-qa-correlation-fix-all.json` records all six QA cleanup/body/late-response/new-new/compile steps.
- `python3 ./harness/scripts/test-run.py all all` passed; artifact `test-results/test-runs/20260710T140636Z-all-all.json` includes the sibling QA steps and all registered project tests.
- Return `ACCEPTANCE-TEST-DEPTH-001` was resolved with Call/Query timeout, closed-tunnel send failure, and Report/Call/Query malformed-body coverage; revised testing approval hash `27c2930083a5a8caab57e93fd3c6d4298831265f3d2bbe38ebb0cc8701e2de64`.
- Fresh post-return module artifact `test-results/test-runs/20260710T141828Z-p2p-frame+sn-client-qa-correlation-fix-all.json` records all eight sibling steps with exit code 0.
- Fresh post-return project artifact `test-results/test-runs/20260710T142242Z-all-all.json` records `all all` with exit code 0; revised testing doc/schema/coverage and isolated four-path scope checks passed.
- Final admission recheck and `quality-check.py` passed; quality gates are explicitly empty.
- `./test-run.sh all all` passed and generated root-shortcut artifact `test-results/test-runs/20260710T142903Z-all-all.json` with 46 steps and exit code 0.
- Acceptance report `docs/versions/v0.1/reviews/p2p-frame-sn-client-qa-correlation-fix-acceptance-2026-07-10.md` concluded accepted on iteration 2 after `ACCEPTANCE-TEST-DEPTH-001`; acceptance report and isolated one-path acceptance scope checks passed.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
| DESIGN-API-001 | design | D-SN-CLIENT-QA-1 | Implementation inspection confirmed `sn/client/mod.rs` publicly re-exports `ActiveSN`, `SNServiceState`, and `SnResp`; deleting their waiter fields/type is a breaking Rust source API change absent from the approved interface table. | Revised approved design names affected external callers, compatibility classification, and migration path; refreshed admission evidence/stamp. |
| IMPLEMENTATION-TEST-COMPILE-001 | implementation | I-SN-CLIENT-QA-1 | Post-implementation lib-test compilation found existing co-located report tests still call the prior private `handle_report_sn(local_id, peer_id, tunnel_id, report)` shape. | Preserve the existing private identity/tunnel inputs while returning `ReportSnResp` through QA; rerun implementation compile and scope evidence before testing resumes. |
| ACCEPTANCE-TEST-DEPTH-001 | testing | T-SN-CLIENT-QA-1 | Acceptance depth audit found proposal-required exit-path evidence still recorded constructible Call/Query timeout, closed-tunnel send failure, and malformed QA body decode branches as gaps. | Add registered tests for the constructible branches, update testing/testplan and artifacts, and retain only genuinely non-injectable encode/partial-read branches with owner/risk/acceptance impact. |

## Prior Completed Plan Archive
The previously active completed plans are preserved below because they contain pre-existing uncommitted evidence.

# Pipeline Plan: Server Runtime Ownership Fix

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/server-runtime-ownership-fix/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "批准，自动处理后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): p2p-frame/server-runtime-ownership-fix, cyfs-p2p, cyfs-p2p-test, sn-miner
- change_id values: server_runtime_required_api_alignment, process_runtime_single_owner, cyfs_config_runtime_injection, runtime_x509_test_registration, sn_miner_role_readiness_evidence

## Acceptance Baseline
- Final acceptance uses the approved sibling proposal as authority.
- `SnServiceConfig::new(..., ServerRuntime)` is the compile-time-required contract and stale missing-runtime error claims must be removed from the new evidence chain.
- Combined SN/stack startup creates one process-owned runtime and passes clones to both consumers.
- CYFS config construction receives a caller-owned runtime and does not start or panic on a hidden runtime.
- Canonical test execution must run the feature-gated runtime test, and owner/serving process tests must prove deterministic readiness rather than liveness only.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-SERVER-RUNTIME-FIX-1 | proposal | confirmed | Define the five correction outcomes, cross-module boundaries, non-goals, and evidence requirements | `docs/versions/v0.1/modules/p2p-frame/server-runtime-ownership-fix/proposal.md` | root | user approval | approved proposal | proposal structure and scope checks pass; approval hash and launch evidence recorded |
| D-SERVER-RUNTIME-FIX-1 | design | confirmed | Define compile-time API alignment, caller-supplied CYFS helper, single-runtime assembly, readiness signal/probe, and migration/error handling | sibling packet `design.md` and required long-lived boundary sync only | root | P-SERVER-RUNTIME-FIX-1 | approved design | design structure, schema, and design scope checks pass; auto-pipeline approval hash recorded |
| I-SERVER-RUNTIME-FIX-1 | implementation | complete | Implement the approved runtime plumbing and post-start readiness observability across tightly coupled caller/callee paths | admitted production paths in p2p-frame/cyfs-p2p/cyfs-p2p-test/sn-miner plus admission evidence | root | D-SERVER-RUNTIME-FIX-1 | production implementation | schema/admission pass before edits; implementation scope check and relevant compile checks pass |
| T-SERVER-RUNTIME-FIX-1 | testing | complete | Generate post-implementation feature-enabled unit coverage and owner/serving readiness success/failure/cleanup tests | sibling testing artifacts, testplan, and test files | root | I-SERVER-RUNTIME-FIX-1 | runnable testing implementation and artifacts | doc/testing coverage checks pass; canonical module and relevant integration entries execute real cases |
| A-SERVER-RUNTIME-FIX-1 | acceptance | complete | Audit proposal/design/code/testing consistency, runtime ownership, readiness evidence, and stale-report replacement | `docs/versions/v0.1/reviews/` | root | T-SERVER-RUNTIME-FIX-1 | acceptance report | quality and fresh required test artifacts pass; acceptance-report-check passes; no blocking findings remain |

Merged implementation reason: runtime creation, clone propagation, and readiness observability form one cross-module startup call flow. Splitting production edits into independent child tasks would leave intermediate signatures uncompilable and obscure ownership; admission and Scope Paths still bind each affected path explicitly.

## Return Routing
- Proposal issue: return to proposal only if the five requested outcomes or non-goals must change.
- Design issue: return to design for incomplete API compatibility, error propagation, runtime lifecycle, readiness semantics, or scope paths.
- Implementation issue: return to implementation for hidden runtime creation, duplicate worker pools, panic error boundaries, or readiness emitted before successful start.
- Testing issue: return to testing for zero-test feature commands, liveness-only evidence, missing early-exit/timeout/cleanup coverage, or unreachable unified entrypoints.
- Acceptance issue: return to the owning stage named by each finding and record the iteration below.

## Exit Condition
- [x] proposal approved
- [x] design approved
- [x] implementation admission passed
- [x] implementation completed
- [x] testing completed
- [x] acceptance completed

## Evidence
- User approval and launch statement: `批准，自动处理后续步骤`
- Proposal approval hash: `c5f4a7a3650d45467ce8b8d1e31dacae7b53fdf15bbb3be9bbc274987d00227c`.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule server-runtime-ownership-fix --docs proposal` passed.
- Proposal stage scope check passed against a temporary index containing only the sibling proposal because the shared worktree already contains unrelated changes.
- Design approval hash: `9ed6c3d36d524caed6eb0cb323802f9e1d5cbb56a76adb20ad56f90578ba4828`.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule server-runtime-ownership-fix --docs design` passed.
- Design stage scope check passed against a temporary index containing only the sibling design because the shared worktree already contains unrelated changes.
- Main implementation admission passed for `process_runtime_single_owner`, `cyfs_config_runtime_injection`, and `sn_miner_role_readiness_evidence`; stamp `harness/evidence/admission/20260710-server-runtime-ownership-fix.p2p-frame.server-runtime-ownership-fix.stamp.json`.
- Independent sn-miner admission passed for `sn_miner_owner_role_startup` and `sn_miner_serving_role_startup`; stamp `harness/evidence/admission/20260710-sn-miner-readiness.sn-miner.stamp.json`.
- `cargo check -p p2p-frame -p cyfs-p2p -p cyfs-p2p-test -p sn-miner --all-targets` passed.
- Main sibling and sn-miner implementation scope checks passed against temporary indexes containing only their admitted production/evidence paths.
- Testing approval hash: `83d4610b25b5ee970fde4608ff7575a3c1e3f3ca66808114241e22f11f9f2a73`.
- Testing document structure, schema, testing coverage, and testing stage scope checks passed.
- `python3 ./harness/scripts/test-run.py p2p-frame/server-runtime-ownership-fix all` passed; artifact `test-results/test-runs/20260710T074336Z-p2p-frame+server-runtime-ownership-fix-all.json` records two x509 runtime tests, five real-process readiness tests, and the all-target compile step.
- `./test-run.sh all all` passed; fresh whole-project artifact `test-results/test-runs/20260710T074910Z-all-all.json` has requested module/level `all all` and exit code 0.
- `python3 ./harness/scripts/quality-check.py` passed with explicitly empty configured gates.
- Acceptance report `docs/versions/v0.1/reviews/p2p-frame-server-runtime-ownership-fix-acceptance-2026-07-10.md` concluded accepted; acceptance stage scope and acceptance-report checks passed.
- Non-blocking unrelated finding F-001: `harness-self-check.py` expects literal wrapper text that the currently passing root shortcut does not contain; routed to optional workspace-harness follow-up.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|

## Prior Completed Plan Archive
The previously active, already completed workspace-harness plan is preserved below because it contained pre-existing uncommitted evidence.

# Pipeline Plan: Workspace Harness Test Run Dedupe

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/workspace-harness/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "确认，自动处理后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): workspace-harness
- change_id values: workspace_harness_test_run_dedupe

## Acceptance Baseline
- Final acceptance uses the approved `workspace-harness` proposal as authority.
- `test-run.py all all` must avoid repeated execution of identical commands inside one run while preserving deterministic order and module/level coverage evidence.
- `testplan.yaml` entries must not be duplicated by static fallback entries for the same module and level.
- Machine-written run artifacts remain the acceptance evidence source.
- Business module test semantics and assertions must not change.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-HARNESS-TEST-RUN-DEDUPE-1 | proposal | confirmed | Define runner dedupe goal, boundaries, non-goals, and evidence requirements | `docs/versions/v0.1/modules/workspace-harness/proposal.md` | root | user approval | approved proposal | proposal doc checks, schema check, approval metadata, and launch evidence recorded |
| D-HARNESS-TEST-RUN-DEDUPE-1 | design | confirmed | Define command identity, testplan precedence, reused-result artifact shape, and opt-out behavior | `docs/versions/v0.1/modules/workspace-harness/design.md` | root | P-HARNESS-TEST-RUN-DEDUPE-1 | approved design | design doc checks, schema checks, and stage-scope checks pass |
| I-HARNESS-TEST-RUN-DEDUPE-1 | implementation | complete | Implement deterministic in-run command dedupe in `test-run.py` | `harness/scripts/test-run.py` plus required admission evidence | root | D-HARNESS-TEST-RUN-DEDUPE-1 | runner implementation and admission evidence | schema/admission passed; implementation scope check result recorded; dry-run and harness checks pass |
| T-HARNESS-TEST-RUN-DEDUPE-1 | testing | complete | Register and execute focused coverage for dedupe, no-dedupe, artifact reuse evidence, and single-entry all/all behavior | workspace-harness testing artifacts and unified runner evidence | root | I-HARNESS-TEST-RUN-DEDUPE-1 | runnable test evidence | lightweight module/all and one root shortcut all/all artifact pass and include reuse evidence |
| A-HARNESS-TEST-RUN-DEDUPE-1 | acceptance | complete | Audit proposal/design/implementation/testing consistency and runnable evidence | `docs/versions/v0.1/reviews/` | root | T-HARNESS-TEST-RUN-DEDUPE-1 | acceptance report | acceptance report check passed and pipeline exit conditions complete |

## Return Routing
- Proposal issue: return to proposal if dedupe scope, artifact evidence expectations, or business-module non-goals must change.
- Design issue: return to design if command identity, fallback precedence, artifact shape, or opt-out behavior is incomplete.
- Implementation issue: return to implementation if repeated commands still execute by default, registered coverage is skipped, or artifacts lose coverage traceability.
- Testing issue: return to testing if targeted dry-run, lightweight module all, or the single root shortcut all/all evidence is missing or stale.
- Acceptance issue: return to the owning stage named by the finding.

## Exit Condition
- [x] proposal approved
- [x] design approved
- [x] implementation admission passed
- [x] implementation completed
- [x] testing completed
- [x] acceptance completed

## Evidence
- User approval and launch statement: `确认，自动处理后续步骤`
- Proposal approval hash: `762b1aa3a4fccf170e9cc01ad65390ca5d6fe0b60eb61d1609f2d7350982650c`
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module workspace-harness --docs proposal` passed.
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module workspace-harness` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage proposal --version v0.1 --module workspace-harness --ignore-untracked` passed.
- Design approval hash: `795f6459810e5110267bd17f2a7b609ab8c9a3b56ecf63ed398d6d5381b36a7d`
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module workspace-harness --docs design` passed.
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module workspace-harness` passed after design approval.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module workspace-harness --change-id workspace_harness_test_run_dedupe --evidence-file harness/evidence/admission/20260707-test-run-dedupe.md` passed; stamp `harness/evidence/admission/20260707-test-run-dedupe.workspace-harness.stamp.json`.
- `python3 -m py_compile harness/scripts/test-run.py` passed.
- `uv run --active python ./harness/scripts/test-run.py all integration --dry-run` passed and showed reused commands with a small cross-module plan.
- `uv run --active python ./harness/scripts/test-run.py sn-miner all --dry-run --no-dedupe` passed and showed duplicate commands without reuse markers with a small module plan.
- Testing approval hash: `15905af1943aae1d3c010a2ace35262add33e8bde419da09e410091f8152e4cd`.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module workspace-harness --docs testing` passed.
- `uv run --active python ./harness/scripts/testing-coverage-check.py --version v0.1 --module workspace-harness --change-id workspace_harness_test_run_dedupe` passed.
- `uv run --active python ./harness/scripts/test-run.py workspace-harness all` passed after testplan minimization; artifact `test-results/test-runs/20260707T095553Z-workspace-harness-all.json` covers focused harness checks without physical project all.
- `./test-run.sh all all` remains the single required physical project-wide run; artifact `test-results/test-runs/20260707T092342Z-all-all.json` includes reused command evidence and exit code 0.
- Direct `uv run --active python ./harness/scripts/test-run.py all all` is no longer a default required verification when `./test-run.sh all all` passes and the wrapper is unchanged.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module workspace-harness --change-id workspace_harness_test_run_dedupe --ignore-untracked` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module workspace-harness --ignore-untracked` passed.
- `uv run --active python ./harness/scripts/quality-check.py` passed with explicitly empty quality gates.
- `git diff --check` passed.
- Acceptance report `docs/versions/v0.1/reviews/workspace-harness-test-run-dedupe-acceptance-2026-07-07.md` accepted the change.
- `uv run --active python ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/workspace-harness-test-run-dedupe-acceptance-2026-07-07.md` passed.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
