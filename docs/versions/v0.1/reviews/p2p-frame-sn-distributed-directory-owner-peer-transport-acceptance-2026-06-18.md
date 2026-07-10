# p2p-frame SN Distributed Directory Owner Peer Transport Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-OWNER-PEER-SCOPE-001 | high | implementation | `stage-scope-check.py --stage implementation --change-id sn_owner_directory_peer_transport` and `--change-id sn_owner_serving_role_boundary` failed | The current worktree is not mechanically isolated to the reviewed owner peer transport changes. The check reports many pre-existing or cross-stage dirty paths and also reports `p2p-frame/src/sn/protocol/sn.rs` outside the two admitted Scope Paths used for this implementation slice. | implementation diff was not bound to admitted design Scope Paths |
| F-OWNER-PEER-TEST-001 | medium | testing | tests cover command mapping/dispatch and module unit entrypoint, but no real multi-owner `TtpNode` network scenario was run | Unit evidence verifies `DefaultCmdNode` command dispatch, heartbeat command dispatch, and role boundary behavior, but does not prove two live `OwnerDirectoryServer` instances exchange commands over real `TtpNode` tunnels. | runtime/integration coverage gap for owner peer transport |

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- change_id values reviewed: `sn_owner_directory_peer_transport`, `sn_owner_serving_role_boundary`
- Review date: 2026-06-18
- In scope: OwnerDirectoryServer peer transport design updates, `TtpNode` listener/connect adapter, `sfo_cmd_server::DefaultCmdNode` command dispatch, heartbeat/publish/query/detail/relay command mapping, removal of owner peer transport responsibility from `SnServer`.
- Out of scope: unrelated dirty files already present in the worktree, full multi-process SN deployment, root-level all-module validation.

## Optional Diff / Status Evidence
- `git status --short` summary: mixed dirty worktree includes the reviewed SN files plus unrelated harness, docs, QUIC, TTP, cyfs-p2p-test, sn-miner, and test-run shortcut paths.
- `git diff --stat` summary for reviewed paths: `sn-distributed-directory` proposal/design plus `p2p-frame/src/sn/inter_sn/mod.rs` and `p2p-frame/src/sn/service/service.rs` carry the main diff; `p2p-frame/src/sn/protocol/sn.rs` also carries internal command payload/code additions.
- `git diff --name-status` summary: not run separately; status and stage-scope output were sufficient to locate scope problems.
- `git diff --check` result: passed for the reviewed doc/code/evidence paths.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| concrete TTP rules live in design, not proposal | user instruction plus `proposal.md` / `design.md` approval metadata | proposal no longer contains `TtpNode`, `DefaultCmdNode`, `SnCmdService`, `TtpClient`, or `TtpServer`; design records these constraints | `schema-check.py` passed | implemented |
| OwnerDirectoryServer uses `TtpNode` for owner peer transport | `design.md` Owner peer transport and command node | `OwnerDirectoryServer::attach_peer_transport(TtpNodeRef, ...)`; `OwnerTunnelFactory` calls `TtpNode.open_control_stream`; `OwnerTunnelListener` calls `TtpNode.listen_control_stream` | `cargo check -p p2p-frame`; unit artifact `test-results/test-runs/20260618T094421Z-p2p-frame-unit.json` | implemented |
| owner commands use `sfo_cmd_server::DefaultCmdNode` | `design.md` Owner directory command node | `TtpInterSnClient::new` creates `DefaultCmdNode`; owner streams are adapted into `CmdTunnel<SnTunnelRead, SnTunnelWrite>` | `sn_distributed_directory_maps_requests_to_owner_cmd_codes`; `sn_distributed_directory_owner_cmd_dispatches_heartbeat`; `sn_distributed_directory_owner_cmd_dispatches_publish_lease` | implemented |
| heartbeat command updates OwnerMember validity after validation | `design.md` command handler and heartbeat requirements | `SnOwnerHeartbeat`, `InterSnCommandCode::Heartbeat`, `OwnerDirectoryService::heartbeat_from_sn`; publish/query refresh happens after command validation | filtered `cargo test -p p2p-frame --lib sn_distributed_directory` passed; unit artifact passed | implemented |
| `TtpClient` / `TtpServer` do not replace owner peer transport | `design.md` Non-goals and Key Decisions | owner peer path references `TtpNodeRef` and `DefaultCmdNode`; `SnServer` no longer starts an owner/inter-SN accept loop with `TtpServer`; remaining `TtpServer` usage is ordinary serving SN listener | targeted `rg` showed `TtpServer` only in `service.rs` serving listener/test paths, not `OwnerDirectoryServer` peer transport | implemented |
| implementation scope isolation | task-entry and acceptance rules | admission checks passed for two change ids, but stage-scope failed because the worktree contains mixed unrelated paths and protocol changes outside these two Scope Paths | stage-scope output for both change ids failed | inconsistent |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `sn_owner_directory_peer_transport` command mapping/dispatch | normal / negative by mismatch path / codec compatibility | inline unit tests in `sn/inter_sn/mod.rs` cover command code mapping, heartbeat dispatch, publish dispatch; command/payload mismatch returns `InterSnResponse::Error` by code inspection | `test-results/test-runs/20260618T094421Z-p2p-frame-unit.json` passed | partial |
| `sn_owner_serving_role_boundary` | normal / compatibility | `SnServer` keeps only serving `TtpServer` listener; owner peer transport attaches via `OwnerDirectoryServer`; role-boundary unit tests remain registered | `test-results/test-runs/20260618T094421Z-p2p-frame-unit.json` passed | adequate at unit level |
| real owner peer transport over `TtpNode` | lifecycle / runtime / cross-module / failure | design calls for `TtpNode` owner peer transport and multi-service command dispatch | no DV/integration run for two live OwnerDirectoryServer instances over TtpNode | gap |
| stage scope isolation | harness/process | admission evidence exists for both reviewed change ids | stage-scope implementation checks failed | gap |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-OWNER-PEER-001 | design | OwnerDirectoryServer owner peer transport uses `TtpNode`, not `TtpClient` or `TtpServer` | code inspection and targeted search | pass |
| AR-OWNER-PEER-002 | design | owner internal commands use `sfo_cmd_server::DefaultCmdNode` | code inspection and unit command dispatch tests | pass |
| AR-OWNER-PEER-003 | design | heartbeat, publish, query, detail, and relay command ids are registered through the owner command node | code inspection and unit tests for heartbeat/publish mapping | pass |
| AR-OWNER-PEER-004 | design | command handlers validate before owner store/health side effects | code inspection of `OwnerDirectoryService` | pass |
| AR-OWNER-PEER-005 | design/runtime | real owner peers can exchange commands over `TtpNode` | DV/integration with two live owner nodes | gap |
| AR-HARNESS-001 | task-entry and acceptance rules | implementation diff is mechanically isolated to admitted Scope Paths | passing stage-scope implementation checks | fail |

## Inputs
- `proposal.md`
- `design.md`
- test implementation and optional `testing.md`
- `testplan.yaml` for completed testing work
- optional `acceptance.md`: not used
- long-lived module doc: `docs/modules/p2p-frame.md`
- implementation: `p2p-frame/src/sn/directory/server.rs`, `p2p-frame/src/sn/inter_sn/mod.rs`, `p2p-frame/src/sn/protocol/sn.rs`, `p2p-frame/src/sn/service/service.rs`
- test code: inline Rust unit tests in `p2p-frame/src/sn/inter_sn/mod.rs` plus existing SN distributed directory/service tests
- test results: `test-results/test-runs/20260618T094421Z-p2p-frame-unit.json`
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
- Proposal authority check: approved proposal covers independent OwnerDirectoryServer peer transport at the outcome level without embedding implementation rules.
- Proposal vs design: consistent with the user's instruction that concrete `TtpNode` / `DefaultCmdNode` rules live in design only.
- Design vs testing implementation: unit-level command mapping and dispatch coverage exists; real `TtpNode` multi-owner runtime coverage is still a gap.
- Design vs long-lived boundary doc: p2p-frame owns `src/sn/**`; no long-lived contradiction found for owner peer transport.
- Design vs implementation: core implementation matches `TtpNode` + `DefaultCmdNode`; heartbeat command was added and side effects now occur after validation.
- Test implementation vs test code vs results: relevant inline unit tests are reachable through `python3 ./harness/scripts/test-run.py p2p-frame unit` and passed.
- Test design adequacy: partial; command-node behavior is covered at unit level, but owner-to-owner `TtpNode` lifecycle/runtime remains unverified.
- change_id traceability: admission passed for `sn_owner_directory_peer_transport` and `sn_owner_serving_role_boundary`; `p2p-frame/src/sn/protocol/sn.rs` command payload/code changes still need cleaner direct Scope Path binding.
- Acceptance criteria traceability: AR-OWNER-PEER-001 through AR-OWNER-PEER-004 pass; AR-OWNER-PEER-005 is a coverage gap; AR-HARNESS-001 fails.
- Cross-module admission: p2p-frame/sn-distributed-directory schema and admission checks passed for the reviewed change ids; mixed dirty worktree prevents clean stage-scope.
- Public API / codec / runtime semantics review: internal owner command code/payload changed; ordinary client-facing SN command protocol remains unchanged by this slice.
- Document logic review: no proposal/design contradiction found after moving concrete transport rules out of proposal and into design.
- Implementation logic review: owner command node registers heartbeat/publish/query/detail/relay; `SnServer` is not used as owner peer transport; publish/query heartbeat refresh now follows validator approval.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): failed for both reviewed change ids due mixed dirty paths and protocol path scope mismatch.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not-applicable; this is feature implementation, not a bugfix.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version <version> --module <module>`: `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory` passed.
- `uv run --active python ./harness/scripts/admission-check.py --version <version> --module <module> --change-id <change_id> --evidence-file harness/evidence/admission/<task-id>.md`: `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_owner_directory_peer_transport --evidence-file harness/evidence/admission/20260618-owner-directory-peer-transport.md` passed; `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --submodule sn-distributed-directory --change-id sn_owner_serving_role_boundary --evidence-file harness/evidence/admission/20260618-owner-serving-role-boundary.md` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage <stage>`: implementation stage-scope failed for both reviewed change ids; see F-OWNER-PEER-SCOPE-001.
- Unit test command through `harness/scripts/test-run.py`: `python3 ./harness/scripts/test-run.py p2p-frame unit` passed with artifact `test-results/test-runs/20260618T094421Z-p2p-frame-unit.json`.
- DV test command through `harness/scripts/test-run.py`: not run; owner-to-owner `TtpNode` runtime scenario remains a coverage gap.
- Integration test command through `harness/scripts/test-run.py`: not run; acceptance is already needs changes due stage-scope failure and runtime coverage gap.
- Module all command through `harness/scripts/test-run.py <module> all`: not run because acceptance is already needs changes.
- Project all command through `harness/scripts/test-run.py all all`: not run because acceptance is already needs changes.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): not available for this acceptance attempt.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: `python3 ./harness/scripts/quality-check.py` passed; `harness/quality-gates.yaml` declares `gates: []`.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not-applicable because no quality gates are configured and no quality artifact was written.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not run because acceptance is already needs changes.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: `python3 ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-sn-distributed-directory-owner-peer-transport-acceptance-2026-06-18.md` passed.
- Targeted migration search, when applicable: `rg -n "TtpClient|TtpServer|DefaultCmdNode|TtpNode|open_control_stream|listen_control_stream|Heartbeat" ...` showed owner peer transport uses `TtpNode` and `DefaultCmdNode`; `TtpServer` occurrences are in ordinary `SnServer` serving listener/test paths.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: the core OwnerDirectoryServer peer transport implementation now matches the approved design at code/unit-test level, but acceptance cannot pass while implementation stage-scope fails and real two-owner `TtpNode` runtime coverage is still missing.
- Supporting test evidence: `test-results/test-runs/20260618T094421Z-p2p-frame-unit.json`
- Residual risk: no DV/integration proof yet for two live OwnerDirectoryServer peers exchanging heartbeat and owner commands over real `TtpNode` tunnels.

## Follow-Up Tasks
- Requirement task: none identified for proposal wording.
- Design task: add or split direct Scope Path coverage for internal protocol command payload/code changes, or move those types under an already admitted technical scope.
- Implementation task: isolate unrelated dirty paths and rerun implementation stage-scope for `sn_owner_directory_peer_transport` and `sn_owner_serving_role_boundary`.
- Testing task: add a DV/integration scenario with two live OwnerDirectoryServer instances using `TtpNode` and `DefaultCmdNode` for heartbeat plus publish/query.
- Testing return reason if coverage is incomplete: runtime owner-to-owner transport behavior is not yet proven beyond unit-level command dispatch.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not-applicable
