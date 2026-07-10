# p2p-frame SN Client TCP Fallback Bind Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | `test-results/test-runs/20260705T164858Z-p2p-frame-unit.json`; `test-results/test-runs/20260705T165519Z-p2p-frame-all.json`; `test-results/test-runs/20260705T171138Z-all-all.json`; focused `cargo test` and `cargo check` commands listed below | no blocking finding recorded | none |

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- change_id values reviewed: `sn_client_protocol_priority`
- Review date: 2026-07-05
- In scope: approved proposal/design/testing/testplan updates for SN client TCP fallback bind behavior; implementation in `p2p-frame/src/sn/client/sn_service.rs`; admission evidence; focused and unified unit evidence.
- Out of scope: unrelated pre-existing untracked review/evidence files, SN command wire changes, TTP target semantic changes, and unrelated p2p-frame change_ids.

## Optional Diff / Status Evidence
- `git diff --stat` summary: 6 tracked files changed, 112 insertions, 49 deletions.
- `git diff --name-status` summary: modified p2p-frame proposal/design/testing/testplan, pipeline plan, and SN client service.
- `git diff --check` result: passed with no whitespace errors.
- Note: diff/status output is discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| Supported outbound protocol without local listener | `proposal.md` row `P-SN-CLIENT-PROTOCOL-PRIORITY-1`; `design.md` row `sn_client_protocol_priority` | `sn_client_protocol_candidates(...)` still adds `local_ep = None` candidates from `NetManager::protocols()` when no matching listener exists | `sn_client_protocol_candidates_include_supported_protocol_without_listener`; artifact `20260705T164858Z-p2p-frame-unit.json` | implemented |
| Preserve QUIC listener `local_ep` | `proposal.md`; `design.md` TCP/QUIC local endpoint boundary | `sn_client_local_ep_for_protocol(...)` returns `Some(local_ep)` for non-TCP protocols, preserving QUIC listener port | `sn_client_protocol_candidates_preserve_quic_listener_local_ep`; artifact `20260705T164858Z-p2p-frame-unit.json` | implemented |
| Do not reuse TCP listener port for outbound bind | `proposal.md`; `design.md` TCP listener normalization boundary | TCP listener candidates with unspecified IP become `None`; concrete TCP listener IP keeps the IP but rewrites port to `0` | `sn_client_protocol_candidates_do_not_bind_unspecified_tcp_listener_port`; concrete-IP case covered by `sn_client_protocol_candidates_preserve_quic_listener_local_ep`; artifact `20260705T164858Z-p2p-frame-unit.json` | implemented |
| Candidate failure diagnostics | `design.md` row `sn_client_protocol_priority` | `ping_proc` logs tunnel and report candidate failures with SN id, protocol, local endpoint, remote endpoint, and error | code review plus `cargo check -p p2p-frame` | implemented |
| No SN/TTP/tunnel wire or maintained-target semantic change | proposal non-goals; design rollback and scope notes | changes are contained to SN client candidate generation and ping/report failure logging | `cargo check -p p2p-frame`; p2p-frame unit artifact | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `sn_client_protocol_priority` listener-optional outbound support | normal, boundary, compatibility | `testing.md` maps `V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT` to supported protocol without listener | `uv run --active python ./harness/scripts/test-run.py p2p-frame unit` | adequate |
| QUIC listener preservation | compatibility, lifecycle | `testing.md` and `testplan.yaml` require QUIC listener `local_ep` preservation | focused `cargo test -p p2p-frame sn_client -- --nocapture`; unified unit artifact | adequate |
| TCP listener port conflict | boundary, negative, error | `testing.md` and `testplan.yaml` require TCP listener local_ep normalization to `None` or port `0` | focused `cargo test -p p2p-frame sn_client_protocol_candidates -- --nocapture`; unified unit artifact | adequate |
| Candidate failure visibility | error | `testing.md` requires code review of candidate failure logs | code review of `ping_proc` logging | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| A-SN-CLIENT-TCP-001 | proposal/design | SN client can attempt a supported SN endpoint protocol even when no local listener is bound for that protocol | helper test and code path using `NetManager::protocols()` | pass |
| A-SN-CLIENT-TCP-002 | proposal/design | QUIC candidates preserve listener `local_ep` | helper test and candidate helper review | pass |
| A-SN-CLIENT-TCP-003 | proposal/design | TCP candidates do not reuse a listener port for outbound bind | helper tests for unspecified IP and concrete IP port `0` normalization | pass |
| A-SN-CLIENT-TCP-004 | design | Candidate tunnel/report failures log SN id, protocol, local endpoint, remote endpoint, and error | `ping_proc` code review | pass |
| A-SN-CLIENT-TCP-005 | proposal non-goals | SN command wire, tunnel wire, TLS identity, TTP target matching, and maintained-target semantics remain unchanged | diff review and `cargo check -p p2p-frame` | pass |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `p2p-frame/src/sn/client/sn_service.rs`
- `harness/evidence/admission/20260705-sn-client-tcp-fallback-bind.md`
- `harness/evidence/admission/20260705-sn-client-tcp-fallback-bind.p2p-frame.stamp.json`
- `test-results/test-runs/20260705T164858Z-p2p-frame-unit.json`
- `test-results/test-runs/20260705T165519Z-p2p-frame-all.json`
- `test-results/test-runs/20260705T171138Z-all-all.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Review approved requirements and acceptance boundaries.
2. Review design against proposal.
3. Generate acceptance rules and expected results from proposal, design, implementation, and test implementation.
4. Review implementation against proposal and design.
5. Review test design coverage in `testing.md` and `testplan.yaml`.
6. Review command evidence and generated test artifact.
7. Produce conclusion.

## Consistency Summary
- Proposal authority check: approved proposal row `P-SN-CLIENT-PROTOCOL-PRIORITY-1` requires QUIC-first/TCP-fallback, listener-optional outbound candidates, QUIC listener `local_ep` preservation, and TCP fallback ephemeral/no-local endpoint behavior.
- Proposal vs design: design directly maps TCP listener normalization and candidate failure diagnostics to `sn_client_protocol_priority`.
- Design vs testing: testing metadata and `testplan.yaml` now distinguish QUIC preservation from TCP normalization.
- Design vs implementation: `sn_client_local_ep_for_protocol(...)` implements the documented normalization, and `ping_proc` logs candidate failures before continuing fallback.
- Test implementation vs results: focused `sn_client` tests pass, and the unified p2p-frame unit runner passes with artifact `test-results/test-runs/20260705T164858Z-p2p-frame-unit.json`.
- Test design adequacy: the changed candidate helper branches are covered by unit tests for supported protocol without listener, QUIC listener preservation, concrete TCP listener port `0`, and unspecified TCP listener `None`; broader module/project runners also pass.
- change_id traceability: proposal, design, testing, testplan, admission evidence, pipeline plan, and acceptance report all cite `sn_client_protocol_priority`.
- Document logic review: no contradiction remains between QUIC listener same-origin preservation and TCP listener outbound bind normalization.
- Implementation logic review: normalization is isolated to SN client candidate construction and does not alter TTP target matching or any SN/TCP/QUIC wire type.
- Public API / codec / runtime semantics review: no public wire type, codec marker, TLS identity path, TTP maintained-target path, or inbound listener/report candidate path was modified.
- Document approval timing: `schema-check.py` passed after proposal/design/testing approval metadata was refreshed.
- Implementation scope: `admission-check.py` passed for `sn_client_protocol_priority`; `stage-scope-check.py --stage implementation` was attempted and reported the expected multi-stage working-tree document diffs from this same auto-pipeline iteration.
- Bugfix red-green evidence: no pre-fix failing run artifact was retained; the concrete reproduction cause was code inspection of TCP outbound bind using the already-listening local port, and regression coverage now asserts TCP listener candidates normalize to `None` or port `0`.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`: passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority --evidence-file harness/evidence/admission/20260705-sn-client-tcp-fallback-bind.md`: passed.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs testing`: passed.
- `uv run --active python ./harness/scripts/testing-coverage-check.py --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority`: passed.
- `cargo test -p p2p-frame sn_client_protocol_candidates -- --nocapture`: passed, 3 tests.
- `cargo test -p p2p-frame sn_client -- --nocapture`: passed, 4 tests.
- `cargo check -p p2p-frame`: passed.
- `uv run --active python ./harness/scripts/test-run.py p2p-frame unit`: passed; artifact `test-results/test-runs/20260705T164858Z-p2p-frame-unit.json`.
- `uv run --active python ./harness/scripts/test-run.py p2p-frame all`: passed; artifact `test-results/test-runs/20260705T165519Z-p2p-frame-all.json`.
- `test-run.py <module> all`: passed via `uv run --active python ./harness/scripts/test-run.py p2p-frame all`; artifact `test-results/test-runs/20260705T165519Z-p2p-frame-all.json`.
- `uv run --active python ./harness/scripts/quality-check.py`: passed; `harness/quality-gates.yaml` declares `gates: []`, so no quality artifact is required.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority --ignore-untracked`: attempted; reported multi-stage working-tree diffs in proposal/design/pipeline docs.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --ignore-untracked`: attempted; reported multi-stage working-tree diffs in proposal/design/pipeline docs and implementation source.
- `uv run --active python ./harness/scripts/test-run.py all all`: passed; artifact `test-results/test-runs/20260705T171138Z-all-all.json`.
- `test-run.py all all`: passed via `uv run --active python ./harness/scripts/test-run.py all all`; artifact `test-results/test-runs/20260705T171138Z-all-all.json`.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: approved proposal/design/testing coverage, admitted implementation, focused helper tests, unified p2p-frame unit test artifact, cargo check, and quality check are consistent with the SN client TCP fallback bind fix.
- Supporting test evidence: `test-results/test-runs/20260705T164858Z-p2p-frame-unit.json`; `test-results/test-runs/20260705T165519Z-p2p-frame-all.json`; `test-results/test-runs/20260705T171138Z-all-all.json`.
- Residual risk: there is no retained pre-fix failing artifact and no live multi-process SN integration run in this turn; the root cause and regression are covered at the candidate-generation level that caused TCP fallback to fail locally.

## Follow-Up Tasks
- Requirement task: none.
- Design task: none.
- Implementation task: none.
- Testing task: optional future multi-process SN client scenario that forces QUIC timeout and observes TCP command tunnel success against a reachable SN TCP endpoint.
- Testing return reason if coverage is incomplete: no blocking return; optional integration would reduce residual runtime-path risk.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
