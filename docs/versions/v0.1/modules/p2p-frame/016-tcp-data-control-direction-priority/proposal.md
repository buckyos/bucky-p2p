---
module: p2p-frame
task_name: 016-tcp-data-control-direction-priority
submodule: 016-tcp-data-control-direction-priority
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# TCP Data Connection Control-Direction Priority Proposal

## Background and Goal

`TcpTunnel::open_channel` currently reuses an existing claimable data connection when possible. When no reusable connection exists, it always tries `create_data_connection(None)` first, so the side opening the stream or datagram locally initiates the new TCP data connection regardless of which side initiated the tunnel's control connection. Only after that local dial fails does it send `OpenDataConnReq` and ask the peer to establish a reverse data connection.

This fixed local-first order is suitable for an `Active` TCP tunnel because the data connection initially follows the established control-connection direction. It is the opposite of the control-connection direction for a `Passive` TCP tunnel, where the control initiator is the peer. In asymmetric-NAT, listener, or reachability conditions, trying the direction already proven by the control connection first is more likely to succeed and avoids an unnecessary failed dial before using the peer-owned direction.

The goal is to make new TCP data connections prefer the control connection's direction: the control initiator should receive the first opportunity to establish the data connection. Failure of the preferred direction must still fall back to the opposite direction so the change improves selection order without removing an existing connectivity path.

## Scope

### In scope

- Change only the connection-attempt order used by TCP stream/datagram channel opens when no existing claimable data connection can be reused.
- For an `Active` TCP tunnel, continue to try a locally created data connection first, then request a peer-created reverse data connection if the local attempt fails.
- For a `Passive` TCP tunnel, request a peer-created data connection first because the peer initiated the control connection, then try a locally created data connection if the peer-created attempt fails.
- Preserve the existing direct and reverse data connection creation mechanisms, request correlation, registration barrier, first-claim ownership, timeout/cancellation cleanup, and error-category fidelity.
- Preserve both failure causes when the preferred and fallback directions fail, with diagnostics ordered or labelled so the actual attempt direction is clear rather than always calling the local attempt `direct` and the peer attempt `reverse` without tunnel-form context.
- Require deterministic post-implementation coverage for `Active` and `Passive` preferred-direction selection, preferred-direction success without an opposite-direction attempt, and preferred-direction failure followed by opposite-direction success/failure.

### Out of scope

- Changing reuse or claim selection for an already registered `Idle` or locally claimable `FirstClaimPending` data connection.
- Removing the opposite-direction fallback, racing both directions, changing retry counts, or adding fixed delays.
- Changing `OpenDataConnReq`, `OpenDataConnResp`, `TcpConnectionHello`, `DataConnReady`, claim frames, identifiers, or other TCP wire encodings.
- Changing the request-correlated reverse registration handoff and requester-owned first claim delivered by `012-tcp-reverse-data-first-claim`.
- Changing TLS identities, certificate selection, listener validation, vport/purpose acceptance, queue limits, heartbeat, tunnel publication, PN/TTP behavior, QUIC behavior, or public `Tunnel` APIs.
- Introducing a new `TunnelForm` variant or speculative TCP `Proxy` construction behavior; the current concrete TCP control-direction decision is based on the existing `Active` and `Passive` forms.
- Modifying production code, tests, design artifacts, or testing artifacts during this proposal stage.

### Boundary with neighboring modules

- `p2p-frame/src/networks/tcp/tunnel.rs` owns `TcpTunnel::open_channel`, current `TunnelForm`-based local/peer identity, data-connection creation, remote-open requests, first claim, and combined error propagation.
- `p2p-frame/src/networks/tcp/network.rs` and `p2p-frame/src/networks/tcp/listener.rs` establish the existing `Active`/`Passive` control-connection forms. This task consumes that established direction and does not change control acceptance or registry behavior.
- `p2p-frame/src/networks/tcp/protocol.rs` remains unchanged unless design inspection finds an unavoidable contradiction; the requested priority can be expressed using the current control commands and must not silently become a wire-format change.
- TTP, PN, `cyfs-p2p`, `cyfs-p2p-test`, and `sn-miner-rust` remain consumers or validation surfaces. They must not implement an alternative direction policy or a transport-specific workaround.

## Requirement Review

- The requested behavior is reasonable. The successful control connection provides a concrete directional reachability signal, while the current local-first policy discards that signal on every passive-side channel open that needs a new data connection.
- "Prefer" is interpreted as ordered fallback, not a hard restriction: follow the control direction first and retain the opposite direction second. Removing fallback would reduce connectivity under changing or asymmetric reachability and is not required by the request.
- Existing pooled data connections remain preferable to establishing any new connection. The request changes new-connection direction only; forcing a new same-direction connection when a reusable connection exists would regress reuse and resource behavior.
- `Active` and `Passive` already encode which side initiated the control connection for concrete `TcpTunnel` instances. The design should centralize the attempt-order decision from that state rather than duplicate separate stream/datagram branches or infer direction from endpoint addresses.
- No TCP frame change is expected. `OpenDataConnReq/Resp` already provides the peer-created path, and the reverse first-claim task already provides the required registration barrier and ownership rules. The design must explicitly verify that reordering calls does not bypass those rules.
- Error diagnostics must follow semantic attempt direction. After reordering the passive path, a fixed `direct failed; reverse failed` message would be misleading unless it clearly distinguishes local-created and peer-created attempts and their order.
- The main risk is runtime ordering and fallback regression, not durable data or API migration. Design and testing must prove exactly one preferred attempt starts first and that the second direction is attempted only after a concrete first-direction failure.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-TDCDP-1 | tcp_data_control_direction_priority | When a TCP channel needs a new data connection, prefer creation in the established control-connection direction: `Active` tries local-created then peer-created, while `Passive` tries peer-created then local-created; reuse stays first, the opposite direction remains fallback, and existing reverse registration/claim safety is preserved | `p2p-frame` TCP tunnel attempt selection and direction-aware diagnostics only; current control/data protocol frames and public consumers remain unchanged | Passive opens add the existing control request/response round trip on their preferred success path, but avoid first attempting a direction opposite to the proven control path; retaining fallback preserves broader reachability at the cost of sequential failure latency when both directions fail | Deterministic tests assert Active and Passive attempt order, no fallback after preferred success, fallback after preferred failure, combined dual-failure diagnostics, reuse before new connection, and unchanged reverse registration/first-claim behavior; a task-scoped TCP DV or integration path demonstrates passive-side channel open using the control initiator's data direction | No wire change, no simultaneous race, no retry/delay tuning, no pool/claim redesign, no PN/TTP workaround, no TLS/listener/public API change |

## Success Criteria

- Concrete system-visible result: when a `Passive` `TcpTunnel` opens a stream or datagram without a reusable data connection, it first asks the peer that initiated the control connection to create the data connection; an `Active` tunnel continues to create locally first.
- Required fallback result: if the preferred control-direction attempt fails, the same channel open attempts the opposite direction and may still succeed; if both fail, the returned error preserves both direction-labelled causes.
- Required invariant evidence: reusable connections are still selected before new connection creation, preferred success does not launch an unnecessary fallback, and the existing request-correlated registration barrier plus requester-owned first claim remain intact.
- Required validation evidence: post-implementation unit cases cover both forms and all preferred/fallback outcomes, and a task-scoped TCP DV or integration case exercises the passive-side preferred direction through the canonical runner.
- Required compatibility evidence: no TCP wire shape, identifier partition, TLS identity, public `Tunnel` API, or downstream consumer contract changes; existing peers continue to understand all exchanged frames.
- Explicit non-goals: no bidirectional racing, no fallback removal, no retry or timeout redesign, no data-pool/claim refactor, and no unrelated transport cleanup.

## Risks

- If `TunnelForm` is interpreted as business-call direction rather than control-connection ownership, the order could be inverted. Design must trace the exact constructors in `network.rs` and `listener.rs` and bind `Active`/`Passive` to control initiator/acceptor.
- A reordered passive flow could accidentally request remote creation after a pending reverse request already exists or fail to clean the request on cancellation. Existing pending-request ownership and cleanup must remain the single mechanism.
- Retrying the opposite direction on every error may be wrong for tunnel closure, protocol corruption, or authentication/validation failure. Design must classify which preferred-direction errors permit fallback and which terminal errors should stop immediately, using existing behavior as the compatibility baseline.
- Passive preferred creation depends on the control connection being writable and the peer being able to service `OpenDataConnReq`; the retained local fallback is required for recovery when either assumption fails.
- Direction-specific tests that observe only final success can miss an unnecessary or reversed first attempt. Test design must expose attempt events or deterministic failure hooks and assert ordering directly.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/src/networks/tcp/tunnel.rs` currently sequences `create_data_connection(None)` before `request_remote_data_connection()` in `open_channel`; reordering when `OpenDataConnReq` is sent changes distributed TCP tunnel behavior even though `protocol.rs` frame shapes remain unchanged | Design maps Active/Passive control ownership to exact call order, records no-wire-change compatibility, classifies fallback errors, and names TTP/PN consumers; testing covers positive/negative direction ordering and verifies current frames remain unchanged | Proposal inspection traced the constructors, current local-first branch, existing peer-open command, and reverse registration barrier | owner: design/testing; reason: exact helper shape, error policy, and executable contract evidence belong to later stages; acceptance impact: missing direction mapping or compatibility evidence blocks admission/acceptance | An asymmetric implementation could make peers disagree about who should create or claim the connection |
| data/schema | no | The affected state in `p2p-frame/src/networks/tcp/tunnel.rs` is in-memory connection/request state; scope excludes persisted schemas, serialized durable state, cache keys, migrations, and reset behavior | Design and diff review confirm no persistent-data surface enters scope | Proposal inspection found no durable data owner or file-format change | owner: none; reason: not applicable; acceptance impact: none | none |
| security/privacy/permission | no | TLS identity and authorization checks remain in existing TCP connect/accept and listener-validation paths; the proposal changes only whether current authenticated local-created or peer-created mechanisms run first and explicitly excludes bypasses | Design/diff review confirms both ordered branches still use existing TLS, tunnel correlation, and listener validation | Proposal inspection confirmed no credential, identity, permission, PII, secret, or trust-policy semantics change | owner: none; reason: not applicable unless design introduces a new bypass or validation surface, which would return to proposal; acceptance impact: any discovered trust-boundary change invalidates this row and blocks implementation | Incorrect later implementation could skip existing validation even though it is outside the approved requirement |
| runtime/integration | yes | `TcpTunnel::open_channel` coordinates network attempts, control requests, timeouts, pending request cleanup, and claims; changing attempt order affects runtime latency, failure propagation, and asymmetric-connectivity behavior | Design describes ordered lifecycle, terminal versus fallback-eligible failures, cancellation/close behavior, and observability; testing asserts exact attempt order, success short-circuit, both fallback directions, timeout/close cleanup, and a TCP DV/integration path | Proposal inspection identified current unconditional local-first runtime ordering and reusable-connection precedence | owner: design/testing; reason: lifecycle details and executable failure injection belong to later stages; acceptance impact: missing ordering or cleanup coverage blocks acceptance | A fallback may start too early, fail to start, or leave a pending request after cancellation |
| build/dependency/config/deployment | no | Scope is limited to existing Rust TCP tunnel logic and tests; no Cargo metadata, dependency, feature, configuration, packaging, generated resource, or deployment default is changed | Design and diff review confirm no build/config surface enters scope | Proposal inspection found no build or deployment input in the current call path | owner: none; reason: not applicable; acceptance impact: none | none |
| ui/datamodel/workflow | no | The task affects transport-internal connection ordering and has no UI state, presentation model, accessibility, localization, navigation, or frontend/backend data contract | Source and scope review confirm no UI paths or presentation contracts are consumers | Proposal boundaries remain within `p2p-frame` TCP runtime behavior | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | The task consumes existing packet, admission, stage-scope, testing, and acceptance machinery and does not change `harness/**`, templates, checker schemas, CI wiring, or governance behavior | Run only the existing stage-owned checks when their inputs change | Proposal packet and scope evidence use the current repository workflow unchanged | owner: later stages; reason: normal task checks remain stage-owned rather than process changes; acceptance impact: missing ordinary evidence blocks completion but does not make this a Harness change | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
