---
task_manifest: task.yaml
status: approved
---

# Signed PNAT Probe Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: This changes the unauthenticated PNAT UDP wire format and introduces identity-key verification on the client. It affects SN reflector runtime behavior, client interoperability, public protocol compatibility, and the security boundary that determines whether an observed public endpoint is trusted.
- Proposal and tier confirmation: User confirmed the displayed proposal and requested automatic completion on 2026-09-04 with the statement “确认，自动完成”. Compatibility direction was resolved earlier the same day: no backward compatibility.

## Background and Goal
The current 32-byte `PNAT` v1 request/response correlates a random token and validates the UDP source address, but a response has neither an authenticated reflector identity nor integrity protection for the reported external endpoint. Add identity-key signatures so a client accepts a probe result only when it was signed by the configured SN reflector identity and covers the complete response semantics.

## Scope

### In scope
- Define a versioned signed PNAT response format that cryptographically binds protocol version, message kind, request token, reflected source IP and port, and the signing SN identity context.
- Make each SN reflector sign responses with its configured `P2pIdentity` private key.
- Make the QUIC listener verify the response with trusted public-key material for the configured SN before delivering it to a pending probe waiter.
- Reject unsigned, malformed, wrong-identity, bad-signature, expired/stale, or source-mismatched responses without updating `NatProfile`; probe failure remains fail-closed to `Unknown`.
- Replace PNAT v1 rather than retaining a v1 decoder, unsigned fallback, feature switch, or mixed-version negotiation path.
- Preserve the reflector's bounded-resource behavior and add protocol, tamper, wrong-signer, replay/correlation, and real listener-socket coverage outside `cyfs-p2p-test`.

### Out of scope
- Do not use PNAT as a tunnel authentication substitute, encrypt probe payloads, or change subsequent QUIC/TLS tunnel authentication.
- Do not widen probe targets beyond the active SN's configured IPv4 reflector set.
- Do not alter unrelated tunnel strategy, NAT classification, or proxy fallback behavior.

### Boundary with neighboring modules
`p2p-frame` owns PNAT encoding/verification, QUIC listener response dispatch, SN client configuration, and SN reflector lifecycle. The feature relies on the existing `P2pIdentity::sign` and `P2pIdentityCert::verify` abstractions; it must not create a second cryptographic identity system.

## Requirement Review
Signing the **response** is the security-critical direction: it protects the reflected endpoint that influences candidate selection. Signing an unauthenticated UDP request alone would not prevent a forged response. A response signature must cover its token and full observed endpoint, and verification must bind the signer to the configured SN identity rather than trusting a certificate supplied by the UDP packet.

The current `P2pSn` configuration carries an ID, name, and endpoints, but not an independently available verification certificate. The design therefore needs an explicit trusted-SN verification-material path (for example, a pinned SN certificate delivered by the existing authenticated control/configuration plane). Sending a certificate in an unauthenticated UDP response is not sufficient by itself and could also violate the current no-amplification property.

The user explicitly chose a clean protocol cutover with no compatibility for old clients or old SN servers. Version mismatch, unsigned response, and missing verification material therefore fail closed to `Unknown`, after which the existing legacy tunnel/PN fallback remains responsible for connectivity. There will be no dual decode, downgrade negotiation, or temporary acceptance of PNAT v1.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-signed-pnat-probe | Replace PNAT v1 with a signed protocol and require an SN-identity signature over token, response kind/version, and observed endpoint; verify it before accepting the response. | Reuse the existing P2P identity abstractions, bind verification to the configured SN, and provide no legacy decoder or unsigned fallback. | Old clients and old SN servers cannot perform NAT probing and require coordinated upgrade. | Valid signed response is accepted; version, bit, token, endpoint, source, and signer mutations are rejected; v1 is rejected; a real listener socket completes a signed probe. | Encrypting the probe, authenticating a tunnel, or mixed-version interoperability. |

## Success Criteria
- Concrete user-visible or system-visible result: Tunnel port prediction consumes only a response verifiably signed by the configured SN identity; tampered UDP responses cannot change the observed NAT endpoint.
- Required evidence: Version/codec tests, signature-positive and signature-negative tests, reflector rate/amplification-bound tests, and real listener-socket integration coverage through the task-scoped runner.
- Explicit non-goals: No legacy PNAT decoder, compatibility negotiation, unsigned fallback, mixed-version interoperability, or claim that PNAT itself establishes a tunnel.

## Risks
- The clean wire-format cutover intentionally breaks mixed client/SN NAT probing; deployment must upgrade clients and SN servers together. A mismatch fails closed and may force legacy tunnel or PN fallback.
- Verification material must be acquired from a trusted configuration/control path and bound to the expected SN ID; accepting an in-band certificate would undermine the intended protection and can create UDP amplification risk.
- RSA identity signatures may make a fixed 32-byte response impossible. The design must retain a strict anti-amplification bound, or reject an implementation whose response could be amplified from an unauthenticated request.
