# Lightweight Acceptance Report

## Object and Scope
- Task manifest: task.yaml
- Workflow tier: trivial / standard
- Change record: not-applicable / docs/changes/<change>.md

## Delivery Summary
- Outcome: summarize the delivered result
- Handoff: concise result and any residual follow-up

## Proposal Consistency
| change_id | Requirement or Boundary | Proposal Source | Delivery Evidence | Finding | Status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-example | proposal requirement or boundary | proposal.md section/item | current implementation, documentation, or concrete no-change evidence | matches / mismatch explanation | pass / fail |

## Independent Defect Discovery
| Category | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|--------------------|-------------------|----------------------------------|--------|
| behavior-and-logic | concrete current implementation/documentation paths | search for missing behavior, wrong conditions, invalid assumptions, and unintended effects | concrete finding or no defect found | pass / fail / not-applicable |
| boundaries-and-failure-paths | concrete input, error, cleanup, or documentation-consumer paths | challenge boundary inputs, failure handling, partial completion, and recovery | concrete finding or task-specific not-applicable reason | pass / fail / not-applicable |
| regression-and-side-effects | concrete callers, interfaces, state, tests, or downstream docs | search for compatibility regressions, stale consumers, state damage, and untested side effects | concrete finding or task-specific not-applicable reason | pass / fail / not-applicable |

## Verification
- Targeted check: exact command/check / not-run
- Result: passed / not-run
- Exception reason: not-applicable / concrete reason no executable check is proportionate

## Findings
| ID | Severity | Evidence | Problem | Blocking |
|----|----------|----------|---------|----------|
| F-1 | none / low / medium / high | proposal and delivery evidence | no issue / concrete issue | no / yes |

## Conclusion
- Accepted / rejected / needs changes: accepted / rejected / needs changes
- Reason: explain why delivery does or does not satisfy the approved proposal
