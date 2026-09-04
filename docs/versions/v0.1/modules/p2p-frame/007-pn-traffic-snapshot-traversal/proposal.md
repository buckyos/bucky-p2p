---
module: p2p-frame
task_name: 007-pn-traffic-snapshot-traversal
submodule: 007-pn-traffic-snapshot-traversal
version: v0.1
status: approved
approved_by: user
approved_at: 2026-07-15T10:12:53+08:00
approved_content_sha256: d14061d8e405cd4112e086d25dd32a3c46d9b377938b56f3d9dc6029a2a59b57
---

# PN Traffic Snapshot Traversal and Disconnected User Cleanup Proposal

## Background and Goal

`PnTrafficManager` currently stores every observed user in one `HashMap<P2pId, Arc<PnUserTrafficEntry>>` and only supports looking up one known user through `snapshot(...)`. An operator therefore cannot discover and inspect all currently tracked users. Returning every user and snapshot in one collection would create latency and memory spikes when a PN server has many users, so discovery must be incremental and bounded.

The same map currently has no removal path. `begin_session(...)` creates source and target entries, but completion of a data or control bridge does not release either entry. Disconnected users therefore retain statistics trackers and limiter objects indefinitely. In addition to cumulative byte and current-speed values, each externally acquired `PnUserTrafficSnapshot` must report how many transmit and receive bytes were added since that user's previous external snapshot acquisition. The goal is to add an incremental traversal surface with interval deltas for active traffic snapshots and to bind statistics entry lifetime to active PN bridge sessions, without changing PN wire behavior or losing explicitly configured traffic-limit policy on reconnect.

## Scope

### In scope

- Let `PnTrafficManager` and the corresponding `PnServer` observation surface traverse currently tracked users and obtain each user's `PnUserTrafficSnapshot` incrementally.
- Yield a user identity together with one value snapshot at a time, or through an equivalently bounded cursor/visitor contract; do not materialize or return a collection containing all users and snapshots.
- Extend `PnUserTrafficSnapshot` with transmit-byte and receive-byte delta values representing traffic added between the current external acquisition and the preceding external acquisition for that user, while preserving the existing cumulative byte and current-speed fields.
- Treat the first external acquisition for a fresh live statistics entry as an interval beginning at entry creation, so its byte deltas equal the cumulative byte values observed at that acquisition.
- Atomically sample cumulative counters and advance a per-user shared acquisition baseline when point lookup or traversal externally acquires a snapshot. Concurrent acquisitions must have a defined linearization order and must not both report the same newly added bytes.
- Keep internal diagnostics, including bridge-stop logging, non-consuming: logging or lifecycle inspection must not advance the external acquisition baseline or steal interval deltas from callers.
- Ensure traversal does not invoke caller-controlled work or suspend while holding the traffic-manager map lock. The design must define bounded lock ownership and concurrent insert/remove behavior.
- Track source and target participation in successful data and control bridge sessions, including multiple concurrent sessions and the case where source and target are the same user.
- Release a user's live statistics entry after that user's last active traffic session ends. Cleanup must be identity/generation safe so an older session cannot remove a newer entry created by a concurrent reconnect.
- Keep explicitly configured per-user traffic-limit policy available across a disconnect/reconnect cycle even though live counters, speed trackers, and per-session limiter state are released.
- Make a disconnected user with no active session disappear from point lookup and traversal after cleanup; a reconnect starts a fresh live statistics snapshot from zero and reuses any retained explicit limit policy.

### Out of scope

- Returning all user snapshots in one `Vec`, map, array, serialized response, or other unbounded aggregate.
- Providing a globally consistent point-in-time snapshot while users concurrently connect, disconnect, or update counters.
- Providing an independent interval-delta baseline for each observer, caller, management connection, or traversal instance; the initial contract uses one shared external-acquisition baseline per live user entry.
- Treating `tx_speed` / `rx_speed` as interval byte counts or changing their existing speed-window semantics; only new byte-delta fields represent traffic added since the previous acquisition.
- Persisting traffic counters or limit configuration across process restart, exporting metrics to a remote service, or adding historical/offline traffic reports.
- Changing the existing source/target byte-accounting direction, speed calculation, source-only limit semantics, or the definition of successfully forwarded bytes.
- Adding target-side limiting, pair-level quotas, billing, durable accounting, or cross-PN aggregation.
- Changing PN `ProxyOpenReq` / `ProxyOpenResp`, control-channel commands, tunnel admission, relay-session registration, TLS-over-proxy, or bridge wire behavior.
- Modifying production code, tests, design, or testing artifacts during this proposal stage.

### Boundary with neighboring modules

- The behavior is owned by `p2p-frame/src/pn/service/pn_server.rs`, where `PnTrafficManager`, `PnTrafficSession`, bridge construction, public traffic lookup, and bridge completion currently reside.
- `sfo-io` continues to own `SfoSpeedStat`, `SpeedLimiter`, tracker, and limit-session mechanics. `p2p-frame` only manages their per-user lifecycle and exposes copied snapshot values.
- `cyfs-p2p`, `cyfs-p2p-test`, and `sn-miner-rust` do not define an alternate traversal or retention model. Any later caller adaptation must consume the `p2p-frame` contract.
- Existing `get_user_traffic_snapshot(...)` and `set_user_traffic_limit(...)` behavior remains a compatibility boundary except that point lookup now consumes the shared interval baseline, returns no live snapshot after final-session cleanup, and a later reconnect starts fresh cumulative counters and a fresh interval baseline.

## Requirement Review

- Incremental traversal is reasonable and necessary because an unbounded result makes observation cost proportional to the entire user population at one allocation/call boundary. A cursor, iterator, or callback-based visitor is acceptable only if it preserves bounded production and lock ownership.
- Adding interval byte counts alongside cumulative values is reasonable because callers otherwise need to retain their own previous totals. A per-user shared baseline is the smallest contract compatible with the existing manager-owned point lookup and the new traversal. It also means multiple external consumers influence one another's intervals; independent consumer baselines require observer identity and lifecycle management and are intentionally deferred.
- Snapshot acquisition must be linearizable at the per-user entry: the operation reads current cumulative totals, computes non-negative deltas from the previous external baseline, and advances that baseline as one logical action. Internal non-consuming snapshots may read cumulative/speed values for logs but must not mutate the baseline.
- Holding the `users` mutex for the full duration of external iteration would block bridge creation and disconnect cleanup when callers are slow. The chosen direction therefore forbids caller-controlled work under that lock and accepts weakly consistent traversal under concurrent churn.
- Cleanup must be session-aware rather than triggered by any one stream closing: a user can be source or target in several concurrent data/control bridges. Removal is allowed only after the last participation ends.
- A simple key-only `HashMap::remove` is unsafe across reconnect races. Cleanup must prove that the map still contains the same entry/generation whose active count reached zero before removing it.
- Traffic-limit configuration is control-plane policy, while counters and speed trackers are live observation state. Preserving explicit limits across reconnect avoids silently making a configured user unlimited merely because the last bridge closed; implementation may separate retained policy from the releasable live entry.
- Traversal is intentionally not a transaction. Each yielded value must be a self-contained snapshot, but concurrent user churn may cause a user to be absent from a particular pass. The design must state progress/termination semantics without promising a globally frozen view.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-PN-TRAFFIC-TRAVERSE-1 | pn_traffic_snapshot_incremental_traversal | Expose a bounded traversal over currently live PN users that yields each `P2pId` with a copied `PnUserTrafficSnapshot` without building an all-user result or executing caller-controlled work while holding the traffic-manager map lock | Limited to `PnTrafficManager` plus the `PnServer` observation surface; existing per-user lookup and accounting meanings remain intact | Accepts weak consistency under concurrent connect/disconnect in exchange for bounded allocation, bounded lock ownership, and forward progress with large user populations | Approved design defines the traversal state/API, concurrency semantics, lock boundary, and exact production paths; post-implementation tests consume entries incrementally, cover an empty manager and multiple users, and exercise concurrent removal without an all-user aggregate | No globally atomic snapshot, history store, remote export, serialization format, or cross-PN aggregation |
| P-PN-TRAFFIC-DELTA-1 | pn_traffic_snapshot_interval_delta | Preserve cumulative bytes and speeds in `PnUserTrafficSnapshot` while adding transmit/receive byte deltas since the same user's prior external snapshot acquisition; first acquisition reports cumulative bytes as deltas, point lookup and traversal share one atomic per-user baseline, and internal diagnostics do not consume it | Limited to snapshot construction/acquisition state in `PnTrafficManager` and the public `PnServer` observation paths; byte-accounting direction and speed semantics do not change | Avoids caller-maintained totals with bounded per-user state, but separate external consumers share and advance the same interval baseline rather than receiving independent deltas | Approved design defines field names, atomicity/locking, first-read/reset behavior, consuming versus non-consuming paths, and interaction with cleanup; tests cover first/subsequent reads, zero-new-traffic reads, point/traversal interaction, concurrent acquisitions without double reporting, logging non-consumption, and reconnect reset | No per-observer cursor identity, durable interval history, speed semantic change, billing-grade exactly-once delivery, or cross-process baseline |
| P-PN-TRAFFIC-CLEANUP-1 | pn_traffic_disconnected_user_release | Count each distinct user's participation in successful data/control traffic sessions and release that user's live statistics entry only after its last session ends; cleanup is reconnect-race safe, same-user source/target safe, and retained explicit limit policy is reapplied to a fresh entry on reconnect | Limited to PN traffic state lifecycle; no PN wire, relay admission/session-registry, byte-accounting direction, speed algorithm, or source-only limiting change | Point lookup/traversal no longer provides historical counters for offline users, and reconnect counters restart at zero; retaining limit policy may require separating policy storage from live statistics state | Approved design maps acquisition/release to both bridge paths and defines identity-safe removal; post-implementation tests cover single and concurrent sessions, source/target roles, source equals target, final disconnect removal, reconnect with zeroed counters, stale-session cleanup races, and retained configured limits | No durable counters, offline reporting, policy persistence across process restart, target limiting, billing, or session-registry redesign |

## Success Criteria

- Concrete user-visible or system-visible result: callers can walk active PN user traffic snapshots with bounded per-step work, each acquired snapshot includes cumulative and since-prior-acquisition byte counts, and users who have no remaining PN traffic session no longer occupy live statistics entries.
- Required evidence: an approved design directly maps all three change IDs to the traffic-manager data structures, consuming/non-consuming snapshot contract, traversal contract, public observation surface, data/control bridge acquisition/release points, concurrency invariants, and tests; post-implementation testing covers interval-delta semantics, traversal scale shape, and cleanup/reconnect edge cases through the repository's canonical p2p-frame test entry.
- Explicit non-goals: no unbounded all-user return, no per-observer independent baseline, no historical offline statistics, no durable persistence, no globally frozen concurrent snapshot, no target-side limiting, no speed/accounting/wire-protocol changes, and no unrelated PN lifecycle refactor.

## Risks

- An iterator that borrows the mutex guard or a visitor called under the guard could stall new sessions and cleanup for an unbounded duration.
- Hash-map churn can invalidate naive positional cursors. The design must choose a cursor/progress model that cannot loop forever or retain an unbounded key copy.
- Point lookup and traversal share one interval baseline, so an unexpected secondary observer can make another caller see a smaller or zero delta. This is an explicit first-version tradeoff and must be visible in API documentation.
- Existing bridge-stop calls to `snapshot(...)` will consume deltas if the implementation does not separate non-consuming diagnostics from external acquisition.
- Concurrent delta acquisition can double-report or lose newly added bytes unless cumulative sampling and baseline advancement are one per-entry atomic/locked operation.
- Releasing on the first closed bridge would make counters disappear while another data/control bridge is active and could reset shared source-side limiting unexpectedly.
- A stale session can race with reconnect and accidentally delete the reconnect's fresh entry unless cleanup checks entry identity or generation under the map lock.
- The same user appearing as both source and target can be double-counted during acquisition or released twice unless participation is deduplicated per traffic session.
- Separating retained limit policy from releasable live state can introduce configuration/live-entry races; applying a limit update and creating a fresh entry must have a defined ordering.
- Observers must not treat traversal as billing-grade history because disconnected entries are intentionally removed and concurrent traversal is weakly consistent.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/src/pn/service/pn_server.rs` currently exposes `PnServer::get_user_traffic_snapshot(...)` and `PnUserTrafficSnapshot`; this task extends the snapshot value, adds a traversal contract, makes public acquisition consume a shared interval baseline, and changes offline point-lookup visibility after final-session cleanup without changing PN wire commands | Design must define field/API/caller compatibility, first/subsequent delta semantics, consuming versus non-consuming paths, traversal completion/error semantics, offline lookup behavior, and positive/negative contract cases; testing must cover old cumulative/speed values plus new point/traversal delta behavior | Proposal inspection identified the existing point lookup, snapshot fields, bridge-stop diagnostic snapshot calls, private manager map, and bridge-owned session construction | owner: design/testing; reason: exact API mapping and runnable contract cases belong to later stages; acceptance impact: missing bounded traversal, delta semantics, or compatibility evidence blocks acceptance | Downstream callers may have implicitly treated every lookup as a side-effect-free cumulative read or accumulated counters as available forever |
| data/schema | no | `PnUserTrafficEntry` and the `users` map in `p2p-frame/src/pn/service/pn_server.rs` are process-local runtime objects; no serialized, cached, database, file, migration, or restart-persistent representation is in scope | Scope review confirms implementation does not introduce persistence or serialized snapshot formats | Proposal inspection found no durable storage path for PN traffic state | owner: none; reason: not applicable; acceptance impact: none | Live counters are intentionally lost at final disconnect and process restart |
| security/privacy/permission | no | The task does not change authenticated `req.from` normalization, target identity, PN admission, TLS, permissions, secrets, or which fields exist in `PnUserTrafficSnapshot`; it changes iteration and retention of the same in-process observations | Design review confirms no new unauthenticated network query or remote exposure is added | Proposal binds the change to the existing local `PnServer` observation surface and excludes wire/API transport export | owner: none; reason: not applicable; acceptance impact: none | A later remote management API would need its own authorization proposal |
| runtime/integration | yes | `PnTrafficManager::begin_session(...)`, `PnUserTrafficEntry::snapshot(...)`, both bridge paths and their diagnostic snapshot calls, the mutex-protected user map, and asynchronous `copy_bidirectional(...)` completion in `p2p-frame/src/pn/service/pn_server.rs` are concurrent runtime lifecycle/observation surfaces | Design must specify atomic delta sampling/baseline advancement, non-consuming diagnostics, acquisition/release, cancellation/drop behavior, lock ordering, stale-entry protection, traversal progress, and limit-update races; testing must include failure/cancellation and concurrency-focused unit or DV coverage | Proposal inspection traced snapshot construction, data/control bridge construction, bridge-stop lookup, and confirmed no current removal path | owner: design/testing; reason: implementation mechanics and executable race coverage belong to later stages; acceptance impact: duplicate/lost deltas, log-consumed deltas, leaks, premature removal, deadlock, or reconnect deletion blocks acceptance | Hard-to-reproduce acquisition and disconnect/reconnect interleavings may remain unless per-entry baseline and lifecycle are explicitly synchronized and cleanup uses identity checks |
| build/dependency/config/deployment | no | The requested behavior is confined to existing Rust runtime types in `p2p-frame/src/pn/service/pn_server.rs`; no Cargo metadata, feature, dependency, environment, config schema, packaging, or deployment resource is requested | Confirm design does not add a dependency or new deployment configuration | Proposal uses existing `std`/`sfo-io` capabilities and current limit configuration surface | owner: none; reason: not applicable; acceptance impact: none | none |
| ui/datamodel/workflow | no | The workspace has no UI path for `PnUserTrafficSnapshot`; this task changes a Rust observation API and internal lifecycle only | Confirm no UI or frontend contract is introduced | Proposal inspection found no UI consumer or presentation artifact | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | The task uses the existing packet, approval, admission, testplan, stage-scope, and acceptance mechanisms and does not change `harness/**`, templates, scripts, CI, or governance | Run only the existing stage-owned checks as their inputs are produced | Proposal packet and scope evidence use current repository mechanisms | owner: none; reason: not applicable; acceptance impact: none | none |

## Approval Record

- approver: user
- approval_date: 2026-07-15
- user_statement: "确认，自动处理后续步骤"
