# p2p-frame SN Distributed Directory OwnerMember Heartbeat Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-HEARTBEAT-SCOPE-001 | high | implementation/testing | `stage-scope-check.py` implementation and testing runs failed; `harness/pipeline-plan.md` return record names the same issue | The current dirty worktree contains pre-existing and cross-stage paths outside this heartbeat task's admitted implementation/testing scopes, so the implementation diff is not mechanically isolated to the admitted Scope Paths. | implementation diff was not bound to admitted design Scope Paths; single-stage scope checks failed |

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- change_id values reviewed: `sn_owner_member_heartbeat_validity`
- Review date: 2026-06-18
- In scope: OwnerMember health state, heartbeat refresh path, HRW owner filtering, proposal/design/testing/admission evidence, targeted unit evidence.
- Out of scope: unrelated dirty files already present in the worktree, full multi-process SN DV, sn-miner runtime startup validation.

## Optional Diff / Status Evidence
- `git status --short` summary: mixed dirty worktree includes heartbeat docs/code plus pre-existing harness, SN, QUIC, cyfs-p2p-test, sn-miner, and test-run shortcut paths.
- `git diff --stat` summary: 15 tracked paths in diff stat, including docs, harness plan, SN directory, inter-SN, service, sn-miner, and unrelated paths.
- `git diff --name-status` summary: not run separately; status and stat were sufficient to locate mixed scope evidence.
- `git diff --check` result: passed with no whitespace errors.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| OwnerMember heartbeat validity filters owner targets | `proposal.md` P-SN-DIST-OWNER-HEARTBEAT-1 and `design.md` Directly Mapped Change Items | `OwnerMemberHealth`, `LocalSnDirectory::owner_set`, `LocalOwnerDirectory::refresh_owner_member`, `OwnerDirectoryServer::refresh_member_heartbeat` | `cargo test -p p2p-frame sn_owner_member_heartbeat_validity -- --nocapture`; unified artifact `test-results/test-runs/20260618T065616Z-p2p-frame+sn-distributed-directory-unit.json` | implemented |
| ServingLease TTL remains separate from OwnerMember health | proposal non-goal and design invariant | health filtering is only applied to OwnerMembership owner selection; `OwnerDirectoryStore::query` still filters leases by `ServingLease::is_fresh` | `testing.md` Design Element Coverage and unit coverage for lease TTL plus heartbeat lifecycle | implemented |
| Invalid OwnerMember can recover after heartbeat refresh | proposal success evidence and design data state | `OwnerMemberHealth::refresh_at` updates freshness and resolver returns refreshed member again | `sn_owner_member_heartbeat_validity_filters_expired_owner_and_recovers` | implemented |
| Implementation scope isolation | task-entry and acceptance rules | admitted scope paths are `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `sn-miner-rust/**` | implementation/testing stage scope checks failed because mixed dirty paths remain | inconsistent |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `sn_owner_member_heartbeat_validity` | normal / boundary / negative / error / compatibility / lifecycle / cross-module | `testing.md` Direct Change Coverage, Case-Type Coverage, Design Element Coverage, Unit Tests; `testplan.yaml` step `sn-owner-member-heartbeat-validity-unit` | `test-results/test-runs/20260618T065616Z-p2p-frame+sn-distributed-directory-unit.json` passed all registered unit steps | adequate |
| Multi-process OwnerMember heartbeat transport | runtime/integration | testing.md records broader distributed runtime gaps from existing packet | DV/integration levels remain disabled with owner/risk/acceptance impact in `testplan.yaml` | gap |
| Stage scope isolation | harness/process | pipeline plan records F-HEARTBEAT-SCOPE-001 return | stage-scope implementation/testing commands failed | gap |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-HEARTBEAT-001 | proposal P-SN-DIST-OWNER-HEARTBEAT-1 | stale OwnerMember health is not returned in owner selection | code inspection plus unit test for expiry | pass |
| AR-HEARTBEAT-002 | proposal P-SN-DIST-OWNER-HEARTBEAT-1 | heartbeat refresh restores a valid OwnerMember | code inspection plus unit test for recovery | pass |
| AR-HEARTBEAT-003 | proposal non-goal and design invariant | OwnerMember health does not replace ServingLease TTL | code inspection plus testing metadata | pass |
| AR-HARNESS-001 | task-entry and acceptance rules | implementation/testing diffs are mechanically isolated to admitted stage scope | passing stage-scope implementation/testing checks | fail |

## Inputs
- `proposal.md`
- `design.md`
- test implementation and optional `testing.md`
- `testplan.yaml` for completed testing work
- optional `acceptance.md`: not used
- long-lived module doc: `docs/modules/p2p-frame.md`
- implementation: `p2p-frame/src/sn/directory/mod.rs`, `p2p-frame/src/sn/directory/client.rs`, `p2p-frame/src/sn/directory/server.rs`
- test code: inline Rust unit test in `p2p-frame/src/sn/directory/mod.rs`
- test results: `test-results/test-runs/20260618T065616Z-p2p-frame+sn-distributed-directory-unit.json`
- optional git diff/status evidence
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Review approved requirements and acceptance boundaries
2. Review design against proposal
3. Generate or finalize acceptance rules and expected results from proposal, design, implementation, and test implementation
4. Review implementation against proposal and design
5. Review whether test design reasonably covers proposal/design/code behavior, including normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases where applicable
6. Review tests and results against generated test evidence, optional `testing.md`, and required completed-testing `testplan.yaml`
7. Review document and implementation logic for contradictions, invalid assumptions, impossible states, and correctness defects
8. Use diff/status output only when helpful to locate evidence
9. Produce conclusion

## Consistency Summary
- Proposal authority check: approved proposal covers `sn_owner_member_heartbeat_validity` and records the user's approval statement.
- Proposal vs design: consistent after correcting design to strict health filtering with empty owner set allowed when all candidates are stale.
- Design vs testing implementation: testing metadata maps `sn_owner_member_heartbeat_validity` to unit step `sn-owner-member-heartbeat-validity-unit`.
- Design vs long-lived boundary doc: p2p-frame long-lived module owns `src/sn/**`; no long-lived contradiction found for directory health filtering.
- Design vs implementation: implementation matches strict health filtering and refresh API in the approved design.
- Test implementation vs test code vs results: inline unit test is registered through `testplan.yaml` and passed via unified test-run artifact.
- Test design adequacy: adequate for unit-level health expiry/recovery; broader DV/integration remains a recorded gap.
- change_id traceability: proposal, design, admission evidence, testing.md, and testplan.yaml all include `sn_owner_member_heartbeat_validity`.
- Acceptance criteria traceability: AR-HEARTBEAT-001 through AR-HEARTBEAT-003 pass; AR-HARNESS-001 fails.
- Cross-module admission: p2p-frame/sn-distributed-directory schema and admission checks passed for the reviewed change_id.
- Public API / codec / runtime semantics review: no client-visible SN protocol shape change identified; new health API is internal directory-side behavior.
- Document logic review: stale design fallback wording was corrected before this report; no remaining heartbeat-specific contradiction found.
- Implementation logic review: health filtering is independent from ServingLease TTL and refreshes after successful owner publish/query or explicit heartbeat refresh.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed after proposal/design/testing approval hash updates.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): failed because the worktree includes mixed unrelated and cross-stage dirty paths.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not-applicable; this is feature implementation, not bugfix regression work.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version <version> --module <module>`: `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory` passed.
- `uv run --active python ./harness/scripts/admission-check.py --version <version> --module <module> --change-id <change_id> --evidence-file harness/evidence/admission/<task-id>.md`: `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_owner_member_heartbeat_validity --evidence-file harness/evidence/admission/20260618-owner-member-heartbeat-validity.md` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage <stage>`: implementation and testing stage-scope checks failed due mixed dirty worktree; see F-HEARTBEAT-SCOPE-001.
- Unit test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory unit` passed with artifact `test-results/test-runs/20260618T065616Z-p2p-frame+sn-distributed-directory-unit.json`.
- DV test command through `harness/scripts/test-run.py`: DV is disabled in `testplan.yaml` with owner/risk/acceptance impact recorded; not run.
- Integration test command through `harness/scripts/test-run.py`: integration is disabled in `testplan.yaml` with owner/risk/acceptance impact recorded; not run.
- Module all command through `harness/scripts/test-run.py <module> all`: not run because acceptance is already needs changes due failing stage-scope gate.
- Project all command through `harness/scripts/test-run.py all all`: not run because acceptance is already needs changes due failing stage-scope gate.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): not available for this acceptance attempt because whole-project tests were not run after the scope-gate failure.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed; `harness/quality-gates.yaml` declares `gates: []`.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not-applicable because no quality gates are configured and no quality artifact was written.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not run because acceptance is already needs changes due failing stage-scope gate.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: `uv run --active python ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-sn-distributed-directory-owner-member-heartbeat-acceptance-2026-06-18.md` passed.
- Targeted migration search, when applicable: not-applicable; no migration search required for the reviewed internal directory health change.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: heartbeat-specific proposal/design/code/testing evidence is coherent and targeted unit tests pass, but implementation/testing stage-scope gates fail because the worktree is not isolated to this admitted heartbeat task.
- Supporting test evidence: `test-results/test-runs/20260618T065616Z-p2p-frame+sn-distributed-directory-unit.json`
- Residual risk: mixed worktree scope prevents clean mechanical acceptance; multi-process SN DV/integration remains a recorded packet-level gap.

## Follow-Up Tasks
- Requirement task: none for heartbeat semantics.
- Design task: none for heartbeat semantics after strict filtering correction.
- Implementation task: isolate or commit unrelated dirty paths, then rerun implementation stage-scope for `sn_owner_member_heartbeat_validity`.
- Testing task: isolate or commit unrelated dirty paths, then rerun testing stage-scope for `sn-distributed-directory`.
- Testing return reason if coverage is incomplete: unit coverage is adequate for heartbeat expiry/recovery; scope gate, not heartbeat unit coverage, blocks acceptance.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not-applicable
