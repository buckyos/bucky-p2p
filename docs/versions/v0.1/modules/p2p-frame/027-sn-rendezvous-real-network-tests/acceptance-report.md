# P2P Frame 027 Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-SRRN-FINAL | none | acceptance | launch-confirmed proposal, current plan/state/testplan, production command registries and send sites, dedicated real-network test, current-input unified artifact | No blocking requirement, design, implementation, transport, lifecycle or testing defect was found | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: one real loopback SN server and two authenticated clients now exercise the complete current SN client/server command family over independently selected TCP and QUIC command tunnels.
- What was verified: Report-driven online registration, exact active-tunnel transport and connection identity, known/unknown Query, Call/Called/CalledResp, Rendezvous/Notify/Resp, action-before-response ordering, rejection, wrong-version rejection, malformed Report handling and a closed ten-code inventory.
- Evidence used: current proposal/plan/state/testplan, `PackageCmdCode`, client/server command registrations and send sites, `p2p-frame/tests/sn_protocol_real_network.rs`, stage manifests and `test-results/test-runs/20260901T022855Z-p2p-frame+027-sn-rendezvous-real-network-tests-all.json`.
- Blocking issues: none.
- Next action: close the automatic pipeline; retain public NAT, multi-host, inter-SN, PN and peer-to-peer tunnel matrices as explicit non-goals.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 027-sn-rendezvous-real-network-tests
- change_id values reviewed: sn_rendezvous_real_socket_command_flow, sn_rendezvous_tcp_quic_transport_matrix, sn_all_command_real_transport_matrix
- Review date: 2026-09-01
- In scope: current PackageCmdCode-owned SN client/server commands, one-SN/two-client loopback lifecycle, authenticated command tunnels, TCP/QUIC parity and protocol-appropriate negative cases.
- Out of scope: production changes, inter-SN/owner commands, PN, NAT-probe UDP, direct peer tunnel transport, public NAT, packet loss, performance and broad workspace quality execution.
- Task-relevant acceptance scope: admitted production sources are unchanged systems under test; task writes are proposal/plan/state/evidence, one dedicated external test file, testplan, run artifact and this report.
- Out-of-scope checks not run: cross-SN network, public multi-host/NAT environment, peer-to-peer tunnel convergence, root `all all`, whole-workspace runtime and broad quality gate.

## Optional Diff / Status Evidence
- The shared worktree contains unrelated existing changes; task stage manifests, not the whole dirty tree, define the review boundary.
- `git diff --check` passed for the task test, packet and task testing manifests.
- A targeted search found no `dispatch_owner_cmd`, `process_rendezvous_request` or `SnInterClient` reference in the new test.
- Diff/status output was used only for task-path discovery and was not treated as correctness proof.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| P-SRRN-1 / sn_rendezvous_real_socket_command_flow | proposal Scope and Success Criteria; plan Exported Interfaces, State Ownership and Failure Flows | unchanged `SnClientService::rendezvous_via_sn`, server rendezvous handler, target notify handler and QA response path | real listener and two clients; exact notify identities/fields; semaphore-held action; rejection and wrong-version cases on TCP and QUIC | implemented |
| P-SRRN-2 / sn_rendezvous_tcp_quic_transport_matrix | proposal transport boundary; plan TCP/QUIC modules and transport-parity failure flow | unchanged endpoint-selected TCP/TLS and QUIC command tunnel implementations | independent TCP and QUIC tests assert listener protocol, active `ActiveSN.protocol`, active `conn_id` and exact classified tunnel equality | implemented |
| P-SRRN-3 / sn_all_command_real_transport_matrix | proposal ten-code table; plan protocol inventory and send/handler mappings | unchanged client sends/handlers and server handlers for Report, Query, Call, CalledResp and Rendezvous plus server notifications | full u8 enum discovery equals ten classified roles; both transports run success and failure cases for every active family | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| real rendezvous flow / sn_rendezvous_real_socket_command_flow | normal / boundary / negative / error / compatibility / lifecycle / ordering | state coverage V-SRRN-REAL and DV step `sn-real-network-tcp-quic-dv` | both transport cases exit 0 with exact field, pending-response, rejection and version assertions | adequate |
| transport parity / sn_rendezvous_tcp_quic_transport_matrix | normal / boundary / negative / error / compatibility / lifecycle | state coverage V-SRRN-TRANSPORT; separate TCP and QUIC test names; serial runner | DV artifact executes two cases and both exit 0 | adequate |
| closed command family / sn_all_command_real_transport_matrix | normal / boundary / negative / error / compatibility / lifecycle | state coverage V-SRRN-INVENTORY; exact enum-role unit plus shared runtime matrix | inventory 1/1 and runtime matrix 2/2 pass; QA response variants are classified without fake standalone commands | adequate |
| neighboring module contracts | cross-module | testplan integration level is disabled with owner, reason, risk and acceptance impact | no public/cross-crate interface changed; the confirmed topology intentionally remains inside p2p-frame | not applicable |

## Defect-Discovery Categories
| Category | Status | Concrete Evidence and Falsification Result |
|----------|--------|--------------------------------------------|
| requirement-and-behavior | pass | The test starts an actual `SnServer` and two `P2pStack` clients, waits for Report-backed online state, and uses public Query/Call/Rendezvous APIs; targeted source search confirms no direct dispatch or in-memory inter-SN substitute. All ten PackageCmdCode values map to the proposal families. |
| logic-and-control-flow | pass | The shared matrix runs setup -> active transport proof -> Query -> Call/Called acknowledgement -> Rendezvous -> malformed Report in that order; malformed input is last because it may invalidate the tunnel. TCP and QUIC invoke the same behavioral function. |
| boundary-and-input | pass | Unknown peer IDs produce empty Query/NotFound Call results, empty endpoint/prediction arrays are asserted, the complete u8 domain feeds enum conversion, a previous rendezvous version and a one-byte malformed Report fail closed. |
| state-and-data-integrity | pass | Both clients must expose exactly one active SN with the expected SN id, protocol and the same connection id returned by the exact tunnel classification. Called and Rendezvous certificates/ids, sequence, tunnel id, payload and result shapes are checked without publishing fake state. |
| error-handling-and-recovery | pass | Bind retry is restricted to address conflicts and explicitly stops already-started server/client services; other startup errors fail immediately. Listener rejection, wrong version and malformed Report cannot be counted as success, and every command wait has a finite bound. |
| resource-lifetime-and-cleanup | pass | Retry branches stop previously created services; online failure stops both clients and server before panic; the rendezvous task is released and joined; the normal path stops both SN clients and server after the final negative case. |
| concurrency-and-ordering | pass | A zero-permit semaphore blocks the target rendezvous listener, the caller task is asserted unfinished after Notify arrival, then one permit releases the action and the task is joined. Tests run with `--test-threads=1` to prevent topology interference. |
| interface-and-compatibility | pass | Production APIs, command ids, codecs and transports are unchanged. Current command/version paths succeed; the prior rendezvous command version fails before the target callback; response-code variants are correctly treated as QA payload roles. |
| security-and-capacity | pass | X509 identities authenticate the SN and clients; queried target, Called caller, target and serving-SN identities are verified from certificates/tunnel context. Payloads and channels are bounded, setup retries are capped at 20 and command/online waits are finite. |
| test-adequacy | pass | Inspection confirmed assertions can distinguish object construction from Report registration, a listener from the actual active tunnel, send completion from target notification, and target notification from action completion. The current-input unified artifact has both registered steps and all three tests passing. |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | topology and all active command families | setup/matrix functions; client/server send and handler registries | The call chain crosses real command tunnels and each family reaches an observable result before teardown. | none | pass |
| termination and progress | startup, QA waits and target action | 30s online, 5s raw command, configured 3s connect/call bounds and semaphore join | Every wait is bounded or deterministically released; no detached test-owned response task remains. | none | pass |
| concurrency and synchronization | Rendezvous action ordering and serial fixtures | semaphore, channel, atomic callback counts, task join and serial test runner | The caller cannot report success before target action acknowledgement, and invalid versions do not race into callbacks. | none | pass |
| resource lifetime and cleanup | sockets, stacks, server and retry attempts | retry/online-failure stop paths and final shutdown | Already-created services are stopped on retry/failure, and normal topologies are stopped after use. | none | pass |
| state and data integrity | active SN, Query/Call/Rendezvous correlation | active list/tunnel equality and exact identity/field assertions | Selected transport and authenticated identities cannot be inferred from unrelated state. | none | pass |
| error handling and recovery | bind, missing peer, rejection, version and malformed body | explicit result/error assertions and bounded setup | Expected failures fail closed without a false successful target action. | none | pass |
| interface boundary and compatibility | PackageCmdCode and unchanged client/server APIs | enum, registries, send sites, testplan API impact and raw version negative | No API/wire/build migration is introduced; actual QA response roles are preserved. | none | pass |
| security and capacity safety | TLS identities, untrusted command bodies and test work bounds | generated X509 identities, certificate checks, malformed/version inputs, bounded channels/retries/timeouts | The test exercises authenticated identity boundaries and introduces no unbounded queue/task/state. | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-SRRN-1 | proposal P-SRRN-1 | caller Rendezvous crosses a real SN tunnel, target action precedes response and rejection/version errors fail closed | exact public call path, pending response proof, callback identity/field assertions and negatives | pass |
| AR-SRRN-2 | proposal P-SRRN-2 | both clients register and use the selected TCP or QUIC SN transport | active SN protocol/id/conn equality plus exact classified tunnel for separate TCP/QUIC cases | pass |
| AR-SRRN-3 | proposal P-SRRN-3 | every current SN client/server package code has one honest role and every active family has real transport behavior | full enum-domain inventory and Report/Query/Call/Called/Rendezvous matrix | pass |
| AR-SRRN-4 | proposal risks | test work is bounded, serial and cleaned up, and no direct handler/mock supplies network evidence | retries/timeouts, stop paths, task join, serial command and source inspection | pass |
| AR-SRRN-5 | proposal non-goals | results claim local socket evidence only | testplan/report explicitly exclude public NAT, inter-SN, PN and peer-tunnel claims | pass |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/027-sn-rendezvous-real-network-tests/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/027-sn-rendezvous-real-network-tests/pipeline/plan.md`
- `docs/versions/v0.1/modules/p2p-frame/027-sn-rendezvous-real-network-tests/pipeline/state.json`
- `docs/versions/v0.1/modules/p2p-frame/027-sn-rendezvous-real-network-tests/testplan.yaml`
- admitted production command, client, service, TCP and QUIC sources
- `p2p-frame/tests/sn_protocol_real_network.rs`
- admission stamp and proposal/design/implementation/testing stage manifests
- `test-results/test-runs/20260901T022855Z-p2p-frame+027-sn-rendezvous-real-network-tests-all.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. An independent delegated reviewer was unavailable because active runtime instructions prohibit unrequested agent delegation; logical acceptance task A-1 therefore restarted from the proposal and ignored prior completion claims.
2. The review traced PackageCmdCode, client/server registrations and send sites, then tried to falsify real-socket use, active transport identity, command completeness, error behavior, cleanup and action ordering.
3. Only after inspecting the test implementation and current run artifact did it audit document consistency and select the conclusion.

## Consistency Summary
- Proposal authority check: the exact user statement `确认，自动完成` launched and confirmed this task's proposal under the auto-pipeline rule.
- Proposal vs design: the pipeline plan preserves the one-SN/two-client, TCP/QUIC, complete PackageCmdCode and test-only boundaries.
- Design vs testing implementation: the dedicated test follows the mapped real topology, active-tunnel ownership, command flows, failure paths and no-production-change decision.
- Design vs long-lived boundary doc: no long-lived module boundary changes were required or made.
- Design vs implementation: admitted production sources remain unchanged systems under test; implementation was a documented no-op source audit.
- Test implementation vs test code vs results: testplan commands resolve to the dedicated file; current evidence-input hash and both step exit codes are recorded in the final artifact.
- Test design adequacy: all applicable normal, boundary, negative, error, compatibility, lifecycle and ordering risks are executable; integration is concretely not applicable because no neighboring crate contract changed.
- change_id traceability: all three change_ids map through proposal, plan, admission, testplan, state coverage, artifact and this report.
- Acceptance criteria traceability: listener/online/tunnel, command inventory, identity/correlation, action ordering, negatives, cleanup and unified evidence each have direct assertions.
- Cross-module admission: not applicable; packet and target module are p2p-frame and production has no task diff.
- Public API / codec / runtime semantics review: no public API, command id, codec, build or production runtime behavior changed.
- Document logic review: proposal, plan, state and testplan agree on the exact protocol owner and excluded neighboring protocols.
- Implementation logic review: source tracing confirms the tests invoke client sends and registered handlers over TTP command tunnels rather than direct service calls.
- Implementation correctness audit completeness and routing: every required category passes; no finding requires return routing.
- Document approval timing (approved_content_sha256 verified by schema-check): auto-pipeline launch replaces manual approval metadata; current proposal and plan hashes are bound by the admission stamp.
- Implementation task paths bound to design Scope Paths: implementation scope passed for the admission evidence/state paths; production source audit introduced no production diff.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not applicable; this task adds tests without changing production behavior.

## Validation Evidence
- Existing schema result: `schema-check.py --version v0.1 --module p2p-frame --submodule 027-sn-rendezvous-real-network-tests` passed before implementation audit.
- Existing admission stamp: `docs/versions/v0.1/evidence/admission/20260901-sn-protocol-real-network-tests.p2p-frame.027-sn-rendezvous-real-network-tests.stamp.json` binds all three change_ids and the current proposal/plan.
- Existing stage-scope result: proposal, design and implementation scopes passed earlier; final testing scope passed for four task paths.
- Existing pipeline-plan result, when applicable: current plan/state passed after final testing evidence update; complete-state validation follows acceptance closeout.
- Task-relevant test run artifact(s): `test-results/test-runs/20260901T022855Z-p2p-frame+027-sn-rendezvous-real-network-tests-all.json`; unit inventory exit 0 and DV TCP/QUIC matrix exit 0.
- Commands rerun because checker-owned inputs changed after their previous pass: the unified runner, testing coverage, testing stage scope and pipeline-plan checker were rerun after strengthening active connection-id/protocol assertions and retry cleanup.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; the task artifact runs only the two task-selected commands.
- Risk-triggered task-local contract kinds and assertions, when applicable: disabled because no public API, export, build, documentation or wire contract changed.
- Scoped evidence input hash current, when risk-triggered: artifact hash `2efd478b23647d251cd7ac7b47fd99fd0cfd5a6a48551975e5fe163f65c458a4` binds the final listed inputs.
- Quality gates: not applicable; no broad quality run was requested.
- Broad quality artifact: absent because no broad quality run was requested.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: not run because no architecture document or boundary changed.
- Acceptance report check after this report was created or modified: run during closeout; failure blocks completion.
- Targeted migration search, only when applicable to the reviewed task: not applicable because no symbol or consumer migration occurred; command send/handler inventory was inspected directly.

## Automated Test Exception
- Applies: no
- Reason: the current machine-written task artifact covers every enabled level and all three change_ids.
- Owner: acceptance
- Risk: local loopback cannot expose public NAT, multi-host routing, loss or deployed TLS configuration defects.
- Acceptance impact: deterministic real-socket evidence supports the scoped result without claiming excluded deployed-environment behavior.
- Alternative evidence: direct source falsification supplements, rather than replaces, the artifact.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: the complete current SN client/server command inventory is honestly classified and its active families pass the required real one-SN/two-client matrix over both authenticated TCP and QUIC command tunnels, with bounded negative, ordering and cleanup evidence.
- Supporting task-relevant test evidence: `test-results/test-runs/20260901T022855Z-p2p-frame+027-sn-rendezvous-real-network-tests-all.json`, 2/2 registered steps and 3/3 tests successful.
- Residual risk: evidence is limited to local loopback; public NAT, multi-host, inter-SN, PN and peer-to-peer tunnel transports remain untested by design.

## Follow-Up Tasks
- Requirement task: none.
- User decision required for proposal issue: none.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable; scoped coverage is complete.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable; acceptance completed in the first review iteration.
