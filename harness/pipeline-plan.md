# Pipeline Plan: TTP Client Connection Lifecycle

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "确定，自动处理后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): p2p-frame
- change_id values: ttp_client_connection_lifecycle

## Acceptance Baseline
- Final acceptance uses the approved `p2p-frame` proposal as authority.
- `TtpClient` must support removing maintained server targets so the maintain loop no longer recreates deleted targets.
- `TtpClient` must release non-maintained cached tunnels after design-defined idle conditions while preserving active stream/control stream/datagram/pending open use.
- Maintained targets must not be cleaned by the non-maintained idle release path.
- The change must not alter TCP/QUIC/PN/TTP wire behavior, public `Tunnel` / `TunnelNetwork` traits, tunnel publish semantics, or existing `TtpServer` lookup-only behavior.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-TTP-CLIENT-LIFECYCLE-1 | proposal | confirmed | Approve TTP client maintained-target removal and non-maintained tunnel idle release requirements | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | root | user approval | approved proposal | proposal doc checks, schema check, and approval metadata recorded |
| D-TTP-CLIENT-LIFECYCLE-1 | design | confirmed | Define TtpClient target removal API, maintain-loop snapshot semantics, idle lease accounting, release behavior, and scope paths | `docs/versions/v0.1/modules/p2p-frame/design.md` | root | P-TTP-CLIENT-LIFECYCLE-1 | approved design | design doc checks, schema checks, stage-scope checks, and auto-pipeline approval metadata recorded |
| I-TTP-CLIENT-LIFECYCLE-1 | implementation | confirmed | Implement minimal production changes for admitted TTP client lifecycle behavior | `p2p-frame/src/ttp/client.rs` plus admission evidence | root | D-TTP-CLIENT-LIFECYCLE-1 | implementation and admission evidence | schema/admission passed and targeted build/unit checks pass |
| T-TTP-CLIENT-LIFECYCLE-1 | testing | confirmed | Add post-implementation tests, test metadata, and unified runner coverage for lifecycle behavior | `docs/versions/v0.1/modules/p2p-frame/testing.md`; `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`; `p2p-frame/src/ttp/tests.rs`; runner wiring if needed | root | I-TTP-CLIENT-LIFECYCLE-1 | runnable unit evidence | testing doc/coverage checks and relevant `test-run.py` levels pass |
| A-TTP-CLIENT-LIFECYCLE-1 | acceptance | pending | Audit proposal/design/implementation/testing consistency and runnable evidence | `docs/versions/v0.1/reviews/` | root | T-TTP-CLIENT-LIFECYCLE-1 | acceptance report | acceptance report check passed with accepted conclusion |

## Return Routing
- Proposal issue: return to proposal only if the maintained target deletion or idle release requirements must change.
- Design issue: return to design if API shape, lease accounting, release semantics, scope paths, or validation seams are incomplete.
- Implementation issue: return to implementation if code does not satisfy approved design or violates admitted scope paths.
- Testing issue: return to testing if runnable evidence does not cover normal, boundary, negative, error, compatibility, lifecycle, and relevant cross-module behavior.
- Acceptance issue: return to the owning stage named by the finding.

## Exit Condition
- [x] proposal approved
- [x] design approved
- [x] implementation admission passed
- [x] implementation completed
- [x] testing completed
- [ ] acceptance completed

## Evidence
- User approval and launch statement: `确定，自动处理后续步骤`
- Proposal approval hash: `5d282a8045885ebafa0aac7fd65a60f3e07dc7e59c174d7a8d20cfe282061b90`
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs proposal` passed.
- `python3 ./harness/scripts/stage-scope-check.py --stage proposal --version v0.1 --module p2p-frame --ignore-untracked` passed.
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` passed after proposal approval.
- Design auto-pipeline approval hash: `5be0bb9a38c4f18a169cca5cc1d1a8d17e7ca3319697fcfdf017919f64e7d3d2`
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs design` passed.
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` passed after design approval.
- `python3 ./harness/scripts/stage-scope-check.py --stage design --version v0.1 --module p2p-frame --ignore-untracked` passed with prior-stage proposal/pipeline diff temporarily isolated.
- `git diff --check -- docs/versions/v0.1/modules/p2p-frame/design.md` passed.
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id ttp_client_connection_lifecycle --evidence-file harness/evidence/admission/20260625-ttp-client-connection-lifecycle.md` passed and wrote `harness/evidence/admission/20260625-ttp-client-connection-lifecycle.p2p-frame.stamp.json`.
- `cargo check -p p2p-frame` passed after implementation.
- `cargo test -p p2p-frame ttp_client -- --nocapture` passed: 4 passed, 0 failed.
- `python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs testing` passed.
- `python3 ./harness/scripts/testing-coverage-check.py --version v0.1 --module p2p-frame --change-id ttp_client_connection_lifecycle` passed.
- Testing auto-pipeline approval hash: `16928519a7b12d2d83e4c0814564ce0e60bdc512d1df455b63a44d402dc4b552`
- `python3 ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --ignore-untracked` passed with prior-stage and implementation diff temporarily isolated.
- `python3 ./harness/scripts/test-run.py p2p-frame unit` passed: 152 passed, 0 failed.
- Implementation scope note: implementation admission and admitted path binding passed; `stage-scope-check.py --stage implementation` cannot be made to see the current approved design changes and simultaneously ignore that design file in this single uncommitted multi-stage pipeline workspace because `.git/index` is read-only, so the mechanical scope isolation was not recordable. The production diff is limited to the admitted `p2p-frame/src/ttp/client.rs` path.
- Acceptance report `docs/versions/v0.1/reviews/p2p-frame-ttp-client-connection-lifecycle-acceptance-2026-06-25.md` was written with conclusion `needs changes`.
- `python3 ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-ttp-client-connection-lifecycle-acceptance-2026-06-25.md` passed.
- `python3 ./harness/scripts/test-run.py all all` was attempted twice but interrupted with exit 130 while waiting in `cargo test -p cyfs-p2p`; no fresh all-all artifact was produced.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
| F-001 | testing | T-TTP-CLIENT-LIFECYCLE-1 | Whole-workspace `all all` run did not complete, so acceptance lacks a fresh passing all-all artifact. | Fresh passing `test-results/test-runs/*-all-all.json` from `python3 ./harness/scripts/test-run.py all all`. |
| F-002 | implementation | I-TTP-CLIENT-LIFECYCLE-1 | Implementation `stage-scope-check.py --stage implementation --change-id ttp_client_connection_lifecycle` could not be recorded as passing in the combined uncommitted multi-stage workspace because design.md remains part of the diff. | Passing implementation stage-scope check in an isolated implementation diff or after separating prior document-stage changes. |
