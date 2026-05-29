# Auto Pipeline Rules

## Goal
- Define the repository's fully automatic downstream workflow after explicit user launch and proposal approval.
- Ensure the pipeline continues until proposal-based acceptance succeeds.

## Start Condition
- This rule is inactive unless the user explicitly asks to enter it.
- `proposal.md` status is `approved`
- user explicitly asks to enable, launch, run, or enter the automatic pipeline
- proposal approval alone does not enter auto-pipeline mode

## User Authorization Precedence
- Explicit user instructions have highest priority for entering auto-pipeline mode, requested pipeline scope, and whether per-stage user confirmation is required
- If the user explicitly asks auto-pipeline mode to handle all subsequent stages or the whole downstream workflow, the pipeline continues through design, implementation, post-implementation testing, and acceptance without stopping for separate user confirmation after each stage
- When a document-producing stage completes, the pipeline auto-confirms that stage by updating the produced stage document front matter to `status: approved`, `approved_by: auto-pipeline`, and `approved_at: <current timestamp>`
- Auto-confirmation happens only after that stage's declared done criteria and required checks pass
- After each child task completes, the pipeline updates the pipeline plan task status to `confirmed` or `complete` before continuing to dependent tasks
- Implementation completion is recorded in the pipeline plan and implementation evidence, and final acceptance is recorded in the pipeline plan and acceptance report
- This does not waive proposal authority, stage write scopes, `stage-scope-check.py`, implementation admission, schema checks, admission checks, required validation, or final acceptance

## Final Acceptance Baseline
- Final acceptance is judged against the approved `proposal.md`
- `design.md`, optional testing artifacts, optional acceptance artifacts, implementation, and tests are supporting evidence and execution artifacts
- If downstream artifacts conflict with the approved proposal, the proposal is authoritative
- If downstream artifacts or code disagree, fixes preserve the approved proposal and route non-requirement defects through design -> implementation/code -> testing implementation
- Code follows design; tests verify proposal/design/code behavior

## Stage Responsibilities
- Proposal responsibility:
  - define the approved baseline outcome for the pipeline
- Pipeline planning responsibility:
  - create the task graph, dependencies, outputs, and done conditions before execution starts
- Design responsibility:
  - convert the approved proposal into executable structure and interfaces
  - keep module and submodule dependencies acyclic
  - split submodules by business logic first, extract shared implementation logic into shared submodules, and isolate clear technical areas such as HTTP interfaces or persistence/database access
  - keep design at module shape level: submodules, dependencies, key call flows, exported interfaces, and external module dependencies
- Implementation responsibility:
  - deliver production code inside approved proposal/design boundaries
- Testing responsibility:
  - after implementation completes, design test cases from proposal, design, and delivered code, then generate test implementation
- Acceptance responsibility:
  - evaluate document coverage, document consistency, document-to-implementation consistency, and logic, then return failures to the correct earlier stage

## Mandatory Planning Step
- The pipeline MUST create or refresh `harness/pipeline-plan.md` before starting downstream execution
- The plan MUST list:
  - top-level stage tasks
  - child tasks per direct submodule when needed
  - stage responsibility for each task
  - dependencies between tasks
  - outputs and done conditions
  - return-routing targets for failed acceptance

## Stage Execution Model
- Design, implementation, testing, and acceptance MUST run as separate child tasks
- Each child task MUST keep writes inside its named stage artifact group unless the user explicitly requested cross-stage synchronization for that task
- Each single-stage child task MUST run `stage-scope-check.py --stage <stage>` before completion and fail on out-of-stage diffs
- Upstream-stage changes MUST NOT automatically edit downstream-stage artifacts; create or reopen the downstream child task instead
- If direct submodules exist, stage tasks SHOULD be decomposed into submodule child tasks
- Independent direct submodules SHOULD use submodule packets under the large module directory, such as `docs/versions/<version>/modules/<module>/<submodule>/proposal.md` and `design.md`; optional post-implementation testing artifacts can live there when generated
- Each child task MUST have:
  - one owner
  - one scope boundary
  - one output
  - clear dependencies
  - observable done criteria

## Implementation Admission
- The task entry gate still applies inside the pipeline: implementation tasks MUST classify scope and run admission before editing production code, build files, or resources.
- Implementation MUST NOT start unless:
  - `proposal.md` exists and `status: approved`
  - `design.md` exists and `status: approved`
- Implementation MUST inspect the approved proposal/design docs and confirm they cover the current task before coding.
- Implementation MUST identify explicit `version`, `module`, and `change_id` values before coding.
- Implementation for a direct submodule packet MUST also identify explicit `submodule`.
- Implementation MUST pass `schema-check.py` and `admission-check.py` for each affected module packet.
- Implementation for a direct submodule packet MUST pass those checks with `--submodule <submodule>`.
- Cross-module implementation MUST pass admission independently for every affected module.
- Cross-submodule implementation MUST pass admission independently for every affected submodule packet.
- If any prerequisite is missing, still draft, or approved-but-incomplete for the task, return work to the owning stage.

## Return Routing
- If acceptance fails, the pipeline MUST return to the correct earlier stage
- Non-requirement failures repeat design -> implementation -> testing, then rerun acceptance.
- If the same unresolved issue remains after more than 5 unsuccessful iterations, stop and report the issue to the user.
- Acceptance MUST apply `harness/rules/acceptance-review-rules.md` and audit admission for every module needed as evidence for accepted behavior.
- Acceptance MUST audit admission for every direct submodule packet needed as evidence for accepted behavior.
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
- Do not let downstream artifacts override the approved proposal
