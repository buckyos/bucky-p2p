# Schema Validation Rules

## Goal
- Define machine-checkable task packet, rule, and validation metadata structure.
- Make task lifecycle checks fail closed on missing fields, invalid approval state, or change-level traceability.

## Scope
- Full lifecycle schema validation is a high-risk workflow mechanism. Trivial and standard flows still create common `task.yaml` plus `proposal.md`, but do not run the high-risk schema/risk/design checks.
- Task packet proposal/design/testing/testplan files, lower-tier `completion-report.md`, and high-risk `acceptance-report.md` under `docs/versions/<version>/modules/<project>/<task-seq>-<task-slug>/`.
- Cross-project task packets under `docs/versions/<version>/modules/globals/<task-seq>-<task-slug>/`.
- Canonical high-risk stage checker `harness-check.py`, legal manual transition command `task-transition.py`, lifecycle receipt checker `lifecycle-check.py`, lower-tier delivery checker `lower-tier-check.py`, plus internal checkers: `completion-report-check.py`, `schema-check.py`, `stage-scope-check.py`, `doc-structure-check.py`, `testing-coverage-check.py`, `acceptance-report-check.py`, and `pipeline-plan-check.py`.

## Required Front Matter
Proposal stage requires only `proposal.md`, regardless of whether auto-pipeline has just been launched; schema validation MUST NOT inspect whether `design.md` or task-local `design/` exists at that stage. From a manual design stage onward, both `proposal.md` and `design.md` are required. In a launched pipeline, stages before `auto_pipeline_start_stage` keep these manual requirements, while automatic design uses `pipeline/plan.md`; manual testing uses `testing.md` plus `testplan.yaml`, while automatic testing uses runtime-state evidence plus `testplan.yaml`. Each human-readable stage document that applies MUST contain YAML-style front matter:

```yaml
task_manifest: task.yaml
status: draft | approved | rejected | superseded
```


Stage documents MUST NOT repeat `module`, `version`, `task_name`, or `submodule`; those identity fields come only from `task.yaml`. A launched pipeline records `auto_pipeline_start_stage`: stages before it use manual document requirements, while automatic design uses plan mappings. Explicit user launch confirms the bound proposal and does not require separate proposal approval metadata.

`schema-check.py --require-approved` MUST fail unless the mandatory manual proposal/design documents have `status: approved`. `harness-check.py --profile pre-edit` MUST use this flag whenever the active design source is manual `design.md`, including later automatic stages launched after manual design; it MUST NOT use the flag when automatic design uses `pipeline/plan.md`.

## Canonical Task Manifest
Every proposal packet MUST contain `task.yaml` with schema version `1`, canonical
`version`, `packet_module`, `task_name`, active `stage`, workflow `mode`, artifact
references, and `workflow_tier`, which is `pending` before confirmation and the
user-selected tier afterward. Confirmed lower-tier manifests bind `completion_report`
and one or more change entries; standard also binds `change_record`.
On lower-tier confirmation, the pending template's blank baseline field is set
to the canonical value. Confirmed lower-tier manifests require `baseline_manifest` and
`changed_paths_file` values. Pre-edit captures the repository baseline and
completion materializes its exact changed-path manifest before report review.
Confirmed high-risk manifests additionally require evidence
paths, task-level `risk_profile`, and one or more change entries. Each high-risk change binds a stable `id` to one concrete `target_module` and
concrete `scope_paths`. Cross-project packets use `packet_module: globals`; ordinary packets
require every target module to equal the packet module.

Stage documents and `testplan.yaml` use `task_manifest: task.yaml` and MUST NOT
repeat task identity fields. Acceptance reports use `Task manifest: task.yaml`. Scope Paths
in `task.yaml` MUST exactly match the corresponding design or pipeline-plan
binding before implementation and at downstream completion.

## Task Risk Profile Schema
- Every confirmed high-risk packet contains exactly one `risk-profile.yaml`, referenced by `task.yaml` as `risk_profile: risk-profile.yaml` and by proposal/design/testing documents as `Risk profile: ./risk-profile.yaml`; lower tiers retain the proposal placeholder and do not create this file.
- The profile contains exactly `contract`, `data`, `security`, `runtime`, `build`, `ui`, and `harness`. Proposal owns each category's `applies` and `evidence`; design owns `required_checks` for applicable categories; testing implements those checks.
- `risk-profile-check.py --prepare --task <packet>/task.yaml` machine-writes the task `change_ids`, proposal path, and active design-source path. Normal validation fails when task/change or source-path binding drifts.
- `harness-check.py` derives context trigger ids only from applicable categories in this profile. Per-stage matrices and per-change `triggers` fields are invalid duplicate sources.

## Approval State
- Machine validation checks only the document `status` value and, when `--require-approved` is used, requires `status: approved`.
- Approver identity, conversation provenance, timestamps, and approval records are intentionally outside the generated schema checker.

## Task Name Sequence Schema
- Task packet directory names and `task.yaml` `task_name` values MUST match `<task-seq>-<task-slug>`.
- `<task-seq>` is a version-local decimal sequence with default width 3 digits. New versions start at `001`; subsequent tasks increment by 1 across all project modules and `globals` in that version.
- `<task-slug>` is the stable human-readable task slug. Use lowercase ASCII words separated by hyphens unless a repo custom rule defines a stricter slug format.
- Machine-owned `.harness/tasks/<version>/tasks.json` MUST record the same sequence-prefixed `task_id` and canonical `task_manifest`; only `task-index.py init/add/remove` may create or mutate it, while `task-index.py list/validate/contains` provide reads and checks.
- The sequence identifies creation order only; active/current task resolution still comes from the user request, module Current/Active Task field, or confirmed unfinished-task index row.
- New task sequence allocation MUST use `UV_CACHE_DIR=.harness/uv-cache uv run --active python ./harness/scripts/task-seq.py next --version <version> --slug <task-slug>`. The allocator increments from the largest sequence already present in the same version; no separate sequence-order or continuity validation is required afterward.

## Change Traceability Schema
- Every implementation-ready change has one stable, specific `change_id`; broad IDs such as `misc`, `cleanup`, `all`, `module`, or `bugfix` are invalid.
- The same `change_id` MUST appear in:
  - `proposal.md` `## Proposal Items`, `change_id` column, with non-empty `proposal_id`, `requirement`, and `success_evidence`.
  - `design.md` `## Directly Mapped Change Items`, keyed by `change_id` plus `target_module`, with non-empty `proposal_id`, `Design Coverage`, and `Scope Paths`.
- Post-implementation testing evidence also references the same `change_id`, but testing files are not implementation prerequisites.
- Mentions in comments, prose, unrelated tables, historical notes, module overviews, or oral explanations do not satisfy direct task coverage.

## Active Module Resolution
- Task lifecycle checks require `task.yaml` to declare explicit `version`, `packet_module`, `task_name`, and one or more change entries.
- The unified checker converts those bindings to legacy checker arguments internally; agents MUST NOT repeat them manually.
- `globals` is a specialized packet-module keyword, never an implementation target. Multi-project requests use `globals/<task-seq>-<task-slug>/` for shared intent, then run implementation scope checks independently for every affected project target.
- A new task MUST NOT reuse an older or approved task packet.
- If a new task clearly belongs to a different module than unfinished records returned by `task-index.py list`, those records are ineligible and the task MUST create a new task packet for the requested module.
- If an active same-module task cannot be determined from the current request, `docs/modules/<module>.md` Current/Active Task, or module-filtered `task-index.py list` output, create a new task packet or stop for confirmation.
- If the active module cannot be determined from paths, module docs, or the user's explicit request, route to proposal or design.

## Testplan Schema
Completed testing MUST include `testplan.yaml` unless a repo-local versioned rule permits missing machine-readable metadata and records reason, owner, risk, and acceptance impact. `schema-check.py` validates `testplan.yaml` when present; `testing-coverage-check.py` enforces mapping unless explicitly allowed.

`harness-check.py --profile completion` runs schema validation for proposal and design and reruns it for testing after optional `testing.md` or `testplan.yaml` changes. Schema results may be reused only while all schema-bearing inputs remain unchanged.

```yaml
schema_version: 1
task_manifest: task.yaml
api_impact:
  public_api: none | backward-compatible | migration-required | breaking
  crate_root_export_change: false
  build_surface_change: false
  documentation_examples_affected: false
evidence_inputs: ["<production-or-consumer-path>"]
contract_checks:
  mode: enabled | disabled
  reason: <required when disabled>
  steps: [] # enabled risk-triggered checks add id/kind/assertion/name/change_ids/run
levels:
  unit|dv|integration:
    mode: enabled | manual | disabled
    summary: <text>
    test_targets: []
    preconditions:
      tools: []
      env: []
      services: []
      notes: []
    steps:
      - id: <stable-id>
        name: <text>
        change_ids: [<change-id>]
        run: [<command>, <arg>]
```

Rules:
- Every testplan declares structured `api_impact`, a non-empty repository-relative `evidence_inputs` list, and `contract_checks` with `enabled` or reasoned `disabled` mode.
- Contract steps define `kind` plus its fixed matching `assertion`; mismatched or arbitrary assertions fail validation.
- Enabled levels need at least one step.
- Enabled steps define `id`, `name`, `change_ids`, and `run`.
- Step ids are unique within the task packet.
- Manual/disabled levels include `change_ids` and a reason in evidence and optional testing metadata.
- Unknown levels fail validation.

## Checker Contract
- `harness-check.py --task <packet>/task.yaml --profile pre-edit|completion` is the canonical per-stage check command. `task-transition.py` wraps the completion profile when advancing or completing a manual high-risk stage.
- `lower-tier-check.py --task <packet>/task.yaml --profile pre-edit` validates the confirmed tier and approved proposal and captures the required canonical task-start repository baseline before trivial/standard project edits. Its `completion` profile requires that baseline, writes the canonical changed-path manifest, then runs `completion-report-check.py`; `task-index.py remove` invokes that profile and fails closed on missing baseline/path evidence, a missing report, incomplete standard change record, uncovered task `change_id`, incomplete or failing proportional defect-discovery coverage, non-passing targeted verification, blocking finding, or non-accepted conclusion.
- Manual high-risk stage changes use `task-transition.py --task <packet>/task.yaml advance`; direct edits to `task.yaml.stage` do not create a valid transition. The command runs completion before writing a content-bound stage receipt to task-packet `lifecycle.json` and advancing the stage.
- `lifecycle-check.py --require-prior acceptance` prevents acceptance from waiving earlier manual stages. `lifecycle-check.py --require-complete` is mandatory before high-risk removal and requires proposal, design, implementation, testing, and acceptance receipts in manual mode; auto-pipeline mode requires receipts for every manual-prefix stage plus `pipeline-plan-check.py --require-complete`.
- When explicit auto-pipeline launch occurs after one or more manual-prefix stages already wrote receipts, run `lifecycle-check.py --task <packet>/task.yaml --refresh-manual-bindings` once after the launch plan validates and before automatic execution. Current versioned receipts must match the exact manual-to-auto launch delta. For unversioned legacy receipts, the validated current plan plus revalidated receipt inputs establishes the migration boundary. The command writes the current binding schema; mode/start-stage, plan-path, Scope Path, change identity, and durable artifact changes remain receipt-invalidating afterward.
- `schema-check.py` derives the active stage from packet `task.yaml`, validates only `proposal.md` during proposal stage, requires `design.md` from manual design stage onward, and validates document status plus optional testplan shape, with `--submodule <task-seq>-<task-slug>` for task directories.
- A task-start working-tree baseline plus exact changed-path manifest is mandatory completion-scope evidence for confirmed trivial/standard tasks. High-risk stages use the same mechanism only when their active flow requires it. Capture copies only already-dirty tracked files and existing non-ignored untracked files; `harness/`, `.harness/`, `docs/`, and Git-ignored paths are excluded from capture and completion diffing.
- `doc-structure-check.py` validates proposal core sections, design UML diagrams, source-language file-level interface blocks, acyclic relationships, useful design sections, testing case coverage, and mandatory tables needed by implementation/testing.
- `testing-coverage-check.py` validates direct `change_id` coverage, gap reasons, testplan mapping, case-type coverage, and unified test entrypoint reachability.
- `test-run.py` writes machine-readable task run artifacts under `.harness/test-results/test-runs/`; artifacts record exact task scope, testplan, `change_id` values, commands, sources, evidence-input paths, and exit codes.
- Changes under `harness/**` or `docs/**` do not invalidate task evidence and MUST NOT trigger package/module or whole-project tests.
- `acceptance-report-check.py` validates task binding, exact `change_id` coverage, every required independent defect-discovery category, concrete evidence or task-specific not-applicable reasons, blocking findings, conclusion consistency, and conditional design/testing document consistency. It enforces review coverage but cannot prove that every real defect was found.
- `task-transition.py complete` requires the canonical acceptance report to conclude `accepted`; `needs changes` uses `return --to <design|implementation|testing>`, and requirement rejection stops without a completion receipt.
- `completion-report-check.py` validates the proportional trivial/standard review: canonical task/report binding, exact task `change_id` coverage, proposal consistency, all three proportional defect-discovery categories, passing targeted verification, standard change-record completion, findings, and accepted conclusion.
- `pipeline-plan-check.py` validates task-local immutable `pipeline/plan.md`, matching runtime `.harness/pipelines/.../state.json`, launch evidence, stage graph dependencies, task statuses, automatic-testing runtime evidence when testing is automatic, and exit-condition evidence. Manual testing remains validated from `testing.md` plus `testplan.yaml` and is not duplicated into runtime testing evidence. `harness-check.py --profile pre-edit` runs full structural validation before every automatic stage starts; scope-binding table parsing alone is not sufficient.
- All checkers MUST exit non-zero on missing mandatory files, invalid document status, missing traceability, ambiguous active module, malformed optional metadata, or out-of-stage paths.
- Stage scope checks fail proposal, design, testing, acceptance, or implementation task manifests that contain paths outside their stage and the explicit durable companion paths above. Runtime `.harness/` writes are omitted. In auto-pipeline mode, child manifests contain only reserved durable direct-write paths; parent-orchestrator merges of `pipeline/plan.md`, shared `testplan.yaml`, indexes, or shared runner registration are recorded and checked as parent-owned coordination updates, while runtime state is never attributed to a parallel child.
- An out-of-stage result blocks workflow completion until the task is returned, split, or corrected.
