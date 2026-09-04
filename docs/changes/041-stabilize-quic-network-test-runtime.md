# Stabilize QUIC network test runtime under parallel load

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/041-stabilize-quic-network-test-runtime/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/041-stabilize-quic-network-test-runtime/proposal.md
- Affected paths:
  - p2p-frame/src/networks/quic/network.rs
  - docs/changes/041-stabilize-quic-network-test-runtime.md

## Approach

The QUIC network unit-test fixture created one `ServerRuntime` per network with
`ServerRuntimeConfig::default()`, which arms `num_cpus` current-thread tokio
worker runtimes per listener. Exercising the whole module in parallel therefore
backgrounded 21 tests × 2 networks × 16 workers = 672 dedicated runtime threads
on top of the tokio test-executor threads. Under that oversubscription, an
occasional QUIC handshake wait was delayed past the fixture's 3-second connect
timeout and surfaced as a misleading `ConnectFailed ... Elapsed(())` panic from
`pair.connect()` at `network.rs:1061` (the failing test name varied run to run;
the reported one was `quic_tunnel_open_datagram_without_listen_stays_pending`).

The test fixture now uses `ServerRuntimeConfig::new().with_workers(1)`: one
worker socket/runtime per test network, matching the existing convention used by
the SN tests, QUIC listener tests, and task 032 resource stabilization. The
3-second connect timeout, the tests' asserted behavior, and all production QUIC
listener/connect code are unchanged. A 60-second-timeout diagnostic run
previously confirmed the handshakes themselves complete in milliseconds even
under the same parallel load, proving the failures were scheduling starvation of
the fixture's own oversized worker fleet rather than a handshake defect.

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: no (production scheduling/runtime unchanged; test-only worker count reduction)
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 networks::quic::network::tests --lib -- --test-threads=21` repeated; user's exact test in isolation
- Result: pass
  - Red baseline (default 16-worker fixture): 5 consecutive parallel runs had 3 failures, all `ConnectFailed(Elapsed)` at `network.rs:1061`
  - Treated baseline (60-second diagnostic): parallel run fully passed, handshakes all millisecond-scale
  - Fixed fixture: 9 consecutive 21-thread runs passed (21/21 each)
  - User's exact test isolated: pass
  - Full `p2p-frame --features x509 --lib` suite: 427/427 passed
- Residual risk or follow-up: the TCP network fixture still uses the default
  worker count and remains a sibling risk if similar parallel starvation is ever
  observed there; low-CPU shared CI runners can still starve any real-socket
  suite, which is not covered by this change.
