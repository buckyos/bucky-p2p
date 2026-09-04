---
module: p2p-frame
task_name: 008-pn-traffic-disconnect-retention
submodule: 008-pn-traffic-disconnect-retention
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# PN Disconnected Traffic Statistics Retention Proposal

## Background and Goal

The completed `007-pn-traffic-snapshot-traversal` task releases a user's live PN traffic statistics as soon as that user's final data or control bridge ends. Immediate release prevents stale live state from accumulating, but it also makes a just-disconnected user's final cumulative and interval traffic snapshot unavailable to an external observer and resets that state even when the user reconnects shortly afterward.

The goal is to replace unconditional immediate release with an externally configurable disconnect-retention duration. After the last active traffic session ends, the user's live statistics remain observable for the configured duration and are reused if that user reconnects before expiry. Once the deadline expires without a reconnect, the manager releases the live statistics automatically. A zero duration preserves the existing immediate-release behavior for compatibility.

## Scope

### In scope

- Add an external `PnTrafficManager` / `PnServer` configuration surface for the disconnected-user traffic-statistics retention duration; the exact Rust API shape belongs to design.
- Define the default retention duration as zero so existing callers retain immediate-release behavior unless they explicitly configure a non-zero duration.
- Capture the configured duration when a user's active traffic-session count transitions to zero. Later configuration changes affect future disconnect transitions and do not silently reschedule users already in a retention period.
- Keep a disconnected user's live counters, speed trackers, shared external-snapshot delta baseline, and limiter live state in the manager until that captured deadline expires.
- Keep retained disconnected users visible through per-user snapshot lookup and incremental all-user traversal during the retention period. Snapshot acquisition continues to advance the same shared interval-delta baseline defined by task 007.
- If the same user reconnects before expiry, cancel or invalidate the pending cleanup, reuse the same live entry and its cumulative/delta state, and resume active-session accounting without resetting counters.
- Automatically release expired live statistics even when no caller performs another lookup, traversal, connection, or configuration operation. Cleanup scheduling must remain bounded and must not create one permanently detached task or unbounded timer resource per disconnected user.
- Make expiry generation/deadline safe: delayed or stale cleanup work must not remove an entry that has reconnected, entered a newer retention period, or been replaced by a newer generation.
- Preserve task 007's behavior for explicit per-user traffic-limit policy: expiry releases the live statistics entry but does not discard retained limit configuration; a later connection creates fresh statistics and reapplies that policy.
- Release all remaining live and retained runtime state, and terminate any cleanup activity, when the owning manager/server is stopped or dropped.

### Out of scope

- Persisting disconnected traffic statistics, retention deadlines, or traffic-limit policy across process restart.
- Providing indefinite offline history, billing-grade records, historical queries, or a separate archive of expired snapshots.
- Providing different retention durations per user, per observer, per session, or per source/target role in this change.
- Rescheduling already-disconnected users when the global retention duration changes.
- Changing the shared per-user external snapshot acquisition baseline, cumulative counters, speed-window semantics, source/target accounting direction, or successful-forwarded-byte definition established by task 007.
- Changing PN wire messages, tunnel admission, relay registration, bridge transport behavior, source-only traffic limiting, or adding target-side/pair-level quotas.
- Modifying production code, tests, design, testing artifacts, or the approved task 007 packet during this proposal stage.

### Boundary with neighboring modules

- The behavior is owned by `p2p-frame/src/pn/service/pn_server.rs`, where `PnTrafficManager`, its live user entries, session guards, snapshot lookup/traversal, and bridge lifetimes are implemented.
- Task `007-pn-traffic-snapshot-traversal` remains the immutable baseline for snapshot fields, shared interval-delta acquisition, incremental traversal, active-session counting, generation-safe removal, and retained traffic-limit configuration. This sibling changes only the time between final disconnect and removal.
- `sfo-io` continues to own `SfoSpeedStat`, `SpeedLimiter`, trackers, and limit sessions; this task only extends how long `p2p-frame` retains their per-user live wrapper state.
- `cyfs-p2p`, `cyfs-p2p-test`, and `sn-miner-rust` do not own an alternate cleanup scheduler or offline-statistics model. Any later caller integration consumes the `p2p-frame` configuration and observation contract.

## Requirement Review

- A configurable grace period is reasonable because operators may poll snapshots less frequently than bridge lifetimes. Retaining the final live entry for a bounded period makes recently disconnected users observable without introducing durable history.
- Zero is the compatibility-preserving default. Deployments that want post-disconnect visibility opt into its memory cost by configuring a non-zero duration.
- The duration is sampled at the active-to-idle transition. This gives each retention deadline stable semantics and avoids expensive or surprising global rescheduling when configuration changes. A future task can propose retroactive rescheduling if needed.
- A retained disconnected entry remains a currently tracked user for lookup and traversal purposes even though its active-session count is zero. It disappears only after successful expiry cleanup.
- Reconnection before expiry reuses the entry, including counters and the shared interval baseline. Reconnection after expiry creates a fresh live statistics entry whose first external delta follows task 007's fresh-entry rule.
- Expiry cannot be purely lazy because an inactive server with no observation calls would otherwise retain all disconnected users forever. The design must provide manager-owned automatic progress and a bounded scheduling model.
- A cleanup event identified only by user key is unsafe. Every expiry action must validate the entry identity/generation and the applicable idle deadline while synchronized with reconnection and later disconnect transitions.
- Explicit traffic-limit configuration remains control-plane policy outside the expiring live statistics entry. Expiring statistics must not silently make a later reconnect unlimited.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-PN-TRAFFIC-RETENTION-1 | pn_traffic_disconnect_retention | Replace unconditional final-disconnect removal with an externally configurable retention duration: zero releases immediately; non-zero keeps the user observable with the same cumulative and interval-delta state until expiry; reconnect before expiry reuses the entry; expiry progresses automatically and stale cleanup cannot remove a reconnected/newer entry | Limited to `PnTrafficManager`, the `PnServer` configuration/observation surface, and PN traffic-session lifecycle in `p2p-frame/src/pn/service/pn_server.rs`; task 007 accounting, traversal, delta, and retained-limit semantics otherwise remain unchanged | Post-disconnect visibility and reconnect continuity consume memory until each deadline; sampling configuration at disconnect gives stable deadlines but configuration changes do not affect users already idle | Approved design specifies API/default/update semantics, active/idle state transitions, bounded scheduler ownership, shutdown, synchronization and generation/deadline validation; post-implementation tests cover zero/non-zero retention, lookup/traversal visibility, automatic expiry without access, reconnect before/after expiry, stale cleanup races, concurrent sessions, future-only configuration updates, limit-policy preservation, and manager shutdown | No durable/offline history, per-user retention policy, retroactive deadline rescheduling, restart persistence, accounting or speed changes, wire changes, target limiting, or unbounded timer-per-user design |

## Success Criteria

- Concrete user-visible or system-visible result: an external caller can configure how long PN user traffic statistics remain available after the last connection ends; retained users remain incrementally observable, short reconnects continue the same counters/delta baseline, and expired entries are eventually released without requiring another access.
- Required evidence: an approved design maps `pn_traffic_disconnect_retention` to the public configuration surface, default/update semantics, traffic-entry state machine, one bounded cleanup owner, reconnect/expiry synchronization, manager shutdown, and task 007 compatibility; post-implementation testing exercises timing and concurrency behavior through the canonical p2p-frame test entry.
- Explicit non-goals: no durable history or persistence, no per-user durations, no retroactive rescheduling, no accounting/delta/speed/wire change, no target-side limiting, and no unrelated PN lifecycle refactor.

## Risks

- A long configured duration multiplied by many disconnected users can consume significant memory; the duration is therefore operator-controlled and zero by default.
- A timer/task per user can create its own scalability problem even if entries eventually expire. Design must centralize or otherwise bound cleanup scheduling resources.
- Timer wakeup, reconnect, final-session drop, traversal, and configuration update can race. Incorrect lock ordering or awaiting while locked could deadlock or stall bridge setup.
- A stale deadline can delete a reconnected entry or a later idle generation unless cleanup validates identity/generation and deadline under the same lifecycle synchronization used by session acquisition.
- Releasing at a deadline while an external snapshot iterator holds an entry handle must have defined value-lifetime semantics without retaining the entry in the manager indefinitely.
- Changing the configuration while users are already idle may surprise callers unless the future-disconnect-only rule is explicit in API documentation.
- Manager shutdown can leak a cleanup worker or keep strong references alive if ownership and cancellation are not explicit.
- Retaining live limiter state across a short disconnect preserves continuity but may retain transient limiter timing state longer than callers previously expected; this is bounded by the configured duration and ends at expiry.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | The task adds an external retention configuration and changes when an inactive user is visible through `PnServer` snapshot lookup/traversal, while leaving PN wire protocol unchanged | Design must define the Rust API, zero default, duration sampling/update behavior, idle visibility, reconnect continuity, and expiry semantics; testing must cover positive, zero, and boundary cases | Proposal fixes compatibility and lifecycle semantics and binds them to the existing task 007 observation contract | owner: design/testing; reason: exact API and runnable cases belong to downstream stages; acceptance impact: ambiguous configuration or visibility semantics blocks acceptance | Callers may assume disconnected users disappear immediately or that a configuration change retroactively reschedules existing deadlines |
| data/schema | no | Retention duration, deadlines, counters, baselines, and traffic-limit configuration remain process-local runtime state; no serialized form, database, file, cache schema, or migration is added | Confirm design and implementation do not add persistence or serialized state | Proposal explicitly excludes restart persistence and historical storage | owner: none; reason: not applicable; acceptance impact: none | Retained statistics are intentionally lost on expiry or process termination |
| security/privacy/permission | no | The task does not add a remote endpoint, change authenticated identities, permissions, secrets, or the contents of a snapshot; it only changes local retention lifetime | Confirm no new unauthenticated network exposure is introduced | Proposal limits configuration and observation to the existing in-process `PnServer` surface | owner: none; reason: not applicable; acceptance impact: none | A later remote administration surface would require separate authorization and privacy review |
| runtime/integration | yes | Final session release, reconnect, iterator-held entries, deadline wakeups, configuration updates, and manager shutdown are concurrent runtime events in `pn_server.rs` | Design must specify the state machine, clock/deadline model, bounded scheduling, lock/await boundaries, stale-event validation, shutdown, and interaction with task 007 session guards; testing must cover deterministic timing and concurrency/failure paths | Proposal defines active-to-idle capture, automatic progress, reconnect reuse, expiry release, and stale-event safety requirements | owner: design/testing; reason: runtime structures and executable race coverage belong to downstream stages; acceptance impact: leaks, premature deletion, deadlock, or non-progressing expiry blocks acceptance | Rare reconnect/expiry interleavings may remain unless tests use controllable timing and identity/deadline assertions |
| build/dependency/config/deployment | yes | The task adds runtime configuration but does not yet select whether it is a constructor option, setter, or configuration object field; no dependency or deployment-file change is requested | Design must choose a backward-compatible configuration surface and confirm whether any Cargo/deployment/config artifacts are needed; implementation must keep the zero default | Proposal establishes externally settable duration and compatibility default without prescribing an unsupported file format | owner: design; reason: API placement belongs to design; acceptance impact: a non-configurable or breaking surface blocks acceptance | A setter-only API may require callers to configure each server instance explicitly |
| ui/datamodel/workflow | no | There is no UI or frontend model for PN traffic retention in scope | Confirm no UI artifact is introduced | Proposal identifies only Rust manager/server consumers | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | The sibling packet uses existing proposal, approval, auto-pipeline/admission, testing, stage-scope, and acceptance mechanisms without changing `harness/**` | Run existing stage-owned checks as each downstream artifact is produced | Proposal and scope evidence use current repository governance | owner: none; reason: not applicable; acceptance impact: none | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
