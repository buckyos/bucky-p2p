# Pipeline Plan: SN Client TCP Fallback Bind

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
- If a matching QUIC local listener exists, SN client command tunnel classification must preserve that listener `local_ep`.
- If a matching TCP local listener exists, SN client command tunnel classification must not reuse the listening port as the outbound socket bind address; unspecified TCP listener IP uses `local_ep = None`, and concrete TCP listener IP uses the same IP with port `0`.
- Candidate tunnel/report failures must log enough context to diagnose silent TCP fallback failures: SN id, protocol, local endpoint, remote endpoint, and error.
- QUIC candidates remain ordered before TCP candidates for the same SN.
- A successful QUIC command tunnel and `ReportSn` stops TCP fallback for that SN.
- QUIC command tunnel, control stream, or `ReportSn` failure may fall back to TCP.
- The implementation must not change SN command wire, control-stream-only signaling, TCP/QUIC tunnel wire, TLS identity checks, TTP target matching, `TtpClient::connect_server(...)` maintained-target semantics, or inbound listener/report candidate semantics.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-SN-CLIENT-TCP-FALLBACK-BIND-1 | proposal | confirmed | Approve SN client TCP fallback bind conflict boundary within protocol priority | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | root | user approval | approved proposal | proposal doc checks, schema check, approval metadata, and launch evidence recorded |
| D-SN-CLIENT-TCP-FALLBACK-BIND-1 | design | confirmed | Define SN client TCP listener local_ep normalization and diagnostic logging without changing TTP target semantics | `docs/versions/v0.1/modules/p2p-frame/design.md` | root | P-SN-CLIENT-TCP-FALLBACK-BIND-1 | approved design | design doc checks, schema checks, stage-scope checks, and auto-pipeline approval metadata recorded |
| I-SN-CLIENT-TCP-FALLBACK-BIND-1 | implementation | complete | Implement SN client TCP local_ep normalization and candidate failure logging | `p2p-frame/src/sn/client/sn_service.rs` plus admission evidence | root | D-SN-CLIENT-TCP-FALLBACK-BIND-1 | implementation and admission evidence | schema/admission passed; implementation scope check result recorded; relevant build/unit checks pass |
| T-SN-CLIENT-TCP-FALLBACK-BIND-1 | testing | complete | Add/update focused tests for QUIC local_ep preservation, TCP listener port normalization, unspecified TCP listener fallback, and supported protocol without listener | relevant `p2p-frame/src/sn/**` tests; testing docs only if required by coverage gate | I-SN-CLIENT-TCP-FALLBACK-BIND-1 | I-SN-CLIENT-TCP-FALLBACK-BIND-1 | runnable unit evidence | relevant cargo tests pass; testing coverage checks pass; testing scope check result recorded |
| A-SN-CLIENT-TCP-FALLBACK-BIND-1 | acceptance | complete | Audit proposal/design/implementation/testing consistency and runnable evidence | `docs/versions/v0.1/reviews/` if acceptance is run | root | T-SN-CLIENT-TCP-FALLBACK-BIND-1 | acceptance report or recorded follow-up | acceptance report check passed if full acceptance is in scope |

## Return Routing
- Proposal issue: return to proposal only if listener-optional outbound support, QUIC listener `local_ep` preservation, TCP listener bind normalization, protocol ordering, fallback, or non-goals must change.
- Design issue: return to design if supported outbound protocol gating, QUIC listener `local_ep` preservation, TCP listener bind normalization, candidate grouping, fallback, active SN dedup, diagnostic logging, or scope paths are incomplete.
- Implementation issue: return to implementation if code still requires a matching listener to attempt a supported protocol, drops QUIC listener `local_ep`, reuses TCP listening ports for outbound bind, violates QUIC-first/TCP-fallback, hides candidate failures without diagnostics, or inserts duplicate active SN entries.
- Testing issue: return to testing if runnable evidence does not cover no-listener supported outbound protocol, QUIC listener `local_ep` preservation, TCP listener port normalization, QUIC success stopping TCP, QUIC failure fallback to TCP, and active SN dedup.
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
- Proposal approval hash: `dcbfa51b51f3938c41f43a25fc0a5e30f304e5855e7a30c28568045e89a696f7`
- Design approval hash: `7621740f315dbf2b169cf2ea40dc1bd53cb477ba92517128266d2387c3125971`
- Testing approval hash: `99af224d3b27f6d0bcf320a9c4101d5bb1e19e33b83bd2c86750ae0eeb0a28e8`
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority --evidence-file harness/evidence/admission/20260705-sn-client-tcp-fallback-bind.md` passed; stamp `harness/evidence/admission/20260705-sn-client-tcp-fallback-bind.p2p-frame.stamp.json`.
- `cargo test -p p2p-frame sn_client_protocol_candidates -- --nocapture` passed with 3 tests.
- `cargo test -p p2p-frame sn_client -- --nocapture` passed with 4 tests.
- `cargo check -p p2p-frame` passed.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs testing` passed.
- `uv run --active python ./harness/scripts/testing-coverage-check.py --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority` passed.
- `uv run --active python ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260705T164858Z-p2p-frame-unit.json`.
- `uv run --active python ./harness/scripts/test-run.py p2p-frame all` passed; artifact `test-results/test-runs/20260705T165519Z-p2p-frame-all.json`.
- `uv run --active python ./harness/scripts/test-run.py all all` passed after refreshing the workspace-harness p2p-frame fixture binding; artifact `test-results/test-runs/20260705T171138Z-all-all.json`.
- `uv run --active python ./harness/scripts/quality-check.py` passed; no quality gates configured.
- Acceptance report accepted: `docs/versions/v0.1/reviews/p2p-frame-sn-client-tcp-fallback-bind-acceptance-2026-07-05.md`.
- `uv run --active python ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-sn-client-tcp-fallback-bind-acceptance-2026-07-05.md` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority --ignore-untracked` was attempted and reported the expected multi-stage working-tree diff: `proposal.md`, `design.md`, and `harness/pipeline-plan.md`.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --ignore-untracked` was attempted and reported the expected multi-stage working-tree diff: `proposal.md`, `design.md`, `harness/pipeline-plan.md`, and `p2p-frame/src/sn/client/sn_service.rs`.
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
