---
task_manifest: task.yaml
status: approved
---

# Sanitize SN-Reported Endpoints Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: This change corrects a public-network authorization boundary in the authenticated SN rendezvous protocol. It changes which caller-supplied addresses are retained and which IPs may cause another peer to punch or reverse-connect, while preserving valid NAT traversal and fallback behavior.
- Proposal and tier confirmation: User confirmed the displayed high-risk proposal and launched automatic completion on 2026-09-04 with the statement “确认，自动完成”.

## Background and Goal
`ReportSn.local_eps` is authenticated as data sent by a peer but is not independently proven to belong to that peer. The SN currently stores those endpoints in `CachedPeerInfo.local_eps`, and rendezvous ownership validation treats both those endpoints and identity-certificate endpoints as proof that the initiator owns their IPs. An authenticated malicious initiator can therefore report a third-party public IP and then request a rendezvous action against the same IP.

Retain only genuine LAN addresses and public addresses whose IP equals the network source IP observed by the SN on the authenticated reporting command tunnel. Keep identity certificates intact for identity verification, but never treat their endpoint declarations as public-address authorization. Independently validate each non-empty rendezvous request against the current request tunnel's observed source IP before any same-SN delivery or inter-SN relay.

## Scope

### In scope
- Sanitize `ReportSn.local_eps` before peer-cache update: retain only structurally valid LAN addresses determined from the socket IP itself, plus public IPv4 addresses whose IP matches the current authenticated report tunnel's SN-observed source IP.
- Reject unspecified, loopback, multicast, broadcast, invalid, zero-port, duplicate, over-budget, and unrelated public reported endpoints; do not trust the caller-supplied endpoint area classification.
- Normalize retained endpoint area from server-side address evidence rather than preserving an attacker-supplied area label.
- Preserve the full encoded identity certificate for identity verification, but remove certificate endpoint declarations and raw reported endpoints from rendezvous public-IP authorization.
- For every non-empty rendezvous request, require every endpoint IP to equal the current authenticated request tunnel's SN-observed source IP; fail closed when that observation is missing or stale.
- Preserve empty `WaitIncoming` requests, existing operation/transport/area/count/deduplication validation, same-SN and inter-SN routing, and existing fallback behavior.
- Add authenticated real-control-socket regression coverage for a forged `ReportSn` followed by a rendezvous request for the same third-party IP, plus positive and lifecycle boundary cases.

### Out of scope
- Do not add a wire field, SN-signed endpoint capability, or cryptographic/per-port ownership proof.
- Do not claim that matching an observed IP proves ownership of every port behind shared NAT or CGNAT.
- Do not change NAT prediction mathematics, socket-binding generation, TTL semantics, tunnel strategy selection, PN behavior, or public tunnel APIs.
- Do not redesign general LAN discovery. Retained LAN endpoints remain untrusted hints and must not authorize public rendezvous actions or cross-SN remote-triggered LAN access.
- Do not change the separate target-response ownership task `052-bind-rendezvous-response-owner`.
- Do not broaden this task into a complete legacy `SnCall.reverse_endpoint_array` authorization redesign; the sanitized report cache must no longer add unrelated public IPs there, but caller-supplied legacy arrays remain a separately reviewable boundary.

### Boundary with neighboring modules
`sn/service` owns authenticated tunnel observations, report sanitization, cache input, and the final initiator-IP authorization check before local delivery or inter-SN relay. `sn/client` may expose test-only report construction needed to exercise malicious authenticated input through a real control socket. `sn/protocol` retains context-free endpoint shape and operation validation and does not acquire network-observation responsibilities. The tunnel manager remains unchanged and consumes only requests already admitted by the SN service.

## Requirement Review
The requested retention rule is reasonable as defense in depth, provided it is not mistaken for the complete authorization boundary. Cache sanitization prevents attacker-selected public IPs from surviving `ReportSn`, but rendezvous must still compare against the current request tunnel because cached observations can become stale and certificate endpoints remain self-asserted. LAN classification must use the actual IP class rather than `EndpointArea`, and retained LAN values remain low-trust discovery hints. Matching only the observed IP deliberately permits a different predicted or mapped port; exact port equality would break NAT prediction. Strong per-port authorization requires a future SN-observed or SN-signed capability and is not part of this repair.

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-sanitize-sn-reported-endpoints | Retain from `ReportSn.local_eps` only server-classified LAN endpoints and public endpoints on the reporting tunnel's SN-observed IP, and authorize non-empty rendezvous request IPs only against the current authenticated request tunnel observation. | Enforce in `sn/service`; certificate contents remain identity material, protocol validation remains context-free, and any client change is test-only. | Legitimate multi-egress nodes whose advertised public IP differs from the active SN control path fail closed and use existing fallback; ports on the observed IP remain intentionally unproven. | Authenticated socket tests demonstrate forged-report/same-IP-request rejection with zero target callbacks, valid observed-IP admission, mixed-list atomic rejection, stale-tunnel rejection, empty `WaitIncoming` preservation, and cross-SN rejection before target action. | No new wire capability, per-port proof, NAT algorithm change, task-052 modification, or full legacy `SnCall` redesign. |

## Success Criteria
- Concrete user-visible or system-visible result: An authenticated initiator cannot make an SN retain an unrelated public IP through `ReportSn` and cannot cause a rendezvous target to punch or reverse-connect that IP, even when it reports the IP before submitting the request.
- Required evidence: Focused report-sanitization tests; authenticated real-control-socket forged-report regression with zero target callbacks; positive observed-IP and empty-request cases; same-SN and cross-SN enforcement; targeted x509 and feature-gated test commands; proportional compile/check closure required by the high-risk lifecycle.
- Explicit non-goals: No proof of individual port ownership, no public-NAT deployment claim, no wire migration, no target-response task modification, and no claim that legacy caller-supplied endpoint arrays are comprehensively redesigned.

## Risks
- Security: retaining or authorizing any public IP from `local_eps`, certificate endpoints, endpoint area, or a client-submitted NAT profile would preserve the reported bypass.
- Compatibility: multi-WAN or policy-routed peers may advertise a legitimate public IP that differs from the current control-tunnel source; fail-closed rejection may increase fallback use.
- LAN boundary: private/link-local addresses are self-reported and may enable internal probing if later forwarded without a separately proven same-LAN policy; this task retains them only as low-trust hints and excludes them from public rendezvous authorization.
- Lifecycle: authorization from a cached union of historical tunnel observations could survive address migration; the request check must bind to the exact current command tunnel.
- Test adequacy: a direct handler or cache-only unit test would not prove the authenticated report-to-request exploit is closed; the negative regression must traverse real listener sockets and the production request handler.
