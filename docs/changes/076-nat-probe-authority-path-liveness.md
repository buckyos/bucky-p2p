# 076 NAT probe authority liveness by observed path

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/076-nat-probe-authority-path-liveness/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/076-nat-probe-authority-path-liveness/proposal.md
- Affected paths: p2p-frame/src/sn/service/service.rs, p2p-frame/src/sn/service/nat_probe_scheduler.rs, p2p-frame/src/sn/mod.rs, p2p-frame/src/sn/tests.rs, p2p-frame/tests/unit/sn_tests/service/service/nat_probe_scheduler_tests.rs
- Explicit tier override: none
- Expanded high-risk packet: none / existing task packet

## Approach

- `CHG-authority-liveness-observed-path`: `NatProbeScheduler::same_observed_path` is the single identity rule (equal transport protocol and equal observed address) now shared by `is_authoritative_report` (report acceptance) and `reconcile_nat_probe_authority` (liveness). The scheduler also exposes `authority_observed_endpoint`, read under the same lock as `authority_registration`. `reconcile_nat_probe_authority` keeps the authority alive when any accepted command stream of that authenticated peer matches either the establishing `authority_tunnel_id` or the registered observed path; the `(authority_tunnel_id, registration_generation)` snapshot guard in `finish_nat_probe_authority_reconcile` is unchanged. Because the gate and the liveness check now share one predicate, the reported "accept on lane B but delete because lane A closed" inconsistency cannot recur within this pair of checks.
- Report acceptance is tightened from address-only to protocol + address. The client keeps one `ActiveSN.sn_endpoint` (protocol included) per SN, so multiplexed streams on the same bearer still match; a same-address/different-protocol stream is now treated as a different path, which also prevents a TCP stream from keeping a dead UDP authority alive.
- `CHG-authority-path-liveness-regression-tests`: `nat_probe_scheduler_observed_path_identity_requires_protocol_and_address` pins the predicate, `nat_probe_scheduler_accepts_multiplexed_stream_on_registered_observed_path` also asserts a same-address `Protocol::Ext(1)` stream is ignored, and `nat_probe_authority_liveness_keeps_registration_while_a_same_path_stream_is_alive` drives the real SN service with a real authenticated command stream, injects the reported state (registered authority command stream no longer accepted while a same-path stream is alive), runs `maintain_nat_probe_state`, and asserts the registration and the published profile survive; the same test then re-registers on a path with no accepted stream and asserts the registration is recycled.
- Test scaffolding: `p2p-frame/src/sn/tests.rs::setup_sn_and_two_clients` became `pub(crate)` and `p2p-frame/src/sn/mod.rs` declares `pub(crate) mod tests` so the service-module test can build a real client while keeping access to the SN service's scheduler. Both are `#[cfg(test)]`/test-only changes; no production API or visibility changed.

## Risk Screen

- Public contract, protocol, or CLI change: no — no SN wire field, command version, signature, certificate, or CLI change; the new predicate/accessor live on the crate-internal `NatProbeScheduler`, and the test-module visibility tweaks are `cfg(test)` only.
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no — authority eligibility still requires an authenticated peer and a UDP command stream on the peer's own registered observed path; the protocol comparison became stricter, not looser.
- Concurrency, lifecycle, or runtime integration change: yes — the reconcile liveness rule and its new per-tunnel endpoint read inside `reconcile_nat_probe_authority` change authority retention timing. Evidence: the new service-level regression test plus the red run recorded under Verification (reverting the presence check makes the test fail on the "registration must survive" assertion).
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no — server-side only; the client, TTP, and `sfo-cmd-server` are untouched.

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib` (517 passed, including the two new cases), `cargo test -p p2p-frame --features x509` (all targets; one run failed only on the pre-existing `tcp_only_registration_never_receives_or_executes_probe` `AddrInUse` port collision at `127.0.0.1:42965`, which passes in isolation and in the following full run), `cargo check --workspace` (clean). Red-green: with the liveness check reverted to the tunnel-id-only form, `nat_probe_authority_liveness_keeps_registration_while_a_same_path_stream_is_alive` fails with `left: None, right: Some(TunnelId(29969))` at the same-path survival assertion.
- Result: passed
- Residual risk or follow-up: the reported end-to-end state ("the authority command stream is closed while a same-path stream survives") cannot be produced through the public client API, because the command client never signals a per-stream close (`ClassifiedCmdSend::drop` only disables the worker and the control stream is only finished by an explicit `shutdown`/FIN), so the server keeps accepting a client-closed stream until its bearer closes. The regression test therefore injects the missing authority identity while keeping a real accepted stream on the registered path; the resulting state is exactly the reported one. `reconcile_nat_probe_authority` now reads the observed endpoint of each candidate stream, which can wait for an in-flight server-side QA on that stream; the scan is bounded by the peer's accepted command stream count. No public/deployed multi-SN or NAT environment verification was performed.
