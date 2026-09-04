---
module: p2p-frame
task_name: 012-tcp-reverse-data-first-claim
submodule: 012-tcp-reverse-data-first-claim
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# TCP Reverse Data First-Claim Handoff Proposal

## Background and Goal

When a TCP tunnel has no reusable data connection, `TcpTunnel::open_channel` first tries to create one locally. If that direct data dial fails, it sends `OpenDataConnReq` and waits for the peer to create a reverse data connection. The reverse connection can complete TCP/TLS and `DataConnReady(Success)`, but the requesting side receives it as `FirstClaimPending` with `created_by_local = false`. The requesting side then immediately calls `claim_entry`, whose first-claim rule permits only `created_by_local = true`, so it returns `ErrorState("connection not claimable")`.

The outer open loop currently treats that local state error as retryable alongside a real simultaneous-claim conflict. Repeating the same reverse flow four times therefore ends as `Conflict("claim retries exhausted")`, even though no peer claim conflict is required. On a PN server this prevents `PnServer` from opening the target-side `proxy_service` stream after the target has successfully connected back over TCP.

The goal is to make reverse data connection fallback complete one safe first channel claim: both peers must agree which side owns that first claim, and the designated claimant must not send it until the other side has completed registration. The result must remove the deterministic retry exhaustion without weakening claim ordering, lease consistency, or mixed-version safety.

## Scope

### In scope

- Correct the TCP `open_channel` fallback used when local data connection creation fails and `OpenDataConnReq` causes the peer to connect back.
- Define an explicit, race-free registration barrier for a reverse data connection before the requesting side may start its first claim.
- Define first-claim ownership for direct data connections and `OpenDataConnReq`-associated reverse data connections so both peers derive the same owner and reject the opposite side.
- Preserve `conn_id`, `request_id`, `lease_seq`, `channel_id`, `claim_nonce`, listener validation, accept-queue limits, drain, reuse, and retire invariants after the first lease is bound.
- Handle direct-dial failure, reverse-connect failure, registration failure, timeout/cancellation, late reverse arrival, duplicate or mismatched readiness, and tunnel close without leaving an unclaimable pooled connection or a pending waiter.
- Stop translating repeated local `ErrorState` failures into a misleading claim-conflict terminal error; terminal errors and diagnostics must retain whether failure came from direct connection creation, reverse registration/handoff, or actual claim arbitration.
- Require a compatibility decision for any TCP wire change. Mixed-version peers must either interoperate safely or fail explicitly before business bytes can be mistaken for handshake bytes; silent corruption is not acceptable.
- Require post-implementation unit coverage of the state machine and a TCP PN/TTP development-validation or integration path that forces direct data dial failure and succeeds through reverse data connection fallback.

### Out of scope

- Increasing the fixed claim retry count, adding a fixed sleep, or allowing every remote-created `FirstClaimPending` entry to be claimed without a registration barrier.
- Changing TLS certificate selection, peer identity verification, TCP listener acceptance, PN admission policy, `ProxyOpenReq` fields, or PN relay bridge semantics.
- Changing the meaning of the PN logical `tunnel_id`, the underlying TCP `tunnel_id`, or requiring those two IDs to match.
- Reworking normal direct data connection creation, established `Idle` connection reuse, drain completion, heartbeat behavior, or QUIC tunnel behavior except where a shared interface must remain source-compatible.
- Adding a general-purpose multiplexing layer or carrying ordinary business payloads on the TCP control connection.
- Modifying production code, tests, design artifacts, or testing artifacts during this proposal stage.

### Boundary with neighboring modules

- `p2p-frame/src/networks/tcp/tunnel.rs` owns reverse data connection request tracking, local entry state, first-claim eligibility, timeout cleanup, and error propagation.
- `p2p-frame/src/networks/tcp/protocol.rs` owns any narrowly required handshake/control representation. A wire change is allowed only when the approved design records its compatibility and rollout behavior.
- `p2p-frame/src/ttp/server.rs` and `p2p-frame/src/pn/service/pn_server.rs` remain consumers of `Tunnel::open_stream`; their public behavior should recover without introducing a PN-specific workaround.
- Existing public `Tunnel`, TTP, and PN APIs remain unchanged unless the design demonstrates that an internal-only solution cannot meet the registration and compatibility requirements.
- `cyfs-p2p`, `cyfs-p2p-test`, and `sn-miner-rust` are downstream compatibility and scenario-validation surfaces, not owners of an alternative TCP claim protocol.

## Requirement Review

- The requested repair is necessary. The current reverse fallback returns a connection to the side that requested it and then applies a first-claim rule that makes that connection unusable by the caller.
- Merely accepting a request-side claim on `FirstClaimPending` is unsafe. `DataConnReady` travels on the data connection while `ClaimConnReq` travels on the control connection; without a barrier, the claim may reach the creator before its data-side task has consumed readiness and registered `conn_id`.
- Increasing retries or delaying by a fixed duration would only hide the cross-connection ordering defect and remain timing-dependent under load or WAN latency.
- The chosen direction is an explicit reverse-registration handoff: the later design must identify a reliable event proving that the physical creator has completed registration, then assign first-claim authority to exactly one side for that reverse request. The exact command/state representation belongs to design, but it must not rely on scheduler timing.
- A narrow handshake extension is preferable to PN-specific handling because the defect belongs to the shared TCP tunnel mechanism and affects every TTP consumer using reverse data fallback.
- Compatibility is material. If the safe handoff requires a wire-visible message or changes the number/order of registration frames, the design must state whether existing peers can negotiate it; otherwise it must define explicit coordinated-deployment rejection rather than claim backward compatibility.
- Error fidelity is part of the repair boundary because the current terminal `Conflict` obscures whether retries were triggered by local state, direct dial, or peer arbitration, making production diagnosis misleading.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-TRDFCH-1 | tcp_reverse_data_first_claim_handoff | A TCP channel open that falls back to `OpenDataConnReq` must complete a race-free reverse registration and exactly one valid first claim, or return the actual setup/timeout/protocol error without exhausting synthetic claim conflicts; both peers must agree on first-claim ownership and mixed versions must not silently corrupt data | TCP data registration/claim state and narrowly required internal wire handling in `p2p-frame`; TTP/PN remain consumers and public PN/TTP behavior and identity boundaries stay unchanged | Adds an explicit synchronization/ownership state and may require a coordinated wire update; this complexity is accepted to remove an impossible first-claim path without timing sleeps or weakened protocol validation | Deterministic state-machine tests force direct dial failure, complete reverse registration, prove the request-side open succeeds only after creator registration, cover early/late/duplicate/timeout/close/error paths, distinguish real conflict from local setup failure, and exercise an A-to-PN-to-B TCP proxy stream through the canonical task test entry | No retry-count workaround, no PN-specific bypass, no unconditional remote-first-claim relaxation, no TLS/identity/tunnel-id change, no broad TCP reuse or QUIC refactor |

## Success Criteria

- Concrete user-visible or system-visible result: when a PN cannot directly create a target-side TCP data connection but the target can connect back, `PnServer` successfully opens the target `proxy_service` stream instead of returning `Conflict: claim retries exhausted`.
- Required evidence: an approved design maps `tcp_reverse_data_first_claim_handoff` to concrete protocol states, ordering, compatibility behavior, failure cleanup, logs, and `Scope Paths`; post-implementation testing provides deterministic positive and negative unit cases plus a TCP reverse-fallback PN/TTP DV or integration run through `harness/scripts/test-run.py`.
- Required compatibility evidence: direct data connection behavior remains compatible; reverse data peers either negotiate/interoperate safely or reject unsupported mixed versions before the data connection becomes a business stream.
- Required failure evidence: timeout, cancellation, late connection, duplicate/mismatched readiness, listener rejection, tunnel close, and genuine claim conflict do not leak waiters/connections and return distinguishable error categories.
- Explicit non-goals: no fixed-delay/retry-count workaround, no PN-only patch, no public PN/TTP API redesign, no TLS/identity change, no tunnel-ID unification, and no unrelated transport refactor.

## Risks

- A claim sent before the creator has registered the reverse `conn_id` can be rejected as `ProtocolError`; the handoff must create an actual happens-before relationship across the data and control connections.
- A wire-visible readiness or ownership message can break mixed-version peers. The design must use negotiation/versioning or document coordinated deployment with explicit failure before extra handshake bytes can enter the business stream.
- If timeout and late arrival race, the requesting side may retain an unclaimable `FirstClaimPending` connection or the creator may wait indefinitely. Both sides need bounded cleanup and deterministic late-arrival policy.
- Incorrect first-claim-owner derivation can let both sides claim or neither side claim. Tests must assert the owner decision from both tunnel forms and both `conn_id` high-bit partitions.
- Reclassifying every `ErrorState` as fatal could regress legitimate local selection races. The design must distinguish stale entry selection from the deterministic reverse-ownership mismatch and preserve bounded recovery only for genuinely recoverable cases.
- PN validation and relay forwarding occur after the transport stream opens; transport recovery must not bypass `proxy_service` listener checks, assigned-target validation, or `ProxyOpenReq` response handling.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/src/networks/tcp/protocol.rs` defines `OpenDataConnReq`, `TcpConnectionHello`, `DataConnReady`, and claim frames; `p2p-frame/docs/tcp_tunnel_protocol_design.md` assigns first claim to the physical creator while `TcpTunnel::open_channel` asks the requester to claim the returned reverse connection | Design records exact state/message ordering, caller/callee impact, direct and reverse compatibility matrix, versioning/rollout decision, and boundary validation; testing includes positive, negative, duplicate, malformed, and mixed-version/framing checks | Proposal inspection traced reverse request, incoming registration, claimant eligibility, peer-side first-claim rejection, and terminal retry mapping | owner: design/testing; reason: wire/state shape and executable compatibility evidence belong to later stages; acceptance impact: missing safe mixed-version decision or contract coverage blocks admission/acceptance | A partial handshake change could be interpreted as business bytes or create asymmetric claim ownership |
| data/schema | no | The affected state is in-memory TCP tunnel state; scope excludes persisted records, file formats, cache keys, migrations, and durable schemas | Design and diff review confirm no persisted-data surface enters scope | Proposal inspection found only runtime connection/request/lease identifiers | owner: none; reason: not applicable; acceptance impact: none | none |
| security/privacy/permission | yes | TCP TLS peer identity remains the trust root, and `check_accept_target` plus PN validators must still reject unauthorized/unlistened purposes; a handoff must not let an unauthenticated or stale reverse connection acquire a lease | Design names identity, request correlation, listener, and validator boundaries; testing covers mismatched request/connection identifiers, unsupported/malformed handoff, listener rejection, and no sensitive log expansion | Proposal inspection confirmed TLS and PN validation are upstream/downstream boundaries rather than the failure source | owner: design/testing; reason: negative implementation evidence is not yet available; acceptance impact: any fail-open claim or validation bypass blocks acceptance | Incorrect request correlation could bind a valid TLS peer connection to the wrong pending open |
| runtime/integration | yes | `p2p-frame/src/networks/tcp/tunnel.rs` coordinates concurrent control/data connections, timeouts, pending request maps, claim retries, connection pools, and close cleanup; the observed PN failure occurs on this distributed ordering path | Design supplies lifecycle/state diagrams and failure behavior; testing covers ordering races, timeout/cancel/late arrival, cleanup, genuine conflict, and TCP PN/TTP DV or integration | Proposal inspection identified the missing registration happens-before edge and lossy retry wrapper | owner: design/testing; reason: deterministic seams and runnable topology are later-stage work; acceptance impact: no forced reverse-fallback execution means the repair is unproven | Rare scheduling orders could deadlock, leak entries, or still exhaust retries |
| build/dependency/config/deployment | yes | A wire compatibility decision can require coordinated binary deployment even if Cargo/config files remain unchanged | Design records capability/version mechanism or coordinated rollout and rollback expectations; testing records supported/unsupported mixed-version matrix | Proposal explicitly forbids silent mixed-version corruption | owner: design/testing/release; reason: exact deployment impact depends on selected handshake; acceptance impact: unspecified rollout or rollback blocks acceptance | Rolling upgrades may temporarily pair incompatible TCP tunnel implementations |
| ui/datamodel/workflow | no | The task changes transport internals and PN/TTP stream establishment; there is no UI state, presentation model, accessibility, localization, or frontend workflow surface | Source and design scope review confirm no UI files or contracts change | Proposal boundary is limited to `p2p-frame` transport behavior and downstream runtime validation | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | The task uses existing proposal/design/admission/testing/acceptance machinery and does not change `harness/**`, templates, checker schemas, CI wiring, or governance rules | Run existing stage-owned checks only when their inputs change | Proposal packet, sequence allocation, index, and scope evidence use current Harness rules | owner: downstream stages; reason: later checks belong to their owning stages; acceptance impact: missing normal gate evidence blocks acceptance | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
