# [Module Name] Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | completed review evidence | no blocking finding recorded | none |

## Object and Scope
- Module:
- Version:
- change_id values reviewed:
- Review date:
- In scope:
- Out of scope:

## Optional Diff / Status Evidence
- `git status --short` summary:
- `git diff --stat` summary:
- `git diff --name-status` summary:
- `git diff --check` result:
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| | | | | implemented / missing / inconsistent / logically invalid |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| | normal / boundary / negative / error / compatibility / lifecycle / cross-module | | | adequate / gap / stale / not runnable |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| | proposal/design/code/tests | | | pass / fail / gap |

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
- Proposal authority check:
- Proposal vs design:
- Design vs testing implementation:
- Design vs long-lived boundary doc:
- Design vs implementation:
- Test implementation vs test code vs results:
- Test design adequacy:
- change_id traceability:
- Acceptance criteria traceability:
- Cross-module admission:
- Public API / codec / runtime semantics review:
- Document logic review:
- Implementation logic review:
- Document approval timing (approved_content_sha256 verified by schema-check):
- Implementation diff bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ...`):
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix:

## Required Command Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version <version> --module <module>`:
- `uv run --active python ./harness/scripts/admission-check.py --version <version> --module <module> --change-id <change_id> --evidence-file harness/evidence/admission/<task-id>.md`:
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage <stage>`:
- Unit test command through `harness/scripts/test-run.py`:
- DV test command through `harness/scripts/test-run.py`:
- Integration test command through `harness/scripts/test-run.py`:
- Module all command through `harness/scripts/test-run.py <module> all`:
- Project all command through `harness/scripts/test-run.py all all`:
- Whole-project test run artifact (cite the `test-results/test-runs/<artifact>.json` path written by that run):
- Quality gates `uv run --active python ./harness/scripts/quality-check.py`:
- Quality run artifact (cite the `test-results/quality-runs/<artifact>.json` path, when gates are configured):
- Root shortcut command (`test-run.bat` or `./test-run.sh`):
- Acceptance report check `uv run --active python ./harness/scripts/acceptance-report-check.py <report>`:
- Targeted migration search, when applicable:

## Conclusion
- Accepted / rejected / needs changes: accepted / rejected / needs changes
- Reason:
- Supporting test evidence:
- Residual risk:

## Follow-Up Tasks
- Requirement task:
- Design task:
- Implementation task:
- Testing task:
- Testing return reason if coverage is incomplete:
- Iteration count:
- Stop reason if more than 5 unsuccessful iterations:
