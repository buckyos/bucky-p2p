# 075 SN report authority and stream-failure recovery

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/075-sn-authority-and-recovery/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/075-sn-authority-and-recovery/proposal.md
- Affected paths: p2p-frame/src/sn/service/nat_probe_scheduler.rs, p2p-frame/src/sn/client/sn_service.rs, p2p-frame/src/sn/tests.rs, p2p-frame/tests/unit/sn_tests/service/service/nat_probe_scheduler_tests.rs, p2p-frame/tests/nat_type_aware/sn_profile_flow_tests.rs
- Explicit tier override: none
- Expanded high-risk packet: none / existing task packet

## Approach

- Authority identity (`CHG-nat-probe-authority-observed-path`): the server no longer requires the report to arrive on the exact command stream that registered the peer. `NatProbeScheduler::is_authoritative_report` accepts a report when it arrives either on the establishing stream or on any command stream whose server-observed remote address equals the registered observed address; the check is applied in `observe_capable_report`, `observe_reported_profile` and `observe_control`. The observed-address comparison is the same one `needs_registration` already uses, so an accepted multiplexed report never bumps the registration generation, while a genuinely different path and any non-UDP tunnel stay ignored. `authority_tunnel_id` still records the establishing stream for `reconcile_nat_probe_authority`.
- Rendezvous remedy (`CHG-rendezvous-ambiguous-sn-failure`): `SNClientService::rendezvous_via_sn` maps SN command QA failures back to `P2pErrorCode::IoError` (the pre-073 code did the same on the pinned stream), so `TunnelManager`'s `is_ambiguous_rendezvous_failure` branch still runs the local punch/wait action with the same `tunnel_id` and waiter instead of aborting. The ambiguity set and its deterministic-error test are unchanged.
- ActiveSN eviction (`CHG-active-sn-eviction-scope`): `send_sn_qa` was split into `acquire_sn_send` (never evicts) plus `send_sn_qa_on`. `SNClientService::report` records a command-stream acquisition failure per SN peer id and treats only the second consecutive failure as whole-SN unavailability; the first failure only marks the ActiveSN due for an early retry on the next ping iteration. A request that obtained a stream but failed to be sent or answered (`send_sn_qa_on`) never evicts the registration, preserving the 073 P-005 boundary.

## Risk Screen

- Public contract, protocol, or CLI change: yes — `p2p-frame` adds the `SnCmdSendGuard` type alias and one field (`report_stream_failures`) to the public `SNServiceState`; no SN wire field, command version, signature, or CLI changes, and `cargo check --workspace` compiles every workspace consumer. This is a source-additive change inside the confirmed scope, recorded as residual risk instead of a tier change.
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no — authority eligibility is still limited to authenticated UDP tunnels; the change only accepts additional command streams that share the peer's already-registered observed address, and a different observed path or a TCP tunnel is still ignored.
- Concurrency, lifecycle, or runtime integration change: yes — SN report stream selection, the rendezvous ambiguous-failure path and ActiveSN eviction timing changed. Evidence: the scheduler received additional concurrent-stream coverage, and the SN client suite includes single-failure tolerance, send-failure tolerance, and repeated-failure eviction/re-registration cases.
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no — SN client and SN service stay inside `p2p-frame`; TTP and `sfo-cmd-server` are unchanged.

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib` (515 passed, including `nat_probe_scheduler_accepts_multiplexed_stream_on_registered_observed_path`, `sn_report_send_failure_does_not_evict_healthy_active_sn`, `sn_rendezvous_command_failure_keeps_ambiguous_error_code`, `unreachable_sn_command_stream_evicts_stale_active_sn_and_recovers`), `cargo test -p p2p-frame --features x509` (515 lib + all integration targets passed, including the 3 `nat_probe_logging_contract` cases), `cargo test -p p2p-frame --features "x509 test-real-socket-matrix" --test real_p2p_tunnel_flow` (7 passed), `cargo check --workspace` (clean).
- Result: passed
- Residual risk or follow-up: the eviction policy is a bounded approximation — a command-stream creation failure that persists across two consecutive reports still evicts a registration whose other command streams are healthy, because `sfo-cmd-server` 0.4 exposes no read-only "is a matching stream alive?" query. A dead SN registration is now detected one report later than before (the first failure only schedules an early retry). The proposal listed `test-run.py p2p-frame/075-sn-authority-and-recovery all` as evidence; that entry point requires a high-risk `testplan.yaml` task registration which standard tier does not create, so the same task-scoped cargo commands above were run directly; the approved success criteria are unchanged. No public/deployed multi-SN or NAT environment verification was performed.
