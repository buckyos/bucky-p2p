# Acceptance Defect-Discovery Gate

This standalone review contract applies to high-risk task packets. Trivial and standard tasks use the proportional defect-discovery contract in `harness/rules/task-entry-gate-rules.md`.

## Primary Goal
- Acceptance is an independent falsification review. Its first responsibility is to discover defects, not to justify completion.
- Assume the delivery may be wrong. Search for concrete counterexamples that would make the requested behavior incorrect, unsafe, incomplete, incompatible, or inadequately validated.
- Process closure, document consistency, passing tests, and report structure are secondary. They support the review but never prove functional correctness.

## Independence And Review Order
1. Start a separate acceptance task after implementation and testing. Use a reviewer that did not implement the change when the environment supports it.
2. Read the current `proposal.md`, current implementation, relevant callers/dependencies, test design, test code, and available runtime evidence. Do not adopt an implementation summary, self-review, previous acceptance conclusion, or pipeline completion claim as truth.
3. Inspect the requirement and implementation directly, generate failure hypotheses, and try to falsify the delivery.
4. Record findings and every defect-discovery category before selecting `accepted`, `rejected`, or `needs changes`.
5. Check design/testing document consistency and lifecycle closure only after the defect search. Never convert a process pass into a correctness pass.

If an independent reviewer is unavailable, the acceptance owner MUST still use this order, ignore prior conclusions until the current evidence has been inspected, and state the concrete evidence used for each category.

## Required Defect-Discovery Categories

The report MUST contain exactly one row for every category below. `requirement-and-behavior` and `test-adequacy` are always applicable. Any other category may be `not-applicable` only with a concrete task-specific reason; generic statements such as "not relevant", "no change", or "tests pass" are invalid.

- `requirement-and-behavior`: ambiguity, contradiction, incorrect acceptance boundaries, missing required behavior, unintended behavior, and behavior outside the stated non-goals.
- `logic-and-control-flow`: incorrect conditions or algorithms, off-by-one behavior, wrong branches, unintended fallthrough, unreachable behavior, invalid assumptions, and termination/progress failures.
- `boundary-and-input`: null/empty/extreme/malformed inputs, equivalence boundaries, numeric limits, encoding/serialization, validation order, and trust-boundary handling.
- `state-and-data-integrity`: illegal transitions, partial updates, ownership violations, corruption, stale state/cache, transaction boundaries, duplicate side effects, idempotency, and retry effects.
- `error-handling-and-recovery`: swallowed/misclassified errors, incorrect fallback, rollback, timeout, cancellation, backpressure, partial failure, and recovery to a usable state.
- `resource-lifetime-and-cleanup`: memory, file, socket, transaction, task, thread, timer, subscription, lock, and handle lifetime on success, failure, timeout, and cancellation.
- `concurrency-and-ordering`: races, deadlocks, lock ordering, lost wakeups, non-atomic operations, visibility/order defects, starvation, reentrancy, and cancellation races.
- `interface-and-compatibility`: exported behavior, consumer assumptions, API/wire/runtime semantics, migration paths, caller compatibility, and cross-module contracts.
- `security-and-capacity`: authorization/authentication, injection, secret exposure, unsafe deserialization, path traversal, amplification, unbounded work/storage, and algorithmic resource exhaustion.
- `test-adequacy`: whether tests can expose the normal, boundary, negative, error, lifecycle, concurrency, compatibility, and cross-module failures applicable to the delivered behavior; passing tests alone are not adequate evidence.

For documentation-only or non-runtime tasks, use the same categories. Mark truly inapplicable runtime categories `not-applicable` with a task-specific explanation and still inspect requirement accuracy, downstream interpretation risk, examples, compatibility, and validation adequacy.

## Evidence Standard
- Evidence names concrete source paths, symbols, call paths, tests, runtime observations, or requirement sections. "Reviewed code", "looks correct", "tests pass", and workflow/checker status are not concrete defect-discovery evidence.
- Review the delivered behavior beyond the changed lines when relevant callers, shared state, interfaces, or cleanup paths can expose a defect.
- Passing tests support a category only when the reviewer inspected what they exercise. Missing, stale, overly broad, or assertion-weak tests are `test-adequacy` findings.
- A report checker can enforce category coverage, evidence shape, and conclusion consistency. It cannot prove that the reviewer found every real defect; AI MUST NOT describe a checker pass as functional correctness.

## Requirement And Document Review
- `proposal.md` is the mandatory requirement baseline. Every task `change_id` MUST have a requirement-coverage row tied to concrete implementation evidence.
- When the task contains GitHub issue information, reread the issue description and verify its behavior and acceptance conditions directly before evaluating process closure. A proposal, design, implementation, test, or report that is internally consistent but misses or narrows the issue outcome cannot be accepted.
- When `design.md` or auto-pipeline `pipeline/plan.md` exists, inspect both whether implementation follows it and whether the design itself can produce incorrect behavior.
- When `testing.md` or `testplan.yaml` exists, inspect both document/implementation consistency and whether the resulting tests can reveal relevant defects.
- Agreement among proposal, design, code, and tests is not sufficient when they share the same invalid assumption.

## Decision And Routing
- A requirement ambiguity, contradiction, or incorrect acceptance boundary first produces a blocking requirement finding and `rejected` result in the canonical report; acceptance then stops and asks the user to decide.
- A design defect returns to design.
- Missing required behavior or a defect in code against an adequate requirement/design returns to implementation.
- Missing, stale, weak, or non-runnable validation that can hide a defect returns to testing.
- A design-document-only mismatch returns to design, and a testing-document-only mismatch returns to testing. A proposal-document problem is a requirement problem and follows the `rejected` user-decision path.
- Acceptance does not repair requirements, design/testing documents, tests, or implementation in the same task.
- `accepted` is allowed only when every required category is `pass` or concretely `not-applicable`, every task `change_id` is covered, no blocking finding remains, and document consistency passes for every present source.
- Use `needs changes` when the approved intent remains valid and a fix can be routed to design, implementation, testing, or a document-owning stage.
- Use `rejected` when the requirement baseline or acceptance boundary cannot be accepted without a user decision, or when the task must be abandoned or re-scoped rather than corrected within the approved intent.
- For a requirement rejection, record the blocking requirement finding and `rejected` result, then stop for the user without completing or automatically returning the task.

## Report
- Put findings first and sort them by severity.
- Record `change_id` requirement coverage and the complete defect-discovery table before the conclusion.
- Classify findings by owning stage: `requirement`, `design`, `implementation`, or `testing`.
- Record conditional document consistency, a plain-language result, and the next action after defect discovery.
- `acceptance-report-check.py` validates this structure and its internal consistency; it is a guard against skipped review work, not a correctness oracle.
