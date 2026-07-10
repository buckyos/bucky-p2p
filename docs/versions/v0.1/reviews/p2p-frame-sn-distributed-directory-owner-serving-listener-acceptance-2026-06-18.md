# P2P Frame SN Distributed Directory Owner Serving Listener Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-001 | high | implementation/testing | `stage-scope-check.py --stage implementation --change-id sn_owner_serving_listener_transport`, `stage-scope-check.py --stage implementation --change-id sn_owner_directory_peer_transport`, and `stage-scope-check.py --stage testing` returned scope violations | Current worktree contains many changed paths outside this task and outside the admitted design scope, so the implementation/testing diff cannot be mechanically bound to this change alone. | implementation diff was not bound to admitted design Scope Paths; single-stage scope checks failed |
| F-002 | medium | testing | `testing.md` and `testplan.yaml` record DV/integration as disabled for these changes | Unit tests cover command dispatch and client connector selection, but no real multi-process owner-to-owner `NetManager + TtpNode + DefaultCmdNodeService` lifecycle or Serving SN -> OwnerDirectoryServer `NetManager + TtpServer + DefaultCmdServerService` lifecycle is runnable yet. | accepted conclusion cannot rely on explicit lifecycle/cross-module gaps |

## Object and Scope
- Module: `p2p-frame`
- Version: `v0.1`
- change_id values reviewed: `sn_owner_directory_peer_transport`, `sn_owner_serving_listener_transport`
- Review date: 2026-06-19
- In scope: OwnerDirectoryServer owner peer command node service, serving-facing command listener, serving-side owner directory client command path, testing metadata, admission evidence.
- Out of scope: unrelated dirty worktree paths, pre-existing TTP/node/PN/SN changes not admitted under this change_id, full workspace acceptance.

## Optional Diff / Status Evidence
- `git status --short` summary: dirty worktree includes this task plus many pre-existing/unrelated docs, harness, TTP, PN, cyfs-p2p-test, and review files.
- `git diff --stat` summary: targeted diff query showed testing/testplan and `p2p-frame/src/sn/service/service.rs`; server/client files are also changed in the dirty index/worktree for this feature.
- `git diff --name-status` summary: targeted query showed modified `testing.md`, `testplan.yaml`, and `p2p-frame/src/sn/service/service.rs`; broader status shows additional SN directory files changed.
- `git diff --check` result: passed for the files relevant to this task.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| OwnerDirectoryServer owner peer command path uses `NetManager + TtpNode + DefaultCmdNodeService` | `proposal.md` P-SN-DIST-OWNER-PEER-TRANSPORT-1; `design.md` `sn_owner_directory_peer_transport` | `p2p-frame/src/sn/inter_sn/mod.rs` creates `DefaultCmdNodeService`, registers owner command handlers, accepts owner command tunnels, and sends owner requests through the command node service. | `test-results/test-runs/20260619T042228Z-p2p-frame+sn-distributed-directory-unit.json` includes owner command code mapping plus heartbeat and publish dispatch tests. | implemented |
| OwnerDirectoryServer serving-facing listener uses `NetManager + TtpServer + DefaultCmdServerService` | `proposal.md` P-SN-DIST-OWNER-SERVING-LISTENER-1; `design.md` `sn_owner_serving_listener_transport` | `p2p-frame/src/sn/directory/server.rs` adds constructor-time dual listen resource creation, `attach_serving_listener`, serving purpose, command codes, `OwnerServingTunnelListener`, and `DefaultCmdServerService` handler registration. | `test-results/test-runs/20260619T042228Z-p2p-frame+sn-distributed-directory-unit.json` includes `owner_serving_listener_dispatches_publish` and reject case. | implemented |
| Serving SN publish/query must not use owner peer listener or same-process shortcut when transport is configured | `proposal.md` non-goal and success evidence for P-SN-DIST-OWNER-SERVING-LISTENER-1; `design.md` Key Call Flows | `p2p-frame/src/sn/directory/client.rs` adds serving connector-backed `DefaultCmdClient` path; `p2p-frame/src/sn/service/service.rs` injects a serving connector when owner membership is configured. | `test-results/test-runs/20260619T042228Z-p2p-frame+sn-distributed-directory-unit.json` includes `owner_serving_listener_client_uses_serving_connector`. | implemented |
| Owner peer transport remains isolated from serving-facing command path | `design.md` Invariants and Directly Mapped Change Items | Owner peer path remains in `sn_inter_service` as `NetManager + TtpNode + DefaultCmdNodeService`; serving command ids are separate `OwnerServingCommandCode`; `OwnerDirectoryServer::new` creates separate owner-peer and serving net managers/listeners from endpoint parameters. | Existing `sn_distributed_directory` targeted step in `test-results/test-runs/20260619T042228Z-p2p-frame+sn-distributed-directory-unit.json` still passes. | implemented |
| Real dual listener lifecycle and cross-process serving transport | `proposal.md` trigger matrix runtime/integration; `design.md` Testing Strategy and Key Call Flows | API and unit-level command path exist, but no DV/integration harness starts real independent owner peer and serving-facing endpoints. | `testing.md` and `testplan.yaml` record DV/integration disabled gaps for this change. | missing |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `sn_owner_directory_peer_transport` owner command node dispatch | normal / boundary / negative / error / compatibility | `testing.md` maps the change to `VAL-SN-OWNER-PEER-TRANSPORT-UNIT` and `sn-distributed-directory-unit`. | `test-results/test-runs/20260619T042228Z-p2p-frame+sn-distributed-directory-unit.json` passed; targeted cargo test also passed with 9 tests. | adequate |
| `sn_owner_serving_listener_transport` serving command dispatch | normal / boundary / negative / error / compatibility | `testing.md` maps the change to `VAL-SN-OWNER-SERVING-LISTENER-UNIT` and `sn-owner-serving-listener-unit`. | `test-results/test-runs/20260619T042228Z-p2p-frame+sn-distributed-directory-unit.json` passed; targeted cargo test also passed with 3 tests. | adequate |
| `sn_owner_serving_listener_transport` serving-side client connector selection | normal / compatibility | `testing.md` Unit Tests row for `StaticOwnerDirectoryClient::publish_serving_lease` records configured connector coverage. | `owner_serving_listener_client_uses_serving_connector` passed in `test-results/test-runs/20260619T042228Z-p2p-frame+sn-distributed-directory-unit.json`. | adequate |
| `sn_owner_serving_listener_transport` constructor-time listen setup | lifecycle / config | `testing.md` records constructor-time dual listen setup as a DV/integration gap because it needs real OS ports. | `cargo check -p p2p-frame` passed; no real dual-port run artifact exists. | gap |
| owner peer and serving real port lifecycle and cross-module transport | lifecycle / cross-module | `testing.md` and `testplan.yaml` explicitly record disabled DV/integration reasons with owner, risk, and acceptance impact. | No runnable DV/integration artifact exists for real OwnerDirectoryServer -> OwnerDirectoryServer `NetManager + TtpNode + DefaultCmdNodeService` or Serving SN -> OwnerDirectoryServer `NetManager + TtpServer + DefaultCmdServerService` transport. | gap |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-001 | proposal/design | OwnerDirectoryServer `new` receives listen endpoint parameters and creates an owner-peer `NetManager + TtpNode + DefaultCmdNodeService` path plus a serving-facing `NetManager + TtpServer + DefaultCmdServerService` command entrypoint. | Code inspection plus targeted unit evidence. | pass |
| AR-002 | proposal non-goal | Serving SN publish/query must not be handled by SnServer listener, owner peer listener, or same-process shortcut when transport is configured. | Client connector implementation and connector-selection unit test. | pass |
| AR-003 | acceptance rules | Implementation and testing changes must pass stage scope checks for the reviewed change. | `stage-scope-check.py` implementation/testing outputs. | fail |
| AR-004 | test design rules | Lifecycle and cross-module behavior must be covered by runnable DV/integration or explicit gaps. | `testing.md`, `testplan.yaml`, and test-run artifact. | gap |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `p2p-frame/src/sn/directory/server.rs`
- `p2p-frame/src/sn/directory/client.rs`
- `p2p-frame/src/sn/service/service.rs`
- `harness/evidence/admission/20260618-owner-serving-listener-transport.md`
- `harness/rules/acceptance-review-rules.md`
- `harness/evidence/admission/20260618-owner-directory-peer-transport.md`
- `test-results/test-runs/20260619T042228Z-p2p-frame+sn-distributed-directory-unit.json`

## Review Order
1. Review approved requirements and acceptance boundaries.
2. Review design against proposal.
3. Generate or finalize acceptance rules and expected results from proposal, design, implementation, and test implementation.
4. Review implementation against proposal and design.
5. Review whether test design reasonably covers proposal/design/code behavior, including normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases where applicable.
6. Review tests and results against generated test evidence, optional `testing.md`, and required completed-testing `testplan.yaml` or its versioned exception.
7. Review document and implementation logic for contradictions, invalid assumptions, impossible states, and correctness defects.
8. Use diff/status output only when helpful to locate evidence.
9. Produce conclusion.

## Consistency Summary
- Proposal authority check: passed; `schema-check.py` verified approved proposal/design hashes for the submodule.
- Proposal vs design: consistent for the reviewed changes; design maps P-SN-DIST-OWNER-PEER-TRANSPORT-1 to `sn_owner_directory_peer_transport` and P-SN-DIST-OWNER-SERVING-LISTENER-1 to `sn_owner_serving_listener_transport`.
- Design vs testing implementation: mostly consistent; testing now maps the unit-level command and client connector coverage, with lifecycle/cross-module gaps explicit.
- Design vs long-lived boundary doc: no long-lived module boundary change was required for this narrow implementation pass.
- Design vs implementation: consistent for command separation and serving-facing command path; real startup lifecycle remains unproven by DV/integration.
- Test implementation vs test code vs results: consistent; `test-run.py p2p-frame/sn-distributed-directory unit` passed and wrote a fresh artifact.
- Test design adequacy: unit coverage is adequate for changed branches reviewed here; DV/integration remains a recorded gap.
- change_id traceability: passed through proposal, design, admission evidence, `testing.md`, and `testplan.yaml`.
- Acceptance criteria traceability: partial; command-layer behavior is evidenced, full real dual-port lifecycle is not.
- Cross-module admission: not accepted as final because stage-scope checks saw unrelated changed paths outside admitted scope.
- Public API / codec / runtime semantics review: internal serving command ids are disjoint; no client-visible SN command shape change observed.
- Document logic review: no contradiction found for the reviewed command-layer design; testing document is draft after regeneration and not an approved artifact.
- Implementation logic review: serving connector path avoids registry shortcut when configured; owner command node dispatch tests confirm `DefaultCmdNodeService` command mapping and handler side effects.
- Document approval timing (approved_content_sha256 verified by schema-check): proposal/design passed schema check; regenerated `testing.md` is draft and not approval evidence.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): failed due dirty worktree and outside-scope paths.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not applicable; this is feature implementation, not a bugfix.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory`: passed via `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory`.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_owner_serving_listener_transport --evidence-file harness/evidence/admission/20260618-owner-serving-listener-transport.md`: passed via `python3` and rewrote admission stamp.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_owner_directory_peer_transport --evidence-file harness/evidence/admission/20260618-owner-directory-peer-transport.md`: passed via `python3` and rewrote admission stamp.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage <stage>`: failed for implementation and testing scope because current worktree contains many unrelated dirty paths.
- Targeted cargo tests: `cargo test -p p2p-frame owner_serving_listener -- --nocapture` passed with 3 tests; `cargo test -p p2p-frame sn_distributed_directory -- --nocapture` passed with 9 tests.
- Cargo check: `cargo check -p p2p-frame` passed.
- Testing coverage checks: `testing-coverage-check.py --change-id sn_owner_serving_listener_transport` and `testing-coverage-check.py --change-id sn_owner_directory_peer_transport` passed.
- Unit test command through `harness/scripts/test-run.py`: passed with `python3 ./harness/scripts/test-run.py p2p-frame/sn-distributed-directory unit`; artifact `test-results/test-runs/20260619T042228Z-p2p-frame+sn-distributed-directory-unit.json`.
- DV test command through `harness/scripts/test-run.py`: disabled in `testplan.yaml` with owner/risk/acceptance impact reason.
- Integration test command through `harness/scripts/test-run.py`: disabled in `testplan.yaml` with owner/risk/acceptance impact reason.
- Module all command through `harness/scripts/test-run.py <module> all`: not run for this needs-changes acceptance because scope gates already failed; submodule unit artifact is cited.
- Project all command through `harness/scripts/test-run.py all all`: not run for this needs-changes acceptance because scope gates already failed.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): not available; conclusion is needs changes.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed via `python3 ./harness/scripts/quality-check.py`; repository declares an explicitly empty gates list.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not generated because no quality gates are configured.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not run for this needs-changes acceptance because scope gates already failed.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: passed via `python3 ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-sn-distributed-directory-owner-serving-listener-acceptance-2026-06-18.md`.
- Targeted migration search, when applicable: `rg "attach_serving_listener|OwnerServingCommandCode|new_with_serving_connector"` used during code audit; no public client-visible SN command migration found.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: command-layer implementation and unit evidence are present, but mechanical scope gates failed and real lifecycle/cross-module transport remains an explicit testing gap.
- Supporting test evidence: `test-results/test-runs/20260619T042228Z-p2p-frame+sn-distributed-directory-unit.json`.
- Residual risk: real owner peer and owner serving listener port startup, shutdown, port conflict, and cross-process command exchange are not proven by DV/integration.

## Follow-Up Tasks
- Requirement task: none for the command-layer behavior; proposal already covers the requested listener separation.
- Design task: consider adding a concrete startup/config assembly design for owner-directory dual ports if future work wires a full owner-directory process role.
- Implementation task: isolate this change from unrelated dirty worktree paths so implementation scope check can pass.
- Testing task: add DV/integration coverage for real OwnerDirectoryServer -> OwnerDirectoryServer `NetManager + TtpNode + DefaultCmdNodeService` and Serving SN -> OwnerDirectoryServer `NetManager + TtpServer + DefaultCmdServerService` lifecycle and failure semantics.
- Testing return reason if coverage is incomplete: lifecycle and cross-module cases are explicit gaps and block accepted conclusion.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
