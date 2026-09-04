# [Module Name] Acceptance Report

## Findings
| ID | Severity | Owning Stage | Correctness Category | Evidence | Problem | Blocking |
|----|----------|--------------|----------------------|----------|---------|----------|
| F-000 | none | none | overall | concrete primary sources inspected | no finding | no |

## Object and Scope
- Task manifest: task.yaml
- Review date:
- In-scope implementation:
- Review mode: independent falsification; conclusion selected after findings and category review

## Requirement Coverage
| change_id | Requirement or Boundary | Source | Implementation Evidence | Finding | Status |
|-----------|-------------------------|--------|-------------------------|---------|--------|
| CHG-example | required behavior or boundary | `proposal.md` section or item | concrete production path/symbol/runtime evidence | no requirement defect or missing behavior | pass / fail |

## Independent Defect Discovery
| Category | Applicable Scope | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|------------------|--------------------|-------------------|----------------------------------|--------|
| requirement-and-behavior | requested behavior and boundaries | concrete proposal rows and implementation paths | search for ambiguity, missing/unintended behavior, and counterexamples | concrete finding or no defect found | pass / fail |
| logic-and-control-flow | changed and behavior-critical control paths | concrete symbols/call paths | challenge conditions, algorithms, branches, assumptions, and termination | concrete finding or no defect found | pass / fail / not-applicable |
| boundary-and-input | externally supplied or derived inputs | concrete validation/parsing paths | exercise malformed, empty, extreme, encoded, and trust-boundary cases | concrete finding or task-specific not-applicable reason | pass / fail / not-applicable |
| state-and-data-integrity | affected state and persistence | concrete owners/transitions/transactions | challenge illegal, partial, duplicate, stale, retry, and idempotency states | concrete finding or task-specific not-applicable reason | pass / fail / not-applicable |
| error-handling-and-recovery | fallible calls and failure paths | concrete error/timeout/cancellation paths | challenge propagation, rollback, fallback, partial failure, and recovery | concrete finding or task-specific not-applicable reason | pass / fail / not-applicable |
| resource-lifetime-and-cleanup | acquired resources and async work | concrete acquire/release paths | inspect cleanup on success, failure, timeout, and cancellation | concrete finding or task-specific not-applicable reason | pass / fail / not-applicable |
| concurrency-and-ordering | shared state and concurrent execution | concrete synchronization/order paths | challenge races, deadlocks, visibility, ordering, starvation, and cancellation | concrete finding or task-specific not-applicable reason | pass / fail / not-applicable |
| interface-and-compatibility | exported and consumed behavior | concrete interfaces/callers/contracts | challenge consumer assumptions, migrations, encodings, and semantic compatibility | concrete finding or task-specific not-applicable reason | pass / fail / not-applicable |
| security-and-capacity | trust and resource boundaries | concrete authorization/input/allocation paths | challenge bypass, injection, exposure, traversal, amplification, and unbounded work | concrete finding or task-specific not-applicable reason | pass / fail / not-applicable |
| test-adequacy | tests and observable evidence | concrete test cases/assertions/results | determine whether applicable failures can escape the current tests | concrete gap/finding or no adequacy defect found | pass / fail |

## Document Consistency
| Document | Source | Implementation Consistency | Finding | Status |
|----------|--------|----------------------------|---------|--------|
| design | `design.md` or `pipeline/plan.md`; use `not-present` when absent | implementation follows the documented solution shape | no mismatch | pass / fail / not-present |
| testing | `testing.md` and/or `testplan.yaml`; use `not-present` when absent | implemented behavior and tests do not contradict the testing document | no mismatch | pass / fail / not-present |

## Result Summary
- Overall result: accepted / rejected / needs changes
- Outcome:
- Blocking issues:
- Next action:

## Conclusion
- Accepted / rejected / needs changes: accepted / rejected / needs changes
- Reason: explain why the completed defect search does or does not support acceptance
