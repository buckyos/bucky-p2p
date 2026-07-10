# SN Miner Real Process Auto Pipeline Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-001 | high | acceptance | `test-results/test-runs/20260622T143945Z-all-all.json` | The required whole-project `test-run.py all all` artifact has `exit_code: 1` because its final workspace-harness integration step invokes `admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive` without `--evidence-file`; all reviewed sn-miner and p2p-frame/sn-distributed-directory tests in that artifact passed before this unrelated harness admission step failed. | required whole-project test evidence lacks a fresh passing `all all` artifact |

## Object and Scope
- Module: `sn-miner`; `p2p-frame`
- Version: `v0.1`
- change_id values reviewed: `sn_miner_startup_audit`, `sn_miner_artifact_defaults`, `sn_miner_config_file_roles`, `sn_miner_owner_role_startup`, `sn_miner_serving_role_startup`, `sn_miner_role_exclusive_validation`, `sn_directory_no_registry_fallback`, `sn_serving_online_route_independent_loops`, `sn_real_process_owner_serving_dv`
- Review date: 2026-06-22
- In scope: config-file owner/serving roles for `sn-miner`, role exclusivity, removed SN directory global fallback, independent serving online vs peer-route publish semantics, and real process test evidence.
- Out of scope: pre-existing untracked review/evidence files and the unrelated `endpoint_area_server_reflexive` workspace-harness admission entry that failed the project-level all run.

## Optional Diff / Status Evidence
- `git status --short` summary: mixed worktree includes reviewed files plus pre-existing untracked evidence/review files; reviewed production paths are `sn-miner-rust/src/main.rs`, `p2p-frame/src/sn/directory/{client,server}.rs`, and `p2p-frame/src/sn/service/service.rs`.
- `git diff --stat` summary: 16 files changed, including proposal/design/testing docs, pipeline plan, SN directory code, sn-miner main, and test files.
- `git diff --name-status` summary: reviewed code/doc/test files are modified; `p2p-frame/tests/sn_command_matrix/five_by_five_command_matrix_tests.rs` is added/modified in the current worktree.
- `git diff --check` result: passed.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| sn-miner supports config-file role selection and rejects mixed owner/serving role config | `docs/versions/v0.1/modules/sn-miner/proposal.md`; `design.md` | `sn-miner-rust/src/main.rs` `--config`, `role=owner`, `role=serving`, and role alias validation | `sn-miner-rust/tests/real_process.rs`; `test-results/test-runs/20260622T141541Z-sn-miner-unit.json`; `20260622T141618Z-sn-miner-dv.json`; `20260622T141739Z-sn-miner-integration.json` | implemented |
| owner role starts OwnerDirectoryServer without Serving SN/PN service in the same process | `sn-miner` proposal/design | owner config branch starts owner directory only and rejects legacy mixed flags | `sn_miner_owner_role_starts_without_serving_service` in real process artifacts | implemented |
| serving role starts Serving SN/PN service without OwnerDirectoryServer in the same process | `sn-miner` proposal/design | serving config branch starts `SnService`/`PnServer` and requires owner serving endpoints | `sn_miner_serving_role_starts_without_owner_directory` in real process artifacts | implemented |
| global Owner/Serving SN registry fallback is removed for production/test/compat paths | `p2p-frame/sn-distributed-directory` proposal/design | `p2p-frame/src/sn/directory/client.rs`, `server.rs`, `service.rs`; static `rg` search found no `InterSnRegistry::global().get/register` or `OwnerServingRegistry::global().get/register` matches | `test-results/test-runs/20260622T142709Z-p2p-frame+sn-distributed-directory-unit.json`; `cargo test -p p2p-frame` | implemented |
| serving online heartbeat and peer-route publish are independent | `p2p-frame/sn-distributed-directory` proposal/design | route publish no longer renews serving online; owner-serving listener test requires explicit online renew | `owner_serving_listener_dispatches_publish`; `sn_report_updates_local_detail_without_publishing_route`; `sn_partitioned_peer_route_filters_revoked_serving_session`; p2p unit artifact | implemented |
| real process DV/integration covers owner/serving role startup and invalid config | both approved proposals/designs | sn-miner process test spawns binary with temporary config files and verifies logs/role boundaries | `test-results/test-runs/20260622T142732Z-p2p-frame+sn-distributed-directory-dv.json`; `20260622T142751Z-p2p-frame+sn-distributed-directory-integration.json`; sn-miner artifacts | implemented |
| whole-project acceptance shortcut evidence | acceptance rule | `python3 ./harness/scripts/test-run.py all all` executed | `test-results/test-runs/20260622T143945Z-all-all.json` exists but failed on unrelated workspace-harness admission | inconsistent |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| sn-miner role exclusivity and config parsing | normal, negative, lifecycle | `docs/versions/v0.1/modules/sn-miner/testing.md`; `testplan.yaml`; `real_process.rs` | sn-miner unit/dv/integration artifacts | adequate |
| owner vs serving startup boundaries | normal, lifecycle, boundary | real process tests inspect logs and process startup behavior for owner-only and serving-only modes | sn-miner real_process artifacts | adequate |
| no global registry fallback | negative, compatibility, cross-module | p2p testing.md maps no-fallback cases to explicit client/transport tests and static search | p2p unit artifact; `cargo test -p p2p-frame` | adequate |
| online heartbeat vs route publish independence | lifecycle, state transition, negative | p2p testing.md maps explicit online renew and route publish no-renew behavior | p2p unit artifact and targeted p2p tests | adequate |
| real-process owner/serving DV integration | lifecycle, error, cross-module | p2p and sn-miner testplans route to sn-miner real_process integration | p2p dv/integration and sn-miner integration artifacts | adequate |
| project-level all evidence freshness | whole-project regression | `test-run.py all all` run was fresh | all-all artifact failed after reviewed tests passed | gap |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-001 | sn-miner proposal/design | one sn-miner process instance starts exactly one configured role | real_process owner/serving tests and implementation review | pass |
| AR-002 | p2p proposal/design | no global registry fallback remains for SN directory communication | implementation review, static search, p2p unit tests | pass |
| AR-003 | p2p proposal/design | serving online heartbeat and peer-route publish are independent | owner-serving listener and route tests | pass |
| AR-004 | testing rules | unit, DV, and integration evidence is reachable through unified entrypoints | module artifacts for sn-miner and p2p submodule | pass |
| AR-005 | acceptance rules | whole-project `all all` run is fresh and passing | `test-results/test-runs/20260622T143945Z-all-all.json` | fail |

## Inputs
- `docs/versions/v0.1/modules/sn-miner/proposal.md`
- `docs/versions/v0.1/modules/sn-miner/design.md`
- `docs/versions/v0.1/modules/sn-miner/testing.md`
- `docs/versions/v0.1/modules/sn-miner/testplan.yaml`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/testplan.yaml`
- `harness/evidence/admission/20260622-sn-miner-real-process.md`
- implementation and test paths listed in this report
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed approved proposals and approved design docs.
2. Checked document approval hashes through schema checks.
3. Reviewed implementation paths against approved role, fallback, and loop-independence requirements.
4. Reviewed testing docs, testplan wiring, real process tests, and p2p unit/matrix tests.
5. Ran module-level unit, DV, and integration entrypoints.
6. Ran project-level all entrypoint and recorded the unrelated workspace-harness admission failure.
7. Produced a needs-changes conclusion because accepted reports require a passing `all all` artifact.

## Consistency Summary
- Proposal authority check: approved proposals cover config-file roles, role exclusivity, no registry fallback, independent loops, and real process validation.
- Proposal vs design: design documents preserve proposal scope and define config schema, startup boundaries, transport behavior, and test seams.
- Design vs testing implementation: testing docs and testplans map the design elements to real_process and p2p targeted tests.
- Design vs long-lived boundary doc: no contradiction found in reviewed module boundaries; no new cross-module dependency inversion found.
- Design vs implementation: implementation follows the approved role split and removes fallback paths; no reviewed mismatch found.
- Test implementation vs test code vs results: module-level artifacts are fresh and passing; all reviewed tests are reachable through unified entrypoints.
- Test design adequacy: adequate for reviewed behavior, with a separate project-level all-run gap.
- change_id traceability: reviewed change_ids are present in proposal/design/testing/admission evidence.
- Acceptance criteria traceability: criteria map to implementation and module-level artifacts; project-level all-run criterion is not satisfied.
- Cross-module admission: sn-miner and p2p submodule admission checks passed with `harness/evidence/admission/20260622-sn-miner-real-process.md`.
- Public API / codec / runtime semantics review: no codec or wire-format change found; runtime semantics intentionally remove fallback and route-publish online renewal per design.
- Document logic review: no contradiction found in reviewed approved docs after testing approval refresh.
- Implementation logic review: no blocking logic defect found in reviewed implementation; missing global fallback now returns explicit NotFound/logged skip paths.
- Document approval timing (approved_content_sha256 verified by schema-check): passed for sn-miner and p2p submodule.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): isolated implementation scope checks passed for both modules.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not a pure bugfix; regression-style coverage exists for removed fallback and role exclusivity.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version <version> --module <module>`: `python3 ./harness/scripts/schema-check.py --version v0.1 --module sn-miner` passed; `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory` passed.
- `uv run --active python ./harness/scripts/admission-check.py --version <version> --module <module> --change-id <change_id> --evidence-file harness/evidence/admission/<task-id>.md`: admission checks passed for all listed sn-miner and p2p submodule change_ids using `harness/evidence/admission/20260622-sn-miner-real-process.md`.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage <stage>`: isolated implementation scope checks passed in `/tmp/p2p-impl-sn-scope-iU9LCy` and `/tmp/p2p-impl-p2p-scope-QbVgYx`; isolated testing scope checks passed in `/tmp/p2p-testing-scope-sn` and `/tmp/p2p-testing-scope-dir`; main-worktree testing scope reports Rust inline-test paths as production-path violations.
- Unit test command through `harness/scripts/test-run.py`: sn-miner unit `test-results/test-runs/20260622T141541Z-sn-miner-unit.json`; p2p submodule unit `test-results/test-runs/20260622T142709Z-p2p-frame+sn-distributed-directory-unit.json`.
- DV test command through `harness/scripts/test-run.py`: sn-miner dv `test-results/test-runs/20260622T141618Z-sn-miner-dv.json`; p2p submodule dv `test-results/test-runs/20260622T142732Z-p2p-frame+sn-distributed-directory-dv.json`.
- Integration test command through `harness/scripts/test-run.py`: sn-miner integration `test-results/test-runs/20260622T141739Z-sn-miner-integration.json`; p2p submodule integration `test-results/test-runs/20260622T142751Z-p2p-frame+sn-distributed-directory-integration.json`.
- Module all command through `harness/scripts/test-run.py <module> all`: covered by separate unit/dv/integration artifacts for both reviewed modules; no separate module-all artifact was generated.
- Project all command through `harness/scripts/test-run.py all all`: `test-results/test-runs/20260622T143945Z-all-all.json` was generated but has `exit_code: 1` due unrelated workspace-harness admission command missing `--evidence-file`.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): `test-results/test-runs/20260622T143945Z-all-all.json` exists and is fresh but failing.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: `python3 ./harness/scripts/quality-check.py` passed; `harness/quality-gates.yaml` declares an explicitly empty gates list, so no quality artifact is required.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not applicable because no quality gates are configured.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not rerun after `test-run.py all all` exposed the blocking workspace-harness admission issue.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: pending for this report.
- Targeted migration search, when applicable: `rg -n "InterSnRegistry::global\\(\\)\\.(get|register)|OwnerServingRegistry::global\\(\\)\\.(get|register)" p2p-frame/src/sn sn-miner-rust/src -S` returned no matches.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: reviewed sn-miner and p2p-frame/sn-distributed-directory implementation and module-level tests satisfy the approved docs, but acceptance cannot be marked accepted while the required fresh whole-project `all all` artifact is failing.
- Supporting test evidence: sn-miner unit/dv/integration artifacts, p2p submodule unit/dv/integration artifacts, `cargo test -p p2p-frame`, `cargo test -p sn-miner`, and diagnostic all-all artifact.
- Residual risk: the unrelated workspace-harness admission command must be fixed or given valid evidence before final accepted conclusion can be issued.

## Follow-Up Tasks
- Requirement task: none for the reviewed approved behavior.
- Design task: none for the reviewed approved behavior.
- Implementation task: none for the reviewed implementation.
- Testing task: fix or scope the workspace-harness `endpoint_area_server_reflexive` all-run admission entry so `test-run.py all all` produces a passing artifact.
- Testing return reason if coverage is incomplete: project-level all-run evidence is incomplete because the all-run artifact failed after reviewed module tests passed.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
