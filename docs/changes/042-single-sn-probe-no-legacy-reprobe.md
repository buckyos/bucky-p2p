# Single SN probe: rendezvous failure path no longer re-probes SN through legacy

- Status: complete
- Owner module: p2p-frame
- Task manifest: `docs/versions/v0.1/modules/p2p-frame/042-single-sn-probe-no-legacy-reprobe/task.yaml`
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/042-single-sn-probe-no-legacy-reprobe/proposal.md`
- Affected paths: `p2p-frame/src/tunnel/tunnel_manager.rs`, `p2p-frame/src/sn/client/sn_service.rs`, `p2p-frame/src/runtime/tokio.rs`, `p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs`
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

`open_tunnel_from_id` is already the single SN probe point: `DefaultDeviceFinder::get_peer_info` runs one `sn_service.query_with_context` and both rendezvous and legacy consume the same `PeerLookupInfo` (endpoints, profiles, SN id, context). The defect is that `open_nat_aware_tunnel` then unconditionally entered `open_nat_aware_tunnel_legacy` after any rendezvous failure, issuing a second SN arrangement (`call_via_sn`) plus a parallel local action.

This change removes that re-probe:

- `open_nat_aware_tunnel` no longer calls `open_nat_aware_tunnel_legacy` from the rendezvous failure branch. All rendezvous failures now end in the proxy fallback, so no SN request is ever made after the single rendezvous request.
- Rendezvous failures are classified inside `open_rendezvous_tunnel`:
  - Deterministic failures (`NotFound`, `Conflict`, `AlreadyExists`, `NotSupport`, `Failed` response, local validation/codec, prediction/policy errors) return immediately; the caller goes straight to proxy and never runs the local action.
  - Ambiguous failures (`IoError` from transport/command timeout, `Unmatch`/`InvalidData` from response validation) mean the target may already have armed the arranged action, so the caller-side `NatPlanAction` is retried once locally with the same rendezvous `tunnel_id`/incoming waiter and the query-derived endpoints/profiles; no SN request is involved. Failure of that retry then proxies.
- A total per-open deadline wraps the rendezvous-attempt chain (rendezvous + ambiguous local retry + proxy fallback). Budget is derived from the SN `call_timeout` plus two `conn_timeout`s (default 10s + 2×5s = 20s); the inner action window keeps its existing `conn_timeout` cap via `deadline.min(now + conn_timeout)`.
- `open_nat_aware_tunnel_legacy` keeps its existing semantics only for the non-rendezvous-plan branch (unreachable from the normal NAT-aware entry), per the approved non-goal.

Implementation finding during verification: the response-success caller action previously dialed only `rendezvous_base_endpoints(...)`, which filters to rendezvous-eligible areas and is empty for LAN-only loopback peers. The removed legacy fallback had masked this by re-running the plan action over the full query endpoints. To keep single-probe connectivity (proven by the real-socket strategy matrix, which had `require_connected=true` for the non-symmetric row), the response-success action now runs the plan's `NatPlanAction` through `execute_nat_action` for all non-predicted variants, consuming the full query-derived endpoints (LAN included) with the existing `nat_candidates` expansion. `use_predicted_response=true` variants keep the response-predicted endpoint path unchanged. This is the only way rendezvous itself, rather than a second SN arrangement, can complete those topologies; it adds no SN request and no extra retry after a successful response.

Implementation specifics:

- `SNClientService` gains a `pub(crate) fn call_timeout()` accessor so `TunnelManager` can derive the deadline without a config/API change.
- `runtime::tokio` re-exports `tokio::time::timeout_at`.
- New helper `is_ambiguous_rendezvous_failure` centralizes the deterministic/ambiguous split and is unit-tested directly.

## Risk Screen

- Public contract, protocol, or CLI change: no (rendezvous wire, query wire, and all public signatures unchanged; `call_timeout()` is `pub(crate)`)
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: yes — bounded single-module change to the rendezvous failure path: failing attempts are classified and either proxy immediately or run one local-action retry without a second SN request, under a derived total deadline. This is the confirmed scope; tier stays standard.
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

## Verification

- Targeted check: in-crate classification and deterministic-path regression tests; updated `rendezvous_sn_inactive_deterministic_failure_skips_action_and_proxies`; live-socket SN matrix; real tunnel-flow strategy matrix; workspace compile closure — `cargo test -p p2p-frame --features x509 --lib` (429/429), `--test sn_protocol_real_network` (3/3), `--test real_p2p_tunnel_flow` with feature test-real-socket-matrix (6/6, all strategy-matrix rows on attempt 0), `--test tunnel_rendezvous_protocol` (7/7), `cargo check --workspace` (clean).
- Result: passed
- Residual risk or follow-up: the unified `test-run.py p2p-frame unit` still runs `cargo test -p p2p-frame` without `--features x509` and fails to compile the x509-gated in-crate test include; that is pre-existing tooling drift in this dirty worktree, outside the confirmed scope. The ambiguous-branch end-to-end execution has no injected SN-command-failure unit test (classification and shared `execute_nat_action` are covered; live-socket suites cover response-success and deterministic paths). The total deadline's multi-slow-SN worst case lacks a dedicated timing test. Loopback real-socket suites are not public-NAT or multi-host evidence.
