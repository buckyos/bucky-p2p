# Auto Pipeline Rules

## Goal
- Define the repository's fully automatic downstream workflow after explicit user launch.
- Make stage planning, child-task execution, return-routing, and exit conditions explicit.

## Trigger
- Auto-pipeline is a high-risk workflow facility. If a trivial or standard task is explicitly launched as an auto-pipeline, update the existing proposal packet to confirmed high-risk and expand it with the required full-lifecycle artifacts before launch.
- This rule is inactive unless the user explicitly asks to enter it.
- Entry signal: user explicitly asks to enable, launch, run, or enter the automatic pipeline
- Agents MUST NOT infer, synthesize, or self-issue this entry signal. `pipeline/plan.md` MUST copy the user's explicit launch instruction verbatim into `User launch statement`; without that current user instruction, `User launch confirmed` remains unset and the normal manual workflow applies.
- Required prerequisite: the bound `proposal.md` exists
- Explicit user launch confirms the bound proposal for this pipeline; separate proposal approval metadata is not required
- The plan MUST bind exactly one task packet by `Version`, `Packet module`, and `Task name` under `## Trigger`; it MUST NOT change document policy for unrelated packets.
- The plan and `task.yaml` MUST record the first automatic stage. The launch stage and every earlier stage retain manual-stage document and checker semantics; automatic execution starts at the immediately following stage.
- If the packet already has completed manual-prefix receipts when explicit launch changes `task.yaml` from `mode: manual` to `mode: auto-pipeline`, validate the launch plan and then run `lifecycle-check.py --task <packet>/task.yaml --refresh-manual-bindings` before the first automatic stage. A manual-prefix receipt written after launch already carries the launched binding and does not need migration.
- `--refresh-manual-bindings` is a one-transition migration, not a stale-receipt repair tool. For current versioned bindings it accepts only the exact task policy with manual mode and no automatic start stage. For an unversioned legacy binding whose old policy fields cannot be recovered, the validated current launch plan becomes the migration boundary. In both cases it revalidates receipt inputs, writes the current binding schema, and freezes the launched policy. Later changes to the first automatic stage, canonical plan path, change ids, target modules, Scope Paths, or durable artifact paths are not migratable.
- `Packet module: globals` is the specialized cross-project packet keyword. Its `Target module(s)` list contains concrete projects, and each project receives an independent implementation scope check with `--target-module`.
- Optional launch command or workflow entry:

## Priority Override
- After explicit user launch, this rule overrides stage document policy only at or after `auto_pipeline_start_stage`. Earlier stages retain normal manual policy.
- The override changes document policy while preserving proposal binding, change mapping, validation, and acceptance; none of these mechanisms controls project file access.

## User Authorization Precedence
- Explicit user instructions have highest priority for entering auto-pipeline mode, requested pipeline scope, and whether per-stage user confirmation is required.
- If the user explicitly asks auto-pipeline mode to handle all subsequent stages or the whole downstream workflow, the pipeline MUST NOT stop after each stage to ask for separate user confirmation before continuing.
- That launch instruction authorizes downstream design, implementation, post-implementation testing, and acceptance execution according to the pipeline plan.
- Automatic design MUST NOT generate or update `design.md` or task-local `design/`; its design source is `pipeline/plan.md`. A design stage before the first automatic stage remains manual and uses normal `design.md` rules.
- Automatic testing MUST NOT generate or update `testing.md` or task-local `testing/`; it MUST generate `testplan.yaml` and record coverage and gaps in `.harness/pipelines/<version>/<packet-module>/<task-name>/state.json`. A testing stage before the first automatic stage retains normal manual testing semantics.
- The packet's single `risk-profile.yaml` is the only seven-category Trigger Matrix. `pipeline/plan.md` and `testplan.yaml` reference it; pipeline design completes applicable evidence paths and `required_checks` there without copying the matrix.
- Manual stages continue running their normal `doc-structure-check.py` checks. Automatic mapping stages do not run it; structure is validated by the schema, pipeline-plan, and testing-coverage checkers as applicable.
- Proposal-stage schema and document checks inspect only `proposal.md`; they MUST NOT test for, read, or reject an existing `design.md` or task-local `design/`. The automatic design stage owns enforcement of its no-`design.md` policy after that stage begins.
- If design is manual because `auto_pipeline_start_stage` is implementation, testing, or acceptance, its `design.md` MUST be approved before the first automatic stage starts. `harness-check.py --profile pre-edit` enforces this approval gate for automatic implementation or testing.
- If testing is manual because `auto_pipeline_start_stage` is acceptance, testing coverage remains in manual `testing.md` plus `testplan.yaml`; pipeline completion MUST NOT require duplicate runtime `state.json.testing_evidence`. Only automatic testing owns runtime-state testing evidence.
- Every script that consumes canonical `task.yaml` fields MUST reuse `harness/scripts/task_manifest.py`; independent regular-expression parsers are forbidden.
- When design is automatic, design outputs MUST be recorded in task-local `pipeline/plan.md` as dependency graphs, interfaces, compatibility/migration decisions, state/failure models, rejected alternatives, implementation order, and concrete `Scope Paths`. When design is manual, the plan records `Design source: design.md` and the normal design document remains authoritative. Automatic testing outputs MUST include `testplan.yaml`, runtime-state coverage/evidence, test code, runner wiring, and machine-written run artifacts.
- When creating or intentionally revising `pipeline/plan.md`, rerun pipeline-plan validation before continuing. A design revision invalidates the earlier plan result and Scope Path binding; ordinary state updates never alter the plan.
- After manual receipt bindings have been migrated, the launch boundary is frozen: changing mode, the first automatic stage, the canonical plan path, task Scope Paths, or other receipt-bound policy requires returning to the appropriate manual stage and recording fresh receipts. Re-running the refresh command cannot bless those policy changes.
- When a repository-local extension adds a document-producing stage, the pipeline MUST auto-confirm that stage by updating the produced stage document front matter to:
  - `status: approved`
- Auto-confirmation happens only after that stage's declared done criteria and required checks pass.
- Schema validation checks only the resulting document status; it does not validate approval provenance.
- Outside an explicitly launched pipeline, the normal approval authority rule applies: agents set `status: approved` only after explicit user approval.
- The parent orchestrator normally coordinates merges for shared artifacts to reduce conflicts. This is an integration convention, not a Harness write permission; child edits remain allowed and are reported for merging.
- After each child task completes, the parent orchestrator MUST update runtime `.harness/pipelines/.../state.json` task status to `confirmed` or `complete` before continuing to dependent tasks.
- The pipeline MUST run the full structural `pipeline-plan-check.py` through `harness-check.py --profile pre-edit` before every downstream auto-pipeline stage begins, and rerun it after modifying `pipeline/plan.md` or runtime `.harness/pipelines/.../state.json`. It MUST use `--require-complete` only after completion state or completion evidence changes. Complete mode verifies the bound task's `testplan.yaml`, successful matching `<module>/<task-name> all` artifact, and reruns the focused acceptance-report checker against the final report. It MUST NOT trigger package/module tests, `all all`, root shortcuts, or quality gates.
- Implementation completion MUST be recorded in runtime `.harness/pipelines/.../state.json` and implementation evidence, and final acceptance MUST be recorded in runtime state and the acceptance report.
- This authorization confirms the bound proposal and keeps workflow validation and final acceptance; none of those mechanisms restrict project file access.

## Acceptance Baseline
- Final acceptance baseline: the user-launch-confirmed `proposal.md`
- Downstream artifacts (pipeline-plan design mappings when design is automatic, runtime-state testing evidence when testing is automatic, manual design/testing documents before the boundary, the acceptance report, implementation, and tests) may refine execution detail but MUST NOT contradict, narrow, or silently expand the proposal.
- When downstream artifacts or code disagree, fixes MUST preserve the launch-confirmed proposal and route non-requirement defects through design -> implementation/code -> testing implementation.
- Code MUST conform to design, and tests MUST verify proposal/design/code behavior.

## Stage Responsibilities
- Proposal responsibility:
  - define the user-confirmed baseline of goals, scope, non-goals, and constraints
- Pipeline planning responsibility:
  - plan stage tasks, dependencies, outputs, and done conditions before execution starts
- Design responsibility:
  - convert the launch-confirmed proposal into submodules, dependencies, key call flows, exported interfaces, external dependencies, and implementation order
  - decompose design top-down from the whole affected module to submodules, nested submodules, and file-level modules
  - for automatic design, record every child mapping in `pipeline/plan.md`; for manual design, use the normal indexed `design.md` / `design/` artifacts
  - exclude test cases, test plans, test strategy, validation IDs, fixtures, testability seams, and test implementation from design-stage outputs
  - produce no persistent `design.md` or task-local `design/` documents only when design is automatic
  - keep module and submodule dependencies acyclic
  - make every dependency row belong to one level and parent, and reject unknown cross-parent dependencies
  - name a concrete consumer and compatibility decision for every exported interface; breaking or migration-required interfaces also name affected callers and a migration path
  - record old symbol -> new path -> concrete repository consumer file -> migration status rows for breaking/migration-required APIs and crate-root/build-surface changes
  - assign every persistent/shared state to exactly one owner and record lifecycle plus failure transitions
  - record handling for every key cross-boundary failure flow
  - record rejected boundary, technical, and collaboration alternatives with concrete reasons
  - split submodules by business logic first, extract shared implementation logic into shared submodules, and isolate clear technical areas such as HTTP interfaces or persistence/database access
- Testing responsibility:
  - after implementation completes, design test cases from proposal, the active design source, and delivered code, then generate test implementation and runnable evidence
  - when testing is automatic, produce no persistent `testing.md` or `testing/`; generate `testplan.yaml` and record coverage and gaps in runtime state
  - generate risk-triggered task-local contract checks and scoped evidence inputs; affected package/workspace compile-only closure is allowed without broad runtime test execution
- Implementation responsibility:
  - deliver the smallest production code changes that satisfy the launch-confirmed proposal and design inputs
  - execute file-level implementation child tasks in the active design source's dependency order
- Acceptance responsibility:
  - independently try to falsify the delivered behavior before selecting a conclusion
  - review every required defect-discovery category, including design correctness and test adequacy
  - treat pipeline status, receipts, passing tests, and document consistency as supporting evidence rather than correctness proof
  - use the task-packet `acceptance-report.md` as the canonical output

## Pipeline Planning Rule
- Before execution starts, the pipeline MUST create a plan for:
  - design tasks that update pipeline-plan mappings when design is automatic, or a manual design prerequisite row when design precedes the automatic boundary
  - implementation tasks
  - testing tasks that use runtime-state evidence when testing is automatic, or a manual testing prerequisite row when testing precedes the automatic boundary
  - acceptance tasks
- The planner MUST declare:
  - task ids
  - stage
  - responsibility
  - scope
  - dependencies
  - outputs
  - done conditions
- The planner MUST declare `Launch stage`, `First auto stage`, `Design source`, and `Execution Mode` for every stage/submodule row. Only `auto-pipeline` rows are automatically dispatched; manual rows are prerequisite gates.
- Implementation tasks start from the pipeline-plan `## File-Level Implementation Sequence`; dependency-related work follows dependency order while independent work may run concurrently.
- Each implementation child task starts with relevant proposal/design context and may read or modify additional project files needed for the request.

## Implementation Preparation
- The task entry classifier and unified preparation profile provide workflow evidence; neither authorizes project file edits.
- Implementation preparation should establish:
  - the user-launch-bound `proposal.md` exists
- The active design source records design coverage, concrete `target_module`, and concrete `Scope Paths` for every current `change_id`: `pipeline/plan.md` for automatic design, otherwise manual `design.md`.
- Implementation tasks MUST read the launch-confirmed proposal and the boundary-selected active design source, then confirm they cover the current task before coding.
- Implementation tasks MUST identify explicit `version`, packet `module`, `target_module`, and `change_id` values before coding. `--module globals` always requires a concrete `--target-module`.
- If the requested task module is clearly different from unfinished task records, the pipeline MUST create a new task packet immediately and MUST NOT consider continuing a different-module unfinished task.
- Implementation tasks for direct submodule packets MUST also identify explicit `submodule`.
- Implementation tasks SHOULD run schema, risk-profile, and pipeline-plan validation for traceability; failures identify follow-up and do not restrict project file access.
- Implementation tasks for direct submodule packets MUST pass those checks with `--submodule <submodule>`.
- Cross-module and cross-submodule mappings SHOULD remain traceable without creating path-based permissions.
- If the launch-confirmed proposal is incomplete, the pipeline MUST stop and require a corrected proposal plus a new explicit user launch; incomplete design coverage returns to the owning design stage without changing its manual/automatic policy.
- Bugfix tasks follow the same rule unless the repository publishes a narrower exception path.
- If any prerequisite is missing or unconfirmed, the task MUST return to the owning upstream stage.

## Stage Execution Rule
- Each automatic stage MUST execute as an independent child task. Manual stage rows complete through the normal manual workflow and are not dispatched as automatic children.
- Before dispatch, derive the agent from `AGENTS.md` `## Harness Step Agent Mapping`; tasks and runtime state do not declare or select roles.
- `guardian` and `/review` MUST NOT be recorded as agent roles: guardian is an approval-review mechanism and `/review` is a code-review workflow. Neither replaces Harness user approval, pipeline launch evidence, schema/stage checks, or final acceptance.
- At every scheduling point, compute dependency-ready work and use available concurrency. Coordination may avoid simultaneous edits to the same file, but no lock or declared scope grants or denies repository file access.
- Parent/child ownership of shared coordination artifacts is a merge convention, not a write prohibition; any necessary project-file edit remains allowed and should be reported for integration.
- Preparation and completion profiles validate workflow documents and evidence rather than granting filesystem permission. Completion MUST reject changed paths outside the active stage artifact group; it MUST NOT reject a path solely because it differs from design `Scope Paths` metadata.
- A testing child task that may edit an existing Rust inline test asks the direct-content capture to copy that exact file under git-ignored `.harness/baselines/<version>/<task-id>-testing/` for line-level comparison; it never synthesizes a Git index, tree, or commit.
- Upstream-stage work may synchronize downstream artifacts when useful; record the synchronization or reopen follow-up explicitly.
- If a stage contains direct submodules, the pipeline MUST create independent child tasks for those submodules or record a concrete merged-task reason in the pipeline plan.
- `.harness/pipelines/<version>/<packet-module>/<task-name>/state.json` MUST record scheduler strategy, shared-artifact ownership, and scheduling waves. Each wave lists automatic child tasks launched together; tasks in one wave MUST have no dependency relationship, and completed pipelines require every automatic task to appear in at least one wave. Manual prerequisite rows are recorded complete but never appear in an automatic wave.
- Each child task MUST have:
  - one owner
  - one clear output
  - expected impact for traceability, never a file permission boundary
  - explicit dependencies
  - observable done criteria
- `pipeline-plan-check.py` validates plan structure, dependencies, task binding, change mapping, evidence, and exit conditions. It MUST NOT fail because `Scope Paths` are broad or actual project edits fall outside them.

## Recommended Stage Order
1. Design planning and design tasks that write pipeline-plan mappings when design is automatic, or a completed manual-design prerequisite when design is before the automatic boundary
2. Implementation tasks
3. Testing planning and testing implementation tasks that generate `testplan.yaml`, write coverage and gaps to `.harness/pipelines/<version>/<packet-module>/<task-name>/state.json`, and produce runnable evidence when testing is automatic, or a completed manual-testing prerequisite when testing is before the automatic boundary
4. Acceptance task

## Recursive Submodule Rule
- If `proposal.md` and the active design source define direct submodules for the same task, the pipeline MUST create independently addressable child tasks in the bound pipeline plan and mirror them in:
  - design child tasks
  - testing child tasks
  - implementation child tasks where ownership can be separated safely
- When design is automatic, layered design artifacts for the same task MUST live in `pipeline/plan.md` design mapping sections; implementation and testing child tasks reference those sections instead of copying their contents or creating `design/<submodule>.md` files. When design precedes the automatic boundary, normal manual `design.md` / `design/` structure remains authoritative and the plan references it.
- Same-task child tasks do not create nested task packets. Independent submodule proposal/design artifacts for a separate new requirement require their own sibling task packet, such as `docs/versions/<version>/modules/<module>/<task-seq>-<task-slug>/proposal.md`; do not hide new requirements in an older packet or nest a sequence-prefixed packet inside it.
- Shared cross-cutting topics may be separate child tasks if they have clear boundaries.

## Acceptance Task Rule
- Final acceptance MUST compare delivered results back to the launch-confirmed `proposal.md`.
- Final acceptance MUST run as an independent child task and MUST NOT adopt implementation-child summaries or pipeline completion state as its conclusion.
- Final acceptance MUST search for requirement, design, implementation, and validation defects across every category required by `acceptance-review-rules.md`.
- When pipeline-plan design mappings or testing documents exist, final acceptance MUST inspect their correctness and adequacy as well as implementation consistency.
- Final acceptance MUST apply `harness/rules/acceptance-review-rules.md`.
- Acceptance MUST output findings, exact requirement coverage, the complete independent defect-discovery table, conditional document-consistency results, and only then an accepted/rejected/needs changes conclusion in `acceptance-report.md`.
- Acceptance MUST NOT generate separate acceptance-rule, expected-result, or command-summary documents; required correctness and test-adequacy evidence stays in the report.

## Return Routing Rule
- Report conclusion `needs changes` is serialized as `acceptance.status: needs-changes` in runtime state. Report conclusion `accepted` or `rejected` is serialized unchanged.
- A `needs changes` acceptance MUST return work to the correct earlier stage instead of exiting.
- If acceptance finds a proposal ambiguity, contradiction, incorrect requirement, or incorrect acceptance boundary, it MUST first finish the canonical report with a blocking requirement finding and `rejected`, set runtime acceptance status to `rejected`, and then stop and ask the user to decide. It MUST NOT infer the intended proposal, create an automatic proposal return task, or append a return record for that rejected run.
- Missing required behavior or implementation defects return to implementation.
- Design defects return to design, and missing or inadequate defect-detection coverage returns to testing.
- If implementation satisfies the requirement but conflicts with an existing design or testing document, return the stale document to its owning stage.
- If the same unresolved issue remains after more than 5 unsuccessful iterations, the pipeline MUST stop and report the issue to the user.
- Mutable return history lives only in runtime `.harness/pipelines/<version>/<packet-module>/<task-name>/state.json` `return_records`: every `needs changes` run MUST append a return record (blocking issue id, owning stage, target task, reason, expected fix output), and the iteration count for an issue is the number of runtime return records with the same blocking issue id. A `rejected` run records its report/status but appends no return record because no stage is reopened. `pipeline/plan.md` contains stable return-routing rules only; do not store mutable return records or counters there, or track them from memory or chat context.
- `pipeline-plan-check.py --require-complete` MUST fail if final exit-condition checkboxes are not complete.

Minimum routable return categories:
- design defect or design consistency issue
- implementation issue
- testing inadequacy or testing-document consistency issue

For each `needs changes` acceptance run, record:
- blocking issue id
- owning stage
- target task to reopen or recreate
- reason for return
- expected fix output

## Exit Condition
- The pipeline MUST continue until:
  - proposal-defined outcomes are satisfied
  - blocking issues are closed
  - required tests and evidence exist
  - final acceptance passes

`task.yaml.stage` is the manual launch-stage cursor in auto-pipeline mode; it does not advance through automatic stages. Runtime `state.json` exclusively records automatic stage progression. Therefore task removal MUST ignore the manifest stage for auto-pipeline tasks and rely on a complete accepted runtime state plus the normal report and lifecycle checks.

## Guardrails
- The pipeline MUST NOT skip planning and jump straight into implementation.
- The pipeline MUST NOT treat missing active-design-source mappings as implementation-ready.
- The pipeline MUST NOT treat missing post-implementation test evidence as acceptance-ready.
- Automatic design/testing stages MUST NOT generate their corresponding Markdown stage documents. Manual stages before the first automatic stage continue using those documents. Automatic testing still MUST generate `testplan.yaml`, record coverage and gaps in runtime state, and produce executable evidence.
- The pipeline MUST NOT treat one failed acceptance as terminal completion.
- The pipeline MUST NOT let downstream documents override proposal intent.
- The pipeline MUST record the ownership or validation boundary for every child task in the pipeline plan; `pipeline-plan-check.py` rejects unnecessary task depth when it creates unclear ownership or evidence.

## Suggested Companion Files
- `pipeline/plan.md` or equivalent generated plan artifact
- child task template
- acceptance report template
