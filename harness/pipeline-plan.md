# Pipeline Plan: SN TCP Source Mapped Only

## Trigger
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- User launch confirmed: confirmed
- Per-stage user confirmation: not required; user said "确认，自动处理后续步骤"
- Auto-confirm completed document stages: yes
- Version: v0.1
- Module(s): p2p-frame
- change_id values: sn_tcp_source_mapped_only

## Acceptance Baseline
- Final acceptance uses the approved `p2p-frame` proposal as authority.
- SN service must not return the raw TCP tunnel source socket address as a normal candidate endpoint.
- SN service must not store the raw TCP tunnel source socket address in peer endpoint state.
- If the client reports `map_ports`, SN service may combine the observed TCP source IP with each reported mapped port and return that as a `Mapped` endpoint.
- The implementation must not change SN command wire, control-stream-only signaling, TCP tunnel wire, TLS identity checks, QUIC `ServerReflexive` semantics, or endpoint codec behavior.

## Stage Graph
| task_id | stage | status | responsibility | scope | parent_task | depends_on | output | done_condition |
|---------|-------|--------|----------------|-------|-------------|------------|--------|----------------|
| P-SN-TCP-SOURCE-MAPPED-ONLY-1 | proposal | confirmed | Approve SN server TCP source address boundary and mapped-port-only source-IP use | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | root | user approval | approved proposal | proposal doc checks, schema check, approval metadata, and launch evidence recorded |
| D-SN-TCP-SOURCE-MAPPED-ONLY-1 | design | confirmed | Define SN server candidate filtering, mapped endpoint construction, response arrays, peer cache boundary, and implementation scope paths | `docs/versions/v0.1/modules/p2p-frame/design.md` | root | P-SN-TCP-SOURCE-MAPPED-ONLY-1 | approved design | design doc checks, schema checks, stage-scope checks, and auto-pipeline approval metadata recorded |
| I-SN-TCP-SOURCE-MAPPED-ONLY-1 | implementation | complete | Implement minimal production changes for admitted SN TCP source mapped-only behavior | `p2p-frame/src/sn/service/service.rs` plus admission evidence | root | D-SN-TCP-SOURCE-MAPPED-ONLY-1 | implementation and admission evidence | schema/admission passed; implementation scope check returned due mixed worktree |
| T-SN-TCP-SOURCE-MAPPED-ONLY-1 | testing | confirmed | Add post-implementation tests and testing metadata for no raw TCP source leak and mapped port construction | `docs/versions/v0.1/modules/p2p-frame/testing.md`; `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`; relevant `p2p-frame/src/sn/**` tests; runner wiring if needed | root | I-SN-TCP-SOURCE-MAPPED-ONLY-1 | runnable unit evidence | testing doc/coverage checks and relevant `test-run.py` levels pass; testing scope check returned due mixed worktree |
| A-SN-TCP-SOURCE-MAPPED-ONLY-1 | acceptance | returned | Audit proposal/design/implementation/testing consistency and runnable evidence | `docs/versions/v0.1/reviews/` | root | T-SN-TCP-SOURCE-MAPPED-ONLY-1 | acceptance report | acceptance report check passed; conclusion is needs changes because stage-scope gates failed |

## Return Routing
- Proposal issue: return to proposal only if TCP source endpoint return/storage semantics or mapped-port construction scope must change.
- Design issue: return to design if response endpoint arrays, peer cache state, candidate filtering, mapped endpoint construction, or scope paths are incomplete.
- Implementation issue: return to implementation if code still returns raw TCP source endpoints or fails to return mapped endpoints from `map_ports`.
- Testing issue: return to testing if runnable evidence does not cover no-map-ports, map-ports, raw port leakage, peer cache, and compatibility behavior.
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
- Proposal approval hash: `e0b802b318c8465e2ea8cd088e4580c1073a8d521dc8ab8210790984817d322d`
- Design approval hash: `c6bb03564c01ebb1f66472c37c41b32a45b6695d9b235d629bc010f7d8375c67`
- Testing approval hash: `0453dfef45c4bc997b90c45a53af2e722cf9540ea99e5ce290f2c01540a47a62`
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs design` passed.
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_tcp_source_mapped_only --evidence-file harness/evidence/admission/20260703-sn-tcp-source-mapped-only.md` passed.
- `cargo test -p p2p-frame sn_tcp_observed_endpoint` passed.
- `cargo test -p p2p-frame sn_non_tcp_observed_endpoint` passed.
- `python3 ./harness/scripts/test-run.py p2p-frame unit` passed; artifact `test-results/test-runs/20260703T102530Z-p2p-frame-unit.json`.
- `uv run --active python ./harness/scripts/doc-structure-check.py --version v0.1 --module p2p-frame --docs testing` passed.
- `uv run --active python ./harness/scripts/testing-coverage-check.py --version v0.1 --module p2p-frame --change-id sn_tcp_source_mapped_only` passed.
- `git diff --check` passed.
- `uv run --active python ./harness/scripts/quality-check.py` passed; quality gates are explicitly empty.
- `uv run --active python ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/p2p-frame-sn-tcp-source-mapped-only-acceptance-2026-07-03.md` passed.

## Return Records
| blocking_issue_id | owning_stage | target_task | reason | expected_fix_output |
|-------------------|--------------|-------------|--------|---------------------|
| F-001 | harness/process | A-SN-TCP-SOURCE-MAPPED-ONLY-1 | Implementation stage-scope failed because the worktree contains proposal/design/testing/pipeline files and a pre-existing `p2p-frame/src/sn/client/sn_service.rs` change outside admitted scope paths. | Rerun implementation scope check against an isolated stage diff or separate the pre-existing `sn/client` change and cross-stage docs before final acceptance. |
| F-002 | harness/process | A-SN-TCP-SOURCE-MAPPED-ONLY-1 | Testing stage-scope failed because production and upstream document files are dirty in the same worktree. | Rerun testing scope check against an isolated testing diff or separate testing artifacts from production/upstream document changes before final acceptance. |
