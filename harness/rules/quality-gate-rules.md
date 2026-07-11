# Quality Gate Rules

## Goal
- Make code-level quality checks (build, lint, typecheck, format checks) a declared, mechanical gate instead of reviewer goodwill.
- Make quality-gate evidence verifiable: gates run through one checker that writes machine-readable run artifacts.

## Scope
- `harness/quality-gates.yaml`
- `harness/scripts/quality-check.py`
- `test-results/quality-runs/` (git-ignored run artifact output)

## Configuration Rule
- The repository MUST declare its mechanical quality gates in `harness/quality-gates.yaml`.
- Each gate MUST have a stable `id` and a non-interactive, deterministic `run` command list with a meaningful exit code.
- A missing `harness/quality-gates.yaml` fails closed: `quality-check.py` exits non-zero until the file exists.
- An explicitly empty `gates: []` list is allowed only with a concrete, non-placeholder `empty_reason`; missing or placeholder-only reasons fail closed.
- Choosing, adding, or removing gates is a harness/process change: it belongs to a harness governance task, not to an implementation task. `stage-scope-check.py` rejects implementation-stage edits to `harness/quality-gates.yaml`.

## Execution Rule
- Run gates through `uv run --active python ./harness/scripts/quality-check.py`; do not run "equivalent" ad hoc commands as gate evidence.
- `quality-check.py` writes a run artifact to `test-results/quality-runs/<timestamp>-quality.json` recording each gate's command, exit code, duration, and `repository_state_sha256`; that artifact is valid only while its repository-state binding matches the current repository.
- Acceptance MUST run every gate declared in `harness/quality-gates.yaml`, and the acceptance report MUST cite the resulting run artifact. `not relevant` is valid only when the file declares `gates: []` with a concrete `empty_reason`.
- `acceptance-report-check.py` reads `harness/quality-gates.yaml`, verifies that cited quality artifact gate ids exactly match the configured gates, and fails an `accepted` conclusion when a configured gate lacks passing evidence.
- Repository-wide validation runs `quality-check.py` through the explicit `harness/scripts/check-all.py` command.

## Guardrails
- Quality gates supplement tests; passing gates is necessary supporting evidence, not an acceptance conclusion by itself.
- Do not weaken, skip, or conditionally bypass a task-relevant failing gate to make a task pass; a failing relevant gate routes the work back to the implementation stage (or to a harness governance task when the gate itself is wrong).
- Keep gates fast enough to run when relevant at acceptance time; long-running validation belongs in test levels under `test-run.py`, not in quality gates.
