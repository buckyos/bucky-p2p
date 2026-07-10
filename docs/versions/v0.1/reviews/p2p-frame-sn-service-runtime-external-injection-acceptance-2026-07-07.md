# p2p-frame SN Service Runtime External Injection Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | completed review evidence for proposal, design, admission, implementation, testing, all-run artifacts, and quality check | no blocking finding recorded | none |

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- change_id values reviewed: sn_service_runtime_external_injection
- Review date: 2026-07-07
- In scope: SN service runtime ownership, `create_sn_service(...)` error boundary, direct workspace callers, test coverage, admission evidence, and no service-local runtime fallback.
- Out of scope: SN command wire, control-stream-only signaling, TCP/QUIC tunnel wire, TLS identity checks, endpoint classification, SN connection validator semantics, and TCP source address filtering.

## Optional Diff / Status Evidence
- `git status --short` summary: task files modified under p2p-frame SN service/tests, sn-miner-rust, cyfs-p2p-test, Cargo lock/dependencies, p2p-frame module docs, admission evidence, pipeline plan, and this acceptance report; pre-existing unrelated untracked review/evidence files remain present.
- `git diff --stat` summary: 13 tracked files changed before acceptance report/pipeline finalization, 176 insertions and 89 deletions, plus new task evidence/report files.
- `git diff --name-status` summary: modified `Cargo.lock`, `cyfs-p2p-test/Cargo.toml`, `cyfs-p2p-test/src/main.rs`, p2p-frame proposal/design/testing/testplan docs, `harness/pipeline-plan.md`, `p2p-frame/src/sn/service/service.rs`, SN tests, and sn-miner files.
- `git diff --check` result: passed with no whitespace errors.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| SN service must not create a service-local `ServerRuntime` fallback | proposal item `P-SN-SERVICE-RUNTIME-EXTERNAL-1`; design Directly Mapped Change Items | `p2p-frame/src/sn/service/service.rs` now requires `config.server_runtime` and returns `P2pResult<SnServerRef>`; `rg -n "ServerRuntime::start|ServerRuntimeConfig" p2p-frame/src/sn/service/service.rs` returned no matches | targeted unit and p2p-frame/all-all runs passed | implemented |
| Missing runtime must be an explicit configuration error | proposal acceptance notes; design runtime API/error boundary | `create_sn_service(...)` returns `P2pErrorCode::InvalidParam` when `SnServiceConfig` omits runtime | `cargo test -p p2p-frame create_sn_service_requires_external_server_runtime -- --nocapture` passed | implemented |
| Workspace direct callers must pass a process or stack level runtime | design scope paths for `sn-miner-rust` and `cyfs-p2p-test` | `sn-miner-rust/src/main.rs` and `cyfs-p2p-test/src/main.rs` create `ServerRuntime::start(ServerRuntimeConfig::default())` outside SN service and call `set_server_runtime(...)`; both crates declare `sfo-reuseport = "0.3"` | `cargo test --workspace --no-run`, `test-run.py all all`, and `./test-run.sh all all` passed | implemented |
| SN protocol and adjacent behavior must remain unchanged | proposal non-goals and design rollback notes | implementation is limited to runtime injection/error handling and caller migration; no SN command/protocol/validator/endpoint logic was changed | p2p-frame unit/integration groups, SN client, SN distributed directory, 5x5 command matrix, sn-miner real_process, and workspace tests passed | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| sn_service_runtime_external_injection | normal / boundary / negative / error / compatibility / lifecycle / cross-module | `testing.md` maps unit and compatibility coverage for explicit runtime injection, missing-runtime error, no service-local fallback, and direct caller migration | targeted unit, `test-run.py p2p-frame unit`, `test-run.py p2p-frame all`, `test-run.py all all`, and `./test-run.sh all all` passed | adequate |
| Missing external runtime regression | negative / error / lifecycle | `testing.md` defines `V-SN-SERVICE-RUNTIME-EXTERNAL-UNIT` for missing `ServerRuntime` returning `InvalidParam` | `create_sn_service_requires_external_server_runtime` passed in targeted and all runs | adequate |
| Cross-module startup compatibility | compatibility / cross-module | `testing.md` defines `V-SN-SERVICE-RUNTIME-EXTERNAL-COMPAT`; `testplan.yaml` requires sn-miner-rust and cyfs-p2p-test callers to pass runtime explicitly | workspace compile and all-all artifacts passed after caller migration | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-SN-RUNTIME-001 | proposal/design/code | `service.rs` contains no production `ServerRuntime::start(...)` fallback | source search plus implementation review | pass |
| AR-SN-RUNTIME-002 | proposal/design/tests | missing `SnServiceConfig.server_runtime` returns an explicit p2p config error | focused unit test and all-run evidence | pass |
| AR-SN-RUNTIME-003 | design/code/tests | direct workspace callers create or receive runtime outside SN service and pass it via `set_server_runtime(...)` | code review, workspace compile, all-run evidence | pass |
| AR-SN-RUNTIME-004 | proposal/design/tests | no unrelated SN wire, validator, endpoint, or TCP source behavior changes are required or observed | diff review plus p2p-frame/SN/matrix/workspace test evidence | pass |

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
- Proposal authority check: approved proposal item `P-SN-SERVICE-RUNTIME-EXTERNAL-1` requires external or stack-level `ServerRuntime` and forbids SN service fallback runtime creation.
- Proposal vs design: design maps the proposal to `SnServiceConfig::set_server_runtime(...)`, `create_sn_service(...) -> P2pResult<SnServerRef>`, explicit missing-runtime error, and direct caller migration.
- Design vs testing implementation: `testing.md` and `testplan.yaml` cover missing-runtime error, explicit runtime injection, source search, workspace caller migration, and compatibility tests.
- Design vs long-lived boundary doc: change remains inside p2p-frame SN service/startup assembly boundary and does not redefine SN wire or network abstractions.
- Design vs implementation: implementation follows the approved scope paths, removes the service-local fallback, propagates a configuration error, and migrates sn-miner-rust/cyfs-p2p-test callers.
- Test implementation vs test code vs results: targeted unit, p2p-frame unit/all, whole-project all, and root shortcut all results are consistent and passing.
- Test design adequacy: normal, boundary, negative, error, compatibility, lifecycle, and cross-module coverage are mapped in testing docs and backed by runnable evidence.
- change_id traceability: `sn_service_runtime_external_injection` appears in proposal, design, admission evidence, testing coverage, testplan, and acceptance evidence.
- Acceptance criteria traceability: acceptance criteria for no internal runtime start, explicit missing-runtime error, and caller migration are each covered by code review and tests.
- Cross-module admission: admission scope includes p2p-frame SN paths, sn-miner-rust, cyfs-p2p-test, and `Cargo.lock`; admission check passed.
- Public API / codec / runtime semantics review: API return type changed to explicit `P2pResult`, runtime ownership is externalized, and codec/wire behavior is unchanged.
- Document logic review: no contradiction found between proposal, design, testing, and admission scope after downstream caller scope expansion.
- Implementation logic review: no service-local runtime fallback remains; error path is deterministic; caller-owned runtimes are constructed at startup boundaries.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed after approved proposal/design/testing metadata updates.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): implementation scope check passed for `sn_service_runtime_external_injection`.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: targeted regression test fails the old fallback expectation by requiring `InvalidParam`; after implementation it passes.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`: passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_service_runtime_external_injection --evidence-file harness/evidence/admission/20260707-sn-service-runtime-external.md`: passed; stamp `harness/evidence/admission/20260707-sn-service-runtime-external.p2p-frame.stamp.json`.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --change-id sn_service_runtime_external_injection --ignore-untracked`: passed; design and testing scope checks also passed for their stages.
- Unit test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260707T031513Z-p2p-frame-unit.json`.
- DV test command through `harness/scripts/test-run.py`: p2p-frame has no DV tests; `test-run.py p2p-frame all` recorded `test-run: no dv tests for p2p-frame` and passed.
- Integration test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py p2p-frame all` passed integration groups; artifact `test-results/test-runs/20260707T032627Z-p2p-frame-all.json`.
- `test-run.py <module> all` (`uv run --active python ./harness/scripts/test-run.py p2p-frame all`): passed; artifact `test-results/test-runs/20260707T032627Z-p2p-frame-all.json`.
- `test-run.py all all` (`uv run --active python ./harness/scripts/test-run.py all all`): passed; artifact `test-results/test-runs/20260707T034122Z-all-all.json`.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): `test-results/test-runs/20260707T034840Z-all-all.json` from `./test-run.sh all all`, exit_code 0.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed; repository declares `gates: []` in `harness/quality-gates.yaml`.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not required because quality gates are explicitly empty.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): `./test-run.sh all all` passed; artifact `test-results/test-runs/20260707T034840Z-all-all.json`.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: passed for `docs/versions/v0.1/reviews/p2p-frame-sn-service-runtime-external-injection-acceptance-2026-07-07.md`.
- Targeted migration search, when applicable: `rg -n "ServerRuntime::start|ServerRuntimeConfig" p2p-frame/src/sn/service/service.rs` returned no matches.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: implementation satisfies the approved runtime ownership change, required callers now inject runtime externally, and required regression/compatibility/all-run evidence passed.
- Supporting test evidence: targeted unit, `p2p-frame unit`, `p2p-frame all`, `test-run.py all all`, `./test-run.sh all all`, workspace compile, source search, admission, stage-scope, and quality checks passed.
- Residual risk: direct callers outside this workspace must migrate to pass `ServerRuntime`; this is an intended API compatibility impact and is documented in design/testing.

## Follow-Up Tasks
- Requirement task: no follow-up required.
- Design task: no follow-up required.
- Implementation task: no follow-up required.
- Testing task: no follow-up required.
- Testing return reason if coverage is incomplete: coverage complete for accepted scope.
- Iteration count: 2
- Stop reason if more than 5 unsuccessful iterations: not applicable.
