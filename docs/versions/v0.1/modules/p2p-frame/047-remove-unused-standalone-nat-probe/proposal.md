---
task_manifest: task.yaml
status: approved
---

# Remove Unused Standalone NAT Probe Proposal

Risk profile: not-created (lower-tier proposal)

## Workflow Tier Judgment
- Proposed tier: trivial
- Final tier: trivial
- Tier rationale / triggered boundaries: The change removes an untracked, repository-unused standalone client helper and its redundant tests within `p2p-frame`. Production NAT probing, wire encoding, the SN reflector, QUIC socket ownership, and runtime behavior remain unchanged. No established public contract or cross-module behavior is affected, and focused Rust tests are available.
- Proposal and tier confirmation: User confirmed the displayed proposal and proposed `trivial` tier on 2026-09-03.

## Background and Goal
`probe_nat_mapping()` and `probe_nat_mapping_with_socket()` implement a standalone NAT probe client using a temporary or caller-owned UDP socket. Current production SN-client probing instead runs through the QUIC listener's bound socket and response demultiplexer. Repository searches show that the standalone helpers are referenced only by their own unit tests. Remove this unused test-oriented production code instead of retaining a second client implementation.

## Scope
### In scope
- Delete `probe_nat_mapping()` and `probe_nat_mapping_with_socket()` from `p2p-frame/src/sn/nat_probe.rs`.
- Delete tests that exist only to exercise those standalone helpers.
- Remove imports and private validation/decoding helpers that become unused as a direct consequence.

### Out of scope
- Do not change SN directive scheduling, report messages, NAT classification semantics, reflector behavior, or QUIC listener probing.
- Do not redesign how a probe is bound to a specific active SN QUIC listener in this task.
- Do not alter unrelated current working-tree changes.

### Boundary with neighboring modules
The production `NatProbeReflector`, PNAT codec pieces used by the QUIC listener, shared endpoint limits, and listener-owned response waiters remain in place. Only the unused standalone client path and tests unique to it are removed.

## Requirement Review
The requested cleanup is reasonable because maintaining two NAT-probe client paths obscures the actual socket-ownership model and leaves test-only behavior exposed in production source. Direct deletion is preferable to moving the standalone implementation into tests because the QUIC listener tests already exercise the production send, receive-demultiplex, reflector, classification, and socket-binding path. Packet codec tests that still cover production codec behavior will be retained and adjusted only as needed.

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-remove-unused-standalone-nat-probe | Remove the repository-unused standalone NAT probe client helpers and tests unique to them. | Preserve reflector, production codec, QUIC listener probe flow, and unrelated edits. | Standalone callers can no longer bind an ephemeral probe socket through this API; no repository production caller exists and the files are not part of the tracked baseline. | Repository reference search plus focused NAT probe and QUIC listener tests pass without dead-code warnings from the removed path. | Changing NAT behavior, scheduling, protocol, or exact-listener selection. |

## Success Criteria
- Concrete user-visible or system-visible result: Production source contains only NAT-probe components used by the actual SN/QUIC flow; the unused temporary-socket probe client is gone.
- Required evidence: `rg` confirms no remaining references to the deleted helpers; focused `p2p-frame` NAT probe/QUIC listener tests pass with the required feature configuration; a fresh proportional defect-discovery pass finds no production dependency or lost unique coverage.
- Explicit non-goals: No behavior change to SN registration, directives, reports, reflector service, NAT classification, or traversal prediction.

## Risks
The main risk is deleting unique protocol validation coverage together with the helper tests. Mitigation is to retain codec tests that exercise production encoding/decoding and verify the existing listener-owned reflector test still covers the actual runtime path. The dirty worktree will be preserved and only task-owned lines will be edited.
