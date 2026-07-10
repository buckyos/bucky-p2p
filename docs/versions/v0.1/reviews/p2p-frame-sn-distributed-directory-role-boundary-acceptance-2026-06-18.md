# SN Distributed Directory Role Boundary Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-ROLE-001 | high | implementation/testing | `harness/pipeline-plan.md` Return Records and prior `stage-scope-check.py` results | Mechanical scope gates cannot close because the worktree contains broad existing dirty paths and multiple stage artifact groups remain dirty together. | implementation/testing scope gate failed |
| F-ROLE-002 | medium | testing | `testing.md` DV/Integration Tests gap rows and unit result `test-results/test-runs/20260618T034112Z-p2p-frame+sn-distributed-directory-unit.json` | Unit coverage includes owner/serving role separation and directory client/server separation, but there is still no multi-process SN-to-SN publish/query/detail/relay DV or integration evidence. | distributed runtime evidence remains incomplete |
| F-ROLE-003 | medium | integration | `testing.md` Integration Tests gap row and `harness/pipeline-plan.md` Return Records | `sn-miner` compatibility remains unclosed; owner membership startup/compatibility has not produced passing integration evidence. | deployment/config check incomplete |

## Object and Scope
- Module: `p2p-frame`
- Submodule: `sn-distributed-directory`
- Version: `v0.1`
- change_id values reviewed: `sn_distributed_directory`, `sn_owner_serving_role_boundary`, `sn_directory_client_server_boundary`, `sn_distributed_query_merge`, `sn_distributed_relay_call`, `sn_inter_service_validation`
- Review date: 2026-06-18
- In scope: updated proposal/design/testing docs, implementation admission, SN directory/service role-boundary implementation, targeted unit evidence.
- Out of scope: reverting or cleaning unrelated dirty workspace files, proving pre-existing harness/bootstrap changes, full multi-process DV implementation.

## Optional Diff / Status Evidence
- `git status --short` summary: broad dirty worktree containing this pipeline's docs/code plus pre-existing harness/docs/review/test-run artifacts.
- `git diff --stat` summary: used for discovery only; final status is based on proposal/design/code/test evidence and command results.
- `git diff --name-status` summary: used for discovery only; scope gates provide the mechanical failure evidence.
- `git diff --check` result: passed for the final touched-file set in this iteration.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| OwnerMembership + HRW + ServingLease owner directory | `proposal.md` P-SN-DIST-DIR-1 and `design.md` Data and State | `p2p-frame/src/sn/directory/mod.rs` | `test-results/test-runs/20260618T034112Z-p2p-frame+sn-distributed-directory-unit.json` | implemented |
| Owner SN / Serving SN role separation | `proposal.md` P-SN-DIST-ROLE-1 and `design.md` owner/serving role boundary | `OwnerDirectoryServer`, `OwnerDirectoryClient`, serving-only `SnService` | `sn_directory_client_server_boundary_keeps_sn_service_serving_only` in run artifact | implemented |
| Directory client/server module boundary | `proposal.md` P-SN-DIST-DIRECTORY-SERVER-1 and `design.md` `sn_directory` client/server split | `p2p-frame/src/sn/directory/mod.rs`; no `sn::owner` module export; `SnService` no longer owns owner membership/resolver/store/owner publish/query handler | `sn_directory_client_server_boundary_keeps_sn_service_serving_only` in run artifact | implemented |
| Query merge via serving SN detail | `proposal.md` P-SN-DIST-QUERY-1 and `design.md` Query merge detail flow | `SnService::query_remote_details`, `InterSnPeer::query_detail_from_sn`, `TtpInterSnClient::query_detail_from_sn` | targeted unit covers registered serving SN detail lookup; no multi-process query evidence | gap |
| Relay call via remote serving SN | `proposal.md` P-SN-DIST-CALL-1 and `design.md` Relay call flow | `SnService::relay_call_to_serving_sn`, `deliver_called_to_local_peer`, `TtpInterSnClient::relay_call_to_sn` | no multi-process TTP relay evidence | gap |
| SN inter-service validation | `proposal.md` P-SN-INTER-AUTH-1 and `design.md` validator interface | `SnInterServiceValidator`, command validation before side effects | targeted unit covers reject blocking owner write | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `sn_distributed_directory` lease state | normal, boundary, negative, error, compatibility, lifecycle, cross-module | `testing.md` Direct Change Coverage and Case-Type Coverage | targeted unit artifact `test-results/test-runs/20260618T034112Z-p2p-frame+sn-distributed-directory-unit.json` | adequate |
| `sn_owner_serving_role_boundary` owner/serving boundary | normal, boundary, negative, error, compatibility, lifecycle, cross-module | `testing.md` role-boundary coverage rows | directory client/server boundary unit passes through unified entrypoint | adequate |
| `sn_directory_client_server_boundary` directory client/server boundary | normal, boundary, negative, error, compatibility, lifecycle, cross-module | `testing.md` directory client/server coverage rows | directory client/server boundary unit passes through unified entrypoint | adequate |
| `sn_distributed_query_merge` distributed query | normal, boundary, negative, error, compatibility, lifecycle, cross-module | `testing.md` records unit coverage and DV/integration gap | targeted unit covers registry fallback detail, not multi-process query | gap |
| `sn_distributed_relay_call` distributed call relay | normal, boundary, negative, error, compatibility, lifecycle, cross-module | `testing.md` records unit coverage and DV/integration gap | no runnable multi-process relay evidence | gap |
| `sn_inter_service_validation` reject side effects | normal, boundary, negative, error, compatibility, lifecycle, cross-module | `testing.md` validator rows | targeted unit reject path passes | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-SN-DIST-001 | proposal/design | single SN without OwnerMembership remains local-only and compatible | owner resolver local-only test and compile | pass |
| AR-SN-DIST-002 | proposal/design | owner directory stores only serving leases, not NAT detail | implementation review and lease tests | pass |
| AR-SN-DIST-003 | proposal/design | owner and serving roles interact through explicit adapter/interface boundaries | `OwnerDirectoryServer`, `OwnerDirectoryClient`, and role-boundary unit | pass |
| AR-SN-DIST-003B | proposal/design | `SnService` contains only Serving SN behavior and owner directory server/client lives in `sn_directory` | absence of owner membership/store/publish/query handler in `SnService`; absence of `sn::owner` module export; unit evidence | pass |
| AR-SN-DIST-004 | proposal/design | multi-SN query/call works across serving SN boundaries | TTP-backed multi-process DV/integration evidence | gap |
| AR-SN-DIST-005 | proposal/design | validator reject blocks connection/command side effects | unit reject evidence | pass |
| AR-SN-DIST-006 | harness rules | implementation/testing diffs stay within stage/scope gates | passing `stage-scope-check.py` results | fail |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/testplan.yaml`
- `harness/evidence/admission/20260618-sn-directory-client-server-boundary.md`
- `harness/evidence/admission/20260618-sn-directory-client-server-boundary.p2p-frame.sn-distributed-directory.stamp.json`
- production code under admitted scope paths
- test result artifact `test-results/test-runs/20260618T034112Z-p2p-frame+sn-distributed-directory-unit.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed approved proposal/design authority and direct change mappings.
2. Reviewed implementation against design ownership, role-boundary interfaces, and Scope Paths.
3. Reviewed testing doc and testplan against proposal/design/code behavior.
4. Reviewed targeted test, compile command, quality gate, and scope command evidence.
5. Applied acceptance rules and recorded blocking findings.

## Consistency Summary
- Proposal authority check: proposal is authoritative and now explicitly requires owner/directory to be the same module domain with client/server separation.
- Proposal vs design: consistent; design maps `sn_directory_client_server_boundary` and defines local fallback through the same directory client boundary.
- Design vs testing implementation: testing maps all six change_id values and updates the targeted unit entrypoint to `sn_directory_client_server_boundary`.
- Design vs long-lived boundary doc: not updated in this task; no acceptance claim depends on it.
- Design vs implementation: materially consistent for role boundary; `SnService` no longer owns owner membership/resolver/store or owner publish/query handler, and owner directory server/client behavior is in `sn_directory`.
- Test implementation vs test code vs results: unified test entrypoint ran both distributed-directory and directory client/server boundary targeted tests successfully.
- Test design adequacy: adequate for unit-level lease, validator, registry fallback, TTP frame, and role-boundary behavior; gap for multi-process SN-to-SN runtime behavior.
- change_id traceability: admission evidence binds all six change_id values to proposal/design quotes.
- Acceptance criteria traceability: owner/serving separation and directory client/server separation are evidenced; full distributed runtime behavior and clean stage-scope evidence are not.
- Cross-module admission: `sn-miner-rust` is in admitted scope, but compile/startup compatibility remains incomplete.
- Public API / codec / runtime semantics review: client-visible SN API remains unchanged; internal owner/serving adapter and inter-SN paths are internal.
- Document logic review: docs record gaps rather than claiming full distributed runtime evidence.
- Implementation logic review: `OwnerDirectoryServer` owns owner lease state; `SnService` remains serving-only and owner directory still does not read `PeerManager`.
- Document approval timing (approved_content_sha256 verified by schema-check): `schema-check.py` passed after proposal/design/testing approvals.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): remains blocked by broad dirty workspace paths outside the admitted scope.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not applicable; this is a feature/refactor continuation.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory`: passed.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --docs proposal`: passed.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --docs design`: passed.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --docs testing`: passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_distributed_directory --change-id sn_owner_serving_role_boundary --change-id sn_directory_client_server_boundary --change-id sn_distributed_query_merge --change-id sn_distributed_relay_call --change-id sn_inter_service_validation --evidence-file harness/evidence/admission/20260618-sn-directory-client-server-boundary.md`: passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation ...`: failed due dirty mixed-stage worktree and changed paths outside admitted design Scope Paths, including `docs/architecture/sn-distributed-directory.md`, `test-run.bat`, and `test-run.sh`.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage testing ...`: failed due dirty mixed-stage worktree, including production code, admission evidence, proposal/design docs, acceptance report, and unrelated harness paths.
- Unit test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory unit` passed, artifact `test-results/test-runs/20260618T034112Z-p2p-frame+sn-distributed-directory-unit.json`.
- DV test command through `harness/scripts/test-run.py`: disabled in `testplan.yaml` with owner/risk/acceptance gap reason.
- Integration test command through `harness/scripts/test-run.py`: disabled in `testplan.yaml` with owner/risk/acceptance gap reason.
- Module all command through `test-run.py <module> all`: not run as acceptance-pass evidence because conclusion is needs changes; unit command passed and DV/integration are disabled with explicit gaps.
- Project all command through `test-run.py all all`: not run because conclusion is needs changes and scope gates already block acceptance.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): not produced for this iteration because conclusion is needs changes.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed; no quality gates configured.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not applicable; `harness/quality-gates.yaml` declares an explicitly empty gates list.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not run; direct harness test-run was used.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-sn-distributed-directory-role-boundary-acceptance-2026-06-18.md`: passed.
- Targeted migration search, when applicable: `rg -n "crate::sn::owner|pub mod owner|SnOwnerService|sn_owner_service_module_boundary" p2p-frame/src/sn docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory -S` verified old owner module and old module-boundary change_id are absent from current code/docs, aside from historical artifacts not used by the current baseline.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: serving-only `SnService`, directory client/server implementation, and unit evidence are in place, but acceptance cannot pass with failing stage-scope gates and missing multi-process distributed SN evidence.
- Supporting test evidence: `test-results/test-runs/20260618T034112Z-p2p-frame+sn-distributed-directory-unit.json`
- Residual risk: unit-tested role and TTP frame behavior may still diverge from true cross-process SN transport; sn-miner owner membership wiring remains incompletely verified.

## Follow-Up Tasks
- Requirement task: not required unless the intended first version no longer requires TTP-backed inter-SN transport.
- Design task: not required for the current directory client/server boundary unless the accepted scope changes again.
- Implementation task: isolate or clean the worktree enough for implementation scope gates to pass, then rerun implementation scope check.
- Testing task: add DV/integration coverage for multi-process SN report/query/call and sn-miner owner membership startup.
- Testing return reason if coverage is incomplete: DV/integration gaps for distributed runtime behavior and sn-miner compatibility.
- Iteration count: 3
- Stop reason if more than 5 unsuccessful iterations: not applicable.
