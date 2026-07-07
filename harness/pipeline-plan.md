# Pipeline Plan: Workspace Harness Test Run Dedupe

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/workspace-harness/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "确认，自动处理后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): workspace-harness
- change_id values: workspace_harness_test_run_dedupe

## Acceptance Baseline
- Final acceptance uses the approved `workspace-harness` proposal as authority.
- `test-run.py all all` must avoid repeated execution of identical commands inside one run while preserving deterministic order and module/level coverage evidence.
- `testplan.yaml` entries must not be duplicated by static fallback entries for the same module and level.
- Machine-written run artifacts remain the acceptance evidence source.
- Business module test semantics and assertions must not change.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-HARNESS-TEST-RUN-DEDUPE-1 | proposal | confirmed | Define runner dedupe goal, boundaries, non-goals, and evidence requirements | `docs/versions/v0.1/modules/workspace-harness/proposal.md` | root | user approval | approved proposal | proposal doc checks, schema check, approval metadata, and launch evidence recorded |
| D-HARNESS-TEST-RUN-DEDUPE-1 | design | confirmed | Define command identity, testplan precedence, reused-result artifact shape, and opt-out behavior | `docs/versions/v0.1/modules/workspace-harness/design.md` | root | P-HARNESS-TEST-RUN-DEDUPE-1 | approved design | design doc checks, schema checks, and stage-scope checks pass |
| I-HARNESS-TEST-RUN-DEDUPE-1 | implementation | complete | Implement deterministic in-run command dedupe in `test-run.py` | `harness/scripts/test-run.py` plus required admission evidence | root | D-HARNESS-TEST-RUN-DEDUPE-1 | runner implementation and admission evidence | schema/admission passed; implementation scope check result recorded; dry-run and harness checks pass |
| T-HARNESS-TEST-RUN-DEDUPE-1 | testing | complete | Register and execute focused coverage for dedupe, no-dedupe, artifact reuse evidence, and all/all behavior | workspace-harness testing artifacts and unified runner evidence | root | I-HARNESS-TEST-RUN-DEDUPE-1 | runnable test evidence | module/all and project/all artifacts pass and include reuse evidence |
| A-HARNESS-TEST-RUN-DEDUPE-1 | acceptance | pending | Audit proposal/design/implementation/testing consistency and runnable evidence | `docs/versions/v0.1/reviews/` | root | T-HARNESS-TEST-RUN-DEDUPE-1 | acceptance report | acceptance report check passed and pipeline exit conditions complete |

## Return Routing
- Proposal issue: return to proposal if dedupe scope, artifact evidence expectations, or business-module non-goals must change.
- Design issue: return to design if command identity, fallback precedence, artifact shape, or opt-out behavior is incomplete.
- Implementation issue: return to implementation if repeated commands still execute by default, registered coverage is skipped, or artifacts lose coverage traceability.
- Testing issue: return to testing if dry-run/module/all/all/root shortcut evidence is missing or stale.
- Acceptance issue: return to the owning stage named by the finding.

## Exit Condition
- [x] proposal approved
- [x] design approved
- [x] implementation admission passed
- [x] implementation completed
- [x] testing completed
- [ ] acceptance completed

## Evidence
- User approval and launch statement: `确认，自动处理后续步骤`
- Proposal approval hash: `762b1aa3a4fccf170e9cc01ad65390ca5d6fe0b60eb61d1609f2d7350982650c`
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module workspace-harness --docs proposal` passed.
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module workspace-harness` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage proposal --version v0.1 --module workspace-harness --ignore-untracked` passed.
- Design approval hash: `795f6459810e5110267bd17f2a7b609ab8c9a3b56ecf63ed398d6d5381b36a7d`
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module workspace-harness --docs design` passed.
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module workspace-harness` passed after design approval.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module workspace-harness --change-id workspace_harness_test_run_dedupe --evidence-file harness/evidence/admission/20260707-test-run-dedupe.md` passed; stamp `harness/evidence/admission/20260707-test-run-dedupe.workspace-harness.stamp.json`.
- `python3 -m py_compile harness/scripts/test-run.py` passed.
- `uv run --active python ./harness/scripts/test-run.py all all --dry-run` passed and showed reused commands.
- `uv run --active python ./harness/scripts/test-run.py all all --dry-run --no-dedupe` passed and showed duplicate commands without reuse markers.
- Testing approval hash: `15905af1943aae1d3c010a2ace35262add33e8bde419da09e410091f8152e4cd`.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module workspace-harness --docs testing` passed.
- `uv run --active python ./harness/scripts/testing-coverage-check.py --version v0.1 --module workspace-harness --change-id workspace_harness_test_run_dedupe` passed.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
