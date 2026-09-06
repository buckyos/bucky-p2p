---
task_manifest: task.yaml
status: approved
---

# Send Only NAT Probe Ports to Clients Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: This deliberately changes the public SN wire contract used by authenticated control reports and on-demand traversal prediction, removes a server startup validation dependency, and changes how the client derives network destinations. The requested lack of backward compatibility narrows migration work but does not remove protocol/runtime risk.
- Proposal and tier confirmation: User confirmed the displayed proposal and requested automatic completion on 2026-09-05 with the statement “确认，自动完成”.

## Background and Goal
NAT probe reflectors listen on wildcard IPv4 sockets, but the current SN service derives a static-WAN IPv4 from its identity, combines it with configured probe ports, and sends complete QUIC endpoints to clients in both `ReportSnResp.nat_probe_endpoints` and `NatProbeDirective.endpoints`. The client already knows the active SN address used to establish its authenticated QUIC control tunnel. Change the protocol to transmit only probe ports and let the client combine those ports with that active SN IPv4, so enabling probe reflectors does not require a separately derived or configured advertised probe IP.

## Scope

### In scope
- Replace NAT probe endpoint lists on the SN report-response and probe-directive wire paths with bounded, unique, non-zero UDP port lists.
- Make the client retain the IPv4 address of the active authenticated QUIC SN endpoint and locally construct WAN QUIC probe endpoints from that address plus received ports.
- Use the same locally reconstructed endpoint snapshot for immediate NAT-profile probing and later rendezvous-time traversal prediction.
- Keep reflectors bound to `0.0.0.0:<port>` and remove the requirement that NAT probe configuration derive exactly one static-WAN IPv4 from the server identity.
- Preserve existing probe correlation, generation, expiry, signer verification, same-socket probing, result reporting, profile invalidation, and failure fallback behavior.
- Update focused protocol, service-configuration, client-directive, scheduler, and real-socket tests for the port-only contract.

### Out of scope
- Do not provide dual encoding/decoding, feature negotiation, rolling-upgrade support, or any other compatibility path for the old endpoint-bearing wire format.
- Do not add a separate NAT probe advertised-address option.
- Do not support probe reflectors hosted on an IP different from the active SN endpoint selected by the client.
- Do not change NAT classification, port prediction mathematics, profile TTL/scheduling, reflector authentication, or tunnel strategy selection.

### Boundary with neighboring modules
`sn/protocol` owns the new port-only wire fields. `sn/service` validates, schedules, and returns configured ports while binding reflectors locally. `sn/client` binds received ports to the active authenticated SN IPv4 and owns the reconstructed snapshot. `tunnel` continues consuming complete internal endpoints and should require no protocol knowledge. `sn-miner-rust` continues supplying only probe ports.

## Requirement Review
The request is reasonable under the explicit deployment invariant that NAT probe reflectors are reachable on the same IPv4 address as the active SN control endpoint. It removes redundant address transmission and the incorrect coupling between wildcard socket binding and a server-local advertised IPv4. Deriving the address from the active authenticated SN route also prevents the server from redirecting probe packets to an unrelated IP. The tradeoff is intentional: deployments that place reflectors on a different IP are no longer representable. Because compatibility is explicitly excluded, the implementation will make a direct wire break and verify only the new contract.

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-nat-probe-port-only-directive | Send only configured NAT probe ports in report responses and probe directives; reconstruct complete probe endpoints from the active authenticated SN IPv4 on the client. | Wire ownership remains in `sn/protocol`; server owns ports and wildcard listeners; client owns address binding and endpoint construction. | A separate reflector IP is no longer supported, and old/new binaries are intentionally wire-incompatible. | Wire round-trip tests contain ports and no probe IP; service starts with wildcard/no static-WAN identity address; client and real-socket tests prove immediate and rendezvous-time probes target the active SN IP with the received ports. | Compatibility decoding, a published probe IP, different-IP reflectors, or NAT algorithm changes. |

## Success Criteria
- Concrete user-visible or system-visible result: An SN configured with NAT probe ports can bind the reflectors on wildcard IPv4 without needing a static-WAN probe address, and clients probe those ports on the IPv4 of their active authenticated SN connection.
- Required evidence: Focused protocol round-trip and malformed-port tests; service configuration/bind tests; client directive and retained-snapshot tests; at least one real-listener probe path proving packets reach the active SN IP; affected crate compilation and registered task tests pass.
- Explicit non-goals: No mixed-version interoperability claim, no separate publish-address configuration, and no support for reflectors on a different host/IP.

## Risks
- This is an intentional breaking change to two SN response extensions; mixed-version peers may silently omit or reject NAT probing and are outside the requested scope.
- Choosing the active SN endpoint as the probe IP requires preserving that exact authenticated registration context across later probes; stale registration cleanup must not reuse an address from a replaced SN connection.
- Removing static-WAN identity validation must not weaken port count, zero, duplicate, reflector bind-failure, or signed-response validation.
- A client connected to an SN through an address that does not route the reflector ports will fail closed to an unknown profile and existing fallback behavior.
