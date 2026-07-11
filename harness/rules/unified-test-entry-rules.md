# Unified Test Entry Rules

## Goal
- Define the canonical runnable test interface for all project validation.
- Ensure every generated or hand-written test can be run through one stable command surface.

## Scope
- project-root test shortcuts: `test-run.bat` on Windows and `test-run.sh` on Unix-like systems
- `harness/scripts/test-run.py`
- generated test implementation
- optional `testing.md`
- `testplan.yaml` for completed testing work, unless a repo-local versioned exception explicitly allows missing machine-readable test metadata

## Canonical Commands
- Project-root shortcut:
  - Windows: `test-run.bat [<module> <level>]`
  - Unix: `./test-run.sh [<module> <level>]`
- `uv run --active python ./harness/scripts/test-run.py <module> unit`
- `uv run --active python ./harness/scripts/test-run.py <module> dv`
- `uv run --active python ./harness/scripts/test-run.py <module> integration`
- `uv run --active python ./harness/scripts/test-run.py <module> all`
- `uv run --active python ./harness/scripts/test-run.py <module>/<task-name> all`
- `uv run --active python ./harness/scripts/test-run.py all all`

## Consistency Rule
- `harness/scripts/test-run.py` is mandatory in generated repositories.
- What each level (`unit` / `dv` / `integration`) must verify and how deep is defined by `harness/rules/test-design-rules.md`; this rule governs how those tests are invoked.
- A generated repository MUST include both project-root one-click test shortcuts: `test-run.bat` for Windows and `test-run.sh` for Unix-like systems.
- The root shortcut MUST check whether `uv` is installed and print an installation hint when it is missing.
- The root shortcuts MUST create a local `.venv` when it is missing, use `uv` to sync or install dependencies when project metadata exists, activate the project virtual environment, and then invoke `harness/scripts/test-run.py` through `uv run --active python`.
- The root shortcut MUST NOT bypass the unified test entrypoint.
- The unified test interface MUST be able to run every project test that is part of the harness evidence chain.
- A testing task is not complete until every new or changed test implementation is registered with, or otherwise reachable through, the unified test interface.
- Every task-local `testplan.yaml` MUST declare `task_name` and register only as `<module>/<task-name>`; it MUST NOT be attached to the bare `<module>` scope.
- Every top-level module MUST define canonical suites independently of task plans. The canonical `all` suite MUST be explicit and non-empty.
- Generated tests, `testing.md`, and `testplan.yaml` must reference the same validation surfaces for completed testing work.
- Test scripts MUST be non-interactive and return meaningful exit codes.
- New test execution paths MUST be added to the canonical entrypoint instead of creating unrelated ad hoc commands.
- Test implementation may use local framework-specific commands internally, but acceptance and pipeline tasks must call them through `harness/scripts/test-run.py`.

## Execution Contract
- Unknown modules or test levels MUST exit non-zero.
- Every real run (not `--list` / `--dry-run`) MUST write a machine-readable run artifact to `test-results/test-runs/` recording requested module/task scope, requested level, covered `change_ids`, each executed command, its registration sources, exit code, duration, git state, and `repository_state_sha256`; acceptance requires that repository-state binding to match the current repository. `test-results/` is generated output, lives outside `harness/`, and MUST be listed in `.gitignore`.
- `<module>/<task-name> all` MUST run only the task's `unit`, `dv`, and `integration` testplan commands.
- `<module> unit|dv|integration` MUST run only that level from the module's canonical suite.
- `<module> all` MUST run only the module's explicit canonical `all` suite; it MUST NOT append current or historical task plans.
- `all all` MUST run only each registered module's explicit canonical `all` suite once, in deterministic module order; it MUST NOT append task plans.
- Exact argv duplicates MUST execute once and the run artifact MUST preserve all contributing registration sources.
- The runner MUST NOT infer semantic containment between different argv values. Broad and filtered framework commands remain distinct unless scope isolation prevents them from being selected together.
- Enabled steps MUST execute in declared order.
- "success without executing steps" MUST be reserved for task scopes whose every selected level is `manual` or `disabled` with a concrete reason; the run artifact records those non-executed levels and is not automated acceptance evidence by itself.
- Each enabled step MUST declare stable machine-readable fields `id`, `name`, `change_ids`, and `run`.
- Each enabled step MUST declare the `change_ids` it validates when it is used as acceptance evidence.
- `harness/scripts/schema-check.py` MUST reject unknown levels, duplicate step ids, enabled levels without steps, enabled steps without `change_ids`, and manual or disabled levels without reasons when `testplan.yaml` exists.
