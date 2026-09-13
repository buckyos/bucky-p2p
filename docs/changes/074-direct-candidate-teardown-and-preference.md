# Close abandoned direct-path candidates explicitly and prefer proven tunnels on reuse

- Status: complete
- Owner module: p2p-frame
- Task manifest: `docs/versions/v0.1/modules/p2p-frame/074-direct-candidate-teardown-and-preference/task.yaml`
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/074-direct-candidate-teardown-and-preference/proposal.md`
- Affected paths: `p2p-frame/src/tunnel/tunnel_manager.rs`, `p2p-frame/src/networks/tunnel.rs`, `p2p-frame/src/networks/quic/tunnel.rs`, `p2p-frame/src/networks/tcp/tunnel.rs`, `p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs`
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

`TunnelManager::open_direct_path` dials every advertised endpoint concurrently and returned on the first success. The losing attempts were dropped while still inside `QuicTunnel::connect`, so their `quinn::Connection` handles were dropped without `close()`; quinn then reports an implicit `ApplicationClose { error_code: 0, reason: b"" }` to the peer. The peer had already registered those candidates as published, usable tunnels, and `select_preferred_tunnel_entry` picked the newest registration — often exactly a candidate the dialer had just abandoned — so the next channel open failed with `ConnectionLost`.

Two behavioural changes remove that failure chain:

- **Abandoned candidates are closed explicitly.** A direct candidate attempt now only returns the tunnel it established; registration, publish and `conn_info_cache` updates happen in `open_direct_path` for the selected candidate only. The losing attempts stay alive in a `FuturesUnordered` that is handed to `spawn_abandoned_direct_candidate_cleanup`, which drains them and calls `TunnelRef::close()` on every tunnel they still produce (bounded by each attempt's own connect budget). A candidate that cannot be registered/published is closed with the same explicitness instead of being dropped. The dialer therefore never hands out a loser and the peer observes a normal close reason instead of an empty-reason application close.
- **Tunnel reuse prefers proven candidates.** `Tunnel` gains a defaulted read-only hook `latest_business_activity_at()`; `TunnelActivity` records it only when a stream/datagram/control channel is actually established (`PendingTunnelActivity::promote`, `TunnelActivity::acquire_work_instances`) — not on establishment, heartbeats, listener registration or failed/aborted opens. QUIC and TCP tunnels expose it. `select_preferred_tunnel_entry` (used by both the published and the hidden-fallback selection) now prefers a candidate that already carried business traffic, most recently used first, and keeps the historical newest-registration order only among candidates that never carried traffic. Proxy-vs-non-proxy precedence is unchanged.

Compatibility: no wire/command/CLI change; the new trait method is additive with a default body, so test doubles and other `Tunnel` implementors (PN, mocks) keep compiling unchanged.

## Risk Screen

- Public contract, protocol, or CLI change: no — one additive `Tunnel` trait method with a default implementation; no wire codes, handshake, configuration or CLI change.
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: yes — bounded single-module change to direct-path candidate ownership and tunnel-reuse ordering. Losers are now drained and closed by a detached task instead of being cancelled by drop, and selection gains an activity-based preference. This is the confirmed scope; tier stays standard.
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib` (512 tests, incl. new `open_direct_path_closes_abandoned_candidates_explicitly`, `select_preferred_tunnel_entry_prefers_confirmed_business_activity`, `select_preferred_tunnel_entry_orders_proven_then_unused_candidates`, `networks::tunnel::activity_tests::business_activity_is_recorded_only_for_an_established_channel`), `cargo test -p p2p-frame --features "x509 test-real-socket-matrix" --test real_p2p_tunnel_flow` (7/7), `cargo check --workspace` (clean). Red-green: with the loser cleanup disabled the new abandonment test fails at `wait_for_close_count(&network, &slow_ep, 1)`, and with the historical `updated_at`-only ordering restored the confirmed-activity test fails.
- Result: passed
- Residual risk or follow-up: an abandoned candidate still exists on the peer until its own connect attempt finishes (bounded by the connect budget), so a cold-start race with no proven candidate still falls back to newest-registration order; closing that window needs protocol-level candidate confirmation or a pre-reuse liveness probe, both recorded as proposal non-goals. In-crate mock coverage and loopback real-socket coverage are not public-NAT, multi-host or deployed evidence. The feature-gated in-crate suite is executed directly because the unified `test-run.py p2p-frame` unit suite does not pass `--features x509` in this worktree (pre-existing tooling gap also recorded by 043).
