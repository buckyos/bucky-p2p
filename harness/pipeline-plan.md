# Pipeline Plan: SN Miner Configured Roles And Real Process SN Validation

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/sn-miner/proposal.md`; `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "批准 sn-miner proposal 和 p2p-frame/sn-distributed-directory proposal，并启动 auto-pipeline 自动处理后续 design、implementation、testing、acceptance。"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): sn-miner; p2p-frame
- Submodule(s): p2p-frame/sn-distributed-directory
- change_id values: sn_miner_startup_audit, sn_miner_artifact_defaults, sn_miner_config_file_roles, sn_miner_owner_role_startup, sn_miner_serving_role_startup, sn_miner_role_exclusive_validation, sn_directory_no_registry_fallback, sn_serving_online_route_independent_loops, sn_real_process_owner_serving_dv

## Acceptance Baseline
- Final acceptance uses the approved `sn-miner` proposal and approved `p2p-frame/sn-distributed-directory` proposal as authority.
- `sn-miner` must support config-file driven `owner` or `serving` role startup, with one process instance starting exactly one of those roles.
- Owner SN startup must expose owner peer/control and serving-facing endpoints without starting Serving SN peer-facing service in the same process.
- Serving SN startup must configure owner membership/endpoints, Serving SN online heartbeat, PeerRoute publish behavior, and peer-facing SN/PN service without starting OwnerDirectoryServer in the same process.
- Serving SN online heartbeat and PeerRoute publish are independent runtime loops with independent config, lifecycle, failure handling, and observations.
- Production, test, and compat paths must not rely on a global registry or same-process fallback for Owner SN / Serving SN directory communication.
- Real process DV/integration evidence must start independent Owner SN and Serving SN processes and cover config success, invalid config, role exclusivity, port/listener behavior, owner/serving transport, online heartbeat, PeerRoute publish/query, and shutdown.
- Existing uncommitted SN 5x5 command-matrix artifacts are prior worktree context and are not authority for this pipeline unless explicitly covered by the change_id list above.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-SN-MINER-REAL-PROC-1 | proposal | confirmed | Approve config-driven SN miner roles, no fallback, independent loops, and real process evidence | `sn-miner/proposal.md`; `p2p-frame/sn-distributed-directory/proposal.md` | root | user approval | approved proposals | proposal docs structure passed and approval metadata records user statement |
| D-SN-MINER-REAL-PROC-1 | design | confirmed | Define sn-miner config schema, owner/serving startup boundaries, p2p no-fallback transport, independent loops, and real process test seams | `docs/versions/v0.1/modules/sn-miner/design.md`; `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md`; required `docs/modules/*.md` boundary sync if needed | root | P-SN-MINER-REAL-PROC-1 | approved design docs | design doc checks, schema checks, stage-scope checks, and auto-pipeline approval metadata recorded |
| I-SN-MINER-REAL-PROC-1 | implementation | confirmed | Implement minimal production/runtime wiring for sn-miner config roles and admitted p2p SN transport/loop behavior | admitted `sn-miner-rust/**` and `p2p-frame/src/sn/**` paths plus admission evidence | root | D-SN-MINER-REAL-PROC-1 | implementation and admission evidence | schema/admission passed for every affected packet and targeted build checks pass |
| T-SN-MINER-REAL-PROC-1 | testing | confirmed | Add post-implementation tests, fixtures, runner wiring, and real process evidence for owner/serving roles and p2p SN semantics | testing docs, testplan entries, tests, fixtures, and unified runner wiring | root | I-SN-MINER-REAL-PROC-1 | runnable unit/DV/integration evidence | coverage checks and relevant `test-run.py` levels pass or blockers are recorded |
| A-SN-MINER-REAL-PROC-1 | acceptance | needs changes | Audit proposal/design/implementation/testing consistency and evidence completeness | `docs/versions/v0.1/reviews/` | root | T-SN-MINER-REAL-PROC-1 | acceptance report | acceptance report check passed; conclusion is needs changes because whole-project `all all` evidence failed |

## Return Routing
- Proposal issue: return to proposal only if the approved role, fallback, loop independence, or real-process evidence boundary must change.
- Design issue: return to design if config schema, role boundaries, transport seams, loop lifecycles, scope paths, or validation seams are incomplete.
- Implementation issue: return to implementation if code does not satisfy approved design or violates admitted scope paths.
- Testing issue: return to testing if runnable evidence does not cover approved success and failure paths.
- Acceptance issue: return to the owning stage named by the finding.

## Exit Condition
- [x] proposal approved
- [x] design approved
- [x] implementation admission passed
- [x] implementation completed
- [x] testing completed
- [ ] acceptance completed

## Evidence
- User approval and launch statement: `批准 sn-miner proposal 和 p2p-frame/sn-distributed-directory proposal，并启动 auto-pipeline 自动处理后续 design、implementation、testing、acceptance。`
- `sn-miner` proposal approval hash: `5c62d47ba0097b578627dc776964e86adb2d224e06bcc433c7d750d067a03644`.
- `p2p-frame/sn-distributed-directory` proposal approval hash: `17968fdefb7882fc64c292fe025c3131c3851157bcb48f9e2e56ffe259f025d0`.
- Initial `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module sn-miner --docs proposal` passed.
- Initial `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --docs proposal` passed.
- Initial `python3 ./harness/scripts/schema-check.py --version v0.1 --module sn-miner` passed.
- Initial `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory` failed because pre-existing `testing.md` approval hash is stale; this pipeline must refresh testing before final acceptance.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module sn-miner --docs design` passed.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --docs design` passed.
- Design approval hash for `sn-miner`: `9bfa9a442900e478c87e461c6ed64a2c7e8ffb84a9380be472b86feff9840f4f`.
- Design approval hash for `p2p-frame/sn-distributed-directory`: `94f3fe3cb83a161c353173747955a0b400726fc8bc0cbe76acabdb1f6dc176a1`.
- Isolated design validation worktree `/tmp/p2p-design-scope-R0xWVp` recorded `python3 ./harness/scripts/stage-scope-check.py --stage design --version v0.1 --module sn-miner --ignore-untracked` passed with one changed path.
- Isolated design validation worktree `/tmp/p2p-design-p2p-scope-11qnYW` recorded `python3 ./harness/scripts/stage-scope-check.py --stage design --version v0.1 --module p2p-frame --submodule sn-distributed-directory --ignore-untracked` passed with one changed path.
- Isolated design validation also recorded `schema-check.py` passing for both affected packets after applying the proposal/pipeline baseline and final design docs.
- Implementation admission evidence: `harness/evidence/admission/20260622-sn-miner-real-process.md`.
- Implementation admission stamps: `harness/evidence/admission/20260622-sn-miner-real-process.sn-miner.stamp.json`; `harness/evidence/admission/20260622-sn-miner-real-process.p2p-frame.sn-distributed-directory.stamp.json`.
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module sn-miner --change-id sn_miner_startup_audit --change-id sn_miner_artifact_defaults --change-id sn_miner_config_file_roles --change-id sn_miner_owner_role_startup --change-id sn_miner_serving_role_startup --change-id sn_miner_role_exclusive_validation --evidence-file harness/evidence/admission/20260622-sn-miner-real-process.md` passed.
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_directory_no_registry_fallback --change-id sn_serving_online_route_independent_loops --change-id sn_real_process_owner_serving_dv --evidence-file harness/evidence/admission/20260622-sn-miner-real-process.md` passed.
- `cargo check -p sn-miner -p p2p-frame` passed after implementation.
- Static search `rg -n "InterSnRegistry::global\\(\\)\\.(get|register)|OwnerServingRegistry::global\\(\\)\\.(get|register)" p2p-frame/src/sn sn-miner-rust/src -S` returned no matches.
- Isolated implementation validation worktree `/tmp/p2p-impl-sn-scope-iU9LCy` recorded `python3 ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module sn-miner --change-id sn_miner_startup_audit --change-id sn_miner_artifact_defaults --change-id sn_miner_config_file_roles --change-id sn_miner_owner_role_startup --change-id sn_miner_serving_role_startup --change-id sn_miner_role_exclusive_validation --ignore-untracked` passed with one changed path.
- Isolated implementation validation worktree `/tmp/p2p-impl-p2p-scope-QbVgYx` recorded `python3 ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_directory_no_registry_fallback --change-id sn_serving_online_route_independent_loops --change-id sn_real_process_owner_serving_dv --ignore-untracked` passed with four changed paths.
- `cargo test -p p2p-frame` passed with 148 unit tests.
- `cargo test -p sn-miner` passed with 3 `real_process` tests.
- `python3 ./harness/scripts/test-run.py sn-miner unit` passed; artifact `test-results/test-runs/20260622T141541Z-sn-miner-unit.json`.
- `python3 ./harness/scripts/test-run.py sn-miner dv` passed; artifact `test-results/test-runs/20260622T141618Z-sn-miner-dv.json`.
- `python3 ./harness/scripts/test-run.py sn-miner integration` passed; artifact `test-results/test-runs/20260622T141739Z-sn-miner-integration.json`.
- `python3 ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory unit` passed; artifact `test-results/test-runs/20260622T142709Z-p2p-frame+sn-distributed-directory-unit.json`.
- `python3 ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory dv` passed; artifact `test-results/test-runs/20260622T142732Z-p2p-frame+sn-distributed-directory-dv.json`.
- `python3 ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory integration` passed; artifact `test-results/test-runs/20260622T142751Z-p2p-frame+sn-distributed-directory-integration.json`.
- Testing approval hash for `sn-miner`: `897bf88a8bfc3bd6c9f3a5f9bfd4b3162157093ed528208f9dd454add954f68b`.
- Testing approval hash for `p2p-frame/sn-distributed-directory`: `55773c1346aef0702acf8c239a61f9fb399a7d42dbe7e61fac57d2ecf9e56442`.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module sn-miner --docs testing` passed.
- `python3 ./harness/scripts/testing-coverage-check.py --version v0.1 --module sn-miner` passed.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame/sn-distributed-directory --docs testing` passed.
- `python3 ./harness/scripts/testing-coverage-check.py --version v0.1 --module p2p-frame/sn-distributed-directory` passed.
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module sn-miner` passed after testing approval.
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory` passed after testing approval.
- Isolated testing validation worktree `/tmp/p2p-testing-scope-sn` recorded `python3 ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module sn-miner` passed with three changed paths.
- Isolated testing validation worktree `/tmp/p2p-testing-scope-dir` recorded `python3 ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --submodule sn-distributed-directory` passed with four changed paths for external testing artifacts. Main-worktree p2p testing scope also included two Rust `#[cfg(test)]` inline-test updates in `p2p-frame/src/sn/directory/server.rs` and `p2p-frame/src/sn/service/service.rs`; the file-level scope checker reports those as production-path violations even though the changed hunks are test-only.
- `python3 ./harness/scripts/quality-check.py` passed; no quality gates are configured.
- `python3 ./harness/scripts/test-run.py all all` generated `test-results/test-runs/20260622T143945Z-all-all.json`; all reviewed sn-miner and p2p-frame/sn-distributed-directory test steps passed, but the artifact exit code is `1` because an unrelated workspace-harness admission step invoked `admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive` without `--evidence-file`.
- Acceptance report: `docs/versions/v0.1/reviews/sn-miner-real-process-auto-pipeline-acceptance-2026-06-22.md`.
- `python3 ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/sn-miner-real-process-auto-pipeline-acceptance-2026-06-22.md` passed.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
| PREEXISTING-P2P-TESTING-STALE-HASH | testing | T-SN-MINER-REAL-PROC-1 | Packet-level schema currently fails because `p2p-frame/sn-distributed-directory/testing.md` has a stale `approved_content_sha256`. | Refresh post-implementation testing artifacts for this pipeline, run testing checks, and auto-confirm testing with a fresh hash. |
| ALL-ALL-ENDPOINT-AREA-ADMISSION-MISSING-EVIDENCE | testing | A-SN-MINER-REAL-PROC-1 | Required whole-project `test-run.py all all` evidence fails in workspace-harness integration because the unrelated `endpoint_area_server_reflexive` admission command lacks `--evidence-file`. | Fix or scope the workspace-harness all-run admission entry so `test-run.py all all` produces a fresh passing artifact, then rerun acceptance. |
