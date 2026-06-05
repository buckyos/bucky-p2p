# p2p-frame SN Control Stream Signaling Acceptance

## Metadata
- version: `v0.1`
- module: `p2p-frame`
- change_id: `sn_control_stream_signaling`
- status: passed
- reviewed_at: `2026-06-05T17:05:04+08:00`

## Scope
- Proposal: `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- Design: `docs/versions/v0.1/modules/p2p-frame/design.md`
- Design supplement: `docs/versions/v0.1/modules/p2p-frame/design/sn-control-stream-signaling.md`
- Testing: `docs/versions/v0.1/modules/p2p-frame/testing.md`
- Test plan: `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- Implementation:
  - `p2p-frame/src/sn/types.rs`
  - `p2p-frame/src/sn/client/sn_service.rs`
  - `p2p-frame/src/sn/service/service.rs`

## Admission
- `schema-check.py --version v0.1 --module p2p-frame`: passed
- `admission-check.py --version v0.1 --module p2p-frame --change-id sn_control_stream_signaling`: passed
- Proposal and design both contain direct `sn_control_stream_signaling` mappings.

## Findings
- No blocking findings.

## Consistency Audit
- Proposal requires SN report/call/called/response or equivalent low-frequency messages to use `Tunnel` control stream and forbids ordinary stream fallback for SN low-frequency signaling.
- Design maps the behavior to a fixed SN purpose through public `Tunnel::open_control_stream(...)` / `listen_control_stream(...)`; bootstrap, control-unavailable, remote-unlistened, or old-version cases fail through the current SN command error model rather than falling back to the existing ordinary stream path.
- Implementation matches that design:
  - `SnClientTunnelFactory::open_cmd_tunnel(...)` calls `TtpClient::open_control_stream(...)` and returns an error on failure; it does not call ordinary `open_stream(...)`.
  - `SnServer::start_cmd_accept_loop(...)` registers `listen_control_stream(...)` for the SN command purpose; it does not register ordinary `listen_stream(...)` for SN commands.
  - Control stream ingress is wrapped by the existing `SnServer::into_cmd_tunnel(...)`, preserving existing `sfo_cmd_server` command handlers and SN command semantics.
  - `sn_cmd_purpose()` centralizes the SN purpose encoding and does not expose internal `control_stream` runtime/frame details.
- Testing artifacts map `sn_control_stream_signaling` to unit and integration coverage.

## Evidence
- `cargo check -p p2p-frame`: passed.
- `cargo test -p p2p-frame sn_server_wraps_sn_control_stream_into_cmd_tunnel -- --nocapture`: passed.
- `python3 ./harness/scripts/test-run.py p2p-frame unit`: passed, 115 lib tests and 0 doc tests.
- `python3 ./harness/scripts/test-run.py p2p-frame integration`: passed, workspace compatibility tests completed. Observed non-blocking existing warning: `sn-miner-rust/src/main.rs` unused import `cyfs_p2p::executor::Executor`.
- Code review: no SN client ordinary stream fallback remains; no SN service ordinary stream listener registration remains for the SN command purpose.

## Residual Risk
- Mixed-version peers or peers that do not listen on the SN control purpose will now fail SN command tunnel creation/send instead of using ordinary stream compatibility. This is consistent with the tightened proposal but should be considered an operational rollout requirement.

## Result
- Acceptance passed for `sn_control_stream_signaling`.
