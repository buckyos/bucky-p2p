# Quality Gate Rules

## Goal
- Make explicitly requested code-level quality checks (build, lint, typecheck, format checks) declared and repeatable.
- Make quality-gate evidence verifiable: gates run through one checker that writes machine-readable run artifacts.

## Scope
- `harness/quality-gates.yaml`
- `harness/scripts/quality-check.py`
- `.harness/test-results/quality-runs/` (git-ignored run artifact output)

## Configuration Rule
- The repository MUST declare its mechanical quality gates in `harness/quality-gates.yaml`.
- Each gate MUST have a stable `id` and a non-interactive, deterministic `run` command list with a meaningful exit code.
- A missing `harness/quality-gates.yaml` fails closed: `quality-check.py` exits non-zero until the file exists.
- An explicitly empty `gates: []` list is allowed only with a concrete, non-placeholder `empty_reason`; missing or placeholder-only reasons fail closed.
- Choosing, adding, or removing gates should be recorded as a harness/process decision, but Harness never prevents another stage from editing `harness/quality-gates.yaml` when needed.

## Execution Rule
- Run gates through `UV_CACHE_DIR=.harness/uv-cache uv run --active python ./harness/scripts/quality-check.py`; do not run "equivalent" ad hoc commands as gate evidence.
- `quality-check.py` writes a run artifact to `.harness/test-results/quality-runs/<timestamp>-quality.json` recording each gate's command, exit code, and duration.
- Task execution, testing, acceptance, auto-pipeline completion, and `check-all.py` MUST NOT invoke quality gates automatically under generated rules. Run `quality-check.py` when the current user explicitly requests it or when a matching highest-priority custom rule requires it.
- Changes under `harness/**` or `docs/**` MUST NOT trigger quality gates.
- A report may mention an explicitly requested quality artifact as optional context, but `acceptance-report-check.py` does not validate or require it.

## Guardrails
- Quality gates supplement tests when explicitly requested or required by a matching custom rule; passing gates is not an acceptance conclusion by itself.
- Do not weaken or bypass a failing explicitly requested gate; report the failure to the user.
- Keep gates suitable for explicit maintenance runs; long-running behavioral validation belongs in task test levels under `test-run.py`.
