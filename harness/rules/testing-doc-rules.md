# Testing Document Rules

## Goal
- Define optional persistent testing artifacts and post-implementation testing responsibilities.

## Scope
- `docs/versions/<version>/modules/<project>/<task-seq>-<task-slug>/testing.md`, `testing/`, and `testplan.yaml`.
- Cross-project testing artifacts under `docs/versions/<version>/modules/globals/<task-seq>-<task-slug>/`.

## Metadata For Optional Testing Documents
- `module`
- `version`
- `status`
- `approved_by`
- `approved_at`

## Required Content
- Test cases designed after implementation from proposal, design, and delivered code.
- Submodule, module, external interface, and direct `change_id` coverage.
- Validation rationale tied to concrete behaviors, risks, and success criteria.
- Case-type coverage for normal, boundary, negative, error, compatibility, lifecycle, and cross-module cases, including the implementing test level.
- Per-level tables: `## Unit Tests`, `## DV Tests`, `## Integration Tests`, following `harness/rules/test-design-rules.md`.
- `## Design Element Coverage` mapping parameter domains, state transitions, failure paths, error categories, invariants, and concurrency declarations to cases or gaps.
- Stable test entrypoints aligned with `testplan.yaml` for completed testing work.
- Explicit gap records for missing direct validation.
- Large-module submodule test documentation when design uses direct submodules.

## Guardrails
- Testing operationalizes approved proposal/design intent against delivered implementation.
- Optional testing docs with approval metadata follow the common approval authority rule; agents MUST NOT approve their own testing docs.
- Testing runs after implementation and MUST inspect proposal, design, and delivered code before designing cases.
- Test design MUST follow `harness/rules/test-design-rules.md`: unit covers changed-code branches, DV covers lifecycle/workflows/failure workflow, and integration covers consumed exported-interface success and failure semantics.
- Test cases MUST derive from design elements and record concrete evidence in `## Design Element Coverage`; `not-applicable` rows need a concrete design source.
- Verify each behavior at the lowest test level that can expose its failure; duplicate cross-level verification records a reason.
- Failure paths, error categories, or state transitions found in code but missing from design return to design.
- Testing tasks are single-stage by default and MUST NOT edit proposal, design, acceptance, production code, `docs/architecture/`, or another task packet unless explicitly requested.
- Testing may modify test code, fixtures, runners, optional testing artifacts, and unified test entrypoint wiring needed to register tests.
- Code inside an existing Rust `#[cfg(test)]` item is test code. Every newly created unit test MUST live in a dedicated test file, test directory, or test-only crate/package; testing MUST NOT add new inline test bodies to production source files.
- Task-local testing artifacts stay in the same task packet, not older `testing/<task-seq>-<task-slug>/` directories.
- Human-authored testing docs MUST stay under 1000 lines, splitting by submodule, responsibility, validation layer, or interface boundary when needed.
- Every implemented change MUST have direct validation coverage or an explicit gap.
- Completed testing MUST generate/update `testplan.yaml` unless a repo-local versioned exception records reason, owner, risk, and acceptance impact.
- Every implemented `change_id` MUST map to validation ids and generated tests or `testplan.yaml` steps unless the validation path is `manual` or `disabled`.
- Every implemented `change_id` MUST have case-type rows; non-covered, manual, disabled, or not-applicable rows need concrete reasons.
- Every generated or changed automated test MUST be reachable through `harness/scripts/test-run.py`.
- The task's `testplan.yaml` MUST declare `task_name`, register only as `<module>/<task-name>`, and be reachable through `<module>/<task-name> all` without being aggregated into the top-level module.
- Module and whole-project regression use canonical module suites: `<module> all` runs that module's explicit canonical `all` suite and `all all` runs each module's canonical `all` suite once.
- Test execution evidence is the machine-written artifact under `test-results/test-runs/`; pasted command output is not evidence.
- Bugfix testing MUST show red-green regression evidence for the bugfix `change_id`, or record why pre-fix reproduction is not feasible.
- Validation paths MUST prove named behavior or risk, not unrelated checks.
- Acceptance-returned testing gaps MUST supplement test design, test implementation, metadata when used, and unified-entrypoint runnable evidence before retry.
- Manual or disabled layers require reasons in generated evidence and optional metadata when present.
- Upstream design or implementation problems route to the owning stage instead of silently widening testing scope.
- Downstream acceptance follow-up is recorded unless cross-stage synchronization is explicitly requested.
- Before completion, run:
  - manual flow: `uv run --active python ./harness/scripts/doc-structure-check.py --version <version> --module <module> --docs testing` only when optional `testing.md` exists
  - auto-pipeline: do not run `doc-structure-check.py --docs testing`, because `testing.md` / `testing/` are forbidden; validate pipeline testing evidence and `testplan.yaml` through `testing-coverage-check.py`
  - `uv run --active python ./harness/scripts/testing-coverage-check.py --version <version> --module <module>`
- Use `testing-coverage-check.py --allow-missing-testplan` only with a recorded repo-local versioned exception.
