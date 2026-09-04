# Invalidate stale Direct connection-info cache entries after consecutive failures

- Status: complete
- Owner module: p2p-frame
- Task manifest: `docs/versions/v0.1/modules/p2p-frame/043-direct-cache-failure-invalidation/task.yaml`
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/043-direct-cache-failure-invalidation/proposal.md`
- Affected paths: `p2p-frame/src/tunnel/tunnel_manager.rs`, `p2p-frame/src/tunnel/connection_info.rs`
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

The Direct branch of `open_known_tunnel_with_options` re-dialed the single cached endpoint on every open, so a NAT-restarted peer whose old port was dead paid one full transport timeout before the candidate matrix ran; the cached entry then stayed preferred (`+10,000`) and kept its never-decaying last-success bonus (`+2,000`), leaving a possible net `+6,000` score forever.

This change gives the cached Direct entry a real invalidation mechanism:

- `EndpointScore.fail_count` is now the consecutive-failure counter: it already increments on every failed dial and resets to zero on the next success, so `DIRECT_CACHE_MAX_FAILURES = 2` means "two consecutive failed dials since the last success".
- `preferred_direct_endpoints` no longer applies the `+10,000` cache-preferred bonus or the `+2,000` last-success bonus to an endpoint whose failure count reached the threshold. Static-WAN `+500` and the capped failure penalty remain unchanged, so the stale entry stops ranking first.
- `open_known_tunnel_with_options` checks the same degraded predicate before the single-endpoint preflight: once degraded it removes the cache entry and falls straight through to the full candidate matrix; if the preflight itself fails past the threshold, the entry is removed immediately. The matrix still includes the old endpoint concurrently, so a restored port can re-connect and the cache re-populates on success.
- `P2pConnectionInfoCache` gains `async fn remove(&self, conn_id)` with an empty default body so existing third-party implementors stay source-compatible; `DefaultP2pConnectionInfoCache` actually deletes the entry. The in-crate `MockDialNetwork` test helper gains a per-endpoint dial counter so the tests assert "no dedicated single-endpoint preflight" directly.

Regression tests cover: healthy cache keeps preference; one success followed by two failures drops the endpoint below a plain working candidate; a degraded cached entry is dialed exactly once (matrix only); an open that only has the stale endpoint removes the cached entry; and repeated opens stop issuing the preflight after the second consecutive failed dial (2 preflight+matrix dials then 1 matrix dial instead of 2+2).

## Risk Screen

- Public contract, protocol, or CLI change: no breaking change — `P2pConnectionInfoCache.remove` is an additive trait method with a default no-op; no wire, configuration, or CLI change.
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: yes — bounded single-module change to the cached-direct open path: stale entries are demoted and removed after consecutive dial failures, and degraded opens skip the dedicated preflight. This is the confirmed scope; tier stays standard.
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib` (432/432), `cargo test -p p2p-frame --features "x509 test-real-socket-matrix" --test real_p2p_tunnel_flow` (6/6), `cargo check --workspace` (clean), plus the three new in-crate regression tests (`degraded_cached_direct_entry_skips_preflight_and_loses_preference`, `degraded_cached_direct_entry_is_removed_when_single_endpoint_fails`, `stale_cache_direct_skips_preflight_after_two_failed_opens`) and the pre-existing `conn_info_cache_direct_preferred_on_reconnect` / `endpoint_score_isolated_by_protocol` cases.
- Result: passed
- Residual risk or follow-up: the unified `test-run.py p2p-frame unit` runner still omits `--features x509` in this dirty worktree, so the feature-gated in-crate suite is executed directly (same pre-existing tooling gap as 042). Mock/loopback coverage is not public-NAT or multi-host evidence; the N=2 threshold means a peer whose NAT just restarted can still pay the pre-existing preflight timeout on up to the first two failing dials before the entry is demoted.
