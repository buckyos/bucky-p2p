# Implementation Rules

## Goal
- Define preparation, traceability, and quality guidance for implementation and bugfix work.
- This rule applies after `task-entry-gate-rules.md` classifies a request as implementation-shaped.

## Scope
- High-risk implementation tasks, bugfix tasks, and production-code changes for versioned task packets.
- Trivial and standard implementation follow the shorter flows in `harness/rules/task-entry-gate-rules.md` and do not require the inputs or entry checks below.

## Required Inputs
- Active task packet selected according to `harness/rules/task-entry-gate-rules.md`.
- Manual flow: approved `proposal.md` and `design.md` with direct `change_id`, target-module, and Scope Path mappings.
- Explicitly launched pipeline: launch-confirmed `proposal.md` plus the boundary-selected design source. Use validated `pipeline/plan.md` mappings when design is automatic; otherwise require approved manual `design.md`.
- Canonical `task.yaml` containing the active stage, target modules, change ids, Scope Paths, and task-level `risk-profile.yaml`.

## Entry Checks
- Run `UV_CACHE_DIR=.harness/uv-cache uv run --active python ./harness/scripts/harness-check.py --task <packet>/task.yaml --profile pre-edit` when preparation evidence is useful.
- Whenever implementation or a later automatic stage uses manual `design.md` as its active design source, the pre-edit profile invokes `schema-check.py --require-approved` and fails unless the prior manual proposal/design documents are approved. Only pipelines whose design stage itself is automatic use `pipeline/plan.md` instead of the manual design approval gate. The profile also validates task identity, the current risk profile, and task/design Scope Path consistency.
- For auto-pipeline flow, the pre-edit profile also runs the complete structural `pipeline-plan-check.py` before implementation starts; scope-row parsing alone is not sufficient admission.
- Approved status alone is not enough: read the proposal plus active design source and confirm that each current `change_id` is directly covered.
- Older packets, broad module overviews, historical notes, oral context, or chat claims do not replace current task-packet coverage.
- Missing direct proposal/design coverage follows task-entry return routing; do not implement first and document later.

## Execution Guardrails
- Implement the minimum production change needed to satisfy the governing proposal/design and current request.
- When the task contains GitHub issue information, treat the issue-described behavior and acceptance conditions as the primary execution target. Use proposal/design/process artifacts to help deliver that target, never as a reason to prioritize Harness housekeeping over required implementation or to accept a narrower outcome.
- Recheck the current implementation direction against the issue after material discoveries or design deviations; spend the next useful step closing an issue requirement or validating it rather than polishing secondary process artifacts.
- When the design lists multiple file-level modules, follow `## File-Level Implementation Sequence` in dependency order and create child tasks in that order when needed.
- Each implementation child task starts with the relevant proposal/design excerpts, child design document, `change_id`, interfaces, and likely source files, and may inspect or change additional files as needed.
- If actual impact differs materially from `Scope Paths`, update traceability evidence when useful.
- Prefer leaving test implementation for post-implementation testing, but combined or supporting edits are allowed when useful for the current request.
- Match surrounding style, naming, and structure.
- Do not refactor, reformat, rename, rewrite comments, add features/options, or clean adjacent code unless covered by the task.
- Remove only artifacts made unused by the current change.
- Record unrelated defects as residual risk or follow-up instead of fixing them inside the implementation task.

## Verification Default
- Repositories may choose a strict default where implementation does not proactively run validation commands.
- In that mode, tests run only when the user asks, debugging needs evidence, or task docs/repo-local rules require validation.

## Rust Formatting Default
- Rust agents MUST NOT automatically run `cargo fmt`; run it only on explicit user request or repo-local rule.

## Return Routing
- Missing/draft proposal or missing direct proposal mapping: proposal task.
- Manual flow missing/draft design, or either flow missing direct design mapping or changed boundaries/interfaces: design follow-up. Auto-pipeline records the correction in pipeline-plan mappings rather than `design.md`.
- Missing active module, task packet, or `change_id`: task entry or the owning upstream document stage.
- Failed schema, risk-profile, or plan checker: owning-stage follow-up.
- Approved docs that do not cover the current task: prefer a sibling or amendment task for history, or update the packet with an explicit reason/status change.
- Upstream contradiction discovered during implementation: owning upstream stage.
