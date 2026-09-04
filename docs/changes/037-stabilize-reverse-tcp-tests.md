# Stabilize reverse TCP tests: port guard and PN cache readiness sync

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/037-stabilize-reverse-tcp-tests/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/037-stabilize-reverse-tcp-tests/proposal.md
- Affected paths:
  - p2p-frame/src/lib.rs
  - p2p-frame/src/test_support.rs
  - p2p-frame/src/networks/tcp/tests/reverse_data_first_claim_tests.rs
  - p2p-frame/src/pn/service/pn_server/tests/reverse_tcp_proxy_tests.rs
  - p2p-frame/src/ttp/runtime (test-only snapshot seam)
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

### reverse_tcp_test_port_guard

Both reverse TCP fixtures used to acquire a reusable local endpoint by binding a
listener to `127.0.0.1:0` and immediately dropping it, leaving a window in
which another concurrent test can grab the same port. Replace that pattern with
a held, non-listening `socket2` TCP socket (Linux `SO_REUSEADDR`): the port
stays occupied from selection until the network's own connecting socket
finishes binding the same local endpoint, after which the guard drops and the
tunnel socket holds the port. Platform semantics were verified locally: two
bound non-listening reuse-address sockets coexist, and a connected socket
coexists with a later listener on the same port.

### pn_cache_readiness_deterministic_sync

The PN proxy test used to wait for B's control tunnel with a 5s `yield_now()` polling
loop over `has_cached_tunnel_for_test` (already non-destructive), but a timeout
gives no lifecycle signal. Diagnostics (test-only cache snapshot plus accept
progress) showed the real root cause in this order: B's TCP tunnel is remembered
while still `Connecting`, then the source FakeTunnel is attached and its
`remember_tunnel_in_multi` runs the production prune, deleting the non-Connected
B entry before the first control frame promotes it. Fix: wait for B to be
cache-ready (Connected) before pushing/attaching the source FakeTunnel, using a
low-frequency sleep poll and a snapshot-reporting panic. The 5s bound stays; no
production cache/TCP/PN semantics changed.

## Risk Screen

- Public contract, protocol, or CLI change: no (test-only)
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: no (production code unchanged)
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no (task docs only)
- Cross-project or architectural boundary change: no

Platform note: the guard relies on Linux `SO_REUSEADDR` semantics already used
by `new_tcp_socket`; the reuse flag is set under the same `target_os = "linux"`
gate as production code.

## Verification

- Targeted check: PN exact test + 12-case parallel pressure + reverse group, then full `p2p-frame --features x509 --lib`
- Result: pass
  - `pn::service::pn_server::tests::` 7-test 8-thread set that previously failed deterministically: 5/5 passes
  - `reverse_data_first_claim` group (20 tests, 8 threads): 3/3 passes
  - PN proxy exact + parallel-pressure pair (8 threads): pass
  - `ttp::` module (22 tests, 8 threads): pass
  - Full `--features x509 --lib` at `--test-threads=4`: 424/424 pass
  - Full `--features x509 --lib` default threads: one run 424/424; one earlier run had a single unrelated `sn_profile_flow_tests` `AddrInUse` (see residual)
- Residual risk or follow-up: the rare SN-side `AddrInUse` remains a pre-existing fixed-port allocation race (`sn/tests.rs` `NEXT_PORT` non-reserved sequential range). It is outside change 037's scope and is the exact remaining subject of unfinished task 032 `sn_test_bind_conflict_recovery` (P-032-2). Not fixed here to keep scope and production semantics unchanged; recommended follow-up is completing task 032 or a sibling.
