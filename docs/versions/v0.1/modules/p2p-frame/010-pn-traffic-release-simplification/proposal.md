---
module: p2p-frame
task_name: 010-pn-traffic-release-simplification
submodule: 010-pn-traffic-release-simplification
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# PN Traffic Release Simplification Proposal

## Background and Goal

Tasks `007-pn-traffic-snapshot-traversal`, `008-pn-traffic-disconnect-retention`, and `009-pn-traffic-async-cleanup-task` introduced active-session accounting, retained idle statistics, reconnect reuse, ordered deadline cleanup, and one asynchronous cleanup task. The resulting release path also defends against stale deadline records and replacement user entries that current production transitions cannot create because acquisition, release, reconnect cancellation, deadline removal, and expiry validation all execute under the same manager-state mutex. It additionally accepts the full `Duration` input domain by searching for the furthest platform-representable `Instant` when direct addition overflows.

The goal is to keep the user-visible lifecycle behavior while simplifying the internal state machine: retain only the checks needed for reachable session/reconnect/cleanup/shutdown interleavings, remove generation-based and artificially injected stale-state defense, and replace full-domain `Duration::MAX` support with a finite documented retention bound.

## Scope

### In scope

- Remove the manager-wide idle generation counter and per-user/per-deadline generation fields from PN traffic retention state.
- Represent an idle user's scheduled cleanup with its exact deadline and user identity, with deadline membership and user state still mutated atomically under the existing manager-state mutex.
- Keep the minimum identity validation needed for a dropping session guard to avoid modifying an entry other than the one it acquired.
- Preserve active-session counting so a user becomes idle or is removed only after that user's final distinct data/control bridge ends.
- Preserve zero-retention immediate removal, non-zero-retention idle visibility, reconnect reuse of the same live counters/delta baseline/limiter state, automatic expiry, retained explicit limit policy, and shutdown clearing.
- Remove production logic and tests whose only purpose is to tolerate a stale deadline or replacement entry manually injected by bypassing the manager's production transition APIs.
- Replace the full-input-domain retention deadline search with a finite, documented maximum retention duration selected in design; values above that maximum are clamped while `PnServer::set_user_traffic_retention(Duration)` remains source compatible.
- Keep one bounded asynchronous cleanup task per manager, its notification/timer wait model, and its no-lock-across-await property.

### Out of scope

- Changing `PnUserTrafficSnapshot`, cumulative or delta accounting, speed semantics, traversal ordering/weak consistency, traffic direction, or source-side limiting.
- Removing active-session counting, RAII session release, exact-deadline validation, the cleanup task, reconnect deadline cancellation, or shutdown state.
- Making cleanup periodic or scan-based, adding a task/timer per user, adding a global cleanup service, or changing cleanup batching.
- Changing PN wire messages, bridge transport, tunnel admission, relay registration, TLS/identity behavior, or neighboring crates.
- Changing the public retention setter signature to return an error or introducing a new configuration/persistence format.
- Preserving exact requested retention for values above the new finite maximum, including `Duration::MAX`.
- Editing the completed 007, 008, or 009 task packets; this sibling supersedes only their generation-based stale-defense and full-`Duration` requirements.

### Boundary with neighboring modules

- The behavior remains owned by `p2p-frame/src/pn/service/pn_server.rs` and its dedicated PN traffic-manager tests.
- `sfo-io` continues to own statistics and limiter primitives; their behavior and lifetime contracts do not change.
- `crate::runtime` and `crate::executor` continue to provide asynchronous sleep and task spawning; this task does not change those abstractions.
- `cyfs-p2p`, `cyfs-p2p-test`, and `sn-miner-rust` continue consuming the existing `PnServer` traffic APIs without signature migration.

## Requirement Review

- The simplification is reasonable because the current cleanup task selects and validates due work while holding the same mutex used by reconnect and release. Production code removes an idle user's exact scheduled deadline before reactivating that user, so no stale cleanup record crosses an unlocked or awaited execution boundary.
- Deadline equality plus active-session state remains necessary: it proves that a due record still represents the user's current idle transition. A session guard's acquired-entry identity remains a cheap safeguard around RAII release and manager shutdown/replacement boundaries.
- Generation fields add a second identity namespace on top of exact deadline membership without protecting an additional reachable interleaving. Tests that manufacture stale records by directly mutating private state should not force production complexity.
- A finite retention maximum is preferable to binary-searching the platform-specific edge of `Instant`. Retention is operational grace-period configuration rather than durable history. Clamping preserves the existing infallible setter and avoids a public API break, but callers requesting a larger duration will observe the documented maximum instead.
- Design must choose and document one maximum that is safely representable on supported targets and operationally sufficient, and must define how it is exposed to tests or callers. It must not silently restore a platform-limit search under another name.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-PN-TRAFFIC-RELEASE-SIMPLE-1 | pn_traffic_release_state_simplification | Remove idle generations and artificial stale-state tolerance while preserving exact-deadline membership, acquired-entry identity, final-session release, reconnect cancellation/reuse, automatic expiry, and shutdown safety under the manager's single state mutex | Limited to private `PnTrafficManager` user/deadline/session lifecycle state and its dedicated tests in `p2p-frame` | Reduces duplicated identity state and unreachable defensive branches; deliberately stops guaranteeing recovery from private-state corruption or manually injected stale deadline records | Approved design demonstrates that no authoritative deadline leaves the mutex for later mutation and maps the reduced state transitions; post-implementation tests cover concurrent sessions, same-user source/target, reconnect before expiry, expiry, cancellation/drop, and shutdown using production APIs only | No weakening of reachable race safety, no periodic sweep, no per-user task, no snapshot/accounting/API/wire change, and no tests that bypass production transitions to fabricate impossible manager state |
| P-PN-TRAFFIC-RETENTION-BOUND-1 | pn_traffic_retention_finite_bound | Replace furthest-representable-`Instant` search and exact `Duration::MAX` handling with a documented finite retention maximum; clamp larger values while preserving the current infallible `Duration` setter | Limited to retention normalization/deadline calculation and focused boundary tests in `p2p-frame` | Makes extreme requested values approximate but removes platform-edge binary search and repeated reliance on enormous monotonic deadlines | Approved design specifies the constant/value, normalization point, supported-target rationale, and deadline calculation; tests cover zero, ordinary, exact-maximum, and above-maximum inputs without requiring `Duration::MAX` to remain exact | No setter signature break, no error-return migration, no persistence, no platform-specific maximum discovery, and no promise that values above the finite bound retain their requested duration |

## Success Criteria

- Concrete user-visible or system-visible result: ordinary PN traffic statistics still disappear immediately at zero retention or after the configured bounded grace period, remain visible and reusable during that period, and are cleared on shutdown; the manager no longer carries idle generation state or searches for the platform's furthest representable `Instant`.
- Required evidence: approved design maps the reduced state machine and finite bound to exact production paths; post-implementation coverage exercises reachable concurrent lifecycle transitions and duration normalization through production APIs; canonical unit/DV/integration checks required by the trigger analysis pass.
- Explicit non-goals: no traffic accounting, traversal, limiter, bridge, wire, runtime abstraction, public setter signature, neighboring-module, or persistence change; no support guarantee for fabricated private stale state or exact retention beyond the finite maximum.

## Risks

- Removing generations would be unsafe if implementation later popped a deadline under the mutex and performed deletion after releasing the mutex. Design and acceptance must keep selection, validation, and removal atomic or return upstream.
- A future change that stops removing the exact old deadline during reconnect could reintroduce stale records; focused invariant tests must cover production reconnect cancellation.
- Clamping is a semantic change for callers that pass very large durations. The chosen maximum must be documented and tested, with zero and ordinary values unchanged.
- If the maximum is chosen without checking supported targets, direct `Instant` addition could still overflow. Design must justify a safely representable bound and retain a simple fail-closed assertion or checked calculation.
- Removing tests that inject impossible state must not remove coverage for reachable cancellation, reconnect/expiry, multiple-session, or shutdown races.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/src/pn/service/pn_server.rs` exposes `PnServer::set_user_traffic_retention(Duration)`; this task preserves its signature but changes above-maximum semantics from platform saturation to finite clamping | Design records the finite maximum, compatibility semantics, and caller impact; testing covers zero, ordinary, boundary, and clamped inputs | Proposal preserves the setter signature and explicitly defines the intended compatibility break only above the new maximum | owner: design/testing; reason: the numerical bound and executable boundary cases belong downstream; acceptance impact: an undocumented or untested clamp blocks acceptance | Existing callers using extreme durations may observe earlier expiry than requested |
| data/schema | no | `PnTrafficManagerState`, idle deadlines, generations, snapshots, and limit policies in `p2p-frame/src/pn/service/pn_server.rs` are process-local and are not serialized or persisted | Confirm design and implementation add no durable representation or migration | Proposal excludes persistence and schema changes | owner: none; reason: not applicable; acceptance impact: none | Runtime statistics remain intentionally lost on expiry or shutdown |
| security/privacy/permission | no | The affected paths are private lifecycle state and an existing in-process setter; no authentication, authorization, identity, secret, remote endpoint, or snapshot content changes | Confirm implementation introduces no new exposure | Proposal confines changes to private release/deadline mechanics | owner: none; reason: not applicable; acceptance impact: none | none |
| runtime/integration | yes | `PnTrafficSessionGuard::drop`, `PnTrafficManager::begin_session`, `release_user`, cleanup scheduling, reconnect cancellation, and shutdown in `p2p-frame/src/pn/service/pn_server.rs` are concurrent runtime lifecycle paths | Design proves single-mutex atomicity and no lock across await; testing covers failure/cancellation, concurrent sessions, reconnect before expiry, due cleanup, clamping, and shutdown; run canonical p2p-frame unit/DV/integration entries | Proposal identifies the reachable state transitions that remain protected and the unreachable injected-state behavior being removed | owner: design/testing; reason: reduced structures and executable concurrency coverage belong downstream; acceptance impact: premature deletion, leaked entries, or a stale reachable cleanup blocks acceptance | Future changes could invalidate the single-mutex stale-record argument unless invariants remain explicit |
| build/dependency/config/deployment | no | The existing Rust setter remains the only configuration surface; `Cargo.toml`, features, dependencies, environment variables, files, packaging, and deployment defaults are unchanged | Confirm no dependency, feature, or deployment artifact changes | Proposal forbids dependency/config-format and runtime-abstraction changes | owner: none; reason: not applicable; acceptance impact: none | The finite bound still needs Rust API documentation even though deployment configuration is unchanged |
| ui/datamodel/workflow | no | No UI, frontend model, navigation, accessibility state, or user workflow consumes the private PN traffic release state | Confirm no UI artifact is introduced | Proposal is limited to Rust runtime internals and an existing setter | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | The task uses existing `harness/rules`, scripts, packet structure, stage-scope evidence, and test entrypoints without changing them | Run existing stage-owned checks only | Proposal introduces no Harness artifact behavior change | owner: none; reason: not applicable; acceptance impact: none | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
