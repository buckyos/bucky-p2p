# p2p-frame SN TCP Source Mapped Only Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-001 | high | implementation | `stage-scope-check.py --stage implementation --change-id sn_tcp_source_mapped_only` failed | Worktree contains out-of-stage proposal/design/testing/pipeline files and a pre-existing `p2p-frame/src/sn/client/sn_service.rs` change outside admitted scope paths. | implementation diff must stay inside admitted design Scope Paths |
| F-002 | medium | testing | `stage-scope-check.py --stage testing` failed | Testing scope could not be isolated because proposal/design/pipeline and production files are dirty in the same worktree. | testing stage scope must contain only testing artifacts |

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- change_id values reviewed: sn_tcp_source_mapped_only
- Review date: 2026-07-03
- In scope: SN service TCP observed endpoint filtering, mapped candidate construction from source IP plus `map_ports`, proposal/design/testing/admission evidence, and unit evidence.
- Out of scope: pre-existing `sn_client_protocol_priority` changes in `p2p-frame/src/sn/client/sn_service.rs`, unrelated untracked reviews/evidence, and whole-project all-all cleanup.

## Optional Diff / Status Evidence
- `git status --short` summary: modified proposal/design/testing/testplan/pipeline, modified `p2p-frame/src/sn/client/sn_service.rs` from prior work, modified `p2p-frame/src/sn/service/service.rs`, new admission evidence and this report.
- `git diff --stat` summary: not pasted; reviewed focused diff for docs and `p2p-frame/src/sn/service/service.rs`.
- `git diff --name-status` summary: not pasted; scope failures list blocking paths.
- `git diff --check` result: passed.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| TCP observed source address is not directly returned | `proposal.md` P-SN-TCP-SOURCE-MAPPED-ONLY-1; `design.md` `sn_tcp_source_mapped_only` | `SnService::is_direct_observed_candidate` excludes TCP before direct observed candidate classification. | `sn_tcp_observed_endpoint_without_map_ports_is_not_returned`; `test-results/test-runs/20260703T102530Z-p2p-frame-unit.json` passed. | implemented |
| TCP source IP plus `map_ports` creates `Mapped` candidates | `proposal.md` and `design.md` mapped-port boundary | `SnService::observed_endpoint_candidates` maps all observed IPs, including TCP, through `mapped_endpoint_from_observed`. | `sn_tcp_observed_endpoint_with_map_ports_returns_only_mapped_candidates`; unit artifact passed. | implemented |
| Non-TCP observed endpoints keep direct classification | `design.md` direct candidate separation | Non-TCP observed endpoints still pass through `classify_observed_endpoint`. | `sn_non_tcp_observed_endpoint_remains_direct_candidate`; unit artifact passed. | implemented |
| Peer cache does not store raw TCP source endpoint | `design.md` Data and State | No new peer cache field was added; service continues storing report desc/local_eps/map_ports. | Code review plus unit candidate helper coverage. | implemented |
| Stage isolation | Harness stage-scope rules | Mixed worktree blocks implementation/testing isolation. | Stage scope commands failed. | inconsistent |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| sn_tcp_source_mapped_only | normal / boundary / negative / error / compatibility / lifecycle / cross-module | `testing.md` Direct Change Coverage and Case-Type Coverage rows for `V-SN-TCP-SOURCE-MAPPED-ONLY-UNIT`. | `cargo test -p p2p-frame sn_tcp_observed_endpoint`, `cargo test -p p2p-frame sn_non_tcp_observed_endpoint`, and `python3 ./harness/scripts/test-run.py p2p-frame unit` passed. | adequate |
| stage-scope isolation | compatibility / process | Harness requires stage-specific dirty-path isolation. | implementation and testing stage-scope failed due mixed worktree. | gap |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| A-SN-TCP-001 | proposal/design | Raw TCP observed socket address must not be returned without `map_ports`. | Targeted unit and implementation review. | pass |
| A-SN-TCP-002 | proposal/design | With `map_ports`, returned mapped endpoint must use source IP plus reported port, not raw TCP source port. | Targeted unit and implementation review. | pass |
| A-SN-TCP-003 | design | Non-TCP observed endpoint direct classification must remain intact. | Targeted unit and implementation review. | pass |
| A-SCOPE-001 | harness rules | Implementation and testing scope checks must pass for the reviewed work. | Stage scope command evidence. | fail |

## Inputs
- `proposal.md`
- `design.md`
- test implementation and optional `testing.md`
- `testplan.yaml` for completed testing work, or a versioned local exception with reason, owner, risk, and acceptance impact
- optional `acceptance.md`
- long-lived module doc
- implementation
- test code
- test results
- optional git diff/status evidence
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Review approved requirements and acceptance boundaries
2. Review design against proposal
3. Generate or finalize acceptance rules and expected results from proposal, design, implementation, and test implementation
4. Review implementation against proposal and design
5. Review whether test design reasonably covers proposal/design/code behavior, including normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases where applicable
6. Review tests and results against generated test evidence, optional `testing.md`, and required completed-testing `testplan.yaml` or its versioned exception
7. Review document and implementation logic for contradictions, invalid assumptions, impossible states, and correctness defects
8. Use diff/status output only when helpful to locate evidence
9. Produce conclusion

## Consistency Summary
- Proposal authority check: approved proposal covers `sn_tcp_source_mapped_only` and the user approval statement.
- Proposal vs design: consistent; design maps TCP source filtering, mapped-port construction, response arrays, and peer cache boundary.
- Design vs testing implementation: consistent; testing.md and testplan.yaml include targeted unit and full unit evidence.
- Design vs long-lived boundary doc: no long-lived boundary update was required for this SN service-local candidate filtering.
- Design vs implementation: implementation follows the mapped scope in `p2p-frame/src/sn/service/service.rs`.
- Test implementation vs test code vs results: targeted and full unit results passed.
- Test design adequacy: adequate for behavior, but process scope remains a gap.
- change_id traceability: proposal, design, testing, admission evidence, and tests all cite `sn_tcp_source_mapped_only`.
- Acceptance criteria traceability: behavior criteria pass; stage-scope criterion fails.
- Cross-module admission: only p2p-frame was admitted; no additional module admission required.
- Public API / codec / runtime semantics review: no SN wire, endpoint codec, TCP tunnel wire, TLS, or public API change observed.
- Document logic review: approved proposal/design/testing are coherent for this change.
- Implementation logic review: helper separates direct observed candidates from mapped endpoint construction as required.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed after approvals.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): failed due mixed worktree and pre-existing out-of-scope `sn/client` file.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not applicable; this is a requirement change.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`: passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_tcp_source_mapped_only --evidence-file harness/evidence/admission/20260703-sn-tcp-source-mapped-only.md`: passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage <stage>`: design, implementation, and testing scope checks failed due mixed worktree paths; see findings.
- Unit test command through `harness/scripts/test-run.py`: `python3 ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260703T102530Z-p2p-frame-unit.json`.
- DV test command through `harness/scripts/test-run.py`: not run; p2p-frame DV is disabled in testplan.
- Integration test command through `harness/scripts/test-run.py`: not run; acceptance is already needs changes because scope gates failed.
- Module all command through `harness/scripts/test-run.py <module> all`: not run; scope gates failed before module-all acceptance.
- Project all command through `harness/scripts/test-run.py all all`: not run; scope gates failed before whole-project acceptance.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): none, because all-all was not run after scope failure.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed; no quality gates configured.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): none; quality gates are explicitly empty.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not run; scope gates failed before root shortcut.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: pending at report creation.
- Targeted migration search, when applicable: not applicable.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: behavior and tests pass, but implementation/testing stage-scope gates fail in the current mixed worktree.
- Supporting test evidence: targeted cargo tests passed; `test-results/test-runs/20260703T102530Z-p2p-frame-unit.json` passed.
- Residual risk: raw behavior appears correct, but final acceptance requires isolated or cleaned stage-scope evidence.

## Follow-Up Tasks
- Requirement task: none for behavior; proposal is approved.
- Design task: none for behavior; design is approved.
- Implementation task: rerun implementation scope check in an isolated worktree or after separating pre-existing `sn/client` and cross-stage document changes.
- Testing task: rerun testing scope check in an isolated worktree or after separating production and upstream document changes.
- Testing return reason if coverage is incomplete: coverage is not incomplete; process isolation is incomplete.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
