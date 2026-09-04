---
task_manifest: task.yaml
status: approved
---

# Restore TunnelManager Business-Idle Cleanup Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: This repair changes when connected TCP, QUIC, Proxy, and custom tunnels are retired. Correctness depends on concurrent open/accept activity, channel-handle lifetime, candidate removal, and proxy-upgrade reconciliation, so it materially affects runtime lifecycle and concurrency rather than only consuming an existing timeout value.
- Proposal and tier confirmation: User confirmed the displayed proposal and explicitly launched the automatic downstream workflow on 2026-09-04 with the statement “确认，自动完成”.

## Background and Goal
`P2pStackConfig::idle_timeout` still reaches `TunnelManager`, but the housekeeping path ignores it and removes only unavailable tunnels. Restore the intended behavior: a connected tunnel with no active or pending business channel is retired after remaining idle for the configured duration, while live or newly opening channels are never mistaken for idle.

## Scope

### In scope
- Use the implementation immediately before `40f8216` as the behavioral reference: restore tunnel-lifetime statistics equivalent to historical `TunnelStat.work_instance_num` and `TunnelStat.latest_active_time`, plus the historical requirement that a candidate itself be in an idle rather than opening/accepting/error state.
- Make each concrete `Tunnel` implementation own its business-activity state. Increment work-instance accounting when stream/datagram/control-stream channel handles are created and decrement it when those handles are released, updating the latest-active timestamp on both transitions as the historical read/write wrappers did.
- Adapt the historical opening/accepting state guard to the current asynchronous `Tunnel` callback model: outbound open work and inbound accept/callback delivery must prevent idle retirement while pending, and the tunnel must atomically arbitrate new activity against retirement.
- Keep a successfully opened stream, datagram, or public control-stream channel active until all handles belonging to that channel are released; start the idle interval from the final release/activity transition.
- Make idle retirement race safely with new opens and incoming delivery so either the activity owns the candidate or retirement wins cleanly; never close a channel already accounted as active.
- On timeout, remove the exact idle candidate, reconcile the remote's remaining candidate topology and proxy-upgrade state under the existing lock-order contract, then close the removed tunnel without holding manager locks.
- Add red-green regression coverage for idle removal, active and pending retention, idle-clock restart, incoming/outgoing channel accounting, candidate isolation, and cleanup/proxy-state interaction.

### Out of scope
- Do not change tunnel wire formats, endpoint selection, NAT traversal, SN/PN routing, heartbeat intervals, or retry/fallback policy.
- Do not redefine `P2pConfig::quic_idle_time`, `PnTunnel`'s independent idle timeout, or `TtpClient` cache release policy.
- Do not use `TunnelEntry::updated_at` as a proxy for business activity.
- Do not require downstream/custom tunnel implementations to duplicate built-in activity accounting. A backward-compatible `Tunnel` lifecycle method may use a conservative default that opts custom tunnels out of manager idle retirement until they explicitly implement it.
- Do not change the existing `P2pStackConfig::idle_timeout` default or the housekeeping interval unless design evidence shows the current interval cannot implement the approved semantics.

### Boundary with neighboring modules
`TunnelManager` owns candidate retention, timeout policy, topology removal, and proxy reconciliation. Each `Tunnel` owns activity facts and the atomic live-to-retired transition. The common tunnel abstraction may host reusable activity/handle primitives, while `StreamManager` and `DatagramManager` retain their existing APIs and metadata wrapping. TCP, QUIC, and PN keep their protocol-specific close and heartbeat behavior; custom tunnels conservatively remain manager-idle-cleanup-ineligible unless they opt in.

## Requirement Review
Restoring the configuration is reasonable and matches both the long-lived tunnel design and the implementation immediately before `40f8216`. That implementation cleaned only when the tunnel was idle, `work_instance_num == 0`, and `now - latest_active_time > idle_timeout`; read/write wrapper creation and drop updated the statistics. The current implementation must preserve those behavioral conditions while translating the removed session/tunnel state machine into the present `Tunnel` callback API. Reusing `TunnelEntry::updated_at` would be unsafe because that timestamp changes on register/publish rather than channel use. The confirmed revision places reusable activity tracking inside TCP, QUIC, and PN tunnels and exposes one atomic idle-retirement decision through `Tunnel`; it removes the manager-facing wrapper, avoids raw/wrapper identity splitting, and keeps the manager responsible only for timeout policy and topology cleanup. Design must verify opening/accepting activity, custom-tunnel fallback, callback ownership, half-close/drop behavior, and manager lock ordering.

Assumptions for confirmation:
- Heartbeat frames and merely registering a listener are not business activity.
- As in the historical wrapper accounting, every live delivered read/write handle is a work instance; a public stream, datagram, or control-stream channel prevents idle retirement until all of its delivered handles are released, even if it currently carries no bytes.
- A failed or cancelled open/accept attempt releases its pending lease and begins a fresh idle interval when no other activity remains.
- `idle_timeout = 0` makes an otherwise idle candidate eligible at the next cleanup pass; normal enforcement precision remains bounded by the housekeeping interval.
- Historical code removed the idle entry and relied on ownership drop; the current adaptation will also call `close()` after exact-candidate removal, matching the current cleanup contract and ensuring deterministic resource release without holding manager locks.

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-restore-tunnel-idle-cleanup | Restore the pre-`40f8216` idle-cleanup semantics: retire a manager-owned candidate only when the candidate itself atomically confirms it is idle, has zero work instances, and its latest activity exceeds `P2pStackConfig::idle_timeout`, followed by exact-candidate removal and close. | Activity ownership belongs to concrete Tunnel implementations; TunnelManager owns timeout policy, candidate topology, and proxy reconciliation. Preserve wire behavior, candidate preference, heartbeat, and independent PN/TTP idle policies; custom Tunnel implementations default to no manager idle retirement. | Adds one backward-compatible lifecycle hook and reusable internal activity leases, while avoiding a manager-facing Tunnel wrapper and dual Arc identity. | A regression test fails against the current closed-only cleanup, then passes; focused tests prove the historical predicate, built-in TCP/QUIC/PN activity ownership, active and pending retention, timestamp restart after final drop, inbound/outbound stream/datagram/control coverage, race safety, custom-tunnel opt-out, candidate isolation, and correct proxy-state reconciliation; task-scoped runner passes. | Blindly restoring deleted pre-refactor types, manager-owned activity duplication, wire changes, timeout-default changes, NAT/selection changes, or replacement of independent transport/PN/TTP timeout policies. |

## Success Criteria
- Concrete user-visible or system-visible result: setting `P2pStackConfig::idle_timeout` once again applies the historical idle + zero-work-instance + elapsed-latest-activity conditions, causing a healthy but business-idle built-in TunnelManager candidate to be removed and closed while any pending or active channel prevents retirement; manager-visible tunnel identity remains the original transport Arc.
- Required evidence: red-green unit evidence for the former no-op configuration and the historical three-condition predicate; deterministic lifecycle/concurrency tests covering tunnel-owned outbound and inbound activity plus stream, datagram, and control-stream handles; conservative custom-tunnel behavior; candidate/proxy reconciliation checks; task-scoped `p2p-frame` verification through the unified runner.
- Explicit non-goals: no protocol, endpoint/NAT strategy, timeout-default, heartbeat, deployment, or unrelated cleanup change; local tests do not claim deployed multi-peer/NAT evidence.

## Risks
- Incorrect lease ownership across read/write halves could retire a tunnel while one half is still in use or retain it forever after both halves drop.
- A cleanup/open race could close a candidate just handed to a caller unless new activity and retirement share an atomic state transition.
- Wrapping manager-visible tunnels could break duplicate-`Arc` recognition, callback identity, endpoint metadata, or close propagation if delegation is incomplete.
- Idle removal must preserve the established `tunnels -> state` lock order and exact candidate identity while reconciling proxy-upgrade state; otherwise it can introduce deadlocks, stale upgrade work, or removal of a replacement candidate.
- The current workspace contains extensive unrelated changes. Implementation and acceptance must use task-local path evidence and avoid formatting or modifying unrelated files.
