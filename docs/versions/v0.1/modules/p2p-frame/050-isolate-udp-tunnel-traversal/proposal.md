---
task_manifest: task.yaml
status: approved
---

# Isolate UDP Tunnel Traversal Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: Removing methods from the public `TunnelNetwork` trait changes an exported Rust contract and its object-safe consumer boundary. The replacement must preserve QUIC NAT traversal behavior while migrating every in-repository caller and verifying that TCP, PN, and generic fake networks no longer inherit UDP-only operations.
- Proposal and tier confirmation: User confirmed this proposal and launched automatic completion on 2026-09-04 with the statement “确定，自动完成”.

## Background and Goal
The common `TunnelNetwork` trait currently declares `punch_only`, `predict_traversal_endpoints`, and `validate_traversal_prediction` with default `NotSupport` implementations. These operations require a UDP listener/socket binding and are implemented in production only by `QuicTunnelNetwork`; TCP, PN, and generic/custom network implementations should not expose them through the common tunnel-network contract.

The goal is to remove all three operations from `TunnelNetwork` and define a dedicated `UdpTunnelNetwork: TunnelNetwork` trait containing them. `TunnelNetwork` gains an object-safe `as_udp_tunnel_network()` capability accessor with a default `None` result, while `QuicTunnelNetwork` returns `Some(self)` and implements `UdpTunnelNetwork`. This preserves the existing single `NetManager` registry without changing QUIC punch, endpoint prediction, validation, SN, or rendezvous behavior.

## Scope

### In scope
- Remove `punch_only`, `predict_traversal_endpoints`, and `validate_traversal_prediction` from `TunnelNetwork`.
- Introduce `UdpTunnelNetwork: TunnelNetwork`; the supertrait constraint requires every UDP traversal implementation to also implement the common tunnel-network contract. The new trait contains exactly `punch_only`, `predict_traversal_endpoints`, and `validate_traversal_prediction` and is currently implemented by `QuicTunnelNetwork` only.
- Add `TunnelNetwork::as_udp_tunnel_network() -> Option<&dyn UdpTunnelNetwork>` with a default `None` implementation; `QuicTunnelNetwork` returns `Some(self)`.
- Keep the existing `NetManager` registry and make SN probing and `TunnelManager` explicitly obtain the UDP capability through `as_udp_tunnel_network()` before invoking UDP-only behavior.
- Migrate all production callers and test doubles to the new boundary.
- Update public API/compile-contract tests, including the existing signed-PNAT fixture, and the long-lived `p2p-frame` module boundary description.

### Out of scope
- Do not change PNAT, SN rendezvous, tunnel, QUIC, TCP, or PN wire formats.
- Do not change punch cadence, source-socket ownership, endpoint prediction, signature verification, TTL/generation validation, candidate selection, timeout, or fallback semantics.
- Do not make PN or TCP implement UDP traversal capability merely because a type reports `is_udp()` or carries datagrams.
- Do not redesign the remaining `TunnelNetwork` creation/listening contract or remove `is_udp()` in this task.

### Boundary with neighboring modules
`networks` owns the common `TunnelNetwork` contract and the separate `UdpTunnelNetwork: TunnelNetwork` contract. `QuicTunnelNetwork` owns the only current UDP implementation because it owns the reusable UDP listener/socket binding. `NetManager` continues exposing `TunnelNetworkRef`; SN client probing and tunnel rendezvous explicitly request its UDP capability through `as_udp_tunnel_network()`. Neighboring crates may continue consuming `TunnelNetwork`, but direct implementations or callers of the removed methods must migrate to `UdpTunnelNetwork`.

## Requirement Review
The requested boundary correction is reasonable: default `NotSupport` methods make unsupported UDP traversal operations appear to be universal tunnel-network behavior and force unrelated implementations/tests to carry an artificial contract. A dedicated `UdpTunnelNetwork: TunnelNetwork` keeps generic tunnel creation/listening separate from UDP socket traversal, while `as_udp_tunnel_network()` provides object-safe capability discovery from the existing generic registry.

The material tradeoff is source compatibility. Removing exported trait methods is intentionally breaking for external code that calls or implements those methods. Keeping deprecated forwarding methods on `TunnelNetwork` would preserve the incorrect ownership boundary, so this proposal does not retain them. The new accessor is additive and defaults to `None`; implementations opt in explicitly. Runtime, registry ownership, and wire behavior remain unchanged.

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-isolate-udp-tunnel-traversal | Remove the three UDP traversal operations from `TunnelNetwork`; define them on `UdpTunnelNetwork: TunnelNetwork`; add the default-`None` `as_udp_tunnel_network()` accessor; make `QuicTunnelNetwork` opt in with `Some(self)`; and migrate all callers. | Generic TCP, PN, fake, and custom tunnel networks retain only the default `None` capability unless they explicitly implement the UDP supertrait; `NetManager` keeps one `TunnelNetworkRef` registry. | Direct users of the removed public trait methods require a source migration; no deprecated forwarding shim is retained. | Repository scan shows the methods absent from `TunnelNetwork` and present on `UdpTunnelNetwork`; compile assertions enforce the supertrait; generic networks return `None`; QUIC returns `Some`; old generic-trait calls fail to compile; existing traversal tests pass. | Any wire-format, traversal-algorithm, timing, candidate-policy, identity/security, registry, or fallback change. |

## Success Criteria
- Concrete user-visible or system-visible result: `TunnelNetwork` exposes only common tunnel-network behavior plus explicit optional capability discovery; QUIC UDP punch/prediction/validation remain available through `UdpTunnelNetwork: TunnelNetwork` with unchanged runtime results.
- Required evidence: API source/compile-contract assertions, affected crate/workspace compile closure, focused UDP punch and traversal prediction/validation tests, and task-scoped real listener-socket traversal coverage where already available.
- Explicit non-goals: No protocol migration, no new UDP transport, no PN/TCP traversal implementation, and no behavioral retuning.

## Risks
- Public Rust API removal is source-breaking for downstream implementors/callers and needs explicit compile-contract evidence.
- Returning a borrowed UDP trait object through the generic trait must remain object-safe and usable across the existing async calls; compile-contract tests must cover this exact access path.
- Missing migration at any SN or rendezvous call site could silently disable NAT probing/punching; repository-wide consumer discovery and focused behavior tests are required.
