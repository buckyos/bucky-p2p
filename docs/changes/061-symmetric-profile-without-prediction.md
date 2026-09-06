# Keep SymmetricLike Profile Without Prediction Hint

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/061-symmetric-profile-without-prediction/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/061-symmetric-profile-without-prediction/proposal.md
- Affected paths: p2p-frame/src/networks/quic/listener.rs, p2p-frame/src/networks/quic/network.rs, p2p-frame/src/networks/udp_network.rs, p2p-frame/src/networks/quic/listener/rendezvous_prediction_tests.rs, p2p-frame/src/sn/client/sn_service.rs, p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs, p2p-frame/tests/unit/networks/network/udp_tunnel_network_tests.rs, p2p-frame/tests/udp_tunnel_network_api_check.py
- Explicit tier override: none
- Expanded high-risk packet: none / existing task packet

## Approach

Split "obtain the mapping profile" from "generate prediction candidates". `QuicTunnelListener` gains `probe_nat_profile` containing the existing probe/classification logic (errors only on real probe failures); `predict_traversal_endpoints` builds on it and keeps returning `NotFound` when no prediction candidates exist, preserving rendezvous semantics. The crate-internal `UdpTunnelNetwork` trait gains the required `probe_nat_profile` method; `QuicTunnelNetwork` delegates to the listener with the same listener-selection logic as prediction, and the three in-repo test mocks add `NotSupport` stubs. The SN client probe entry (`SNService::probe_endpoints`) calls the profile-only API so a `SymmetricLike` observation without a usable prediction hint is reported instead of degraded to `Unknown`; a compatible `BoundedBestEffort` plan then becomes reachable for unpredictable symmetric peers.

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: no
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

The `UdpTunnelNetwork` trait gains a required method; every current implementer is inside this repository and is updated in the same change, so no compatibility break is introduced. Wire formats, scheduler, and connect-plan behavior are unchanged.

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib` (487 passed, includes the two new `nat_profile_probe_*` regression tests, tunnel-manager and udp-tunnel-network mocks, and SN profile flow tests); `cargo test -p p2p-frame --features x509 --tests` (all integration targets passed); `cargo check --workspace --all-targets --features p2p-frame/x509`; `python3 p2p-frame/tests/udp_tunnel_network_api_check.py --mode positive|negative` (updated to cover `probe_nat_profile`), `nat_probe_ports_api_check.py` and `signed_pnat_api_check.py` both modes.
- Result: passed
- Residual risk or follow-up: none known. Repo-wide `cargo fmt --check` and clippy show pre-existing drift/warnings unrelated to this change (see task 051 revert-format-only-changes precedent); the added/edited lines are rustfmt-clean and clippy-silent.
