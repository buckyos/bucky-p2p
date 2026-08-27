# Task Entry Gate Rules

## Goal
- Classify governed work by workflow tier, then select the lightest sufficient default flow.
- Apply this rule after the explicit all-harness-rules opt-out in `AGENTS.md` and before any stage-specific rule.

## GitHub Issue Execution Priority
- When the current task includes GitHub issue content or explicitly points to a GitHub issue, read the available issue description, acceptance criteria/checklists, and requirement-defining comments before making implementation decisions.
- During execution, satisfying the issue-described behavior is the primary objective. Plans, proposal/design documents, process steps, checkers, existing implementation, and agent summaries are supporting context; they MUST NOT displace, narrow, or substitute for the issue outcome.
- Keep implementation choices, debugging, tests, and completion judgment anchored to the issue description. When deciding what to do next, prefer work that closes an issue requirement or exposes whether it is satisfied over work that only improves Harness documentation or process evidence.
- The presence of issue information alone MUST NOT create an additional proposal field, task artifact, checker, workflow stage, or higher workflow tier. Use the normal tier and artifacts justified by the task's actual risk.
- If issue content is unavailable or materially conflicts with another requirement, surface that conflict instead of guessing. Otherwise continue through the normal workflow without adding issue-specific ceremony.
- This execution priority remains subject to system/developer instructions, safety requirements, filesystem permissions, and the current user's explicit resolution of a requirement conflict.

## Project Custom-Rule Precedence
- Matching project-added rules indexed under `harness/custom-rules/` are the highest-priority repository-owned Harness policy.
- Evaluate and apply every matching custom rule before this task-entry gate, every other generated rule, and every generated checker. When repository-owned Harness policies conflict, the matching custom rule wins.
- A custom rule that replaces a generated mechanical gate MUST name the replacement command or versioned exception evidence so the project decision remains mechanically auditable.
- This repository precedence does not override system/developer instructions, safety requirements, filesystem permissions, the current explicit user instruction, or an explicit all-Harness-rules opt-out.

## Direct Harness Rule Maintenance
- When the current request's primary purpose is to add, update, remove, or refresh repository-local Harness policy files under `harness/rules/` or `harness/custom-rules/`, including either directory's rule index, do not enter the task workflow. Execute the requested rule maintenance directly.
- Evaluate this exception before mandatory proposal responsibility. Do not create or select a task packet, mutate the unfinished-task index, classify a workflow tier or stage, request proposal confirmation, create downstream task artifacts, or run task lifecycle commands for the rule-maintenance scope.
- Direct execution still includes the inspection and focused validation needed to make the requested rule update safely. It does not waive system/developer instructions, safety requirements, explicit user scope, or non-Harness repository constraints.
- This exception is limited to repository-local Harness rule policy. A mixed request that also changes product behavior, runtime behavior, build behavior, tests, or non-rule Harness tooling uses the normal task workflow for that additional scope.

## Mandatory Proposal Responsibility
- Every governed task creates the same proposal packet before execution: allocate `<task-seq>-<task-slug>`, use the canonical single-project or `globals` packet path, create `task.yaml` plus draft `proposal.md`, and register `task.yaml` in the version's unfinished-task index.
- Before confirmation, `task.yaml` uses `workflow_tier: pending`. Draft `proposal.md` records the requested outcome, in-scope behavior, out-of-scope behavior or non-goals, success signal, material assumptions or tradeoffs, proposed tier, and concrete rationale or triggered boundaries.
- Proposal confirmation is mandatory for every tier. Before confirmation, only read-only project inspection and proposal-packet/index maintenance are allowed; do not modify project files or start a tier's execution flow.
- The confirmation request MUST show the proposal path, requested outcome, scope, non-goals, success criteria, proposed tier, tier rationale/triggered boundaries, and every unresolved question. Offer confirmation as proposed, confirmation with a replacement `trivial`/`standard`/`high-risk` tier, or proposal revision.
- On confirmation, write the user-selected tier to `task.yaml` and `proposal.md`, record the confirmation, and set the matching proposal to `status: approved`. For `trivial` or `standard`, also set `baseline_manifest` to `.harness/baselines/<version>/<task-name>-delivery/manifest.json`; leave it blank for high-risk unless the active stage requires selected-file comparison evidence. The selected tier becomes final for generated routing and artifacts, subject only to system/developer instructions, safety requirements, or non-Harness repository constraints. Surface known residual risk when the user selects a lower tier.
- A tier-only reply confirms the displayed proposal and selects that tier only when the confirmation request listed no unresolved proposal question. Otherwise it resolves only the tier; do not infer answers to requirement, scope, tradeoff, or acceptance questions.

## Workflow Tier Classification
- Classify risk before selecting a stage or creating durable artifacts. Use `trivial`, `standard`, or `high-risk`.
- `trivial` requires all of the following: the request is clear; impact is localized to one project module; no material public contract/protocol/CLI, persistent data/schema/migration, security/privacy, concurrency/lifecycle/runtime integration, dependency/build graph or supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility/rollback, UI/accessibility workflow, Harness-process, cross-project, or architectural-boundary impact is known; and a targeted verification signal is available.
- `standard` is the default for bounded single-project feature, bugfix, or refactor work that is not trivial and has no high-risk trigger.
- `high-risk` is mandatory by default when evidence confirms any material impact above, rollback or compatibility requires coordination, ambiguity materially affects scope or acceptance, or the user explicitly requests a staged proposal/design/testing/acceptance artifact or auto-pipeline. A matching path, filename, or broad risk category is screening evidence only and never confirms high-risk by itself.
- Documentation-only or configuration-only work remains `trivial` or `standard` by default when it changes no governed intent, runtime behavior, public contract, data, security boundary, dependency/build graph, supply-chain trust, produced artifact, production default/rollout, release/deployment surface, compatibility, or rollback requirement. Classify by consequence, not file type.
- Task size alone never downgrades risk. Before proposal confirmation, start at the lowest tier supported by evidence and automatically revise the proposed tier when investigation reveals a higher-risk condition; the single mandatory proposal confirmation settles the final tier.
- An explicit current-user tier selection wins over the generated default unless it conflicts with system/developer instructions, safety requirements, or a non-Harness repository constraint. Selecting a tier changes the generated rules and mechanical gates that apply; it does not waive gates within the selected tier. Surface known risk when following an explicit downgrade.
- Before running a tier's side-effecting default flow or creating post-proposal artifacts, stabilize the recommendation through read-only inspection and proposal routing, then obtain user confirmation.

## Tier Default Flows
- All tiers retain the approved common `task.yaml` plus `proposal.md` packet and use the same task name and storage rules.
- For any lower-tier task containing GitHub issue information, implementation, targeted verification, and the completion review MUST judge the delivered behavior against the issue description first; Harness packet/report completion remains secondary evidence and adds no issue-specific artifact.
- `trivial`: after confirmation, run `lower-tier-check.py --profile pre-edit` before project mutation; it validates the approved proposal and captures the same required task-start working-tree baseline used by high-risk stages. It copies only tracked files already dirty before the task and existing non-ignored untracked files, excluding `harness/`, `.harness/`, and `docs/`. Inspect relevant code, make the smallest change, and run the narrowest useful repository-native check. Then perform a fresh proportional falsification pass over behavior/logic, boundaries/failure paths, and regression/side effects; do not reuse the implementation self-assessment as the review conclusion. Write task-local `completion-report.md`, then run `lower-tier-check.py --profile completion`; it must compare the saved state with final dirty tracked/untracked paths to produce the canonical changed-path manifest before validating the report. Remove only after every task `change_id`, the actual delivery scope, concrete independent defect-discovery evidence, targeted verification, and accepted conclusion pass. Do not create a risk profile, change record, design/testing document, testplan, or full `acceptance-report.md`.
- `standard`: after the same working-tree-baseline-backed lower-tier pre-edit check, create one lightweight `docs/changes/<change>.md` from `docs/changes/_template.md`, link the common task/proposal, record approach, compact risk screen, affected implementation evidence, and verification; implement and verify in one continuous flow; mark the record complete; then write and validate the same task-local defect-discovery `completion-report.md`. The reviewer actively searches for counterexamples instead of proving the change record complete. Completion first generates the canonical changed-path manifest from the required baseline. Removal requires the complete bound change record plus accepted proposal/current-delivery consistency, complete proportional defect-discovery coverage, and passing targeted verification. Do not create separate design/testing/full-acceptance artifacts or a risk profile.
- `high-risk`: after confirmation, add `risk-profile.yaml` and use the design, implementation, testing, lifecycle-check, and acceptance rules below in the already-created packet.
- The lower-tier completion report is a proportional close gate, not a high-risk lifecycle stage: it does not add design/testing receipts, a testplan, or the full acceptance report. Upgrade only when current evidence requires it.
- Lower-tier report conclusions use the same meanings as high-risk acceptance: `needs changes` for a correction within the approved intent, `rejected` for a user decision/abandonment/re-scope, and `accepted` only after the proportional defect search passes.

## Automatic Upgrade
- Before initial confirmation, a confirmed higher-risk condition changes the proposed tier immediately. Update `proposal.md` and rerun proposal routing; do not ask for a separate upgrade decision because the mandatory proposal/tier confirmation will settle it.
- After confirmation, the user-selected tier remains authoritative. Newly requested scope or a requirement change returns the task to proposal, sets the proposal to draft, recomputes the recommendation, and requires confirmation again before further project mutation. Newly discovered risk within the confirmed scope is surfaced and recorded; it does not silently replace the user's tier.
- If a reconfirmed tier changes from `trivial` to `standard`, create the standard change record and continue. If it changes to `high-risk`, add the risk profile and missing full-lifecycle artifacts to the existing packet and continue from the earliest responsible stage. Never allocate a second task name or packet for the same confirmed requirement revision.
- A lower-tier `general` route never carries into high-risk. On a user-confirmed high-risk transition, reclassify it to proposal, design, implementation, testing, or acceptance and reroute before further project edits.

## Task Classification
- Use `general` only for trivial/standard analysis, diagnosis, explanation, internal documentation, or non-behavioral maintenance that does not fit another responsibility stage. It is a routing fallback, not a high-risk lifecycle stage.
- The mandatory formal `proposal.md` does not by itself select `high-risk`. An explicit request for the full staged lifecycle, a separate design/testing/acceptance artifact, or auto-pipeline selects the high-risk workflow for that scope; the stage remains a responsibility, not a file write scope.
- In high-risk work, a request that changes goals, scope, non-goals, supported/unsupported behavior, acceptance boundaries, or success evidence is proposal work. Lower tiers record the decision in their handoff or single change record.
- A production-code, bugfix, optimization, refactor, runtime, UI-behavior, or build-behavior change is implementation-shaped. High-risk work uses `harness/rules/implementation-rules.md`; lower tiers use their tier default flow.
- Harness governs workflow responsibility and evidence, never repository permissions. Read or write any project file needed for the current user request; no task packet, stage, `Scope Paths`, changed-path manifest, router result, or checker result grants or revokes that access. A failed workflow check blocks completion or routes follow-up, not file access.
- Never ask the user to skip Harness rules to inspect or modify repository files.
- Ambiguity that affects behavior, scope, risk, module, task packet, or `change_id` returns to the owning upstream stage instead of being guessed.

## Task Packet Selection
- This section applies to every governed task. Every new task creates and registers one common proposal packet before confirmation; high-risk later expands that same packet.
- Every new task begins with canonical `task.yaml`, `workflow_tier: pending`, and draft `proposal.md`. After confirmation, replace `pending` with the final tier. Create `risk-profile.yaml` and downstream stage artifacts only for final `high-risk` work.
- Every new task name MUST use `<task-seq>-<task-slug>` regardless of final tier.
- Single-project packets live at `docs/versions/<version>/modules/<project>/<task-seq>-<task-slug>/`.
- Cross-project packets live at `docs/versions/<version>/modules/globals/<task-seq>-<task-slug>/`; `globals` is a packet-module keyword, never a production target.
- Maintain machine-owned `.harness/tasks/<version>/tasks.json` only through `harness/scripts/task-index.py`: run `init --version <version>` to create it, `add --task <packet>/task.yaml` after packet creation, `list --version <version> --module <module>` for selection, `remove --task <packet>/task.yaml` only after successful completion, and `validate --version <version>` for integrity. Never edit or parse the JSON by hand; other scripts must reuse `task-index.py` helpers.
- For manual high-risk work, never edit `task.yaml.stage` directly. Use `task-transition.py advance` after each stage and `task-transition.py complete` in acceptance; it records content-bound completion receipts in task-packet `lifecycle.json`. Final `task-index.py remove` fails unless the full receipt chain is valid.
- Select an existing packet only from an explicit current-user reference or a module's Current/Active Task field. Never infer it from sequence, directory order, timestamp, old code, chat history, or a broad module overview.
- A clearly different-module request gets a new packet. If module-filtered `task-index.py list` output contains multiple unfinished packets that could apply and the user did not identify one, ask which packet to use; clearly new work still gets a sibling packet.
- An approved packet document is frozen for the confirmed requirement. Clearly new requirements use a sibling packet; revisions to the current unfinished task return its proposal to draft, record the reason, and require confirmation again.

## Approval Authority
- Every tier's common `proposal.md` requires explicit current-user confirmation before execution. Other standalone stage-document approvals apply only to high-risk.
- Every proposal confirmation request MUST include the proposed tier and rationale. If the user supplies a different tier, apply that tier immediately, reroute, and use only that tier's required artifacts. A tier-only reply does not approve unresolved questions listed separately in the confirmation request.
- Proposal-stage work ends at `status: draft`; after the user confirms the displayed file and tier, record the final tier and set it to `status: approved`. Any material difference from what was displayed remains draft and requires confirmation again. High-risk downstream document approval may also use the auto-pipeline transition defined by `harness/rules/auto-pipeline-rules.md`.
- Silence or inferred intent is not approval.
- `schema-check.py` validates only the status value. It intentionally does not validate approver identity, conversation provenance, timestamps, or approval records.

## Stage Ownership
- Separate stage ownership is the high-risk default. Trivial and standard tasks combine their responsibilities in one continuous flow.
- A stage names the task's primary responsibility and expected evidence. Completion fails when the task's changed-path manifest contains artifacts owned by another stage.
- If work needs artifacts owned by another stage, return or split the work before completion and record the synchronization so the artifact chain remains understandable.
- Proposal owns requirements, acceptance boundaries, and scripted new-task registration. Design owns implementation shape and Scope Paths. Implementation owns design-bound production changes. Testing owns test design/implementation and task test evidence. Acceptance owns independent defect discovery, requirement/design/implementation/testing findings, secondary document consistency, reporting, and scripted removal of a successfully completed task from `tasks.json`.
- Detailed content and completion guidance live in the current stage's authoritative rule files listed in `AGENTS.md`.
- After provisional tier and stage classification, run `harness/scripts/context.py` with both values and the common packet. Proposal-document rules apply to every tier; downstream high-risk stage rules remain excluded from lower-tier routes while matching risk triggers can still inform the pre-confirmation recommendation. Router output is never an exhaustive read list; inspect any additional repository files needed. Loading the quality-gate rule does not execute its gates.

## Path Evidence
- Changed-path manifests are required evidence where the selected tier/stage says so. Design `Scope Paths` remain planned-impact metadata; `stage-scope-check.py` rejects changed paths outside the active stage's artifact group.
- `.harness/` remains the git-ignored runtime root for unfinished-task indexes, generated evidence, caches, pipeline state, and run results.
- `stage-scope-check.py` MUST return non-zero for a changed path outside the active stage's artifact group. It MUST NOT reject a path solely because it is outside declared design `Scope Paths`.

## Implementation Transition
- This section applies only to high-risk implementation. Trivial and standard implementation begins after their tier-specific preparation above.
- Record explicit `version`, `packet_module`, `task_name`, implementation targets, and concrete `change_id` values in `task.yaml` when maintaining task traceability.
- Apply `harness/rules/implementation-rules.md` for preparation and design mapping.

## Return Routing
- Missing packet, missing proposal coverage, or a requirement/acceptance-boundary defect: proposal.
- Missing or inadequate manual design, auto-pipeline design mapping, interface, state/failure model, implementation sequence, Scope Paths, or a design defect found during acceptance: design.
- Missing task-test coverage, weak defect-detection coverage, or non-runnable task evidence: testing.
- Implementation defect against adequate governing sources: implementation.
- Proposal ambiguity or contradiction found during acceptance: finish the canonical report with a blocking requirement finding and `rejected`, then stop and ask the user; do not auto-return it.
