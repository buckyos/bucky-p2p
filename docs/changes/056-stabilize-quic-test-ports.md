# Stabilize QUIC network test listener ports

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/056-stabilize-quic-test-ports/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/056-stabilize-quic-test-ports/proposal.md
- Affected paths: p2p-frame/src/networks/quic/network.rs
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

Replace `127.0.0.1:0` in the existing QUIC network test fixture with process-local monotonic ports from a QUIC-only range below the SN test allocator. This prevents independent `sfo-reuseport` listeners from joining the same UDP reuse-port group while retaining parallel real QUIC/TLS handshakes and the existing timeouts.

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: no; the production runtime is unchanged and the edit only coordinates test fixture addresses
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no; only an existing module-local fixture changes
- Cross-project or architectural boundary change: no

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 networks::quic::network::tests::quic_tunnel_open_stream_without_listen_stays_pending --lib -- --exact`; six consecutive runs of `cargo test -q -p p2p-frame --features x509 networks::quic::network::tests --lib -- --test-threads=21`
- Result: passed
- Residual risk or follow-up: the exact test passed 1/1 and all six parallel runs passed 21/21; before the fix the same parallel command failed 20/21 with a drifting test and `no server certificate chain resolved`. Explicit test ports can conflict with unrelated host processes; such a bind failure remains visible and is preferable to silently reaching another test identity. The broader `cargo test -p p2p-frame --features x509 --lib` run passed every QUIC test but finished 463/467 because four existing SN endpoint/rendezvous tests failed; `sn_client_query_registered_peer_returns_full_info` also fails alone, and the task baseline proves no SN path changed here.
