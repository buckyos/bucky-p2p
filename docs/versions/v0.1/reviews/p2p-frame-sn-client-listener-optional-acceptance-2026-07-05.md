# p2p-frame SN Client Listener Optional Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | `test-results/test-runs/20260705T032628Z-all-all.json`; `test-results/test-runs/20260705T032919Z-p2p-frame-all.json` | no blocking finding recorded | none |

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- change_id values reviewed: `sn_client_protocol_priority`
- Review date: 2026-07-05
- In scope: approved proposal/design/testing updates for listener-optional SN client outbound protocol selection; implementation in `p2p-frame/src/sn/client/sn_service.rs`; admission evidence; module/all test artifacts.
- Out of scope: unrelated pre-existing untracked review/evidence files, SN wire format changes, TTP target semantic changes, and unrelated p2p-frame change_ids.

## Optional Diff / Status Evidence
- `git status --short` summary: tracked changes in p2p-frame proposal/design/testing/testplan, `harness/pipeline-plan.md`, and `p2p-frame/src/sn/client/sn_service.rs`; pre-existing untracked review/evidence files remain present.
- `git diff --stat` summary: 6 tracked files changed, 162 insertions, 100 deletions.
- `git diff --name-status` summary: modified p2p-frame proposal/design/testing/testplan, pipeline plan, and SN client service.
- `git diff --check` result: passed with no whitespace errors.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| Supported outbound protocol without local listener | `proposal.md` row `P-SN-CLIENT-PROTOCOL-PRIORITY-1`; `design.md` row `sn_client_protocol_priority` | `sn_client_protocol_candidates(...)` adds `local_ep = None` candidates from `NetManager::protocols()` when no matching listener exists | `sn_client_protocol_candidates_include_supported_protocol_without_listener`; artifacts `20260705T030802Z-p2p-frame-unit.json`, `20260705T032919Z-p2p-frame-all.json`, `20260705T032628Z-all-all.json` | implemented |
| Preserve listener `local_ep` when matching listener exists | `proposal.md` row `P-SN-CLIENT-PROTOCOL-PRIORITY-1`; `design.md` row `sn_client_protocol_priority` | candidate list stores `Some(listener.local)` and passes it to `open_cmd_tunnel` and `SnTunnelClassification::new(...)` | `sn_client_protocol_candidates_preserve_listener_local_ep`; same module/all artifacts | implemented |
| QUIC-first and TCP fallback candidate order | `proposal.md` question and row for `sn_client_protocol_priority`; `design.md` helper/order text | `sn_client_protocol_priority(...)` orders QUIC before TCP and is used by listener and protocol candidate helpers | `sn_client_listener_entries_are_quic_first_then_tcp`; `cargo test -p p2p-frame sn_client`; module/all artifacts | implemented |
| Stop after successful report and dedup active SN | `design.md` row `sn_client_protocol_priority` | `ping_proc` breaks the SN candidate loop after successful `ReportSn` and checks existing `active_sn` by `peer_id` before insert | code review plus `p2p-frame all` and `all all` artifacts | implemented |
| No SN/TTP/tunnel wire or maintained-target semantic change | `proposal.md` non-goals; `design.md` rollback and scope notes | changes are contained to SN client candidate collection/classification and do not edit wire structs or TTP target code | workspace tests and all-all artifact pass; code review found no wire-scope edits | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `sn_client_protocol_priority` listener-optional outbound support | normal, boundary, compatibility | `testing.md` maps `V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT` to supported protocol without listener and listener preservation | `python3 ./harness/scripts/test-run.py p2p-frame unit`; `uv run --active python ./harness/scripts/test-run.py p2p-frame all` | adequate |
| Protocol ordering and fallback safety | normal, error, lifecycle | `testing.md` requires QUIC/TCP/Ext ordering and code review of fallback loop behavior | `cargo test -p p2p-frame sn_client`; `p2p-frame all`; `all all` | adequate |
| Active SN duplication risk | lifecycle, cross-module | `testing.md` requires code review that active SN insertion dedups by `sn_peer_id` | code review of `ping_proc`; workspace integration through `all all` | adequate |
| Regression risk to existing SN/TTP/PN behavior | compatibility, cross-module | `testplan.yaml` keeps p2p-frame workspace and targeted SN/TTP/PN commands in module/all and all/all plans | `test-results/test-runs/20260705T032628Z-all-all.json` covers workspace, SN distributed directory, TTP, PN, and sn-miner related commands | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| A-SN-CLIENT-001 | proposal/design | SN client can attempt a supported SN endpoint protocol even when no local listener is bound for that protocol | helper test and code path using `NetManager::protocols()` | pass |
| A-SN-CLIENT-002 | proposal/design | When a matching listener exists, command tunnel classification uses the listener `local_ep` without dropping or rewriting it | helper test and call-site review | pass |
| A-SN-CLIENT-003 | proposal/design | QUIC candidates are attempted before TCP fallback for the same SN | helper ordering test and module/all artifacts | pass |
| A-SN-CLIENT-004 | proposal/design/code | Successful QUIC report stops later TCP attempts and active SN entries are deduplicated by `sn_peer_id` | `ping_proc` review and workspace artifacts | pass |
| A-SN-CLIENT-005 | proposal non-goals | SN command wire, tunnel wire, TLS identity, TTP target matching, and maintained-target semantics remain unchanged | diff review and `all all` artifact | pass |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `p2p-frame/src/sn/client/sn_service.rs`
- `harness/evidence/admission/20260705-sn-client-listener-optional.md`
- `harness/evidence/admission/20260704-test-run-p2p-frame-admission.md`
- `test-results/test-runs/20260705T030802Z-p2p-frame-unit.json`
- `test-results/test-runs/20260705T032919Z-p2p-frame-all.json`
- `test-results/test-runs/20260705T032628Z-all-all.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Review approved requirements and acceptance boundaries
2. Review design against proposal
3. Generate or finalize acceptance rules and expected results from proposal, design, implementation, and test implementation
4. Review implementation against proposal and design
5. Review whether test design reasonably covers proposal/design/code behavior
6. Review tests and results against testing metadata and `testplan.yaml`
7. Review document and implementation logic for contradictions, invalid assumptions, impossible states, and correctness defects
8. Use diff/status output only when helpful to locate evidence
9. Produce conclusion

## Consistency Summary
- Proposal authority check: approved proposal row `P-SN-CLIENT-PROTOCOL-PRIORITY-1` explicitly requires supported outbound protocol gating without local listener gating and listener `local_ep` preservation when present.
- Proposal vs design: design directly maps `sn_client_protocol_priority` to SN client candidate ordering, local endpoint preservation, `local_ep = None` without listener, fallback, and active SN dedup.
- Design vs testing implementation: testing metadata and testplan include `V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT` plus `sn_client` runner coverage.
- Design vs long-lived boundary doc: no crate boundary or public SN/TTP wire boundary change was required.
- Design vs implementation: `sn_client_protocol_candidates(...)` and `ping_proc` implement the documented candidate model without changing wire structs or TTP target code.
- Test implementation vs test code vs results: unit helper tests are present in `sn_service.rs` and pass in unit, module all, and whole-project all artifacts.
- Test design adequacy: direct helper tests cover local listener absence, local listener preservation, and ordering; fallback/dedup behavior is covered by code review plus existing workspace integration.
- change_id traceability: proposal, design, testing, testplan, admission evidence, and pipeline plan all cite `sn_client_protocol_priority`.
- Acceptance criteria traceability: each proposal acceptance point maps to implementation evidence and at least helper or runner evidence above.
- Cross-module admission: `all all` passed after refreshing the workspace-harness p2p-frame admission fixture document binding to the current proposal/design hashes and regenerating its stamp with admission-check.
- Public API / codec / runtime semantics review: no public wire type, codec marker, TLS identity path, or TTP maintained-target path was modified.
- Document logic review: no contradiction found between the listener-optional behavior and the non-goals; supported protocol remains required through `NetManager::protocols()`.
- Implementation logic review: candidate generation uses supported protocol gating; listener endpoints are preserved; no fake local endpoint is fabricated for missing listeners.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed after proposal/design/testing auto-pipeline approval metadata was written.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): implementation scope check passed for `sn_client_protocol_priority`.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: focused helper tests were added for the previously missing no-listener and listener-preservation cases; no pre-change failing run was retained.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`: passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_client_protocol_priority --evidence-file harness/evidence/admission/20260705-sn-client-listener-optional.md`: passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage <stage>`: proposal/design/implementation/testing scope checks passed; implementation used `--change-id sn_client_protocol_priority`; testing scope was isolated for docs/testplan because helper tests live in an implementation-scoped source file.
- Unit test command through `harness/scripts/test-run.py`: `python3 ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260705T030802Z-p2p-frame-unit.json`.
- DV test command through `harness/scripts/test-run.py`: p2p-frame has no configured DV tests in this runner; `p2p-frame all` records `test-run: no dv tests for p2p-frame`.
- Integration test command through `harness/scripts/test-run.py`: integration steps are included and passed in `test-results/test-runs/20260705T032919Z-p2p-frame-all.json`.
- Module all command through `harness/scripts/test-run.py <module> all`: `uv run --active python ./harness/scripts/test-run.py p2p-frame all` passed; artifact `test-results/test-runs/20260705T032919Z-p2p-frame-all.json`.
- Project all command through `harness/scripts/test-run.py all all`: `uv run --active python ./harness/scripts/test-run.py all all` passed; artifact `test-results/test-runs/20260705T032628Z-all-all.json`.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): `test-results/test-runs/20260705T032628Z-all-all.json`.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed; `harness/quality-gates.yaml` declares `gates: []`, so no quality artifact is required.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not applicable because no quality gates are configured.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not rerun; canonical `test-run.py p2p-frame all` and `test-run.py all all` both passed.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: passed for `docs/versions/v0.1/reviews/p2p-frame-sn-client-listener-optional-acceptance-2026-07-05.md`.
- Targeted migration search, when applicable: not applicable; this change does not rename public API or codec values.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: approved proposal/design/testing, admitted implementation, focused helper tests, module all, whole-project all, and quality checks are consistent with the listener-optional SN client requirement.
- Supporting test evidence: `test-results/test-runs/20260705T030802Z-p2p-frame-unit.json`; `test-results/test-runs/20260705T032919Z-p2p-frame-all.json`; `test-results/test-runs/20260705T032628Z-all-all.json`.
- Residual risk: fallback/report failure paths are validated by code review and existing workspace coverage rather than a new multi-process network integration scenario.

## Follow-Up Tasks
- Requirement task: none.
- Design task: none.
- Implementation task: none.
- Testing task: optional future multi-process SN client scenario for report failure fallback.
- Testing return reason if coverage is incomplete: no blocking return; optional integration would reduce residual runtime-path risk.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
