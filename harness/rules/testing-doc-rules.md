# Testing Document Rules

This rule is part of the high-risk workflow. Trivial and standard tasks run proportionate targeted verification without creating Harness testing documents or `testplan.yaml` by default.

## Goal
- Define optional persistent testing artifacts and post-implementation testing responsibilities.

## Scope
- `docs/versions/<version>/modules/<project>/<task-seq>-<task-slug>/testing.md`, `testing/`, and `testplan.yaml`.
- Cross-project testing artifacts under `docs/versions/<version>/modules/globals/<task-seq>-<task-slug>/`.

## Metadata For Optional Testing Documents
- `task_manifest: task.yaml`
- `status`
- Do not repeat `module`, `version`, `task_name`, or `submodule`; canonical identity comes from `task.yaml`.

## Required Content
- Reference `Risk profile: ./risk-profile.yaml`; implement the applicable profile's `required_checks` through task test metadata/evidence, and do not copy a Trigger Matrix into testing artifacts.
- Test cases designed after implementation from proposal, design, and delivered code.
- Submodule, module, external interface, and direct `change_id` coverage.
- Validation rationale tied to concrete behaviors, risks, and success criteria.
- Case-type coverage for normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases, including the implementing test level.
- Per-level tables: `## Unit Tests`, `## DV Tests`, `## Integration Tests`, following `harness/rules/test-design-rules.md`.
- `## Design Element Coverage` mapping parameter domains, state transitions, failure paths, error categories, invariants, and concurrency declarations to cases or gaps.
- Stable test entrypoints aligned with `testplan.yaml` for completed testing work.
- For breaking/migration-required public APIs, crate-root export changes, or build-surface changes: a file-level repository consumer closure and risk-triggered contract checks derived from design.
- Explicit gap records for missing direct validation.
- Large-module submodule test documentation when design uses direct submodules.

## Guardrails
- Testing operationalizes approved proposal/design intent against delivered implementation.
- When the task contains GitHub issue information, design and prioritize tests around the issue-described behavior, boundaries, and acceptance conditions. Process-document coverage is secondary and cannot substitute for evidence that the issue outcome works.
- Optional testing docs use the approval authority in `harness/rules/task-entry-gate-rules.md` and metadata schema in `harness/rules/schema-validation-rules.md`.
- Testing runs after implementation and MUST inspect proposal, design, and delivered code before designing cases.
- Case depth, design-element derivation, lowest-level placement, test-file placement, and design-return conditions are owned by `harness/rules/test-design-rules.md`.
- Baseline capture is project-local generated state. `.harness/` MUST be git-ignored, and testing tasks MUST NOT use `GIT_INDEX_FILE`, `git read-tree`, `git write-tree`, or `git commit-tree` to create synthetic Git baseline objects.
- Human-authored testing docs MUST stay under 1000 lines, splitting by submodule, responsibility, validation layer, or interface boundary when needed.
- Every implemented change MUST have direct validation coverage or an explicit gap.
- Completed testing MUST generate/update `testplan.yaml` unless a repo-local versioned exception records reason, owner, risk, and acceptance impact.
- Every implemented `change_id` MUST map to validation ids and generated tests or `testplan.yaml` steps unless the validation path is `manual` or `disabled`.
- Every implemented `change_id` MUST have case-type rows; non-covered, manual, disabled, or not-applicable rows need concrete reasons.
- Every generated or changed automated test and completed task plan MUST be runnable through the task-scoped interface defined by `harness/rules/unified-test-entry-rules.md`.
- Bugfix testing MUST show red-green regression evidence for the bugfix `change_id`, or record why pre-fix reproduction is not feasible.
- Validation paths MUST prove named behavior or risk, not unrelated checks.
- Acceptance-returned testing gaps MUST supplement test design, test implementation, metadata when used, and unified-entrypoint runnable evidence before retry.
- Manual or disabled layers require reasons in generated evidence and optional metadata when present.
- Upstream design or implementation problems route to the owning stage instead of silently widening testing scope.
- Downstream acceptance follow-up is recorded unless cross-stage synchronization is explicitly requested.
- Before completion, run:
  - manual flow: the unified completion profile validates optional `testing.md` when it exists
  - auto-pipeline: do not run `doc-structure-check.py --docs testing`, because `testing.md` / `testing/` are forbidden; validate pipeline testing evidence and `testplan.yaml` through `testing-coverage-check.py`
  - `UV_CACHE_DIR=.harness/uv-cache uv run --active python ./harness/scripts/harness-check.py --task <packet>/task.yaml --profile completion`
- Use `testing-coverage-check.py --allow-missing-testplan` only with a recorded repo-local versioned exception.
