# workspace-harness Test Run Dedupe Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | proposal, design, admission, runner artifacts, quality check, and scope checks reviewed | no blocking finding recorded | none |

## Object and Scope
- Module: workspace-harness
- Version: v0.1
- change_id values reviewed: workspace_harness_test_run_dedupe
- Review date: 2026-07-07
- In scope: `harness/scripts/test-run.py` command source precedence, in-run command-result reuse, additive run artifact fields, `--no-dedupe`, workspace-harness governance script scope exception, testing metadata, and pipeline evidence.
- Out of scope: business module protocol/runtime/test assertion changes and unrelated pre-existing untracked evidence files.

## Optional Diff / Status Evidence
- `git status --short` summary: tracked diff was empty during final discovery; many pre-existing untracked review/admission files remain outside this change.
- `git diff --stat` summary: no tracked diff reported at final discovery.
- `git diff --name-status` summary: no tracked diff reported at final discovery.
- `git diff --check` result: passed.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| Avoid repeated physical execution of identical commands while preserving coverage records. | `proposal.md` P-HARNESS-TEST-RUN-1 and `design.md` Overall Approach | `test-run.py` uses a per-run cache keyed by `(root.resolve(), tuple(command))` and records reused steps with `deduped` and `reused_from_step`. | Root shortcut artifact `test-results/test-runs/20260707T092342Z-all-all.json` shows reused `cargo test --workspace`, reused `sn-miner real_process`, and exit code 0. | implemented |
| Prefer testplan commands over static fallback for the same module and level. | `design.md` Overall Approach and Key Decisions | `commands_for(...)` returns testplan commands when present and falls back only when absent. | Targeted dry-runs showed no appended fallback duplicate for testplan-covered module/level entries. | implemented |
| Preserve debug opt-out for strict physical reruns. | `proposal.md` Decision needed before approval and `design.md` Key Decisions | `test-run.py` exposes `--no-dedupe` and bypasses cache reuse when set. | `uv run --active python ./harness/scripts/test-run.py sn-miner all --dry-run --no-dedupe` passed and printed duplicate commands without reuse markers. | implemented |
| Keep root shortcut behavior stable. | `proposal.md` Success Evidence and `design.md` Invariants to Preserve | `./test-run.sh all all` still invokes the canonical runner surface. | `./test-run.sh all all` passed with artifact `test-results/test-runs/20260707T092342Z-all-all.json`. | implemented |
| Allow workspace-harness implementation to modify its admitted governance scripts without weakening other modules. | `design.md` Directly Mapped Change Items and Key Decisions | `stage-scope-check.py` permits non-evidence `harness/**` implementation paths only when module is `workspace-harness`; admission scope still binds paths. | `stage-scope-check.py --stage implementation --module workspace-harness --change-id workspace_harness_test_run_dedupe --ignore-untracked` passed. | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| workspace_harness_test_run_dedupe command scheduling | normal / boundary / compatibility | `testing.md` and `testplan.yaml` cover py_compile, targeted dedupe dry-run, targeted no-dedupe dry-run, and one root all/all artifact. | `uv run --active python ./harness/scripts/test-run.py workspace-harness all` passed; artifact `test-results/test-runs/20260707T095553Z-workspace-harness-all.json`. | adequate |
| Reused result artifact traceability | normal / compatibility / cross-module | Design requires additive fields and schema remains 1. | Root shortcut whole-project artifact `test-results/test-runs/20260707T092342Z-all-all.json` includes `deduped: true` and `reused_from_step`. | adequate |
| No-dedupe investigation path | boundary / negative | Testing plan includes explicit no-dedupe dry-run to prove cache bypass scheduling. | `uv run --active python ./harness/scripts/test-run.py sn-miner all --dry-run --no-dedupe` passed and printed repeated commands without reuse markers. | adequate |
| Full workspace behavior through canonical entrypoints | lifecycle / cross-module / compatibility | Proposal success criteria require focused module all and one root shortcut project all. | `./test-run.sh all all` passed with fresh all/all artifact. | adequate |
| Governance scope and quality gates | error / compatibility | Design includes the workspace-harness-only scope exception and keeps existing gates. | schema/admission/stage-scope checks passed; `quality-check.py` passed with explicitly empty gates. | adequate |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| A-HARNESS-DEDUPE-1 | proposal/design/code/tests | `commands_for` uses testplan commands instead of appending fallback commands for an already covered module/level. | Code inspection plus targeted dry-run output showing reuse behavior rather than fallback append duplicates. | pass |
| A-HARNESS-DEDUPE-2 | proposal/design/code/tests | Repeated argv in one run reuses the earlier result by default and records traceability. | Fresh all/all artifacts with `deduped: true`, `reused_from_step`, `requested_module: all`, `requested_level: all`, and `exit_code: 0`. | pass |
| A-HARNESS-DEDUPE-3 | proposal/design/code/tests | `--no-dedupe` disables result reuse for strict rerun investigation. | `uv run --active python ./harness/scripts/test-run.py sn-miner all --dry-run --no-dedupe` passes and shows duplicate commands without reuse markers. | pass |
| A-HARNESS-DEDUPE-4 | proposal/design/tests | Canonical project behavior is covered by one root shortcut all/all run. | Root shortcut artifact `test-results/test-runs/20260707T092342Z-all-all.json`. | pass |
| A-HARNESS-DEDUPE-5 | design/code/governance | Scope checker relaxation is limited to module `workspace-harness` and admitted paths. | Admission evidence `harness/evidence/admission/20260707-test-run-dedupe.md` and implementation stage-scope check pass. | pass |

## Inputs
- `proposal.md`
- `design.md`
- test implementation and `testing.md`
- `testplan.yaml`
- long-lived module doc `docs/modules/workspace-harness.md`
- implementation in `harness/scripts/test-run.py` and `harness/scripts/stage-scope-check.py`
- test results under `test-results/test-runs/`
- git diff/status evidence
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
- Proposal authority check: approved proposal P-HARNESS-TEST-RUN-1 authorizes runner dedupe, testplan precedence, artifact preservation, and root shortcut evidence.
- Proposal vs design: design covers command identity, fallback precedence, artifact fields, opt-out behavior, and the workspace-harness scope exception needed to implement admitted harness scripts.
- Design vs testing implementation: `testing.md` and `testplan.yaml` cover py_compile, targeted dedupe dry-run, targeted no-dedupe dry-run, and one root shortcut all/all artifact.
- Design vs long-lived boundary doc: workspace-harness remains the owner of harness scripts and unified validation entrypoints.
- Design vs implementation: `test-run.py` implements testplan precedence, command-result cache, additive artifact fields, and `--no-dedupe`; `stage-scope-check.py` implements the module-limited harness script exception.
- Test implementation vs test code vs results: runner tests execute through `test-run.py`; artifacts show requested module/level, command steps, reuse markers, and exit code 0.
- Test design adequacy: coverage includes normal dedupe, boundary no-dedupe, artifact compatibility, root shortcut compatibility, and governance scope.
- change_id traceability: proposal, design, admission evidence, testing coverage, and acceptance all use `workspace_harness_test_run_dedupe`.
- Acceptance criteria traceability: each success criterion maps to at least one generated acceptance rule and fresh runnable artifact.
- Cross-module admission: no business module code was admitted or changed by this review; all/all execution validates cross-module compatibility.
- Public API / codec / runtime semantics review: no public runtime protocol, codec, or business API semantics changed.
- Document logic review: no contradiction found between approved proposal, approved design, testing plan, and acceptance evidence.
- Implementation logic review: cache key uses resolved root and argv tuple, reused failures preserve exit code, and artifact fields are additive.
- Document approval timing (approved_content_sha256 verified by schema-check): schema-check passed for workspace-harness after approval metadata was present.
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`): implementation scope check passed for `workspace_harness_test_run_dedupe`.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: not a bugfix; optimization evidence is dry-run comparison plus fresh all/all artifacts.

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module workspace-harness`: passed.
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module workspace-harness --change-id workspace_harness_test_run_dedupe --evidence-file harness/evidence/admission/20260707-test-run-dedupe.md`: passed and wrote `harness/evidence/admission/20260707-test-run-dedupe.workspace-harness.stamp.json`.
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation --version v0.1 --module workspace-harness --change-id workspace_harness_test_run_dedupe --ignore-untracked`: passed.
- Unit test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py workspace-harness unit` covered by module all artifact `test-results/test-runs/20260707T095553Z-workspace-harness-all.json`.
- DV test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py workspace-harness dv` covered by module all artifact `test-results/test-runs/20260707T095553Z-workspace-harness-all.json`.
- Integration test command through `harness/scripts/test-run.py`: `uv run --active python ./harness/scripts/test-run.py workspace-harness integration` covered by module all artifact `test-results/test-runs/20260707T095553Z-workspace-harness-all.json`.
- Module all command through `harness/scripts/test-run.py <module> all`: `uv run --active python ./harness/scripts/test-run.py workspace-harness all` passed; artifact `test-results/test-runs/20260707T095553Z-workspace-harness-all.json`.
- Project all command through `harness/scripts/test-run.py all all`: not separately required when wrapper is unchanged; covered by `./test-run.sh all all` invoking the same runner and writing `test-results/test-runs/20260707T092342Z-all-all.json`.
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run): root shortcut artifact `test-results/test-runs/20260707T092342Z-all-all.json` has `requested_module: all`, `requested_level: all`, and `exit_code: 0`.
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`: passed; `harness/quality-gates.yaml` declares an explicitly empty gates list.
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured): not applicable because quality gates are explicitly empty.
- Root shortcut command (`test-run.bat` or `./test-run.sh`): `./test-run.sh all all` passed; artifact `test-results/test-runs/20260707T092342Z-all-all.json`.
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`: `uv run --active python ./harness/scripts/acceptance-report-check.py docs/versions/v0.1/reviews/workspace-harness-test-run-dedupe-acceptance-2026-07-07.md` passed.
- Targeted migration search, when applicable: not applicable; no migration or API rename.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: The approved runner optimization is implemented and evidenced by focused module checks plus one root shortcut project run; artifacts preserve coverage traceability for reused commands and no business module semantics were changed.
- Supporting test evidence: `test-results/test-runs/20260707T095553Z-workspace-harness-all.json` and `test-results/test-runs/20260707T092342Z-all-all.json`.
- Residual risk: `--no-dedupe` should be used when intentionally investigating flakiness through repeated physical execution.

## Follow-Up Tasks
- Requirement task: none.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: coverage is complete for the approved scope.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
