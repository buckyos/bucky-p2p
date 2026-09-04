---
task_manifest: task.yaml
status: approved
---

# Clear Proxy Upgrade State After Tunnel Close Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: The change is localized to `p2p-frame`, but it changes a background retry scheduler's lifecycle and the ownership relationship between live proxy tunnel candidates and `proxy_upgrade_states`. The repository classifies material concurrency/lifecycle/runtime-integration changes as high-risk.
- Proposal and tier confirmation: confirmed by user statement `确认，自动完成`; auto-pipeline launched with design as the first automatic stage.

## Background and Goal
Registering a proxy tunnel creates or resets the remote's `proxy_upgrade_states` entry. `cleanup_closed_tunnels` removes unavailable tunnel candidates and re-tracks remotes that still have only published proxy candidates, but it does not clear the upgrade entry when the last candidate for a remote is removed. A later failed upgrade therefore advances the retry schedule indefinitely and can continue SN queries and active connection attempts after the connection is no longer needed.

The goal is to make proxy-upgrade scheduling end when cleanup removes the last tunnel candidate for that remote.

## Scope
### In scope
- When closed-tunnel cleanup removes the last tunnel candidate for a remote, remove that remote's proxy-upgrade state.
- Preserve upgrade tracking when cleanup leaves at least one published proxy candidate and no published non-proxy candidate.
- Ensure an already-running failed attempt cannot re-schedule work after cleanup has removed the state.
- Add focused unit regression evidence for the last-proxy-close lifecycle and retained-proxy behavior where needed.

### Out of scope
- No changes to proxy/direct tunnel selection policy, retry intervals, SN wire protocol, PN behavior, or public APIs.
- No broader refactor of `TunnelManager` state ownership or unrelated tunnel cleanup paths.
- No change to successful direct-upgrade behavior.

### Boundary with neighboring modules
The change remains within `p2p-frame` tunnel orchestration. SN and PN modules remain consumers/providers of existing behavior and do not change contracts.

## Requirement Review
The requested removal is reasonable and fixes a resource-lifecycle leak. Clearing only after confirming that cleanup has left no candidate avoids suppressing upgrades for a still-live proxy. Because cleanup and upgrade attempts run asynchronously, the implementation must use the existing removal semantics so a late failure observes the missing state and does not recreate or re-schedule it. The smallest appropriate direction is to reconcile cleanup results with `proxy_upgrade_states`, rather than changing retry policy or adding a second scheduler.

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-clear-proxy-upgrade-after-close | Remove a remote's upgrade state when cleanup deletes its final unavailable tunnel candidate, while retaining tracking for a remote that still has only published proxy candidates. | `TunnelManager` cleanup and proxy-upgrade lifecycle only. | Adds explicit cleanup/state reconciliation; does not redesign scheduler ownership. | A regression test demonstrates state removal after final proxy close and verifies a late failed attempt cannot schedule another retry; existing/focused proxy-upgrade tests remain green. | Changing retry cadence, connection selection, SN/PN contracts, or public APIs. |

## Success Criteria
- Concrete user-visible or system-visible result: after the last proxy tunnel for a remote closes and cleanup runs, no `proxy_upgrade_states` entry remains and the background upgrade loop no longer queries or initiates connections for that obsolete remote.
- Required evidence: focused unit red-green regression coverage for cleanup/state removal and late-failure behavior, plus targeted `p2p-frame` compilation/test evidence required by the finalized high-risk test design.
- Explicit non-goals: no protocol, public API, retry-interval, or unrelated tunnel-lifecycle changes.

## Risks
- A cleanup/attempt race could allow an obsolete in-flight failure to touch a newly registered state for the same remote; downstream design must explicitly confirm whether existing map removal semantics are sufficient or whether attempt ownership needs generation binding.
- Clearing too aggressively could disable upgrades while a valid published proxy remains; regression evidence must cover retention for that case.
- The target source file is already modified in the current worktree; lifecycle evidence must preserve and distinguish those pre-existing edits.

## Approval Record
- approver: user
- approval_date: 2026-09-03
- user_statement: `确认，自动完成`
