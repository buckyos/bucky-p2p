---
module: p2p-frame
task_name: 026-simplify-sn-rendezvous-protocol
submodule: 026-simplify-sn-rendezvous-protocol
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# Simplify SN Rendezvous Protocol Proposal

## Background and Goal

Task `024-tunnel-rendezvous-protocol` introduced a rendezvous wire family in `p2p-frame/src/sn/protocol/sn.rs`. Its current request and response repeat information already supplied by the authenticated command tunnel, command header, endpoint encoding, and request-local lifecycle. It also splits small wire messages into envelope/body/helper structures and exposes detailed result and terminal types that are not required to choose the caller's fallback behavior.

This sibling correction narrows that protocol to the same style as the existing `SnCall` / `SnCalled` request-notify-response flow: each wire message is a flat structure, contains only information the receiver cannot obtain from its authenticated transport or local request state, and uses a small generic result value. The goal is to reduce the wire surface and implementation state without changing NAT strategy selection, endpoint ownership, real tunnel success criteria, or the legacy `SnCall` fallback.

## Scope

### In scope

- Replace the current envelope/body/digest-oriented rendezvous messages with flat request, notify, and response messages.
- Use the following minimum semantic fields as the protocol baseline:

| Message | Required fields | Reason each field remains |
|---------|-----------------|---------------------------|
| initiator -> SN request | `seq`, `tunnel_id`, `to_peer_id`, `operation`, `end_point_array`, `need_predict_endpoint` | request correlation, logical tunnel binding, SN routing, target action, concrete action targets, and optional target-owned prediction cannot be reconstructed from one another |
| SN -> target notify | `seq`, `tunnel_id`, `peer_info`, `operation`, `end_point_array`, `need_predict_endpoint` | the target needs the initiating identity certificate for the direct TLS peer; the target itself is implicit in the authenticated destination tunnel |
| response | `seq`, `result`, `predicted_endpoint_array` | correlate with the request, distinguish accepted from failed, and return target-owned concrete endpoints only when requested |

- Keep `operation` as one mutually exclusive action enum; it replaces invalid combinations of multiple action booleans.
- Keep the existing endpoint count, ownership, protocol, address, and port validation. The SN derives the initiator from the authenticated control tunnel and supplies the verified cached/report certificate in the target notification; request payload identity fields never override authenticated identity.
- Reuse the command header version instead of carrying a second rendezvous body version. Reuse `seq` plus `tunnel_id` as the request/attempt key instead of carrying a separate `attempt_id` and `request_digest`.
- Treat predicted endpoints as non-cacheable, request-scoped output. The target validates its current socket generation before responding, and the initiator consumes the response immediately within the existing bounded command/connect timeout.
- Replace the detailed rendezvous result enum with the same compact success/failure representation used by existing SN response messages. Detailed local causes remain `P2pError` values and logs; all wire failures cause the same existing compatibility or PN fallback decision.
- Remove the dedicated rendezvous Complete/Cancel commands and terminal wire structure. Both peers must retain bounded local action timeouts and owner cancellation; real tunnel establishment naturally ends the winning action, while failure/timeout performs local cleanup and fallback without an additional best-effort terminal exchange.
- Update all current producers, authenticated SN relay paths, target consumers, in-memory correlation state, and public/internal exports that consume the superseded rendezvous types.
- Preserve mixed-version behavior: a peer/SN that does not support the simplified command version is treated as unsupported and uses the existing legacy `SnCall` or PN fallback; old and new layouts must not be decoded as each other.

### Out of scope

- Changing NAT type detection, `NatProfile`, `NatTraversalContext`, the NAT combination strategy matrix, endpoint prediction math, or QUIC socket ownership.
- Adding NAT type, prediction hints, socket generation, validity timestamps, peer IDs already known from authenticated context, or policy snapshots back into the simplified wire payload.
- Changing TLS identity verification, endpoint ownership checks, actual tunnel registration/publish success criteria, PN protocol, or public `Tunnel` APIs.
- Modifying task 024's frozen artifacts or claiming wire compatibility with the uncommitted/under-development 024 layout; this task defines a replacement layout before that protocol is released as a stable baseline.
- Refactoring unrelated NAT probe and SN query protocol extensions that happen to share `sn.rs`.
- Modifying production or test code in the proposal stage.

### Boundary with neighboring modules

- `p2p-frame/src/sn/protocol/**` owns the flat wire structures, compact result value, command-version boundary, encoding limits, and removal of obsolete rendezvous command codes.
- `p2p-frame/src/sn/client/**` maps authenticated serving-SN context to request/notify handling and converts generic failure into the existing fallback path.
- `p2p-frame/src/sn/service/**` authenticates the initiator, resolves the target, inserts verified initiator `peer_info` into the notify message, relays the response, and bounds request state by `(authenticated initiator, seq, tunnel_id)`.
- `p2p-frame/src/sn/inter_sn/**` relays the minimum request/notify information without restoring redundant identity or lifecycle fields.
- `p2p-frame/src/tunnel/**` owns local timeouts, collision handling, action cancellation, waiter cleanup, fallback, and the only real success condition: a registered tunnel.
- `p2p-frame/src/networks/quic/**` continues to validate and use the current traversal socket for prediction/punch/connect; its generation remains a local guard rather than a wire field.

## Requirement Review

The requested simplification is reasonable and should happen before treating the new rendezvous protocol as stable. The current payload contains three separate correlation concepts (`attempt_id`, `tunnel_id`, and `request_digest`), repeats three identities even though both command hops are authenticated, duplicates protocol versioning already present in the command header, and exposes local prediction lifetime metadata to a consumer that uses the result immediately.

The minimum contract above deliberately keeps `seq` and `tunnel_id`: `seq` correlates the SN request/response in the established protocol style, while `tunnel_id` binds the subsequent reverse/direct connection to the logical tunnel waiter. It also keeps `peer_info` only on the SN-to-target notify because the target needs the initiator certificate to authenticate and name the direct peer; the SN must source it from authenticated registration state rather than trust a client-supplied duplicate.

Removing terminal messages trades prompt remote cancellation for a smaller and more reliable request/response contract. This is acceptable only if design preserves a short hard timeout on every target action and proves cleanup on response failure, caller cancellation, collision, control-tunnel loss, and timeout. If that bounded cleanup cannot be demonstrated, design must return this task to proposal rather than silently reintroduce the existing terminal envelope.

The simplified layout is a replacement, not an additive extension of task 024's new layout. Because the current repository work is not yet a released compatibility baseline, coordinated replacement is preferable to permanently carrying unused fields. Existing legacy `SnCall` compatibility remains unchanged.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-SSRP-1 | simplify_sn_rendezvous_wire_contract | Replace the rendezvous envelope/body/digest/terminal family with flat minimum request, notify, and response messages containing only the fields listed in this proposal; use compact result semantics and remove redundant identity, version, transport, deadline, generation, validity, digest, and terminal fields | `p2p-frame` SN protocol/client/service/inter-SN producers and consumers plus tunnel-local correlation; legacy `SnCall` remains separate | Removes detailed wire failure categories and prompt terminal notification in exchange for a substantially smaller contract; local logs, bounded timeouts, and existing fallback retain operational behavior | source-level field inventory proves no removed field remains on wire; positive/negative codec tests prove exact flat layouts and bounds; caller/callee closure tests prove authenticated routing, prediction true/false behavior, generic failure fallback, timeout/cancel cleanup, same-SN and cross-SN flow, mixed-version rejection, and real tunnel success semantics | no NAT strategy, prediction algorithm, TLS, PN, public Tunnel API, NAT probe, or SN query change |

## Success Criteria

- Concrete system-visible result: `sn.rs` exposes only the flat minimum rendezvous request, notify, response, and action types; it no longer exposes rendezvous envelope/body/digest/terminal structures or their redundant fields.
- The request and notify field sets exactly match the tables above; the response contains only `seq`, `result`, and `predicted_endpoint_array`.
- SN routing and target identity are derived from authenticated control-tunnel context; the target initiator identity is derived from SN-supplied verified `peer_info`, with spoofed/mismatched identities rejected.
- `need_predict_endpoint=false` produces an empty predicted endpoint list; `true` produces a validated non-empty list on success or a generic failure that triggers fallback.
- Every local rendezvous action has a fixed upper bound and deterministic owner/waiter cleanup without Complete/Cancel wire messages; no detached punch/connect task survives timeout, fallback, collision, or tunnel success.
- Existing `SnCall` and PN fallback remain operational, and unsupported/mixed rendezvous versions fail closed into those paths rather than being misdecoded.
- Required evidence: design must provide the exact old-to-new field/type/command mapping, source-level producer/consumer closure, timeout/cancellation state diagram, compatibility classification, and concrete Scope Paths; post-implementation testing must include codec boundaries, identity abuse cases, lifecycle failure modes, same-SN/cross-SN integration, and task-local unified test artifacts.
- Explicit non-goals: no change to NAT classification, prediction math, endpoint ownership, QUIC/TLS tunnel success, PN wire protocol, or unrelated SN extensions.

## Risks

- Removing explicit body versioning is safe only while the command header version is checked consistently at every client, SN, and inter-SN entry point.
- Removing repeated identity fields can become fail-open if any handler trusts message routing without also checking the authenticated command peer and verified certificate source.
- Removing `request_digest` requires state to reject conflicting reuse of `(authenticated initiator, seq, tunnel_id)` using internal request comparison or an equivalent local invariant.
- Removing generation/validity fields requires prediction to be checked immediately before response construction and consumed only inside the bounded request lifecycle; predicted endpoints must not enter a long-lived cache.
- Removing terminal messages can leave remote actions running until their local deadline. The deadline must be short, mandatory, owner-bound, and covered for all exit paths.
- The replacement wire layout is intentionally incompatible with task 024's current new layout; all participating new components must be upgraded together, while legacy `SnCall` remains the rolling-upgrade fallback.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/src/sn/protocol/sn.rs` currently defines the rendezvous envelope, request/response bodies, response, terminal, digest input, detailed result enum and validation; `p2p-frame/src/sn/protocol/common.rs` owns four rendezvous command codes | design field/type/command mapping, compatibility and version note, caller/callee impact review, positive/negative codec tests, boundary validation | proposal fixes the minimum semantic field sets and removal baseline | owner: design/testing; reason: exact Rust names/layout and executable codecs follow approval; acceptance impact: any extra unproved wire field, misdecode, or missing consumer blocks acceptance | coordinated new-component deployment is required |
| data/schema | no | `p2p-frame/src/sn/service/rendezvous_state.rs` stores only TTL-bounded in-memory attempt state; no database, file, descriptor, or durable schema is in scope | design confirms no durable path and acceptance audits Scope Paths | proposal forbids persistent/cached predicted endpoint state | owner: acceptance; reason: path audit occurs after implementation; acceptance impact: any durable schema change returns to proposal | short-lived in-memory duplicates remain possible until cleanup |
| security/privacy/permission | yes | `p2p-frame/src/sn/service/service.rs::handle_rendezvous` authenticates the command peer and currently copies `initiator_peer_info`; `p2p-frame/src/sn/client/sn_service.rs::on_rendezvous_notify` validates the initiator certificate | trust-boundary review, spoofed sender/target/certificate and third-party endpoint negatives, secret/log review | proposal assigns identity to authenticated tunnel plus SN-supplied verified certificate and preserves endpoint ownership validation | owner: design/testing; reason: exact denied paths require the replacement handlers; acceptance impact: any payload-controlled identity or missing abuse case blocks acceptance | a compromised authenticated SN remains trusted to supply peer identity, matching the existing SN model |
| runtime/integration | yes | `p2p-frame/src/tunnel/tunnel_manager.rs` currently uses terminal messages, absolute deadline and attempt owner state; `p2p-frame/src/sn/service/rendezvous_state.rs` coordinates same/cross-SN request state | timeout/cancellation lifecycle design, conflicting-key behavior, failure-mode tests, same-SN/cross-SN DV/integration, log review | proposal requires fixed bounded owner cleanup and internal conflicting-request rejection | owner: design/testing; reason: exact timeout and state machine are downstream; acceptance impact: detached work, duplicate publication, missing fallback, or unbounded state blocks acceptance | remote cleanup may be delayed until the short local timeout after caller loss |
| build/dependency/config/deployment | no | requested paths are existing Rust source/tests/docs; no `Cargo.toml`, lockfile, feature, config, packaging, or deployment surface is needed | acceptance Scope Paths audit | proposal introduces no dependency or configuration | owner: acceptance; reason: final path audit; acceptance impact: unexpected build/config changes return to proposal | coordinated binary rollout is an operational compatibility note, not a build-system change |
| ui/datamodel/workflow | no | rendezvous is an internal Rust SN/tunnel contract with no UI consumer in `docs/modules/p2p-frame.md` or inspected call paths | acceptance confirms no UI paths | proposal defines no UI state or workflow | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | task uses existing packet, admission, stage-scope, testing and acceptance machinery without modifying `harness/**`, templates, CI, or rules | run normal task checkers only | proposal creates a standard sibling packet and index entry | owner: none; reason: not applicable; acceptance impact: normal checker failure still blocks progression | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
