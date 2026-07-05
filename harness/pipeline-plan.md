# Pipeline Plan: SN Client Protocol Priority Listener Optional

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "确认，自动处理后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): p2p-frame
- change_id values: sn_client_protocol_priority

## Acceptance Baseline
- Final acceptance uses the approved `p2p-frame` proposal as authority.
- SN client must try SN endpoints for protocols supported for outbound command tunnel creation even when no matching local listener is bound.
- If a matching local listener exists, SN client command tunnel classification must preserve that listener `local_ep`.
- QUIC candidates remain ordered before TCP candidates for the same SN.
- A successful QUIC command tunnel and `ReportSn` stops TCP fallback for that SN.
- QUIC command tunnel, control stream, or `ReportSn` failure may fall back to TCP.
- The implementation must not change SN command wire, control-stream-only signaling, TCP/QUIC tunnel wire, TLS identity checks, TTP target matching, `TtpClient::connect_server(...)` maintained-target semantics, or inbound listener/report candidate semantics.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-SN-CLIENT-PROTOCOL-PRIORITY-1 | proposal | confirmed | Approve SN client protocol priority and listener-optional outbound connection boundary | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | root | user approval | approved proposal | proposal doc checks, schema check, approval metadata, and launch evidence recorded |
| D-SN-CLIENT-PROTOCOL-PRIORITY-1 | design | confirmed | Define SN client supported outbound protocol gating, listener `local_ep` preservation, candidate ordering, fallback, active SN dedup, and implementation scope paths | `docs/versions/v0.1/modules/p2p-frame/design.md` | root | P-SN-CLIENT-PROTOCOL-PRIORITY-1 | approved design | design doc checks, schema checks, stage-scope checks, and auto-pipeline approval metadata recorded |
| I-SN-CLIENT-PROTOCOL-PRIORITY-1 | implementation | complete | Implement minimal production changes for admitted SN client listener-optional protocol priority behavior | `p2p-frame/src/sn/client/sn_service.rs` plus admission evidence | root | D-SN-CLIENT-PROTOCOL-PRIORITY-1 | implementation and admission evidence | schema/admission passed; implementation scope check passed for `sn_client_protocol_priority`; relevant build/unit checks pass |
| T-SN-CLIENT-PROTOCOL-PRIORITY-1 | testing | confirmed | Add post-implementation tests and testing metadata for supported protocol without listener, listener `local_ep` preservation, QUIC-first ordering, fallback, and active SN dedup | `docs/versions/v0.1/modules/p2p-frame/testing.md`; `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`; relevant `p2p-frame/src/sn/**` tests; runner wiring if needed | I-SN-CLIENT-PROTOCOL-PRIORITY-1 | I-SN-CLIENT-PROTOCOL-PRIORITY-1 | runnable unit evidence | testing doc/coverage checks and relevant `test-run.py` levels pass; testing scope check passed or recorded with isolated diff evidence |
| A-SN-CLIENT-PROTOCOL-PRIORITY-1 | acceptance | complete | Audit proposal/design/implementation/testing consistency and runnable evidence | `docs/versions/v0.1/reviews/` | root | T-SN-CLIENT-PROTOCOL-PRIORITY-1 | acceptance report | acceptance report check passed and conclusion is accepted |

## Return Routing
- Proposal issue: return to proposal only if listener-optional outbound support, listener `local_ep` preservation, protocol ordering, fallback, or non-goals must change.
- Design issue: return to design if supported outbound protocol gating, listener `local_ep` preservation, candidate grouping, fallback, active SN dedup, or scope paths are incomplete.
- Implementation issue: return to implementation if code still requires a matching listener to attempt a supported protocol, drops listener `local_ep` when present, violates QUIC-first/TCP-fallback, or inserts duplicate active SN entries.
- Testing issue: return to testing if runnable evidence does not cover no-listener supported outbound protocol, listener `local_ep` preservation, QUIC success stopping TCP, QUIC failure fallback to TCP, and active SN dedup.
- Acceptance issue: return to the owning stage named by the finding.

## Exit Condition
- [x] proposal approved
- [x] design approved
- [x] implementation admission passed
- [x] implementation completed
- [x] testing completed
- [x] acceptance completed

## Evidence
- User approval and launch statement: `确认，自动处理后续步骤`
- Proposal approval hash: `a3fe146b3f92f0345a26e786c58ef614de62488d95123414ed0d2c72294cfbb2`
- Design approval hash: `434b780b64b00d713296031069cd20a6ae2c58b2d90535f06f63b68856fa8a4b`
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority --evidence-file harness/evidence/admission/20260705-sn-client-listener-optional.md` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority --ignore-untracked` passed with proposal/design/plan temporarily hidden from git status to isolate the implementation child task.
- `cargo test -p p2p-frame sn_client_listener_entries_are_quic_first_then_tcp -- --nocapture` passed.
- `cargo test -p p2p-frame sn_client_protocol_candidates -- --nocapture` passed.
- Testing approval hash: `9c642ff2b533e11e4eaf5d0a98d1372612bcc455e88b3f9ad33ebdbe42c3216d`
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs testing` passed.
- `uv run --active python ./harness/scripts/testing-coverage-check.py --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority` passed.
- `python3 ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260705T030802Z-p2p-frame-unit.json`.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --ignore-untracked` passed for testing docs/testplan with proposal/design/plan and `sn_service.rs` temporarily hidden from git status. The `sn_service.rs` test additions live in a `#[cfg(test)]` module inside an implementation-scoped source file; the path-only scope checker cannot distinguish that content from production code.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive --evidence-file harness/evidence/admission/20260704-test-run-p2p-frame-admission.md` passed after refreshing the workspace-harness fixture document binding to the current p2p-frame proposal/design hashes.
- `uv run --active python ./harness/scripts/test-run.py p2p-frame all` passed; artifact `test-results/test-runs/20260705T032919Z-p2p-frame-all.json`.
- `uv run --active python ./harness/scripts/test-run.py all all` passed; artifact `test-results/test-runs/20260705T032628Z-all-all.json`.
- `uv run --active python ./harness/scripts/quality-check.py` passed; `harness/quality-gates.yaml` declares `gates: []`, so no quality artifact is required.
- Acceptance report accepted: `docs/versions/v0.1/reviews/p2p-frame-sn-client-listener-optional-acceptance-2026-07-05.md`.
- `uv run --active python ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-sn-client-listener-optional-acceptance-2026-07-05.md` passed.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
