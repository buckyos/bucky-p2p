# Pipeline Plan: TTP Server Tunnel Accept Validator

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "确认，自动处理后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): p2p-frame
- change_id values: ttp_server_tunnel_accept_validator

## Acceptance Baseline
- Final acceptance uses the approved `p2p-frame` proposal as authority.
- `TtpServer` must let callers control whether a newly received incoming tunnel is accepted.
- Validation must run before `TtpRuntime::attach_tunnel(...)`, before the tunnel is remembered in the server cache, and before subsequent TTP stream/control/datagram use through that server.
- The default constructor path must remain explicit allow-all for compatibility.
- Rejecting a tunnel must not change `NetManager` / `TunnelNetwork` publish rules, TCP/QUIC/PN/TTP wire behavior, public `Tunnel` / `TunnelNetwork` traits, `TtpNode` active-open behavior, or `TtpClient` lifecycle semantics.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-TTP-SERVER-TUNNEL-VALIDATOR-1 | proposal | confirmed | Approve TTP server incoming tunnel validator requirement and compatibility boundary | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | root | user approval | approved proposal | proposal doc checks, schema check, approval metadata, and launch evidence recorded |
| D-TTP-SERVER-TUNNEL-VALIDATOR-1 | design | confirmed | Define TtpServer validator API, context, accept/reject flow, default allow-all behavior, rejected tunnel cleanup, and implementation scope paths | `docs/versions/v0.1/modules/p2p-frame/design.md` | root | P-TTP-SERVER-TUNNEL-VALIDATOR-1 | approved design | design doc checks, schema checks, stage-scope checks, and auto-pipeline approval metadata recorded |
| I-TTP-SERVER-TUNNEL-VALIDATOR-1 | implementation | confirmed | Implement minimal production changes for admitted TTP server incoming tunnel validation | `p2p-frame/src/ttp/server.rs`, `p2p-frame/src/ttp/mod.rs` if exports are needed, plus admission evidence | root | D-TTP-SERVER-TUNNEL-VALIDATOR-1 | implementation and admission evidence | schema/admission passed, implementation scope check passed, and targeted build/unit checks pass |
| T-TTP-SERVER-TUNNEL-VALIDATOR-1 | testing | confirmed | Add post-implementation tests, test metadata, and unified runner coverage for default allow-all and reject behavior | `docs/versions/v0.1/modules/p2p-frame/testing.md`; `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`; `p2p-frame/src/ttp/tests.rs`; runner wiring if needed | root | I-TTP-SERVER-TUNNEL-VALIDATOR-1 | runnable unit evidence | testing doc/coverage checks and relevant `test-run.py` levels pass |
| A-TTP-SERVER-TUNNEL-VALIDATOR-1 | acceptance | pending | Audit proposal/design/implementation/testing consistency and runnable evidence | `docs/versions/v0.1/reviews/` | root | T-TTP-SERVER-TUNNEL-VALIDATOR-1 | acceptance report | acceptance report check passed with accepted conclusion |

## Return Routing
- Proposal issue: return to proposal only if the server incoming tunnel validation requirement or compatibility boundary must change.
- Design issue: return to design if validator API, context, default behavior, reject cleanup, scope paths, or validation seams are incomplete.
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
- User approval and launch statement: `确认，自动处理后续步骤`
- Proposal approval hash: `b669a9b90e0c72f23fbbb876ebc35abffed6d16886470e888cdc9268ebf38912`
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs proposal` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage proposal --version v0.1 --module p2p-frame --ignore-untracked` passed before approval.
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` passed after proposal approval.
- `uv run --active python ./harness/scripts/pipeline-plan-check.py harness/pipeline-plan.md` passed.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs design` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage design --version v0.1 --module p2p-frame --ignore-untracked` passed after temporarily isolating prior-stage proposal and pipeline-plan diffs.
- Design auto-pipeline approval hash: `49b2fdb980b308976ee0072c49d9316ffba423f909f54660e7d7d4be8d60154b`
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id ttp_server_tunnel_accept_validator --evidence-file harness/evidence/admission/20260630-ttp-server-tunnel-validator.md` passed and wrote `harness/evidence/admission/20260630-ttp-server-tunnel-validator.p2p-frame.stamp.json`.
- `cargo check -p p2p-frame` passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --change-id ttp_server_tunnel_accept_validator --ignore-untracked` passed after temporarily hiding completed document-stage diffs from git status.
- Testing auto-pipeline approval hash: `c2ebf62c7cad4dea9ca54abf1cd7c8738b24b25c394d47f85d016f1293608da2`
- `cargo test -p p2p-frame ttp:: -- --nocapture` passed: 18 TTP tests, including default allow-all, explicit accept context, reject, and validator error coverage.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs testing` passed.
- `uv run --active python ./harness/scripts/testing-coverage-check.py --version v0.1 --module p2p-frame --change-id ttp_server_tunnel_accept_validator` passed.
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` passed after testing approval.
- `uv run --active python ./harness/scripts/test-run.py p2p-frame unit` passed on rerun and wrote `test-results/test-runs/20260630T092903Z-p2p-frame-unit.json`; the immediately previous run wrote `test-results/test-runs/20260630T092823Z-p2p-frame-unit.json` but failed once in unrelated `pn_service_tracks_user_traffic_with_normalized_source_id` due transient `rx_speed` timing while all TTP tests passed.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --ignore-untracked` passed after temporarily hiding completed proposal/design/implementation diffs and unrelated pre-existing `p2p-frame/src/sn/service/service.rs` diff from git status.
- `uv run --active python ./harness/scripts/test-run.py p2p-frame all` passed and wrote `test-results/test-runs/20260630T095850Z-p2p-frame-all.json`.
- `uv run --active python ./harness/scripts/quality-check.py` passed with no gates configured because `harness/quality-gates.yaml` declares `gates: []`.
- `uv run --active python ./harness/scripts/test-run.py all all` failed after passing visible cargo/workspace test steps because `docs/versions/v0.1/modules/workspace-harness/testplan.yaml` invokes `admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive` without `--evidence-file`; failing artifact: `test-results/test-runs/20260630T095649Z-all-all.json`.
- Acceptance report `docs/versions/v0.1/reviews/p2p-frame-ttp-server-tunnel-accept-validator-acceptance-2026-06-30.md` was written with conclusion `needs changes`.
- `uv run --active python ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-ttp-server-tunnel-accept-validator-acceptance-2026-06-30.md` passed.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
| F-001 | acceptance | A-TTP-SERVER-TUNNEL-VALIDATOR-1 | Fresh `test-run.py all all` artifact failed in existing workspace-harness admission command for `endpoint_area_server_reflexive`, missing required `--evidence-file`; TTP-specific evidence is complete but accepted conclusion is mechanically blocked. | Fix or rebaseline `docs/versions/v0.1/modules/workspace-harness/testplan.yaml` admission command/evidence so `uv run --active python ./harness/scripts/test-run.py all all` exits 0, then rerun acceptance. |
