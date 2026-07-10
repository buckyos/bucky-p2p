# SN Distributed Directory Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-001 | high | implementation/testing | `stage-scope-check.py --stage implementation ...` and `stage-scope-check.py --stage testing ...` | Mechanical scope gates cannot close because the worktree contains broad existing dirty/untracked paths outside this task and multiple stage artifact groups remain dirty together. | implementation/testing scope gate failed |
| F-002 | medium | testing | `testing.md` DV/Integration Tests gap rows and unit result `test-results/test-runs/20260615T163301Z-p2p-frame+sn-distributed-directory-unit.json` | TTP inter-SN client/listener code and frame unit coverage exist, but there is still no multi-process SN-to-SN publish/query/detail/relay DV or integration evidence. | approved proposal/design require distributed SN behavior, not only unit-level transport evidence |
| F-003 | medium | integration | `timeout 120s cargo check -p sn-miner` attempt | `sn-miner` compatibility check did not reach code diagnostics because third-party `libsecp256k1` build script ran too long and the command exited 124. | deployment/config check incomplete |

## Object and Scope
- Module: `p2p-frame`
- Submodule: `sn-distributed-directory`
- Version: `v0.1`
- change_id values reviewed: `sn_distributed_directory`, `sn_distributed_query_merge`, `sn_distributed_relay_call`, `sn_inter_service_validation`
- Review date: 2026-06-16
- In scope: submodule proposal/design/testing docs, admission evidence, SN directory/inter-SN/service/protocol implementation, `sn-miner` owner membership wiring, targeted unit evidence.
- Out of scope: reverting unrelated dirty workspace files, proving existing unrelated harness/bootstrap files, full workspace cleanup.

## Optional Diff / Status Evidence
- `git status --short` summary: broad dirty worktree, including this task's files plus pre-existing harness/docs/test-run artifacts and unrelated untracked docs.
- `git diff --stat` summary: used for discovery only; final status is based on proposal/design/code/test evidence and command results.
- `git diff --name-status` summary: used for discovery only; scope gates provide the mechanical failure evidence.
- `git diff --check` result: not run; acceptance already has blocking findings.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| OwnerMembership + HRW + ServingLease owner directory | `proposal.md` P-SN-DIST-DIR-1 and `design.md` Data and State | `p2p-frame/src/sn/directory/mod.rs`, `SnService::publish_serving_lease` | `test-results/test-runs/20260615T163301Z-p2p-frame+sn-distributed-directory-unit.json` | implemented |
| Query merge via serving SN detail | `proposal.md` P-SN-DIST-QUERY-1 and `design.md` Query merge detail flow | `SnService::query_remote_details`, `InterSnPeer::query_detail_from_sn`, `TtpInterSnClient::query_detail_from_sn` | targeted unit covers registered serving SN detail lookup and TTP publish frame path; no multi-process query evidence | gap |
| Relay call via remote serving SN | `proposal.md` P-SN-DIST-CALL-1 and `design.md` Relay call flow | `SnService::relay_call_to_serving_sn`, `deliver_called_to_local_peer`, `TtpInterSnClient::relay_call_to_sn` | no multi-process TTP relay evidence | gap |
| SN inter-service validation | `proposal.md` P-SN-INTER-AUTH-1 and `design.md` validator interface | `SnInterServiceValidator`, command validation before side effects | targeted unit covers reject blocking owner write | implemented |
| sn-miner owner membership config | `proposal.md` sn-miner config boundary and `design.md` sn_miner_config | `sn-miner-rust/src/main.rs --owner-members` | `cargo check -p sn-miner` did not complete | missing |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `sn_distributed_directory` lease state | normal, boundary, negative, error, compatibility, lifecycle, cross-module | `testing.md` Direct Change Coverage and Case-Type Coverage | targeted unit artifact `test-results/test-runs/20260615T163301Z-p2p-frame+sn-distributed-directory-unit.json` | adequate |
| `sn_distributed_query_merge` distributed query | normal, boundary, negative, error, compatibility, lifecycle, cross-module | `testing.md` records unit coverage and DV/integration gap | targeted unit covers registry fallback detail and TTP frame dispatch, not multi-process query | gap |
| `sn_distributed_relay_call` distributed call relay | normal, boundary, negative, error, compatibility, lifecycle, cross-module | `testing.md` records unit coverage and DV/integration gap | no runnable multi-process relay evidence | gap |
| `sn_inter_service_validation` reject side effects | normal, boundary, negative, error, compatibility, lifecycle, cross-module | `testing.md` validator rows | targeted unit reject path passes | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-SN-DIST-001 | proposal/design | single SN without OwnerMembership remains local-only and compatible | owner resolver local-only test and compile | pass |
| AR-SN-DIST-002 | proposal/design | owner directory stores only serving leases, not NAT detail | implementation review and lease tests | pass |
| AR-SN-DIST-003 | proposal/design | multi-SN query/call works across serving SN boundaries | TTP-backed multi-process DV/integration evidence | gap |
| AR-SN-DIST-004 | proposal/design | validator reject blocks connection/command side effects | unit reject evidence | pass |
| AR-SN-DIST-005 | harness rules | implementation/testing diffs stay within stage/scope gates | passing `stage-scope-check.py` results | fail |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/testplan.yaml`
- `harness/evidence/admission/20260615-sn-distributed-directory.md`
- `harness/evidence/admission/20260615-sn-distributed-directory.p2p-frame.sn-distributed-directory.stamp.json`
- production code under admitted scope paths
- test result artifact `test-results/test-runs/20260615T163301Z-p2p-frame+sn-distributed-directory-unit.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed approved proposal/design authority and direct change mappings.
2. Reviewed implementation against design ownership, interfaces, and Scope Paths.
3. Reviewed testing doc and testplan against proposal/design/code behavior.
4. Reviewed targeted test and compile command evidence.
5. Applied acceptance rules and recorded blocking findings.

## Consistency Summary
- Proposal authority check: proposal remains the authority; downstream artifacts do not change client-visible SN API goals, but delivered DV/integration evidence does not fully satisfy distributed multi-process behavior.
- Proposal vs design: consistent after design scope return added `p2p-frame/src/sn/mod.rs`, then endpoint-aware owner members and dedicated inter-SN TTP purpose.
- Design vs testing implementation: testing records unit coverage plus DV/integration gaps for multi-process transport.
- Design vs long-lived boundary doc: not updated in this task; no acceptance claim depends on it.
- Design vs implementation: materially consistent for code review; directory, validator, service config, registry fallback, and TTP-backed inter-SN control-stream client/listener code exist. Multi-process runtime proof remains a testing gap.
- Test implementation vs test code vs results: targeted unit tests pass and match `testplan.yaml`.
- Test design adequacy: adequate for lease/validator unit behavior; gap for multi-process SN-to-SN query/call.
- change_id traceability: admission evidence binds all four change_id values to proposal/design quotes.
- Acceptance criteria traceability: single-SN fallback, lease TTL/sequence, validator side effects, and TTP frame dispatch are evidenced; full distributed runtime behavior is not.
- Cross-module admission: `sn-miner-rust` is in admitted scope, but its compile check did not complete.
- Public API / codec / runtime semantics review: client-visible `PackageCmdCode` range is unchanged; internal protocol structs were added.
- Document logic review: docs record gaps rather than claiming complete distributed runtime evidence.
- Implementation logic review: TTP/control-stream client/listener code is present, while registry fallback remains useful for endpoint-less members and unit coverage.
- Document approval timing (approved_content_sha256 verified by schema-check): `schema-check.py` passed after proposal/design/testing approvals.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): failed because of broad dirty workspace paths outside the admitted scope.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not applicable; this is a feature implementation.

## Required Command Evidence
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory`: passed.
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_distributed_directory --change-id sn_distributed_query_merge --change-id sn_distributed_relay_call --change-id sn_inter_service_validation --evidence-file harness/evidence/admission/20260615-sn-distributed-directory.md`: passed.
- `python3 ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id ...` and `python3 ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --submodule sn-distributed-directory`: failed due dirty mixed-stage worktree and unrelated paths outside admitted scope.
- Unit test command through `harness/scripts/test-run.py`: `python3 ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory unit` passed.
- DV test command through `harness/scripts/test-run.py`: disabled in `testplan.yaml` with owner/risk/acceptance gap reason.
- Integration test command through `harness/scripts/test-run.py`: disabled in `testplan.yaml` with owner/risk/acceptance gap reason.
- Module all command through `test-run.py <module> all`: dry-run reached unit and reported no DV/integration tests; full run not used as acceptance-pass evidence.
- Project all command through `test-run.py all all`: not run for this acceptance because conclusion is needs changes.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): previous artifact exists `test-results/test-runs/20260615T035658Z-all-all.json`, not used for acceptance because conclusion is needs changes.
- `python3 ./harness/scripts/quality-check.py`: passed; no quality gates configured.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not applicable; `harness/quality-gates.yaml` declares an explicitly empty gates list.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not run; direct harness test-run was used.
- Acceptance report check `python3 ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-sn-distributed-directory-acceptance-2026-06-15.md`: passed.
- Targeted migration search, when applicable: `rg -n "SnServiceConfig|create_sn_service|set_connection_validator|OwnerMembership|owner" -S` used to find config call sites.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: implementation and unit tests landed, but acceptance cannot pass with failing stage-scope gates and missing TTP-backed multi-process distributed SN evidence.
- Supporting test evidence: `test-results/test-runs/20260615T163301Z-p2p-frame+sn-distributed-directory-unit.json`
- Residual risk: unit-tested TTP frame behavior may diverge from real cross-process SN transport; sn-miner owner membership wiring has not completed compile verification.

## Follow-Up Tasks
- Requirement task: not required unless the intended first version no longer requires TTP-backed inter-SN transport.
- Design task: not required for the current TTP transport direction unless the accepted scope changes again.
- Implementation task: isolate or clean the worktree enough for implementation scope gates to pass.
- Testing task: add DV/integration coverage for multi-process SN report/query/call and sn-miner owner membership startup.
- Testing return reason if coverage is incomplete: DV/integration gaps for distributed runtime behavior and sn-miner compatibility.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
