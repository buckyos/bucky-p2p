# Auto Pipeline Rules

## Goal
- Define the repository's fully automatic downstream workflow after explicit user launch.
- Ensure the pipeline continues until proposal-based acceptance succeeds.

## Start Condition
- This rule is inactive unless the user explicitly asks to enter it.
- the bound `proposal.md` exists
- user explicitly asks to enable, launch, run, or enter the automatic pipeline
- explicit user launch confirms the bound proposal; separate proposal approval metadata is not required
- agents never infer or synthesize launch; the pipeline plan records the user's explicit launch instruction verbatim as `User launch statement`
- `pipeline/plan.md` binds the launched pipeline to one `Version`, `Packet module`, and `Task name`; unrelated task packets keep their normal document policy

## Priority Override
- After explicit launch, this rule overrides normal task-entry and implementation-admission requirements for persistent `design.md` and `testing.md` only.
- Auto-pipeline uses validated `pipeline/plan.md` dependency graphs, interface consumers/compatibility, state ownership/failure transitions, failure-flow handling, rejected alternatives, design mappings, and concrete `Scope Paths`; every other approval, admission, scope, validation, and acceptance gate remains mandatory.

## User Authorization Precedence
- Explicit user instructions have highest priority for entering auto-pipeline mode, requested pipeline scope, and whether per-stage user confirmation is required
- If the user explicitly asks auto-pipeline mode to handle all subsequent stages or the whole downstream workflow, the pipeline continues through design, implementation, post-implementation testing, and acceptance without stopping for separate user confirmation after each stage
- Auto-pipeline design and testing stages do not produce `design.md` or `testing.md`; testing produces `testplan.yaml`, and mappings live in `pipeline/plan.md`.
- If a repository-local extension adds another document-producing stage, the pipeline auto-confirms it with `status: approved`, `approved_by: auto-pipeline`, `approved_at`, and `approved_content_sha256`.
- Auto-confirmation happens only after that stage's declared done criteria and required checks pass
- The parent orchestrator is the sole writer of shared `pipeline/plan.md`, runtime `.harness/pipelines/.../state.json`, unfinished-task indexes, shared `testplan.yaml`, and shared test-runner registration; child agents return scoped results for parent merging
- After each child task completes, the parent updates runtime `.harness/pipelines/.../state.json` task status to `confirmed` or `complete` before continuing to dependent tasks
- Implementation completion is recorded in runtime `.harness/pipelines/.../state.json` and implementation evidence, and final acceptance is recorded in runtime state and the acceptance report
- This does not waive proposal authority, stage write scopes, `stage-scope-check.py`, implementation admission, schema checks, admission checks, required validation, or final acceptance

## Final Acceptance Baseline
- Final acceptance is judged against the approved `proposal.md`
- Pipeline-plan design/testing mappings, optional acceptance artifacts, implementation, and tests are supporting evidence and execution artifacts
- If downstream artifacts conflict with the launch-confirmed proposal, the proposal is authoritative
- If downstream artifacts or code disagree, fixes preserve the launch-confirmed proposal and route non-requirement defects through design -> implementation/code -> testing implementation
- Code follows design; tests verify proposal/design/code behavior

## Stage Responsibilities
- Proposal responsibility:
  - define the approved baseline outcome for the pipeline
- Pipeline planning responsibility:
  - create the task graph, dependencies, outputs, and done conditions before execution starts
- Design responsibility:
  - convert the launch-confirmed proposal into executable structure and interfaces recorded in `pipeline/plan.md`, without generating `design.md`
  - keep module and submodule dependencies acyclic
  - split submodules by business logic first, extract shared implementation logic into shared submodules, and isolate clear technical areas such as HTTP interfaces or persistence/database access
  - keep design at module shape level: submodules, dependencies, key call flows, exported interfaces, and external module dependencies
  - decompose design top-down and index child design mappings in `pipeline/plan.md` without generating task-local design documents
  - finalize a file-level implementation sequence ordered by dependencies
  - exclude test cases, test plans, test strategy, validation IDs, fixtures, testability seams, and test implementation from design-stage outputs
- Implementation responsibility:
  - deliver production code inside launch-confirmed proposal/design boundaries
- Testing responsibility:
  - after implementation completes, design test cases from proposal, pipeline-plan mappings, and delivered code, then generate test implementation and `testplan.yaml` without generating `testing.md`
- Acceptance responsibility:
  - evaluate document coverage, document consistency, document-to-implementation consistency, and logic, then return failures to the correct earlier stage

## Mandatory Planning Step
- The pipeline MUST create or refresh `pipeline/plan.md` before starting downstream execution
- The plan MUST list:
  - top-level stage tasks
  - child tasks per task packet when needed
  - stage responsibility for each task
  - dependencies between tasks
  - outputs and done conditions
  - return-routing targets for failed acceptance

## Stage Execution Model
- Checker execution is change-triggered. Reuse passing schema, admission, stage-scope, pipeline-plan, and task-test evidence while their owned inputs remain unchanged; stage transitions and acceptance do not replay them.
- Design, implementation, testing, and acceptance MUST run as separate child tasks
- At each scheduling point, launch the maximum dependency-ready set whose reserved write scopes do not overlap, up to all runtime-available child-agent slots; immediately backfill free slots and do not wait while a ready non-conflicting task can run
- Serialize only for an explicit dependency, an overlapping write scope, or exhausted runtime capacity, and record the reason with each scheduler wave in `.harness/pipelines/.../state.json`
- Each child task MUST keep direct writes inside its reserved stage scope unless the user explicitly requested cross-stage synchronization. Children return proposed shared-artifact changes; only the parent applies them. Testing children may write exclusively owned runner files and `.harness/test-results/test-runs/*.json` evidence
- Each single-stage child task MUST record its durable changed paths, then run `stage-scope-check.py --stage <stage> --changed-paths-file .harness/evidence/<version>/stage-scope/<task-id>.paths` before completion and fail on out-of-stage task paths; `.harness/` runtime paths are omitted from the manifest
- Upstream-stage changes MUST NOT automatically edit downstream-stage artifacts; create or reopen the downstream child task instead
- If task packets exist, stage tasks MUST be decomposed into submodule child tasks or the pipeline plan MUST record a concrete merged-task reason
- Every new task MUST use a task packet under `docs/versions/<version>/modules/<project>/<task-seq>-<task-slug>/`; cross-project tasks use `docs/versions/<version>/modules/globals/<task-seq>-<task-slug>/`; optional post-implementation testing artifacts live in the same packet
- Each child task MUST have:
  - one owner
  - one scope boundary
  - one output
  - clear dependencies
  - observable done criteria

## Implementation Admission
- The task entry gate still applies inside the pipeline: implementation tasks MUST classify scope and run admission before editing production code, build files, or resources.
- Implementation MUST NOT start unless the user-launch-bound `proposal.md` exists and `pipeline/plan.md` contains validated design coverage and concrete `Scope Paths` for every admitted `change_id`.
- Implementation MUST inspect the launch-confirmed proposal and pipeline-plan mappings, record task-specific evidence, and pass `admission-check.py --evidence-file ...` before coding.
- Implementation MUST identify explicit `version`, `module`, and `change_id` values before coding.
- Implementation child tasks MUST follow the approved file-level implementation sequence and limit context to the current file-level module.
- Implementation for a task packet MUST also identify explicit `submodule`.
- If the requested task module is clearly different from unfinished task records, the pipeline MUST create a new task packet immediately and MUST NOT consider continuing a different-module unfinished task.
- Implementation MUST pass `schema-check.py` and `admission-check.py` for each affected project-level implementation scope.
- Implementation for a task packet MUST pass those checks with `--submodule <task-seq>-<task-slug>`.
- Cross-project implementation uses the specialized `globals` packet keyword and passes admission independently for every concrete target with `--module globals --submodule <task-name> --target-module <project>`.
- Cross-submodule implementation MUST pass admission independently for every affected task packet.
- If any prerequisite is missing or still draft, return work to the owning stage; if approved docs are incomplete for the task, create or route to a sibling task packet or amendment/fix task instead of editing the approved packet.

## Return Routing
- If acceptance fails, the pipeline MUST return to the correct earlier stage
- Non-requirement failures repeat design -> implementation -> testing, then rerun acceptance.
- If the same unresolved issue remains after more than 5 unsuccessful iterations, stop and report the issue to the user.
- Acceptance MUST apply `harness/rules/acceptance-review-rules.md` and audit admission for every module needed as evidence for accepted behavior.
- Acceptance MUST audit admission for every task packet needed as evidence for accepted behavior.
- Acceptance MUST fail on missing documented behavior, document inconsistency, document-to-implementation mismatch, or document/implementation logic defects even when tests pass.
- Minimum return targets:
  - proposal clarification
  - design
  - implementation
  - testing implementation
- The failed acceptance report MUST name:
  - issue id
  - blocking status
  - target return stage
  - task to reopen or recreate
  - expected fix output

## Exit Condition
- The automatic pipeline exits only when:
  - all proposal-defined outcomes are satisfied
  - all blocking acceptance issues are closed
  - required tests and evidence exist
  - the final acceptance task reports success

## Prohibited Shortcuts
- Do not skip planning
- Do not treat draft or missing design artifacts as implementation-ready
- Do not treat missing post-implementation test evidence as acceptance-ready
- Do not merge stage boundaries into one large child task without justification
- Do not treat one failed acceptance as terminal completion
- Do not let downstream artifacts override the launch-confirmed proposal
