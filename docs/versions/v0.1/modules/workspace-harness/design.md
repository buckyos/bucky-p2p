---
module: workspace-harness
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-07-07T17:15:00+08:00
approved_content_sha256: c5dde33f6c2c2e99ed371c0cd0658f7f273c8b57867ca0dc2894842e31154d18
---

# workspace-harness Design

> This file describes module shape, dependencies, key flows, exported interfaces, and external dependencies.

## Design Scope
### Goals
- Preserve `harness/scripts/test-run.py` as the canonical unified test entrypoint.
- Avoid repeated physical execution of identical commands inside one `test-run.py` invocation.
- Preserve deterministic module/level traversal and machine-written run artifact evidence.
- Prefer module `testplan.yaml` entries over static fallback commands for the same module and level.
- Provide an explicit no-dedupe mode for flaky investigation or strict physical reruns.

### Non-goals
- Do not change business module test assertions, runtime behavior, or protocols.
- Do not remove required `all all`, `<module> all`, or `<module> <level>` entrypoints.
- Do not change acceptance-report artifact freshness requirements.
- Do not make dedupe hide non-zero command results.

## Overall Approach
- Keep the existing command discovery shape: known modules come from static fallback registry plus discovered `testplan.yaml` files.
- Change `commands_for(root, module, level)` so testplan commands, when present for a module/level, replace static fallback commands for that exact module/level.
- Add a per-run result cache keyed by command identity. The initial implementation uses argv plus root workdir as the command identity because current runner commands execute under the same root and do not pass per-command env.
- Update `stage-scope-check.py` so implementation-stage scope checks allow `workspace-harness` itself to modify admitted `harness/scripts/**` paths. Other modules still cannot modify governance scripts during implementation.
- Before executing a command, check the cache unless `--no-dedupe` is set. If a previous identical command exists, append a step that records the same exit code, `deduped: true`, and `reused_from_step` pointing to the earlier step index. Do not execute the command again.
- For physical executions, append a step with `deduped: false`, duration, and command result, then store it in the cache.
- Preserve fail-fast behavior: if a reused result is non-zero, the current module/level observes the non-zero exit code and stops just like a physically executed failure.
- Extend artifacts compatibly by adding optional per-step fields; keep artifact schema at `1` because existing consumers ignore unknown fields and required top-level fields do not change.

## Simplicity Check
- Smallest sufficient approach: add testplan precedence and a small in-memory result cache inside `main()`.
- Existing components or patterns reused: existing module discovery, command parsing, run artifact writer, and fail-fast loop.
- New abstractions introduced: a tiny command identity tuple and cached step metadata.
- Why each new abstraction is necessary: command identity is needed to avoid string parsing, and cached metadata is needed to preserve coverage evidence without physical reruns.

## Current Structure
- `harness/scripts/test-run.py` has static `TEST_COMMANDS`, discovers module `testplan.yaml` files, then appends static and testplan commands in `commands_for(...)`.
- `all all` iterates all known modules and levels. This currently repeats exact commands when static and testplan entries overlap.
- Several module integration levels register `cargo test --workspace`, causing repeated full workspace test runs in one invocation.
- Artifacts currently record one step per requested command execution with `module`, `level`, `command`, `exit_code`, and `duration_s`.

## Invariants to Preserve
- `test-run.py all all` must still visit every registered module and level in deterministic order.
- Unknown modules or levels must still fail closed.
- Every real run must still write `test-results/test-runs/*.json`.
- Whole-project acceptance can still cite fresh passing `requested_module: all`, `requested_level: all`, `exit_code: 0` artifacts.
- `--dry-run` must not execute commands and must still write no artifact.
- Root shortcut scripts continue invoking `test-run.py` through the same command surface.

## Submodules
| Submodule | Type | Responsibility | Depends On | Exported Interface | Notes |
|-----------|------|----------------|------------|--------------------|-------|
| unified-test-entry | technical | Command discovery, execution scheduling, dedupe, and run artifact generation for the workspace. | module packets, testplans, local shell commands | `harness/scripts/test-run.py` CLI and JSON run artifact | Existing responsibility inside workspace-harness; no new packet needed. |
| governance-checkers | technical | Schema, admission, scope, pipeline, and acceptance checks. | module packets, run artifacts | `harness/scripts/*-check.py` CLIs | Existing scope retained; only used for validation. |

## Boundary Rationale
| Boundary | Classification | Why Separate | Shared Logic / Technical Area | Notes |
|----------|----------------|--------------|-------------------------------|-------|
| unified test entry vs business tests | technical | Runner scheduling is governance infrastructure; business modules own test content. | command discovery and artifact writing | Keep optimization in harness without editing business tests. |
| runner artifact vs acceptance report checker | technical | Runner writes evidence; acceptance checker consumes it. | JSON artifact compatibility | Optional step fields avoid checker changes for this proposal. |

## Boundary Decision Matrix
| boundary | classification | business_responsibility | shared_logic_or_technical_area | decision |
|----------|----------------|-------------------------|--------------------------------|----------|
| test-run scheduling | technical | none: governance runner only | unified test entry command scheduling | keep in `harness/scripts/test-run.py` with a small local helper instead of new package |
| artifact compatibility | technical | none: evidence format only | JSON artifact step fields | extend per-step records compatibly; do not change top-level schema |

## Dependency Graph
| Source | Depends On | Reason | Cycle Check |
|--------|------------|--------|-------------|
| unified-test-entry | module packets/testplans | discover registered commands | acyclic |
| acceptance-report-check | test-run artifacts | verify fresh project evidence | acyclic |
| root shortcut | unified-test-entry | invoke canonical runner | acyclic |

## Key Call Flows
| Flow | Caller | Callee / Submodule Path | Purpose | Failure Handling | Notes |
|------|--------|--------------------------|---------|------------------|-------|
| all/all command discovery | CLI user or root shortcut | unified-test-entry | build ordered `(module, level, command)` list | unknown module fails closed; no commands matched fails closed | deterministic module and level order preserved |
| command execution with dedupe | unified-test-entry | local subprocess runner | execute or reuse identical command result | non-zero executed or reused result stops current run with that exit code | `--no-dedupe` bypasses cache |
| artifact writing | unified-test-entry | `test-results/test-runs` | write evidence for executed and reused steps | artifact write failure remains warning, matching current behavior | top-level schema unchanged |

## Large Module Submodule Decision
| Submodule | Source Proposal | Decision | Design Packet | Reason |
|-----------|-----------------|----------|---------------|--------|
| unified-test-entry | P-HARNESS-TEST-RUN-1 / workspace_harness_test_run_dedupe | existing | `not-applicable: retained in workspace-harness design.md` | The change is a focused script scheduling optimization and does not require a direct submodule packet. |

## Trigger Matrix
| trigger_category | applies | evidence | design_coverage | required_checks | deferred_checks_and_reason |
|------------------|---------|----------|-----------------|-----------------|----------------------------|
| contract/protocol | yes | JSON run artifacts are consumed by acceptance. | Overall Approach; Key Call Flows; Interfaces and Dependencies | artifact inspection and acceptance-report-check | none |
| data/schema | yes | Step records gain optional `deduped` and `reused_from_step` fields. | Overall Approach; Data and State | inspect generated JSON artifact | none |
| security/privacy/permission | no | Runner remains local command execution and no permission model changes. | Invariants to Preserve | none | none |
| runtime/integration | yes | Runner executes module and workspace tests. | Key Call Flows; Testability | dry-run, lightweight workspace-harness all, and one root shortcut all/all physical run | none |
| build/dependency/config/deployment | yes | Runner invokes cargo and Python harness commands. | Implementation Order; Risks and Rollback | module/all focused checks and one root shortcut all/all command | none |
| ui/datamodel/workflow | no | No UI or product workflow. | Non-goals | none | none |
| harness/process | yes | This is a harness process optimization. | Directly Mapped Change Items | schema/admission/scope/pipeline/acceptance checks | none |

## Directly Mapped Change Items
| change_id | proposal_id | Design Coverage | Scope Paths | Interface / Boundary Impact | Notes |
|-----------|-------------|-----------------|-------------|-----------------------------|-------|
| workspace_harness_acceptance_review_gate | P-HARNESS-REVIEW-1 | acceptance remains responsible for full diff and evidence review. | `harness/rules/acceptance-task-rules.md`, `harness/rules/acceptance-review-rules.md`, `harness/rules/trigger-rules.md` | none for this task | Existing governance coverage retained. |
| workspace_harness_direct_admission_gate | P-HARNESS-ADMISSION-1 | schema/admission/check-implementation remain fail-closed. | `harness/rules/task-entry-gate-rules.md`, `harness/rules/direct-change-mapping-rules.md`, `harness/rules/module-doc-exception-rules.md`, `harness/rules/schema-validation-rules.md`, `harness/scripts/schema-check.py`, `harness/scripts/admission-check.py`, `harness/scripts/verify-workspace-harness.py` | none for this task | Existing governance coverage retained. |
| workspace_harness_pipeline_plan_current_change | P-HARNESS-PIPELINE-1 | pipeline plan records current auto-pipeline graph and exit evidence. | `harness/pipeline-plan.md` | none for this task | Current task updates the plan as auto-pipeline evidence. |
| workspace_harness_test_run_dedupe | P-HARNESS-TEST-RUN-1 | command source precedence, in-run command-result reuse, artifact step fields, no-dedupe opt-out, and workspace-harness self-admission for governance scripts are defined in this design. | `harness/scripts/test-run.py`, `harness/scripts/stage-scope-check.py`, `docs/versions/v0.1/modules/workspace-harness/testing.md`, `docs/versions/v0.1/modules/workspace-harness/testplan.yaml` | backward-compatible CLI extension: adds optional `--no-dedupe`; artifact step fields are additive; scope checker change is limited to module `workspace-harness` and admitted scope paths | No business module test commands are changed. |

## Implementation Order
| Phase | Goal | Preconditions | Outputs | Depends On | Parallel |
|-------|------|---------------|---------|------------|----------|
| 1 | Apply design approval and admission | approved proposal and design | admission evidence and stamp | proposal/design | no |
| 2 | Implement runner precedence and dedupe | admission passed | `test-run.py` changes | phase 1 | no |
| 3 | Update testing metadata | implementation complete | testing docs and testplan rows for dedupe coverage | phase 2 | no |
| 4 | Validate and accept | testing evidence available | acceptance report and final pipeline evidence | phase 3 | no |

## Key Decisions
| Decision | Chosen | Alternatives Considered | Rejection Reason |
|----------|--------|-------------------------|------------------|
| Dedupe identity | `(root.resolve(), tuple(command))` | shell string comparison | argv tuple avoids quoting/parsing ambiguity and matches current subprocess invocation semantics. |
| Testplan precedence | use testplan commands for a module/level when present, fallback only when absent | append static and testplan commands | append caused observed duplicate commands and long all/all runs. |
| Artifact compatibility | additive per-step fields with schema `1` | increment top-level schema | existing consumers only require top-level schema and standard fields; additive fields are sufficient and less disruptive. |
| Flaky investigation | add `--no-dedupe` | remove dedupe through environment variable only | CLI flag is explicit, discoverable, and deterministic. |
| Workspace-harness script admission | allow module `workspace-harness` implementation scope to modify admitted `harness/scripts/**` paths | keep hard reject for all modules | hard reject contradicts workspace-harness ownership of governance scripts and makes approved harness-script changes impossible. |

## Data and State
| Data or State | Owner Submodule | Access For Others | State Transitions |
|---------------|-----------------|-------------------|-------------------|
| per-run command result cache | unified-test-entry | internal only | empty at run start -> stores executed command result -> discarded at process exit; reused results never mutate cached command output |
| run artifact step record | unified-test-entry | acceptance and humans read JSON | executed step records `deduped: false`; reused step records `deduped: true` and points to `reused_from_step`; failure exit codes propagate |

## Testability
- Isolation seams per submodule: `--dry-run` exposes command scheduling without executing tests; generated JSON artifacts expose executed vs reused steps.
- Replaceable external boundaries: subprocess execution remains isolated in `run_command(...)`; dedupe can be disabled with `--no-dedupe`.
- How error/boundary cases will be triggered: use targeted dry-runs for default reuse and `--no-dedupe` scheduling; inspect the single root-shortcut all/all artifact for reused `cargo test --workspace`; run lightweight module/all focused checks plus one physical root all/all.
- Untriggerable failure paths and their alternative verification: flaky rerun behavior is covered by `--no-dedupe` scheduling evidence rather than inducing flaky tests.

## Interfaces and Dependencies
### Exported interfaces
| Interface | Consumer | Compatibility | Notes |
|-----------|----------|---------------|-------|
| `harness/scripts/test-run.py <module> <level>` | all modules and acceptance workflow | backward-compatible | Existing arguments remain; optional `--no-dedupe` is additive. |
| run artifact step records | acceptance reports and humans | backward-compatible | Standard fields remain; optional `deduped` and `reused_from_step` are additive. |

### External module dependencies
- Python standard library only.
- Existing Rust/Cargo commands remain unchanged.

### External service or protocol constraints
- No external services or network protocols are introduced.

## Document Index
| Document | Topic | Scope |
|----------|-------|-------|
| `design.md` | workspace harness runner dedupe and retained governance boundaries | full module |

## Risks and Rollback
- Risk: reused failed results could be confusing. Mitigation: step records mark `deduped` and preserve fail-fast exit code.
- Risk: a repeated command was intentionally used for flaky detection. Mitigation: `--no-dedupe` restores physical reruns.
- Risk: artifact readers assume only old step keys. Mitigation: additive fields only; top-level schema unchanged.
- Rollback: disable dedupe by default or revert `test-run.py` and stage-scope exception changes; existing command registry and testplans remain valid.

## Design Guardrails
- Do not rewrite approved proposal intent in `design.md`.
- If this module has no direct submodules, say so explicitly.
- If this is a large module with many independent submodules, model each new independent feature as its own direct submodule unless this design explains why it belongs inside an existing submodule.
- Keep module and submodule dependencies acyclic. Resolve circular dependencies before implementation.
- Keep dependency direction business -> shared / technical: shared and technical submodules never depend on business submodules, and nothing depends on assembly.
- Split by business logic first: different business responsibilities go into different business submodules.
- Extract implementation logic shared by multiple business submodules into a shared submodule; a shared submodule needs at least 2 consumers and one nameable responsibility.
- Give every persistent datum or shared state exactly one owning submodule; other submodules go through the owner's exported interface.
- Use dedicated submodules for technically distinct implementation areas when they have clear boundaries, such as HTTP interfaces, database/persistence access, external adapters, codecs, schedulers, or storage.
- Describe only the implementation layout that affects ownership, exported interfaces, external dependencies, or key call flow.
- Keep detailed independent submodule documentation in a submodule packet under this module directory, such as `<submodule>/design.md`; do not accumulate all module documentation in one file or place independent submodule docs under `design/<submodule>/`.
- Keep human-authored docs under 1000 lines where practical; if a doc would exceed 1000 lines, split it and update the document index.
- For existing code, describe current structure first, then the change.
- Do not introduce idealized architecture that the proposal did not approve.
- Prefer the simplest design that satisfies the approved proposal and documented constraints.
- Do not add speculative extension points, configuration, or abstractions for single-use code.
- Every implementation-ready design item must carry the same `change_id` used in `proposal.md`.
- For multi-module or cross-boundary work, list each affected module and explain whether it needs separate implementation admission.

## Approval Record
- approver: auto-pipeline
- approval_date: 2026-07-07T17:15:00+08:00
- user_statement: "确认，自动处理后续步骤"
