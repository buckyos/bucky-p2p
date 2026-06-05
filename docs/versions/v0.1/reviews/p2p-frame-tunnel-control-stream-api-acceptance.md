# p2p-frame tunnel_control_stream_api Acceptance Report

## Metadata
- module: `p2p-frame`
- version: `v0.1`
- change_id: `tunnel_control_stream_api`
- status: `accepted`
- reviewed_at: `2026-06-04T16:26:18+08:00`

## Scope
- Proposal: `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- Design: `docs/versions/v0.1/modules/p2p-frame/design.md`
- Design supplement: `docs/versions/v0.1/modules/p2p-frame/design/tunnel-control-stream-api.md`
- Testing: `docs/versions/v0.1/modules/p2p-frame/testing.md`
- Testing supplement: `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-control-stream-api.md`
- Testplan: `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`

## Admission Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`: passed
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id tunnel_control_stream_api`: passed

## Implementation Evidence
- Public `Tunnel` trait now includes `open_control_stream(...)` and `listen_control_stream(...)`.
- Internal `networks/control_stream.rs` owns private frame/runtime logic and is only exposed as `pub(crate) mod control_stream`.
- TCP, QUIC and PN control commands each include a single `Data` payload command for control stream frames.
- `cargo check -p p2p-frame`: passed
- `cargo test -p p2p-frame control_stream -- --nocapture`: passed, 3 tests executed
- `python3 ./harness/scripts/test-run.py p2p-frame unit`: passed, 110 lib tests executed and doc tests passed with 0 tests
- `cargo test -p p2p-frame --features x509 control_stream -- --nocapture`: passed, 6 tests executed, including TCP/QUIC/PN tunnel control stream round-trip tests
- `cargo check -p p2p-frame --features x509`: passed

## Findings
- No blocking findings.

## Non-Blocking Notes
- No evidence found that `ControlStreamRuntime` or `ControlStreamFrame` is publicly re-exported.
- Static search confirms TCP/QUIC/PN have `Data` control command bodies and handling paths.
- Existing tests no longer call the removed `Executor::init()` / `Executor::init_new_multi_thread(...)` API. The disabled `async_std_executor.rs` implementation still contains its legacy methods and was intentionally left unchanged.

## Result
Acceptance passed for `tunnel_control_stream_api` based on approved proposal/design alignment, admission evidence, implementation review and target unit-test evidence.
