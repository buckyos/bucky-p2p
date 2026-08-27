# Acceptance Task Rules

This separate acceptance stage is required only for high-risk work by default. Trivial and standard flows use a proportional independent defect-discovery review in their completion report.

## Goal
- Independently try to falsify the claim that the delivered behavior is correct by applying `harness/rules/acceptance-review-rules.md`.

## Primary Output
- The canonical acceptance output is the active packet's `acceptance-report.md` and, for auto-pipeline, final/return status under `.harness/pipelines/`.

## Execution
1. Before dispatching acceptance, run `lifecycle-check.py --task <packet>/task.yaml --require-prior acceptance` as an entry-eligibility check. A passing receipt chain permits the stage to start but is not correctness evidence; a missing receipt returns to its owning stage.
2. Start acceptance as a task separate from implementation and testing. Prefer a reviewer that did not implement the change when the environment supports it.
3. Read current primary sources: any GitHub issue description supplied by the task, `proposal.md`, delivered implementation, relevant callers/dependencies, test design, test code, and available runtime evidence. When issue information exists, judge the delivered outcome against it before process closure or document consistency. Do not adopt an implementation self-review, previous acceptance conclusion, lifecycle status, or pipeline completion claim.
4. Generate failure hypotheses and inspect every required category in `acceptance-review-rules.md`. Record concrete evidence, findings, or a task-specific `not-applicable` reason.
5. Verify exact requirement coverage for every task `change_id` and, when issue information exists, every issue-described behavior and acceptance condition. Missing, narrowed, or contradicted issue behavior is a blocking requirement finding. Investigation may interleave requirement coverage and category review; the report records requirement coverage before the category table, and both precede result selection.
6. Review design correctness and consistency when a design source exists. Review test adequacy and testing-document consistency when testing sources exist.
7. Write findings and defect-discovery sections before choosing a conclusion.
8. If the result is `needs changes`, run `task-transition.py --task <packet>/task.yaml return --to <design|implementation|testing>` for the owning stage. If a requirement decision is needed, finish the canonical report with a blocking requirement finding and `rejected`, then stop for the user without a transition.
9. Only for `accepted`, run `task-transition.py --task <packet>/task.yaml complete`; it records the acceptance receipt after the report coverage check passes.
10. Only after accepted completion, run `harness/scripts/task-index.py remove --task <packet>/task.yaml`; task removal closes bookkeeping and MUST NOT be described as proof that the feature is defect-free.
