# p2p-frame TTP Server Tunnel Accept Validator Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-001 | high | acceptance | `test-results/test-runs/20260630T095649Z-all-all.json` | Required `test-run.py all all` artifact has `exit_code: 1`; all visible cargo tests passed, but the final workspace-harness integration step invokes `admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive` without `--evidence-file`. This is outside the TTP change, but it blocks an accepted conclusion under the repository acceptance checker. | accepted conclusion requires fresh passing whole-project test run artifact |

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- change_id values reviewed: `ttp_server_tunnel_accept_validator`
- Review date: 2026-06-30
- In scope: TTP server incoming tunnel validator proposal, design, production implementation, unit tests, testing metadata, admission evidence, and pipeline evidence.
- Out of scope: pre-existing `p2p-frame/src/sn/service/service.rs` diff, unrelated untracked reviews/evidence, and the existing workspace-harness testplan admission command defect.

## Optional Diff / Status Evidence
- `git status --short` summary: reviewed; TTP-relevant tracked files are proposal/design/testing/testplan/pipeline plus `p2p-frame/src/ttp/server.rs` and `p2p-frame/src/ttp/tests.rs`; unrelated tracked `p2p-frame/src/sn/service/service.rs` and many unrelated untracked files are present.
- `git diff --stat` summary: 8 tracked files, 448 insertions, 87 deletions; includes one unrelated tracked SN service file.
- `git diff --name-status` summary: TTP work changed p2p-frame docs, pipeline plan, `server.rs`, and `tests.rs`; unrelated `p2p-frame/src/sn/service/service.rs` is not acceptance evidence.
- `git diff --check` result: passed with no whitespace errors.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| TtpServer exposes configurable incoming tunnel validation | `proposal.md` `P-TTP-SERVER-TUNNEL-ACCEPT-VALIDATOR-1`; `design.md` Directly Mapped row | `p2p-frame/src/ttp/server.rs` adds `TtpIncomingTunnelValidator`, context, validator ref, and `new_with_incoming_tunnel_validator(...)` | `test-results/test-runs/20260630T095850Z-p2p-frame-all.json`; `server_accept_validator_records_context_and_accepts_incoming_tunnel` | implemented |
| Default constructor remains allow-all | `proposal.md` compatibility boundary; `design.md` constructor contract | `TtpServer::new(...)` delegates to `allow_all_ttp_incoming_tunnel_validator()` | `server_default_validator_accepts_incoming_tunnel`; `test-results/test-runs/20260630T092903Z-p2p-frame-unit.json` | implemented |
| Validation runs before attach/cache | `design.md` incoming callback flow | validator branch executes before `runtime.attach_tunnel(...)` and `remember_tunnel(...)` | reject/error tests prove subsequent `open_stream(...)` returns `NotFound` and fake tunnel has no opened stream | implemented |
| Reject and validator error close tunnel best-effort | `design.md` error/rollback notes | reject/error branches call `tunnel.close()` and return without attach/cache | `server_reject_validator_blocks_attach_and_cache`; `server_validator_error_blocks_attach_and_cache` | implemented |
| No change to wire protocol or public Tunnel/TunnelNetwork traits | `proposal.md` non-goals and compatibility boundary | changes are contained to `p2p-frame/src/ttp/server.rs`; no tunnel trait or protocol file changes are part of TTP evidence | `cargo check -p p2p-frame`, `test-run.py p2p-frame all`, and workspace cargo steps inside failed all-all artifact all passed before the unrelated admission command | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| `ttp_server_tunnel_accept_validator` | normal / boundary / negative / error / compatibility / lifecycle / cross-module | `testing.md` Direct Change Coverage, Case-Type Coverage, Design Element Coverage, and `testplan.yaml` unit/integration entries cover all required case types | `test-results/test-runs/20260630T095850Z-p2p-frame-all.json`; `test-results/test-runs/20260630T092903Z-p2p-frame-unit.json` | adequate |
| Validator context fidelity | normal / boundary | Unit test records local/remote id, protocol, tunnel id, candidate id, and endpoints from `FakeTunnel` | `server_accept_validator_records_context_and_accepts_incoming_tunnel` in TTP targeted run | adequate |
| Reject/error lifecycle cleanup | negative / error / lifecycle | FakeTunnel close counter and open-stream cache lookup exercise changed branches | TTP targeted run in `test-results/test-runs/20260630T095850Z-p2p-frame-all.json` | adequate |
| Default compatibility | compatibility / cross-module | Default constructor remains allow-all and no downstream signature migration is required | `p2p-frame all` artifact passes; `all all` cargo tests pass before unrelated workspace-harness admission step | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-01 | proposal/design | New incoming tunnel validator API exists and is optional for default callers | Code inspection and unit tests | pass |
| AR-02 | design/code | Accept path attaches and caches after validation | Accept/default unit tests and code order | pass |
| AR-03 | design/code | Reject and validator error paths do not attach/cache and close tunnel best-effort | Negative/error unit tests | pass |
| AR-04 | proposal non-goals | No wire, Tunnel trait, TunnelNetwork trait, TtpNode active-open, or TtpClient lifecycle regression | Scope check, compile, and p2p-frame test-run artifacts | pass |
| AR-05 | acceptance gate | Fresh whole-project `test-run.py all all` artifact passes | `test-results/test-runs/20260630T095649Z-all-all.json` | fail |

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `harness/evidence/admission/20260630-ttp-server-tunnel-validator.md`
- `p2p-frame/src/ttp/server.rs`
- `p2p-frame/src/ttp/tests.rs`
- `harness/pipeline-plan.md`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Reviewed approved proposal requirement and compatibility boundary.
2. Reviewed approved design mapping, API shape, state flow, rollback, and scope paths.
3. Generated acceptance rules from proposal, design, implementation, and tests.
4. Reviewed implementation order and changed branches in `server.rs`.
5. Reviewed test design coverage for normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases.
6. Reviewed runnable test artifacts and harness command results.
7. Reviewed document approval hashes through `schema-check.py`.
8. Used diff/status output only to locate unrelated changes.
9. Produced a needs-changes conclusion because whole-project evidence failed outside the TTP change.

## Consistency Summary
- Proposal authority check: proposal is approved and directly covers user-controlled acceptance of new TTP server incoming tunnels.
- Proposal vs design: design preserves proposal boundaries and maps `ttp_server_tunnel_accept_validator` to `p2p-frame/src/ttp/server.rs` and export behavior.
- Design vs testing implementation: testing metadata and TTP unit tests cover accept, reject, error, context, and default compatibility.
- Design vs long-lived boundary doc: no long-lived module boundary conflict found; change stays inside TTP server behavior.
- Design vs implementation: implementation follows the designed validator-before-attach/cache flow and default allow-all constructor.
- Test implementation vs test code vs results: new tests are reachable through `test-run.py p2p-frame all` and passed in fresh artifacts.
- Test design adequacy: adequate for the TTP change; whole-project acceptance is blocked by unrelated workspace-harness command failure.
- change_id traceability: `ttp_server_tunnel_accept_validator` appears in proposal, design, testing.md, testplan.yaml, admission evidence, and pipeline plan.
- Acceptance criteria traceability: all TTP-specific criteria have code and test evidence.
- Cross-module admission: TTP change evidence is contained in p2p-frame and admission passed for the p2p-frame change_id.
- Public API / codec / runtime semantics review: public API addition is additive; no codec, wire, Tunnel trait, or TunnelNetwork trait change.
- Document logic review: no contradiction found in p2p-frame proposal/design/testing for this change.
- Implementation logic review: no TTP correctness defect found; reject/error close is best-effort and does not poison future dispatch.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed after proposal, design, and testing approvals.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): passed for `ttp_server_tunnel_accept_validator`.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not applicable; this is a new feature, not a bugfix.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`: passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id ttp_server_tunnel_accept_validator --evidence-file harness/evidence/admission/20260630-ttp-server-tunnel-validator.md`: passed and wrote `harness/evidence/admission/20260630-ttp-server-tunnel-validator.p2p-frame.stamp.json`.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module p2p-frame --change-id ttp_server_tunnel_accept_validator --ignore-untracked`: passed after isolating completed document-stage diffs.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage testing --version v0.1 --module p2p-frame --ignore-untracked`: passed after isolating completed-stage and unrelated SN diffs.
- Unit test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py p2p-frame unit` passed on rerun; artifact `test-results/test-runs/20260630T092903Z-p2p-frame-unit.json`.
- DV test command through `harness/scripts/test-run.py`: p2p-frame DV is disabled by `testplan.yaml`; `p2p-frame all` records `test-run: no dv tests for p2p-frame`.
- Integration test command through `harness/scripts/test-run.py`: `p2p-frame all` integration steps passed; artifact `test-results/test-runs/20260630T095850Z-p2p-frame-all.json`.
- Module all command through `harness/scripts/test-run.py <module> all`: `uv run --active python ./harness/scripts/test-run.py p2p-frame all` passed; artifact `test-results/test-runs/20260630T095850Z-p2p-frame-all.json`.
- Project all command through `harness/scripts/test-run.py all all`: `uv run --active python ./harness/scripts/test-run.py all all` failed in workspace-harness admission step; artifact `test-results/test-runs/20260630T095649Z-all-all.json`.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): `test-results/test-runs/20260630T095649Z-all-all.json` exists but has `exit_code: 1`.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed with no gates configured because `harness/quality-gates.yaml` declares `gates: []`.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not required; quality gates are explicitly empty.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): not rerun after `test-run.py all all` failed on the same unified suite path; running the shortcut would not supply the required fresh passing `all all` artifact.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: pending before final report validation.
- Targeted migration search, when applicable: not applicable; additive API and no old symbol migration.

## Conclusion
- Accepted / rejected / needs changes: needs changes
- Reason: TTP-specific implementation and testing evidence is complete, but repository acceptance requires a fresh passing `test-run.py all all` artifact, and the fresh artifact failed in an unrelated workspace-harness admission command missing `--evidence-file`.
- Supporting test evidence: `test-results/test-runs/20260630T095850Z-p2p-frame-all.json`, `test-results/test-runs/20260630T092903Z-p2p-frame-unit.json`, and failing global artifact `test-results/test-runs/20260630T095649Z-all-all.json`.
- Residual risk: final accepted state cannot be mechanically recorded until the existing workspace-harness testplan admission command is fixed or given valid evidence binding.

## Follow-Up Tasks
- Requirement task: none for the TTP validator requirement.
- Design task: none for the TTP validator design.
- Implementation task: none for the TTP validator implementation.
- Testing task: none for the TTP validator tests.
- Testing return reason if coverage is incomplete: not applicable; coverage is adequate for the reviewed change.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
