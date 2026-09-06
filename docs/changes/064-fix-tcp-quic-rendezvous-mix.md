# Repair TCP/QUIC mixed rendezvous candidates being rejected locally

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/064-fix-tcp-quic-rendezvous-mix/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/064-fix-tcp-quic-rendezvous-mix/proposal.md
- Affected paths: p2p-frame/src/tunnel/tunnel_manager.rs, p2p-frame/tests/unit/tunnel/rendezvous_endpoint_policy_tests.rs
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

`rendezvous_base_endpoints` for non-punch operations selected `Quic | Tcp`, so a single candidate array could carry both transports, while the pre-send validator `validate_rendezvous_endpoints` (sn.rs) requires one protocol per array. The mismatch returned `InvalidParam` before the request was sent and silently fell back to legacy.

Fix on the construction side only: for non-punch operations anchor the whole candidate set to one transport — QUIC when any eligible QUIC candidate exists, otherwise TCP. Punch operations keep their existing QUIC-only filter. Area/port filtering, dedup, `MAX_NAT_PLAN_CANDIDATES` truncation and non-empty semantics are preserved. No wire/protocol/sn.rs change.

## Risk Screen

- Public contract, protocol, or CLI change: no (request now satisfies the existing single-transport validator invariant; wire unchanged)
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: no
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

Residual risk: ReverseConnectOnly no longer advertises both transports at once; QUIC is preferred and TCP is only used when no eligible QUIC candidate exists. This is a deliberate net improvement over always being rejected locally; a scenario depending on dual-transport advertisement would be new scope.

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib reverse_connect`
- Result: pass
- Residual risk or follow-up: none beyond the recorded deliberate transport-preference change