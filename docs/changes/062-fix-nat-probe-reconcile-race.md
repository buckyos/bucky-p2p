# Ensure stale NAT-probe reconciliation spares a rebuilt registration

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/062-fix-nat-probe-reconcile-race/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/062-fix-nat-probe-reconcile-race/proposal.md
- Affected paths: `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/service/nat_probe_scheduler.rs`, `p2p-frame/tests/unit/sn_tests/service/service/nat_probe_scheduler_tests.rs`
- Explicit tier override: none
- Expanded high-risk packet: none / existing task packet

## Approach

`SnService::reconcile_nat_probe_authority` reads the authority tunnel under the
scheduler lock, then awaits an async connection-list scan, then removed the
registration by `peer_id` and invalidated the peer NAT profile unconditionally.
A reconnect completing during the await could replace the registration, so the
stale cleanup deleted the new registration and its freshly published profile.

- `NatProbeScheduler::authority_registration` returns a `(tunnel_id,
  registration_generation)` snapshot; `reconcile_nat_probe_authority` reads it
  before the await.
- New `NatProbeScheduler::remove_peer_if_authority` re-validates that snapshot
  under the scheduler lock and removes only when both tunnel and generation
  still match, returning `false` (a debug `reason=stale_snapshot` trace) on any
  mismatch so the caller gives up.
- Removal and `peer_mgr.invalidate_net_profile` now share one scheduler-lock
  scope (`finish_nat_probe_authority_reconcile`), and every other peer NAT
  profile write (`apply_nat_probe_transition`, `maintain_nat_probe_state`
  expiry, `set_nat_probe_ports`) is moved under the scheduler lock. Because all
  registration and profile decisions serialize on that single mutex and
  `PeerManager` never takes the scheduler lock (ordering scheduler -> peer_mgr),
  a newer registration cannot slip a profile write between a stale removal's
  check and its invalidation, and no deadlock is introduced.

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: yes (this fixes a
  cross-await cleanup race by broadening a `std::sync::Mutex` scope across
  synchronous `PeerManager` calls; no await inside the lock, lock ordering is
  single-direction and peer_mgr-owned locks have no reverse dependency).
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

## Verification

- Targeted check: scheduler-level mismatch tests (tunnel, generation, match,
  absent), a service-level deterministic reconnect interleaving test proving the
  rebuilt registration and its published profile survive stale reconciliation,
  and a regression test proving a genuinely missing authority is still removed.
  `cargo test -p p2p-frame --features x509 --lib` passed (490/490); the
  nat-probe suite (33/33) and `nat_probe_logging_contract` (3/3) passed;
  `cargo check -p p2p-frame` clean.
- Result: pass
- Residual risk or follow-up: `on_peer_disconnected` removes by `peer_id`
   without the disconnecting tunnel id in its event, so a disconnect arriving
   after a concurrent re-registration on another tunnel is not re-checked here;
   this is a separate, out-of-scope observation.