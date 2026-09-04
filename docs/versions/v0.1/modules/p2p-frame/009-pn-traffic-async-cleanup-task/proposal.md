---
module: p2p-frame
task_name: 009-pn-traffic-async-cleanup-task
submodule: 009-pn-traffic-async-cleanup-task
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# PN Traffic Asynchronous Cleanup Task Proposal

## Background and Goal

The completed `008-pn-traffic-disconnect-retention` task implements correct configurable retention, reconnect reuse, generation-safe expiry, bounded deadline state, and shutdown behavior, but its cleanup executor is a dedicated `std::thread` that blocks on a `Condvar`. The requested runtime model is an asynchronous task rather than a newly created operating-system thread.

The goal is to preserve task 008's externally visible traffic-statistics behavior and replace only the cleanup execution mechanism with one project-runtime-managed asynchronous task. The task must wait through asynchronous timer/notification primitives, perform no blocking wait on a runtime worker, remain promptly wakeable when the earliest deadline changes or shutdown begins, and terminate without retaining the traffic manager indefinitely.

## Scope

### In scope

- Replace the PN traffic cleanup `std::thread::Builder`, `std::thread::JoinHandle`, `Condvar`, and blocking `wait` / `wait_timeout` loop with exactly one cleanup future spawned through the repository's runtime/executor abstraction.
- Use asynchronous notification and timer waiting so the cleanup future yields rather than blocking an executor worker while the deadline set is empty or a future deadline is pending.
- Retain one cleanup task per `PnTrafficManager`; do not create one task/timer per disconnected user or per disconnect event.
- Preserve the ordered deadline set with at most one scheduled item per idle user and all task 008 key, active-count, generation, and deadline validation semantics.
- Wake the asynchronous cleanup task when final-session release adds or changes the earliest deadline, reconnect removes a deadline, or shutdown/cancellation begins. Notifications must not be lost in a way that delays an earlier deadline until a previous later sleep finishes.
- Ensure no synchronous manager mutex guard is held across `.await`, asynchronous notification, timer sleep, task cancellation, or task completion handling.
- Define cleanup-task ownership so the future does not hold a strong `PnTrafficManager` reference across idle waits and thereby prevent manager destruction.
- Preserve deterministic cleanup on `PnServer::stop` and manager/server drop: live entries, retained limit configuration, deadlines, and retention configuration are cleared, the cleanup task is notified or cancelled, and it cannot mutate manager state afterward.
- Preserve the existing synchronous public `PnServer::stop()` compatibility unless design proves a repository-wide migration is required; exact task-handle cancellation/completion mechanics belong to design.
- Preserve task 008's full `Duration` behavior, including checked saturation to the furthest representable monotonic deadline for extreme values. The async timer must receive only a runtime-safe bounded sleep duration and recheck the absolute deadline after wakeup.
- Preserve default-zero immediate release, future-disconnect-only configuration updates, idle lookup/traversal visibility, shared interval-delta continuity, reconnect entry reuse, automatic expiry without observation, explicit limit-policy retention after expiry, and late-session non-registration after shutdown.

### Out of scope

- Changing the public retention setter, its default, captured-deadline semantics, or already-idle configuration-update behavior.
- Changing snapshot fields, cumulative/delta acquisition, traversal ordering/weak consistency, traffic accounting, limiter direction, or explicit traffic-limit policy ownership.
- Adding per-user cleanup tasks, detached fire-and-forget timers, an unbounded async channel, polling once per user, or a new global cleanup service shared by unrelated managers.
- Changing PN wire messages, proxy bridge transport, tunnel admission, relay registration, TLS/identity behavior, source normalization, speed calculation, target limiting, or neighboring modules.
- Persisting statistics/deadlines across restart or adding offline history.
- Adding dependencies, changing runtime features, or changing executor abstractions unless downstream design demonstrates that existing runtime facilities cannot satisfy the requirement and returns upstream before implementation.
- Modifying production code, tests, design, testing artifacts, or task 008 during this proposal stage.

### Boundary with neighboring modules

- The implementation remains owned by `p2p-frame/src/pn/service/pn_server.rs`, specifically the task 008 cleanup owner, wake mechanism, handle lifecycle, and `PnServer` stop/drop integration.
- Existing `crate::runtime`, `crate::executor`, and their supported feature backends own task spawning and asynchronous sleeping. This sibling consumes those abstractions rather than creating a private thread runtime.
- Task `008-pn-traffic-disconnect-retention` remains the immutable behavioral baseline. This sibling changes only the executor/wait mechanism for cleanup.
- No `cyfs-p2p`, `cyfs-p2p-test`, `sn-miner-rust`, `sfo-io`, codec, or wire implementation change is authorized.

## Requirement Review

- Replacing a dedicated thread with one asynchronous task is reasonable because cleanup is predominantly waiting and short mutex-protected state transitions. An async wait avoids reserving an OS thread per manager while preserving a single cleanup owner.
- An async task must not simply move the current blocking `Condvar` loop into an async future; blocking a runtime worker would violate the requested execution model and can reduce executor capacity.
- A timer alone is insufficient because a new earlier deadline, reconnect cancellation, or shutdown must interrupt the current wait. The task therefore needs an async notification/cancellation signal combined with a bounded timer and must re-read the authoritative deadline set after every wake.
- Notifications and deadlines remain hints around manager-owned state. Correctness comes from rechecking the locked state after wakeup, not from assuming one notification corresponds to one cleanup event.
- The cleanup future must not keep the manager alive forever. The selected direction is weak/shared-state ownership with explicit shutdown/cancellation; design must prove there is no strong-reference cycle and define what synchronous `stop`/`Drop` can guarantee without blocking on an async join.
- Existing dual runtime/executor support is a compatibility boundary. Design must use the repository abstraction available to the affected build features or explicitly record why a backend-specific primitive remains portable in this crate.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-PN-TRAFFIC-ASYNC-CLEANUP-1 | pn_traffic_async_cleanup_task | Replace the dedicated thread/Condvar cleanup loop with one runtime-managed asynchronous cleanup task that waits through async timer/notification primitives, wakes for earlier deadlines/reconnect/shutdown, holds no synchronous lock across await, preserves all task 008 retention semantics, and cannot outlive or retain the manager incorrectly | Limited to the cleanup executor, wake signal, task handle, and stop/drop integration in `p2p-frame/src/pn/service/pn_server.rs`; retention state, public configuration, observation, accounting, and PN wire behavior remain unchanged | Removes one OS thread per manager and integrates waiting with the runtime, but requires explicit lost-wakeup, cancellation, runtime-backend, and synchronous-drop reasoning | Approved design maps the async task lifecycle, notification/timer select loop, lock/await boundaries, handle ownership, runtime compatibility, cancellation and stop/drop behavior to the single source path; post-implementation tests prove no OS cleanup thread is created, idle waits yield, earlier deadlines preempt later waits, expiry/reconnect/stale-generation semantics remain correct, and shutdown/drop prevents later mutation | No task per user, blocking wait inside async code, unbounded channel, global cleanup service, retention/API semantic change, new dependency, wire/accounting change, or unrelated refactor |

## Success Criteria

- Concrete user-visible or system-visible result: PN traffic retention behaves exactly as in task 008, while cleanup is owned by one asynchronous runtime task and no dedicated PN traffic cleanup OS thread is created.
- Required evidence: an approved design defines async task creation, notification/timer selection, lock release before await, runtime/backend compatibility, handle ownership, cancellation, synchronous stop/drop guarantees, and exact production Scope Path; post-implementation task tests cover scheduling, wakeup ordering, cleanup, reconnect, shutdown/drop, and task 008 regressions through the canonical task entry.
- Explicit non-goals: no public retention semantic change, no per-user task/timer, no blocking mutex/Condvar wait inside async code, no persistence/history, no dependency/runtime-feature expansion without an upstream return, and no PN wire/accounting/neighbor-module change.

## Risks

- A lost async notification can make an earlier newly scheduled deadline wait behind an older later timer unless the loop combines notification and timer correctly and always rechecks state.
- Holding `std::sync::MutexGuard` across `.await` can block other sessions, make the future non-`Send`, or deadlock shutdown.
- A cleanup future that strongly owns the manager while awaiting indefinitely creates a lifetime cycle and prevents `Drop` from cancelling it.
- Cancelling a task without first marking shutdown/clearing state can race with a final cleanup mutation; clearing state without preventing later task work can recreate or remove the wrong entry.
- Synchronous `Drop` cannot blindly block on the same executor that runs the cleanup future. Handle abort/detach/completion semantics must be compatible with all supported runtimes and leave no post-drop manager mutation.
- A notification primitive with permit/coalescing semantics must be treated as a wake hint; assuming one wake per deadline operation can lose progress or create unnecessary loops.
- Extremely distant deadlines must retain task 008's checked saturation while avoiding timer APIs that reject or overflow huge durations; bounded sleep/recheck remains required.
- Tests that depend on wall-clock millisecond precision or executor thread-name inspection can be flaky; downstream testing needs deterministic task-lifecycle and wakeup evidence where feasible.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | no | `PnServer::set_user_traffic_retention`, snapshot lookup/traversal, `PnUserTrafficSnapshot`, and PN wire commands retain task 008 signatures and semantics; only the private cleanup executor changes | Design confirms no public or wire migration | Proposal compares the requested mechanism to the immutable task 008 contract | owner: design; reason: exact private handle/wakeup types belong to design; acceptance impact: any public semantic drift returns upstream and blocks acceptance | Runtime timing remains observable only through the already approved expiry guarantees |
| data/schema | no | Deadline, idle state, notification, and task handle remain process-local runtime state in `pn_server.rs`; no serialized storage or migration is introduced | Confirm implementation adds no persistence | Proposal excludes restart persistence and history | owner: none; reason: not applicable; acceptance impact: none | retained state is intentionally lost on expiry or shutdown |
| security/privacy/permission | no | No remote API, identity, admission, authorization, secret, or snapshot content changes | Confirm async task does not expose a new endpoint | Proposal confines work to private cleanup scheduling | owner: none; reason: not applicable; acceptance impact: none | none |
| runtime/integration | yes | The requested replacement changes task scheduling, timer/notification waiting, cancellation, shutdown/drop, executor capacity, and concurrency in `p2p-frame/src/pn/service/pn_server.rs` | Design must define runtime-compatible spawn/handle/wakeup primitives, select/recheck algorithm, lock/await boundaries, cancellation and ownership; testing must cover earlier-deadline wake, no-access expiry, reconnect/stale work, shutdown/drop, yield behavior, and task 008 lifecycle regressions | Proposal identifies the current thread/Condvar loop and existing runtime/executor abstractions | owner: design/testing; reason: exact task model and executable evidence belong to downstream stages; acceptance impact: blocking executor work, lost wakeups, leaks, or post-shutdown mutation blocks acceptance | Scheduler interleavings require targeted runtime tests rather than timing-only smoke checks |
| build/dependency/config/deployment | no | No Cargo feature, dependency, config format, environment, packaging, or deployment change is requested; existing runtime/executor support is a compatibility boundary | Design confirms existing abstractions cover supported backends | Proposal explicitly forbids dependency/runtime-feature expansion without upstream return | owner: design; reason: backend choice belongs to design; acceptance impact: an unrecorded feature/dependency change blocks implementation | Existing runtime backends may expose different task-handle cancellation capabilities that design must reconcile |
| ui/datamodel/workflow | no | No UI, frontend model, or user workflow consumes the private cleanup executor | Confirm no UI artifact is added | Proposal is limited to Rust runtime internals | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | The sibling uses existing packet, auto-pipeline/admission, testing, scope, and acceptance mechanisms without changing `harness/**` | Run existing stage-owned checks only | Proposal and scope evidence use current governance | owner: none; reason: not applicable; acceptance impact: none | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
