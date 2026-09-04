---
task_manifest: task.yaml
status: approved
---

# Bind Rendezvous Response Endpoints to the Target Owner Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: The change closes a public-network trust-boundary defect in the authenticated SN rendezvous protocol. It controls which remote IP addresses may receive caller connect or punch traffic and must remain correct across local-serving and inter-SN relay paths.
- Proposal and tier confirmation: User confirmed the displayed solution and requested automatic completion on 2026-09-04 with the statement “确认，自动完成”.

## Background and Goal
`SnTunnelRendezvousResp` currently validates response correlation and endpoint shape but does not bind predicted endpoints returned by target B to an IP that B owns. A malicious authenticated B can therefore return a structurally valid third-party public endpoint and cause initiator A to connect or punch it. Bind every successful predicted endpoint response to a current public IP observation made by B's serving SN before the response can leave that trust boundary.

## Scope

### In scope
- Keep protocol-level response validation responsible for correlation, result, shape, transport, address class, port, count, and deduplication.
- At B's serving SN, require every predicted endpoint IP to equal a current non-LAN IPv4 address observed on an authenticated B command tunnel.
- Permit predicted ports to differ from the observed command-tunnel port.
- Apply the same validation before returning either a local-serving response or an inter-SN relayed response.
- Fail closed when the target has no current trusted public observation or returns any unowned IP; do not forward the endpoint list to A.
- Add regression coverage for accepted same-IP/different-port predictions and rejected third-party, self-reported-only, missing-observation, and cross-SN responses.

### Out of scope
- Do not add a new rendezvous wire field, SN-signed prediction attestation, or proof for ownership of each future predicted port.
- Do not change NAT prediction mathematics, socket-binding generation, TTL semantics, tunnel strategy selection, or PN fallback behavior.
- Do not treat self-reported `ReportSn.local_eps`, identity-certificate endpoints, or a target-submitted `NatProfile` as trusted address ownership evidence.
- Do not broaden accepted transports or endpoint areas.

### Boundary with neighboring modules
`sn/protocol` retains context-free structural validation. `sn/service` owns identity-bound serving-SN observations and enforces target endpoint ownership before local or inter-SN response forwarding. `tunnel` continues consuming only a successful validated response and remains unchanged.

## Requirement Review
Matching the predicted IP, but not the predicted port, to a serving-SN network observation is the smallest fail-closed fix for arbitrary third-party reflection. Exact port equality would invalidate prediction itself. Self-reported or certificate-listed endpoints are insufficient because an authenticated malicious target can still assert an address it does not control. IP binding does not prove ownership of every port behind shared NAT/CGNAT; a future SN-signed, attempt-bound prediction proof would be required for that stronger property and is intentionally outside this repair.

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-bind-rendezvous-response-owner | Before a successful prediction response leaves B's serving SN, reject it unless every predicted endpoint uses a current serving-SN-observed public IPv4 address of authenticated target B. | Enforce in `sn/service`, where target identity and live authenticated tunnel observations coexist; retain context-free wire validation in `sn/protocol`. | Multi-egress targets without a matching serving-SN observation fail closed and use existing fallback, in exchange for preventing arbitrary cross-IP traffic steering. | Red-green regression tests cover same-IP/different-port acceptance; third-party, self-reported-only, missing-observation, and inter-SN rejection; targeted task runner passes. | Per-port ownership proof, new wire format, or NAT strategy changes. |

## Success Criteria
- Concrete user-visible or system-visible result: A malicious rendezvous target cannot make an initiator connect or punch an IP that the target's serving SN has not currently observed for that authenticated target.
- Required evidence: Focused unit/behavior tests for the ownership predicate and serving-SN delivery boundary, plus a cross-SN regression demonstrating that an unowned response is rejected before relay completion.
- Explicit non-goals: This task does not claim cryptographic ownership of predicted ports, public-NAT deployment validation, or a new prediction attestation protocol.

## Risks
- A strict serving-SN observation requirement may reject legitimate multi-WAN predictions whose traversal socket exits through an IP not observed on any active authenticated command tunnel; rejection is deliberately fail-closed and leaves existing fallback available.
- Validating at only A's SN would be insufficient for cross-SN routing because that SN may not own B's live session; the target serving SN must be the enforcement point.
- Reusing self-reported cached endpoints would preserve the vulnerability despite making the new check appear identity-aware.
