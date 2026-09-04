---
module: p2p-frame
task_name: 027-sn-rendezvous-real-network-tests
submodule: 027-sn-rendezvous-real-network-tests
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# SN Client/Server Protocol Real Network Matrix Proposal

## Background and Goal

Task `026-simplify-sn-rendezvous-protocol` has codec, dispatch, same-SN and cross-SN coverage. The inter-SN dispatch test at `p2p-frame/tests/unit/sn_tests/inter_sn_rendezvous_tests.rs` intentionally calls `dispatch_owner_cmd` with an `InterSnPeer` mock, so it does not prove that an SN command traverses a real socket. The existing SN tests exercise several Report, Query, Call, Called and rendezvous behaviors on loopback QUIC, but they do not expose a reusable TCP/QUIC matrix, do not explicitly prove that both clients' active SN command tunnels use the selected transport, and the existing “all SN commands” inventory still lists only the eight pre-rendezvous `PackageCmdCode` values.

This sibling task adds dedicated runtime tests for every current SN client/server protocol family without changing their wire contracts. Each scenario must start one real `SnServer` on a loopback port, start two independent SN client stacks and wait for both authenticated SN sessions. The test matrix must then exercise registration/report, query, call/called acknowledgement and tunnel rendezvous over real command tunnels. The same observable contracts must be exercised with both TCP and QUIC client-to-SN connections.

## Scope

### In scope

- Add dedicated test-only setup that can start one `SnServer` and two client stacks using an explicitly selected loopback transport: `Protocol::Tcp` or `Protocol::Quic`.
- Require the SN to bind a non-zero local `127.0.0.1` port and require both clients to become online through that server before the explicit command cases begin.
- Prove for each transport that both clients have a non-zero command tunnel associated with the selected SN endpoint and that no direct call to `dispatch_owner_cmd`, `process_rendezvous_request`, or an in-memory `SnInterClient` substitutes for the client-to-SN socket path.
- Derive the complete SN client/server inventory mechanically from `PackageCmdCode`, the server's `register_sn_cmd_handler`, the client's `register_cmd_handler`, and the actual request/response send sites. The current required inventory is:

| protocol family | client -> SN | SN -> client or response | required real-network observation |
|-----------------|--------------|--------------------------|-----------------------------------|
| registration/report | `ReportSn` (`0x24`) | `ReportSnResp` payload (`0x25` remains part of the package-code inventory) | both clients become registered/online and the response state belongs to the selected SN tunnel |
| query | `SnQuery` (`0x26`) | `SnQueryResp` payload (`0x27` remains part of the package-code inventory) | known and unknown peer results, correlation and identity/endpoint fields |
| call | `SnCall` (`0x20`) | `SnCallResp` payload (`0x21` remains part of the package-code inventory) | caller response result, target identity and request correlation |
| called acknowledgement | `SnCalledResp` (`0x23`) | `SnCalled` (`0x22`) | exact Called fields reach the target and the target acknowledgement returns over its selected SN tunnel |
| tunnel rendezvous | `SnTunnelRendezvous` (`0x2c`) | `SnTunnelRendezvousNotify` (`0x2d`) and `SnTunnelRendezvousResp` payload | exact notify/response fields, target action ordering and generic failure semantics |

- Treat all ten current `PackageCmdCode` variants as a closed inventory. Where `SnCallResp`, `ReportSnResp` and `SnQueryResp` are response payloads on the command server's QA exchange rather than independently registered inbound commands, record and test that actual transport shape instead of fabricating an unsupported standalone send.
- For Report, Query, Call/Called and Rendezvous, provide at least one expected success/result case and one protocol-appropriate failure or boundary case on both TCP and QUIC. The design may combine cases in a shared transport fixture, but the testplan must preserve a mechanically auditable protocol-family × transport coverage table.
- For Report, prove that the initial real command exchange registers both authenticated clients and yields usable active-SN state; do not count object creation or only opening a tunnel as Report coverage.
- For Query, prove a registered-peer result and an unknown-peer result, including sequence/identity/endpoint semantics defined by the current protocol.
- For Call/Called, prove that a caller request reaches the SN, the `SnCalled` notification reaches the second client with the expected caller certificate, target, tunnel type, tunnel ID and payload, the target sends `SnCalledResp`, and the caller receives the matching `SnCallResp`; also cover a missing/rejected target result.
- Exercise the public request path `SnClientService::rendezvous_via_sn`: caller request -> SN request handler -> target `SnTunnelRendezvousNotify` handler -> target listener -> response -> caller.
- Assert field-level correlation and identity on the received notification and response: `seq`, `tunnel_id`, target operation, endpoint list, prediction flag, serving SN ID, initiator certificate ID, result and predicted endpoint list.
- Prove action-before-response ordering by holding the target listener and asserting that the caller response remains pending until the target releases its acknowledgement.
- Cover at least one real-socket failure path for both TCP and QUIC, such as target listener rejection or absence, and require the caller to receive the defined generic rendezvous failure within the configured timeout without a false successful callback.
- Add real-tunnel malformed/version rejection where practical through the existing command client surface, with mandatory wrong-version coverage for the independently versioned rendezvous commands. If a negative cannot be expressed without production instrumentation, design must record the exact limitation and retain the lowest-level existing negative rather than adding a production test hook.
- Register the new tests through the task-local `testplan.yaml` and unified `harness/scripts/test-run.py` entry during the post-implementation testing stage.
- Keep all new runtime tests in dedicated `p2p-frame` test files or test directories; do not place them in `cyfs-p2p-test` and do not add inline test bodies to production source files.

### Out of scope

- Changing any Report, Query, Call, Called, rendezvous request/response type, command ID, command version, validation rule or fallback semantic.
- Changing production TCP, QUIC, SN, TLS, tunnel selection or connection lifecycle code solely to make the tests pass. A discovered production defect or missing required public behavior must return to a new implementation-capable task rather than being silently repaired in this testing task.
- Testing an SN-to-SN network connection, owner-directory routing or cross-SN TTP transport. This task has one SN server and two clients; the selected transport is the client-to-SN command transport.
- Testing `InterSnCommandCode`, owner-election/control commands, PN commands, NAT probe UDP datagrams or non-SN tunnel control protocols; “all protocols” in this task means the complete `PackageCmdCode`-owned SN client/server families listed above.
- Requiring a client-to-client direct tunnel to use the same transport as its SN command tunnel. Direct peer tunnel establishment, NAT traversal, punching and prediction remain covered by their owning tasks.
- Public NAT, multi-host routing, packet capture, loss injection, throughput, congestion, TLS algorithm negotiation or transport performance benchmarking.
- Treating object construction, direct handler invocation, mock dispatch or successful compilation as real-network evidence.
- Modifying the frozen artifacts of tasks 024 or 026.

### Boundary with neighboring modules

- `p2p-frame/src/sn/protocol/common.rs::PackageCmdCode`, `p2p-frame/src/sn/client/sn_service.rs` and `p2p-frame/src/sn/service/service.rs` define the closed SN client/server protocol inventory and remain production systems under test; this task does not alter their contracts.
- Dedicated test files under `p2p-frame/tests/**` own server/client setup, transport selection, synchronization, field assertions, negative cases and bounded cleanup.
- Existing `p2p-frame/src/sn/tests.rs` test-only helpers may be reused only through existing test wiring; any edit to an existing `#[cfg(test)]` file requires a testing-stage baseline snapshot and must remain mechanically test-only.
- `p2p-frame/src/networks/tcp/**` and `p2p-frame/src/networks/quic/**` are observed through real loopback listeners and command tunnels; their production code is outside this test-only task.
- `cyfs-p2p-test` is not a formal evidence source for this high-risk protocol task.

## Requirement Review

The expanded real-network coverage is reasonable. The current direct inter-SN dispatch test proves command-to-handler routing but cannot expose socket binding, authenticated tunnel registration, command framing, transport-specific startup or request/response ordering failures. Existing QUIC tests already provide parts of Report, Query, Call/Called and rendezvous evidence, so the better approach is to make the real two-client topology and selected transport explicit, reuse one transport-parametric fixture, add TCP parity and close the protocol inventory rather than creating another mock layer.

TCP and QUIC must be separate runnable cases rather than a test that merely changes an endpoint value without proving which tunnel became active. Each case therefore needs positive evidence from both clients' classified command tunnels before protocol traffic counts. End-to-end success proves that the active sender, server/client handler, body codec, response correlation and transport framing agree; existing codec tests remain responsible for byte-exact layouts and invalid discriminants.

“All protocols” must not be implemented as a hand-written count that can silently become stale. Design and testing must enumerate every `PackageCmdCode` variant and map it to the actual current send/handler/response shape. The three `*Resp` package-code variants that are not independently registered inbound commands still belong to the inventory, but their evidence must come from the corresponding QA response payload and a source-level reachability classification. A newly added future `PackageCmdCode` must make the coverage checker/test inventory fail until classified.

The requested wording could also be read as requiring the eventual peer-to-peer tunnel to be TCP or QUIC. That is not selected here because the stated topology and purpose are specifically client/server SN protocol validation. This proposal defines the transport matrix as caller-to-SN and target-to-SN command tunnels. Expanding it to peer-to-peer tunnel establishment would add TunnelManager/NAT strategy scope and requires another explicit requirement.

Loopback tests are deterministic evidence of real socket and protocol execution, not evidence of public NAT behavior. Port allocation, parallel execution and cleanup must be designed so one test cannot reuse or collide with another test's listener. If stable isolation requires serial execution, the task-local runner must make that constraint explicit.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-SRRN-1 | sn_rendezvous_real_socket_command_flow | Start one loopback SN server and two authenticated clients, then prove the complete rendezvous request/notify/response path crosses real client-to-SN command tunnels with exact field, identity and action-before-response assertions plus a bounded failure case | dedicated `p2p-frame` tests using public SN client APIs and real server listeners; production SN protocol remains unchanged | runtime tests cost more and require careful port/task cleanup, but expose framing, registration and ordering defects that mocks cannot | real non-zero listener/tunnel evidence, exact notify/response assertions, held target action proving response ordering, failure result and no false callback, unified run artifact | no mock/direct-handler substitute, production protocol change, cross-SN network or public NAT claim |
| P-SRRN-2 | sn_rendezvous_tcp_quic_transport_matrix | Execute the same required SN rendezvous success and failure semantics with both TCP and QUIC SN endpoints and prove both clients use the selected transport | client-to-SN command tunnels only; peer-to-peer tunnel transport is separate | duplicates a small high-value runtime matrix to prevent one transport implementation from masking the other | independently named TCP and QUIC cases, per-client classified tunnel IDs, selected endpoint protocol assertions and all cases passing through the unified entry | no mixed TCP/QUIC topology, transport performance or requirement that the final peer tunnel match the SN transport |
| P-SRRN-3 | sn_all_command_real_transport_matrix | Close the complete SN client/server protocol inventory and exercise Report/ReportResp, Query/QueryResp, Call/CallResp, Called/CalledResp and Rendezvous/Notify/Resp semantics across the same real TCP and QUIC two-client topology | all `PackageCmdCode` variants and their actual current send/handler/QA response shapes; excludes inter-SN, PN and NAT-probe UDP protocols | increases runtime and matrix size, but prevents a green rendezvous-only path from masking broken registration, query or call behavior and makes future command additions visible | source-derived closed inventory plus auditable protocol-family × transport success/failure mappings and unified artifacts; unregistered response-code variants explicitly classified | no invented standalone response commands, protocol implementation change, inter-SN command matrix or broad performance suite |

## Success Criteria

- Concrete system-visible result: the task runner starts a real loopback SN and two real clients and passes an auditable complete SN client/server protocol matrix over independently selected TCP and QUIC command tunnels without direct handler or in-memory relay substitution.
- For each transport, the SN listener address is loopback with a non-zero port; both clients reach online state and expose a non-zero command tunnel classified by that SN endpoint before the request begins.
- Every current `PackageCmdCode` variant appears exactly once in a source-derived inventory classification, and every active Report, Query, Call, Called and rendezvous flow maps to runnable TCP and QUIC evidence rather than a numeric count alone.
- Both transports prove Report-driven registration, known/unknown Query semantics, successful and missing/rejected Call semantics, target `SnCalled` plus returned `SnCalledResp`, and rendezvous success/failure semantics.
- The target observes exactly one valid notification with the caller's authenticated certificate identity and the request's `seq`, `tunnel_id`, operation, endpoints and prediction flag.
- The caller cannot receive a success response until the target action acknowledges the notify; after acknowledgement it receives a matching successful response with the expected prediction shape.
- Each transport has a bounded negative path that returns the defined failure semantics, does not invoke an invalid success path and leaves no test-owned task waiting after completion.
- Existing command-ID/version and codec tests remain green; real-tunnel malformed/version cases fail closed without invoking an invalid downstream handler, with wrong-version rendezvous coverage mandatory.
- Required evidence: approved design of the topology and cleanup, post-implementation test cases registered at DV/integration level, task-local unified test artifact, and source evidence that no mock/direct dispatch supplies the claimed real-network path.
- Explicit non-goals: no production protocol change, no inter-SN/owner/PN/NAT-probe protocol matrix, no cross-SN network, no peer-to-peer transport matrix, no public NAT validation and no broad workspace/quality run unless separately requested.

## Risks

- A test can appear networked while still calling the service or dispatch function directly. Acceptance must trace the request from `rendezvous_via_sn` through the active command tunnel and reject mock substitution.
- A server identity may advertise TCP or QUIC while a client reuses another active endpoint. Each client must prove the tunnel associated with the selected SN endpoint before behavioral assertions count.
- Static or incremented test ports can collide with concurrent processes or parallel tests. Setup needs bounded bind retries or another deterministic allocation strategy, and fixed-port cases may need serial execution.
- Dropping stacks without waiting for cleanup can leave listener/task state that makes later cases flaky. Design must specify bounded teardown and avoid detached test tasks.
- Loopback cannot falsify NAT, MTU, packet-loss or multi-host routing defects. Results must be described as real local socket evidence only.
- A raw wrong-version probe can accidentally bypass the same tunnel used by the public client path or assert only a local encoding error. It counts only if it is sent through the established command client tunnel and the target remains uncalled.
- `PackageCmdCode::is_sn()` currently spans numeric gaps and the existing “all commands” test enumerates only eight pre-rendezvous variants. A hard-coded count without equality against the current enum/source inventory can produce false completeness.
- `SnCallResp`, `ReportSnResp` and `SnQueryResp` package-code variants are not independently registered handlers in the current command-server path. Tests must describe their real QA response-body role and must not claim a standalone command exchange that production does not perform.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/src/sn/protocol/common.rs::PackageCmdCode`, server `register_sn_cmd_handler`, client `register_cmd_handler` and current send sites define Report, Query, Call/Called and rendezvous traffic | source-derived complete command inventory; per-family caller/callee call chain; TCP/QUIC positive and negative semantics; version/codec compatibility and future-command fail-closed inventory check | proposal enumerates all current client/server families and distinguishes QA response payloads from independently registered commands without changing wire semantics | owner: design/testing; reason: runnable topology, reachability classification and cases follow approval; acceptance impact: any omitted command variant/family, fake standalone response or missing transport mapping blocks acceptance | loopback does not prove deployed-network compatibility |
| data/schema | no | inspected scope is ephemeral SN/client runtime and dedicated test code; no durable state, database, descriptor, migration or cache schema is changed | acceptance path audit | proposal forbids durable state changes | owner: acceptance; reason: final path audit occurs after tests; acceptance impact: any durable path returns to proposal | none |
| security/privacy/permission | yes | SN command tunnels authenticate peers; Report registers identity; Query/Call return certificates; `SnCalled` and rendezvous Notify carry caller identity consumed by target handlers | assert serving SN, queried peer, caller and target certificate identities on both transports; retain fail-closed invalid/missing-target and wrong-version evidence; review logs for secrets | proposal requires authenticated online sessions and identity assertions across every identity-bearing family | owner: design/testing; reason: exact fixtures follow approval; acceptance impact: payload-only identity evidence or fail-open behavior blocks acceptance | loopback uses generated test identities, not deployed certificates |
| runtime/integration | yes | `p2p-frame/src/sn/tests.rs` starts real loopback SN/client stacks and currently has partial QUIC Report/Query/Call/rendezvous coverage; requested scope adds complete family and TCP parity | real listener and two-client lifecycle, TCP/QUIC Report/Query/Call/Called/rendezvous success/failure cases, action-before-response ordering, bounded timeout/cleanup, task-local DV/integration run | existing partial coverage and command send/handler inventory were inspected; no runtime tests changed in proposal stage | owner: design/testing; reason: topology implementation and execution are downstream; acceptance impact: mock-only, incomplete-family, flaky, hanging or single-transport evidence blocks acceptance | local sockets do not reproduce public network conditions |
| build/dependency/config/deployment | no | requested coverage can use existing `p2p-frame` test dependencies, runtime factories and loopback endpoint configuration; no manifest, lockfile, feature or deployment change is required | acceptance verifies no unexpected build/config paths | proposal forbids new production/build configuration | owner: acceptance; reason: final scope audit; acceptance impact: unexpected dependency/config work returns to proposal | platform socket scheduling can still affect timing |
| ui/datamodel/workflow | no | `SnTunnelRendezvous` is an internal Rust network protocol with no UI, form, localization or frontend data contract in the inspected module boundary | acceptance scope audit only | proposal contains no UI requirement | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | the task uses existing packet, stage-scope, testplan and unified runner contracts and does not change `harness/**`, templates, schemas, CI or process rules | run normal task checkers only | standard sibling packet and proposal manifest are used | owner: none; reason: not applicable; acceptance impact: normal checker failures still block progress | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
