# Pipeline Plan: SN Service Runtime External Injection

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "确认，自动处理后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): p2p-frame
- change_id values: sn_service_runtime_external_injection

## Acceptance Baseline
- Final acceptance uses the approved `p2p-frame` proposal as authority.
- SN service must consume a `sfo_reuseport::ServerRuntime` supplied by the caller or stack-level assembly.
- `p2p-frame/src/sn/service/service.rs` must not create a service-local runtime with `ServerRuntime::start(...)` or an equivalent fallback.
- Missing runtime must follow the design-defined API or error boundary instead of silently starting a new runtime.
- The implementation must not change SN command wire, control-stream-only signaling, TCP/QUIC tunnel wire, TLS identity checks, endpoint classification, SN connection validator semantics, or TCP source address filtering.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-SN-SERVICE-RUNTIME-EXTERNAL-1 | proposal | confirmed | Approve SN service external ServerRuntime ownership boundary | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | root | user approval | approved proposal | proposal doc checks, schema check, approval metadata, and launch evidence recorded |
| D-SN-SERVICE-RUNTIME-EXTERNAL-1 | design | confirmed | Define SN service runtime API/error boundary and stack-level ownership without changing SN protocol behavior | `docs/versions/v0.1/modules/p2p-frame/design.md` | root | P-SN-SERVICE-RUNTIME-EXTERNAL-1 | approved design | design doc checks, schema checks, stage-scope checks, and auto-pipeline approval metadata recorded |
| I-SN-SERVICE-RUNTIME-EXTERNAL-1 | implementation | complete | Remove SN service-local ServerRuntime creation and require external runtime injection | `p2p-frame/src/sn/service/service.rs` plus admission evidence | root | D-SN-SERVICE-RUNTIME-EXTERNAL-1 | implementation and admission evidence | schema/admission passed; implementation scope check result recorded; relevant build/unit checks pass |
| T-SN-SERVICE-RUNTIME-EXTERNAL-1 | testing | complete | Add focused coverage for explicit runtime injection and no service-local runtime fallback | relevant `p2p-frame/src/sn/**` tests; testing docs only if required by coverage gate | I-SN-SERVICE-RUNTIME-EXTERNAL-1 | I-SN-SERVICE-RUNTIME-EXTERNAL-1 | runnable unit evidence | relevant cargo tests pass; testing coverage checks pass; testing scope check result recorded |
| A-SN-SERVICE-RUNTIME-EXTERNAL-1 | acceptance | complete | Audit proposal/design/implementation/testing consistency and runnable evidence | `docs/versions/v0.1/reviews/` | root | T-SN-SERVICE-RUNTIME-EXTERNAL-1 | acceptance report | acceptance report check passed and pipeline exit conditions complete |

## Return Routing
- Proposal issue: return to proposal only if runtime ownership, missing-runtime behavior, or protocol non-goals must change.
- Design issue: return to design if the runtime API, stack ownership, scope paths, or compatibility boundary are incomplete.
- Implementation issue: return to implementation if SN service still creates a runtime internally, does not require or receive external runtime, or changes unrelated SN behavior.
- Testing issue: return to testing if runnable evidence does not cover explicit runtime injection, missing-runtime behavior, and code search for removed fallback.
- Acceptance issue: return to the owning stage named by the finding.

## Exit Condition
- [x] proposal approved
- [x] design approved
- [x] implementation admission passed
- [x] implementation completed
- [x] testing completed
- [x] acceptance completed

## Evidence
- User approval and launch statement: `确认，自动处理后续步骤`
- Proposal approval hash: `d9b2faa02356d04f89a1ee9d856461f47cf2a42f830949d8c74e20e92fee7342`
- Design approval hash: `dbf30508ba1e16677bcbf0fba3211c21bdf3a46f1057c8950af2e827a17f8650`
- Testing approval hash: `6fe48e8f439928923d5372b038fba794dec42b420ece11a0f44a1d096d88329f`
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs design` passed.
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage design --version v0.1 --module p2p-frame --ignore-untracked` passed with proposal and pipeline plan temporarily hidden from git status to isolate the design child task.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_service_runtime_external_injection --evidence-file harness/evidence/admission/20260707-sn-service-runtime-external.md` passed; stamp `harness/evidence/admission/20260707-sn-service-runtime-external.p2p-frame.stamp.json`.
- `cargo check -p p2p-frame --lib` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --change-id sn_service_runtime_external_injection --ignore-untracked` passed with proposal/design/plan temporarily hidden from git status to isolate the implementation child task.
- Code search confirmed `p2p-frame/src/sn/service/service.rs` no longer contains `ServerRuntime::start` or `ServerRuntimeConfig`.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs testing` passed.
- `uv run --active python ./harness/scripts/testing-coverage-check.py --version v0.1 --module p2p-frame --change-id sn_service_runtime_external_injection` passed.
- `cargo test -p p2p-frame create_sn_service_requires_external_server_runtime -- --nocapture` passed.
- `uv run --active python ./harness/scripts/test-run.py p2p-frame unit` passed; run artifact `test-results/test-runs/20260707T031513Z-p2p-frame-unit.json`.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --ignore-untracked` passed with proposal/design/pipeline plan and implementation-scoped `service.rs` temporarily hidden from git status to isolate the testing child task.
- Acceptance review returned once to design/implementation/testing because downstream `sn-miner-rust` and `cyfs-p2p-test` direct callers also needed explicit process-level `ServerRuntime` injection.
- `cargo test --workspace --no-run` passed after direct caller migration.
- `uv run --active python ./harness/scripts/test-run.py p2p-frame all` passed; run artifact `test-results/test-runs/20260707T032627Z-p2p-frame-all.json`.
- `uv run --active python ./harness/scripts/test-run.py all all` passed; run artifact `test-results/test-runs/20260707T034122Z-all-all.json`.
- `./test-run.sh all all` passed; run artifact `test-results/test-runs/20260707T034840Z-all-all.json`.
- `uv run --active python ./harness/scripts/quality-check.py` passed; quality gates are explicitly empty in `harness/quality-gates.yaml`.
- Acceptance report: `docs/versions/v0.1/reviews/p2p-frame-sn-service-runtime-external-injection-acceptance-2026-07-07.md`.
- `uv run --active python ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-sn-service-runtime-external-injection-acceptance-2026-07-07.md` passed.
- `uv run --active python ./harness/scripts/pipeline-plan-check.py harness/pipeline-plan.md --require-complete` passed.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
| A-SN-RUNTIME-CALLERS-1 | design | D-SN-SERVICE-RUNTIME-EXTERNAL-1 | Initial implementation exposed that direct workspace startup callers also needed to be in design/admission scope after `create_sn_service(...)` became fallible and required runtime injection. | Expanded design scope, updated admission evidence, migrated `sn-miner-rust` and `cyfs-p2p-test`, refreshed testing coverage, and reran workspace/all validation. |
