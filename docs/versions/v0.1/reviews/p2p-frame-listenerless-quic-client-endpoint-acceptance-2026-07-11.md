# p2p-frame Listenerless QUIC Client Endpoint Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-001 | low | testing | approved `testing.md` explicit gap rows and source review of the IPv4/IPv6 cache branches | IPv6 loopback, deterministic client-endpoint bind failure, and Quinn client `local_ip() == Some(...)` cannot be triggered reliably by the current CI/runtime seams; IPv4 real-connect behavior and the structurally identical IPv6 selection branch are covered/reviewed | none |

## Object and Scope
- Module: p2p-frame root packet; implementation responsibility is `networks/quic`, consumed by `sn/client` through TTP and `TunnelNetwork`.
- Version: v0.1.
- change_id values reviewed: `sn_client_protocol_priority`.
- Review date: 2026-07-11.
- In scope: listener-first QUIC active open, network-owned listenerless client endpoints, IPv4/IPv6 endpoint selection, lazy concurrent reuse, TLS/identity reuse, tunnel local metadata, listener isolation, `close_all_listener()` lifecycle, connect error propagation, and SN TCP fallback compatibility.
- Out of scope: SN/QUIC wire changes, TLS identity changes, TTP target matching, maintained-target semantics, incoming-listener behavior, same-source UDP punch without a listener, endpoint publication, and unrelated shared-worktree changes.

## Optional Diff / Status Evidence
- `git status --short` summary: the reviewed change is mixed into a pre-existing dirty worktree; task-owned paths are the root p2p-frame design/testing/testplan, QUIC network implementation/tests, two admission evidence pairs, one workspace-harness fixture-path update, the pipeline plan, and this report.
- `git diff --stat` summary: the task adds one private per-IP-family client endpoint cache and listenerless connect path, replaces the obsolete no-listener `NotFound` expectation, and adds lifecycle/error/listener-preservation assertions; unrelated existing diffs were excluded from isolated scope checks.
- `git diff --name-status` summary: production behavior is confined to `p2p-frame/src/networks/quic/network.rs`; stage documents and evidence are separately governed.
- `git diff --check` result: passed for the reviewed production, design, testing, testplan, fixture, evidence, and pipeline paths.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| A supported outbound QUIC protocol is attempted even with no local listener | approved proposal `P-SN-CLIENT-PROTOCOL-PRIORITY-1` and approved design listenerless active-open section | `create_tunnel_with_intent(...)` falls through to `open_with_client_endpoint(...)` only when no matching-family listener exists | pre-fix red artifact plus real loopback listenerless success in `test-results/test-runs/20260710T163817Z-p2p-frame-all.json` | implemented |
| Client endpoint is network-owned, lazy, reusable, and split by IP family | approved design Data and State plus Interfaces and Dependencies | `QuicClientEndpoints { ipv4, ipv6 }` is protected by one mutex and retained by `QuicTunnelNetwork` | two concurrent first connections share a nonzero ephemeral port; a third connection reuses it | implemented |
| Client endpoint remains outside listener/incoming state and survives listener shutdown | approved design Overall Approach and lifecycle invariant | client endpoints are not stored in `listeners`; `close_all_listener()` only drains listener objects | `listener_infos()` remains empty before/after connects, and a post-`close_all_listener()` tunnel succeeds | implemented |
| Existing matching listener remains preferred and preserves its local endpoint | approved design compatibility/invariant and proposal same-source boundary | matching-family listener calls `open_or_connect(...)`; shared finalization receives `listener.bound_local()` without client-only routed-IP rewriting | mixed-identity listener-backed test asserts tunnel `local_ep == listener.bound_local()` and passes in module/all evidence | implemented |
| TLS, transport configuration, identity resolution, tunnel/candidate IDs, and remote metadata stay on existing paths | approved design no-wire/no-identity-change constraints | `connect_with_ep(...)`, `resolve_remote_identity(...)`, and factored `finish_connect(...)` reuse the existing configuration and tunnel construction | real x509 QUIC handshakes, existing stream/datagram tests, module and workspace runs pass | implemented |
| Listenerless failures are concrete connect errors available to SN fallback | approved proposal fallback rule and approved design failure handling | endpoint creation maps to `ConnectFailed`; connect/handshake errors propagate; no-listener lookup no longer emits `NotFound` | unreachable loopback port returns `ConnectFailed`, while existing SN QUIC-first/TCP fallback tests pass | implemented |
| Explicit-local-endpoint API keeps its listener requirement | approved design interface boundary | `create_tunnel_with_local_ep_and_intent(...)` is unchanged and still returns `NotFound` without the requested listener | existing explicit wrong-local-endpoint test passes | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| Listenerless active open / `sn_client_protocol_priority` | normal / boundary / negative / error / lifecycle / concurrency / cross-module | approved testing direct-change, case-type, branch and integration rows | concurrent real connects, post-close connect, unreachable error, SN candidate/fallback tests in the module artifact | adequate |
| Listener compatibility and local endpoint identity | normal / compatibility / lifecycle | approved testing listener-present branch plus acceptance-return refinement | listener-backed mixed-identity handshake now asserts exact local endpoint; existing listener lifecycle tests remain green | adequate |
| Endpoint state and resource ownership | boundary / lifecycle / concurrency | approved design state transition and testing state-transition coverage | single mutex-protected lazy cache is exercised by two simultaneous cache misses and later cache hits | adequate |
| TLS/identity and tunnel behavior | normal / negative / compatibility / cross-module | existing QUIC network test suite and testing submodule mapping | x509 identity handshake, stream/control/datagram behaviors, module and workspace artifacts pass | adequate |
| IPv6/bind/local-IP rare branches | boundary / error / compatibility | explicit testing gap rows name owner, risk, and acceptance impact | source review plus IPv4 isomorphic runtime coverage; no unsupported automated claim is made | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| A-QUIC-CLIENT-1 | proposal/design | `create_tunnel(...)` without a matching listener establishes QUIC using an outbound-only ephemeral endpoint | real listenerless client/server connection with nonzero client port | pass |
| A-QUIC-CLIENT-2 | design/code/tests | concurrent first opens produce one cached endpoint per IP family and later opens reuse it | same local endpoint across concurrent and sequential tunnels | pass |
| A-QUIC-CLIENT-3 | design/code/tests | client endpoint is never exposed as a listener and is unaffected by `close_all_listener()` | empty listener info and successful post-close connection | pass |
| A-QUIC-CLIENT-4 | proposal/design/code/tests | a matching listener remains first choice and tunnel metadata retains its bound local endpoint | exact listener-backed `local_ep` assertion | pass |
| A-QUIC-CLIENT-5 | proposal/design/code/tests | unreachable listenerless QUIC returns a connect error rather than listener lookup `NotFound`, preserving TCP fallback | focused error assertion plus SN candidate/fallback tests | pass |
| A-QUIC-CLIENT-6 | proposal/design | no public trait, SN/QUIC wire, TLS identity, TTP matching, incoming listener, or UDP-punch contract changes | API/source review, compilation, existing workspace tests | pass |
| A-QUIC-CLIENT-7 | harness rules | approvals, admission, stage scopes, red-green evidence, canonical test entries, quality and report checks pass | hashes, stamps, isolated scopes and machine-written artifacts | pass |

## Inputs
- `proposal.md`
- `design.md`
- test implementation and optional `testing.md`
- `testplan.yaml` for completed testing work
- long-lived module doc
- implementation
- test code
- test results
- optional git diff/status evidence
- `harness/rules/acceptance-review-rules.md`
- `harness/rules/acceptance-task-rules.md`
- `harness/checklists/module-acceptance-checklist.md`

## Review Order
1. Reviewed the approved proposal requirement and explicit non-goals.
2. Reviewed the approved design mapping, state ownership, failure semantics, lifecycle and Scope Paths against the proposal.
3. Generated acceptance rules from proposal, design, code, tests and harness requirements.
4. Reviewed the production implementation, including concurrency, resource lifetime, listener selection and error propagation.
5. Returned the listener-local-endpoint compatibility issue to implementation and added a direct regression assertion.
6. Reviewed refreshed test design, runnable test code, red-green evidence and unified results.
7. Reviewed remaining document/code assumptions and explicit non-blocking gaps.
8. Used isolated scope checks to exclude unrelated shared-worktree changes.
9. Produced the accepted conclusion after the return was closed.

## Consistency Summary
- Proposal authority check: approved proposal item `P-SN-CLIENT-PROTOCOL-PRIORITY-1` explicitly requires an outbound candidate with `local_ep=None` when no listener exists and requires QUIC failure to reach TCP fallback; no chat-only requirement is used as acceptance authority.
- Proposal vs design: the approved design makes the candidate requirement executable through a network-owned, per-IP-family, outbound-only Quinn endpoint with listener-first selection, explicit lifecycle and failure boundaries.
- Design vs testing implementation: testing exercises real listenerless establishment, concurrent cache creation, sequential reuse, listener invisibility, listener shutdown isolation, unreachable error propagation, and the refined listener-backed local endpoint invariant.
- Design vs long-lived boundary doc: `docs/modules/p2p-frame.md` already assigns transport active-open and listener behavior to `networks`; no crate or public trait boundary changes.
- Design vs implementation: private endpoint state, lazy creation, `connect_with_ep(...)`, listener-first selection, client-only local-IP refinement and unchanged explicit-local-endpoint behavior match the design.
- Test implementation vs test code vs results: the registered `p2p-frame-quic-listenerless-client` step runs both new tests; the refreshed module artifact and root artifact are passing and include the changed test code.
- Test design adequacy: all constructible core outcomes are automated; IPv6 host availability, deterministic bind failure and Quinn client `local_ip()==Some` remain explicit low-risk gaps with code-review evidence and no hidden acceptance claim.
- change_id traceability: `sn_client_protocol_priority` appears in proposal/design/testing, admission evidence/stamp, pipeline plan and this report; isolated production scope contains one admitted path.
- Acceptance criteria traceability: A-QUIC-CLIENT-1 through A-QUIC-CLIENT-7 cover connectivity, cache concurrency, lifecycle/isolation, listener compatibility, failure/fallback, non-goals and governance.
- Cross-module admission: no new production module is changed outside admitted p2p-frame Scope Paths; the workspace-harness testplan change only refreshes a stale admission fixture path and passed its own isolated testing scope.
- Public API / codec / runtime semantics review: no public signature or codec changes; client endpoint creation uses the caller's supported Tokio/Quinn runtime as required by the proposal, while the network retains the endpoint for its lifetime; listener server runtime and listener behavior are unchanged.
- Document logic review: proposal, design and refreshed testing agree that listenerless active open is supported, explicit-local-endpoint open still requires a listener, and listenerless UDP punch is not claimed.
- Implementation logic review: one mutex serializes per-family lazy creation without await; creation failures are not cached; connect failures retain a reusable endpoint; listener errors are preserved when a matching listener was selected; client-only metadata refinement cannot alter listener `local_ep`.
- Document approval timing (approved_content_sha256 verified by schema-check): user-approved design and auto-pipeline-approved refreshed testing hashes pass schema validation; implementation admission was rechecked after the acceptance return.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): isolated one-path implementation check passed; refreshed two-path testing-doc check and workspace-harness fixture testing check also passed.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: `test-results/test-runs/20260710T160009Z-p2p-frame+listenerless-quic-client-endpoint-red-unit.json` fails on the old `no listener found` `NotFound` branch; the same real-connect scenario passes in the refreshed module and root artifacts.

## Required Command Evidence
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`: passed after refreshed testing approval.
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority --evidence-file harness/evidence/admission/20260710-listenerless-quic-client-endpoint.md`: passed in final acceptance and refreshed the content-bound stamp.
- `python3 ./harness/scripts/stage-scope-check.py --stage <stage>`: isolated design, one-path implementation, two-path testing, workspace-harness testing and acceptance-report scope checks passed.
- Unit test command through `harness/scripts/test-run.py`: module all run passed the listenerless targeted step, listener-backed assertion and existing unit suite.
- DV test command through `harness/scripts/test-run.py`: p2p-frame DV is explicitly disabled by the approved testplan; the unified runner reports no DV tests instead of claiming an unexecuted workflow.
- Integration test command through `harness/scripts/test-run.py`: module all run passed the workspace integration step and x509 SN/QUIC neighboring flows.
- Module all command through `harness/scripts/test-run.py <module> all`: `python3 ./harness/scripts/test-run.py p2p-frame all` passed; artifact `test-results/test-runs/20260710T163817Z-p2p-frame-all.json`.
- Project all command through `harness/scripts/test-run.py all all`: the root shortcut invoked the canonical project run and passed; artifact `test-results/test-runs/20260710T164336Z-all-all.json`.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): `test-results/test-runs/20260710T164336Z-all-all.json`, requested `all all`, exit code 0.
- Quality gates `python3 ./harness/scripts/quality-check.py`: passed; `harness/quality-gates.yaml` declares an explicitly empty gate list.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not applicable because no quality gates are configured.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): `./test-run.sh all all` passed and wrote `test-results/test-runs/20260710T164336Z-all-all.json`.
- Acceptance report check `python3 ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-listenerless-quic-client-endpoint-acceptance-2026-07-11.md`: passed.
- Targeted migration search, when applicable: not applicable; no public API or migration boundary changes.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: listenerless QUIC now establishes a real authenticated tunnel through a reusable network-owned client endpoint, listener behavior remains isolated and preferred, concrete errors preserve SN TCP fallback, the acceptance-found compatibility issue was corrected, and all governance plus runnable evidence gates pass.
- Supporting test evidence: red artifact `test-results/test-runs/20260710T160009Z-p2p-frame+listenerless-quic-client-endpoint-red-unit.json`, refreshed module artifact `test-results/test-runs/20260710T163817Z-p2p-frame-all.json`, and root whole-project artifact `test-results/test-runs/20260710T164336Z-all-all.json`.
- Residual risk: IPv6-only hosts, deterministic local UDP bind exhaustion, and Quinn returning a concrete client local IP are not directly injected in CI; these remain explicit low-risk test gaps. The endpoint driver also relies on the supported caller Tokio runtime remaining alive, consistent with existing async active-open assumptions.

## Follow-Up Tasks
- Requirement task: none.
- Design task: none.
- Implementation task: none after `ACCEPTANCE-LISTENER-LOCAL-EP-001`.
- Testing task: none blocking; optional future IPv6/fault-injection CI can close F-001.
- Testing return reason if coverage is incomplete: remaining gaps need host capability or a new socket-factory seam that the approved design intentionally does not introduce; core approved behavior is fully runnable.
- Iteration count: 2
- Stop reason if more than 5 unsuccessful iterations: not applicable.
