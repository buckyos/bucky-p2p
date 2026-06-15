# Pipeline Plan: PN Server Default Allow-All Validator

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "确认，自动处理后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): p2p-frame
- change_id values: pn_multi_server_assigned_target

## Acceptance Baseline
- Final acceptance uses the approved `pn_multi_server_assigned_target` proposal item as authority.
- `PnServer::new(...)` and the default `PnConnectionValidator` must remain explicit allow-all for unconfigured deployments.
- Multi-PN assigned target enforcement must be enabled through an explicit validator or policy path and must not rely on the default allow-all path to hide wrong-PN failures.
- The change must not introduce a library-owned peer-to-PN directory, automatic PN switching, cross-PN relay bridge, or PN wire protocol change.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-PN-DEFAULT-ALLOW-ALL-1 | proposal | confirmed | Approve revised PN default validator requirement | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | root | user launch | approved proposal | proposal approved with `pn_multi_server_assigned_target` requiring default allow-all |
| D-PN-DEFAULT-ALLOW-ALL-1 | design | complete | Synchronize PN server constructor and admission design with approved proposal | `docs/versions/v0.1/modules/p2p-frame/design.md`, `docs/versions/v0.1/modules/p2p-frame/design/pn-server.md` | root | P-PN-DEFAULT-ALLOW-ALL-1 | approved design | design maps default allow-all compatibility and explicit assigned target policy path |
| I-PN-DEFAULT-ALLOW-ALL-1 | implementation | complete | Change production PN server default validator only after admission | `p2p-frame/src/pn/service/pn_server.rs`, necessary allowed scope paths from design | root | D-PN-DEFAULT-ALLOW-ALL-1 | production code | `PnServer::new(...)` uses allow-all while explicit validator path still controls admission |
| T-PN-DEFAULT-ALLOW-ALL-1 | testing | complete | Add or update post-implementation validation coverage and metadata | tests, `docs/versions/v0.1/modules/p2p-frame/testing.md`, `docs/versions/v0.1/modules/p2p-frame/testplan.yaml` | root | I-PN-DEFAULT-ALLOW-ALL-1 | testing artifacts and runnable evidence | default allow-all and explicit reject behavior are covered or gaps recorded |
| A-PN-DEFAULT-ALLOW-ALL-1 | acceptance | blocked | Audit proposal/design/implementation/testing consistency | `docs/versions/v0.1/reviews/` | root | T-PN-DEFAULT-ALLOW-ALL-1 | acceptance report | implementation/testing scope checks are blocked by unrelated dirty workspace paths |

## Return Routing
- Proposal issue: return to proposal if default allow-all conflicts with a broader requirement.
- Design issue: return to design if constructor compatibility, explicit assigned target policy, or wrong-PN failure semantics are ambiguous.
- Implementation issue: return to implementation if default constructor still rejects by default or explicit validator no longer controls admission.
- Testing issue: return to testing if default allow-all compatibility or explicit rejection has no runnable or recorded validation path.
- Acceptance issue: return to the owning stage named by the finding.

## Exit Condition
- [x] proposal approved
- [x] pipeline plan checked
- [x] design approved
- [x] implementation admission passed
- [x] production code updated
- [x] testing completed
- [ ] acceptance completed

## Evidence
- User approval and launch statement: `确认，自动处理后续步骤`
- `uv run --active python ./harness/scripts/pipeline-plan-check.py harness/pipeline-plan.md` passed.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs design` passed.
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id pn_multi_server_assigned_target --evidence-file harness/evidence/admission/20260615-pnserver-default-allow-all.md` passed.
- Admission stamp: `harness/evidence/admission/20260615-pnserver-default-allow-all.p2p-frame.stamp.json`.
- Production code: `p2p-frame/src/pn/service/pn_server.rs` default `PnServer::new(...)` uses `allow_all_pn_connection_validator()`; explicit reject and assigned-target policy paths remain available.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs testing` passed.
- `uv run --active python ./harness/scripts/testing-coverage-check.py --version v0.1 --module p2p-frame --change-id pn_multi_server_assigned_target` passed.
- `cargo test -p p2p-frame pn_connection_validator -- --nocapture` passed: 2 tests passed.
- `uv run --active python ./harness/scripts/test-run.py p2p-frame unit` passed: 122 lib unit tests passed, doc tests 0, targeted PN validator step passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --change-id pn_multi_server_assigned_target` failed because the workspace already contains broad cross-stage dirty/untracked changes outside this task's admitted scope; this task did not revert those user/workspace changes.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame` failed for the same broad dirty workspace condition.
- `uv run --active python ./harness/scripts/quality-check.py` passed; `harness/quality-gates.yaml` declares an explicitly empty gates list.
- Acceptance report: `docs/versions/v0.1/reviews/p2p-frame-pnserver-default-allow-all-acceptance-2026-06-15.md`; conclusion `needs changes` because stage-scope gates cannot close in the current dirty workspace.
- `uv run --active python ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-pnserver-default-allow-all-acceptance-2026-06-15.md` passed.
