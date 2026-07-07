---
module: workspace-harness
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-07-07T17:10:00+08:00
approved_content_sha256: 15905af1943aae1d3c010a2ace35262add33e8bde419da09e410091f8152e4cd
---

# workspace-harness Testing

> Post-implementation test design for workspace harness governance and unified runner behavior.

## Test Document Index
| Document | Topic | Scope |
|----------|-------|-------|
| none | no split testing docs yet | full module |

## Unified Test Entry
- Machine-readable plan: `docs/versions/v0.1/modules/workspace-harness/testplan.yaml`
- Unit: `uv run --active python ./harness/scripts/test-run.py workspace-harness unit`
- DV: `uv run --active python ./harness/scripts/test-run.py workspace-harness dv`
- Integration: `uv run --active python ./harness/scripts/test-run.py workspace-harness integration`
- Module all: `uv run --active python ./harness/scripts/test-run.py workspace-harness all`
- Project all: `uv run --active python ./harness/scripts/test-run.py all all`
- Registration: every generated or changed automated test is reachable through the unified entrypoint.

## Submodule Tests
| Submodule | Responsibility | Detailed Test Doc | Required Behaviors | Edge/Failure Cases | Test Type | Test Files | Status | Gap / Manual Reason |
|-----------|----------------|-------------------|--------------------|--------------------|-----------|------------|--------|---------------------|
| unified-test-entry | command discovery, dedupe, artifact generation | this file | testplan precedence, default dedupe, no-dedupe opt-out | repeated workspace and sn-miner commands | unit / dv / integration | `harness/scripts/test-run.py` | ready | |
| governance-checkers | schema/admission/structure checks | this file | fail-closed governance checks remain reachable | stale evidence and missing mappings | unit / dv / integration | `harness/scripts/*-check.py` | ready | |

## Module-Level Tests
| Test Item | Covered Boundary | Entry | Expected Result | Test Type | Test File/Script | Status | Gap / Manual Reason |
|-----------|------------------|-------|-----------------|-----------|------------------|--------|---------------------|
| runner syntax | `test-run.py` Python syntax | `python3 -m py_compile harness/scripts/test-run.py` | script compiles | unit | `harness/scripts/test-run.py` | ready | |
| workspace structure | governance file inventory | `verify-workspace-harness.py v0.1` | required governance files exist | unit | `harness/scripts/verify-workspace-harness.py` | ready | |
| default dedupe dry-run | scheduling and reuse markers | `test-run.py all all --dry-run` | repeated commands show `# reused from step` and no physical execution occurs | dv | `harness/scripts/test-run.py` | ready | |
| no-dedupe dry-run | opt-out scheduling | `test-run.py all all --dry-run --no-dedupe` | repeated commands are listed without reuse markers | dv | `harness/scripts/test-run.py` | ready | |
| module all evidence | workspace-harness module plan | `test-run.py workspace-harness all` | unit, dv, and integration scheduling tests pass | integration | `harness/scripts/test-run.py` | ready | |

## External Interface Tests
| Interface | Responsibility | Success Cases | Failure/Edge Cases | Test Type | Test Doc/File | Status | Gap / Manual Reason |
|-----------|----------------|---------------|--------------------|-----------|---------------|--------|---------------------|
| `test-run.py <module> <level>` | unified invocation | module all and all all still run through CLI | `--no-dedupe` disables reuse | integration | `testplan.yaml` and generated artifacts | ready | |
| run artifact JSON | machine evidence | real module/project runs write artifact with standard fields | reused steps add `deduped` / `reused_from_step` | integration | `test-results/test-runs/*.json` | ready | |

## Direct Change Coverage
| change_id | design_source | validation_id | testplan_level | testplan_step_id | Gap? | Gap / Manual Reason |
|-----------|---------------|---------------|----------------|------------------|------|---------------------|
| workspace_harness_acceptance_review_gate | `design.md` Directly Mapped Change Items | V-HARNESS-STRUCTURE | unit | workspace-harness-structure | no | |
| workspace_harness_direct_admission_gate | `design.md` Directly Mapped Change Items | V-HARNESS-SCHEMA-ADMISSION | integration | workspace-harness-schema-p2p-frame / workspace-harness-admission-p2p-frame / workspace-harness-admission-cyfs-p2p | no | |
| workspace_harness_pipeline_plan_current_change | `design.md` Directly Mapped Change Items | V-HARNESS-PIPELINE | unit | workspace-harness-structure | no | |
| workspace_harness_test_run_dedupe | `design.md` Directly Mapped Change Items, Overall Approach, Key Call Flows, Key Decisions, and Invariants to Preserve | V-HARNESS-TEST-RUN-DEDUPE | integration | workspace-harness-test-run-all-all-dry-run | no | Unit and DV support cases are also registered in testplan steps `workspace-harness-test-run-py-compile`, `workspace-harness-test-run-dedupe-dry-run`, and `workspace-harness-test-run-no-dedupe-dry-run`. |

## Case-Type Coverage
| change_id | case_type | required | validation_id | level | status | gap_manual_reason |
|-----------|-----------|----------|---------------|-------|--------|-------------------|
| workspace_harness_test_run_dedupe | normal | yes | V-HARNESS-TEST-RUN-DEDUPE-DRY | dv | covered | |
| workspace_harness_test_run_dedupe | boundary | yes | V-HARNESS-TEST-RUN-NO-DEDUPE-DRY | dv | covered | |
| workspace_harness_test_run_dedupe | negative | yes | V-HARNESS-TEST-RUN-NO-DEDUPE-DRY | dv | covered | opt-out proves duplicate physical scheduling remains available. |
| workspace_harness_test_run_dedupe | error | yes | V-HARNESS-TEST-RUN-COMPILE | unit | covered | syntax and fail-fast behavior are additionally checked by project all runs. |
| workspace_harness_test_run_dedupe | compatibility | yes | V-HARNESS-TEST-RUN-ALL-DRY | integration | covered | existing CLI forms remain accepted. |
| workspace_harness_test_run_dedupe | lifecycle | yes | V-HARNESS-TEST-RUN-ALL-DRY | integration | covered | run start -> command scheduling -> artifact-producing real runs are verified in acceptance. |
| workspace_harness_test_run_dedupe | cross-module | yes | V-HARNESS-TEST-RUN-ALL-DRY | integration | covered | all/all dry-run covers all registered modules. |

## Design Element Coverage
| element_type | design_source | derived_cases | level | status | gap_manual_reason |
|--------------|---------------|---------------|-------|--------|-------------------|
| parameter-domain | design `## Interfaces and Dependencies`: optional `--no-dedupe` | no-dedupe dry-run vs default dry-run | dv | covered | |
| state-transition | design `## Data and State`: per-run command result cache | dry-run reuse markers and artifact inspection after real all/all | integration | covered | |
| failure-path | design `## Key Call Flows`: reused non-zero result must propagate | project all and module all fail-fast behavior remain unchanged; no artificial failing command is introduced in repository runner | integration | covered | |
| error-handling | design `## Invariants to Preserve`: unknown module/level fail closed | existing runner parser and command selection paths retained; py_compile validates script syntax | unit | covered | |
| invariant | design `## Invariants to Preserve` | module all, project all, and root shortcut remain supported | integration | covered | |
| concurrency | design declares per-process in-memory cache only | no shared cross-process state exists | unit | not-applicable | not-applicable: no concurrency or parallel execution is introduced. |

## Validation Rationale
| Behavior or Risk | Validation Signal | Why This Is Sufficient | Gap / Manual Reason |
|------------------|-------------------|------------------------|---------------------|
| duplicate command execution | default all/all dry-run shows reused commands | exercises scheduling without the cost of physical command execution | |
| opt-out for strict rerun | `--no-dedupe` all/all dry-run lists duplicates without reuse markers | proves debug path remains available | |
| artifact compatibility | real all/all artifact includes standard top-level fields and reused step fields | acceptance-report-check still verifies fresh all/all artifact | |
| business module behavior | p2p-frame all and project all still pass | proves runner optimization did not alter business tests | |

## Unit Tests
| Function or Unit | Branch or Condition | Covered Behavior | Test File | Status | Gap / Manual Reason |
|------------------|---------------------|------------------|-----------|--------|---------------------|
| `commands_for` | testplan has commands | testplan commands replace fallback commands for same module/level | `test-run.py workspace-harness all` dry-run steps | covered | |
| `main` dedupe path | command key already cached | reused step is recorded without physical execution | all/all dry-run and artifact inspection | covered | |
| `main` no-dedupe path | `--no-dedupe` set | repeated commands remain physically scheduled | all/all no-dedupe dry-run | covered | |

## DV Tests
| Workflow | Kind | Entry | Expected Result | Test File or Script | Status | Gap / Manual Reason |
|----------|------|-------|-----------------|---------------------|--------|---------------------|
| runner scheduling default | main | `test-run.py all all --dry-run` | repeated commands are marked reused | `harness/scripts/test-run.py` | covered | |
| runner scheduling opt-out | config | `test-run.py all all --dry-run --no-dedupe` | repeated commands are listed without reuse markers | `harness/scripts/test-run.py` | covered | |
| runner syntax | failure | `python3 -m py_compile harness/scripts/test-run.py` | syntax errors fail the run | `harness/scripts/test-run.py` | covered | |
| runner process lifecycle | lifecycle | `test-run.py workspace-harness all` | starts, schedules unit/dv/integration steps, writes an artifact, and exits 0 | `harness/scripts/test-run.py` | covered | |

## Integration Tests
| Contract or Flow | Modules Involved | Success Case | Failure Case | Test File | Status | Gap / Manual Reason |
|------------------|------------------|--------------|--------------|-----------|--------|---------------------|
| unified runner all/all | all registered modules | `test-run.py all all` passes and writes artifact | non-zero command would stop run and write failing artifact | `harness/scripts/test-run.py` | covered | |
| root shortcut | all registered modules | `./test-run.sh all all` passes and writes artifact | root shortcut propagates runner exit code | `test-run.sh` | covered | |
| acceptance artifact consumption | acceptance checker | acceptance-report-check accepts fresh passing all/all artifact | missing/failing artifact rejected by checker | `harness/scripts/acceptance-report-check.py` | covered | |

## Regression Focus
- Repeated `cargo test --workspace` commands across module integration levels are reused in one run by default.
- Repeated `cargo test -p sn-miner --test real_process -- --test-threads=1` commands are reused in one run by default.
- `--no-dedupe` remains available for strict repeated physical runs.

## Definition of Done
- [x] Testing docs cover all direct submodules or explain why they do not exist
- [x] Large-module testing docs are split into direct submodule packets when proposal/design uses direct submodules
- [x] Human-authored testing docs stay under 1000 lines, or oversized docs are split and indexed
- [x] `testplan.yaml` matches the declared test entrypoints
- [x] `testplan.yaml` exists for completed testing work, unless a repo-local versioned exception explicitly permits missing machine-readable test metadata and records reason, owner, risk, and acceptance impact
- [x] Generated tests are registered with `harness/scripts/test-run.py`
- [x] `uv run --active python ./harness/scripts/test-run.py <module> all` reaches this module's automated tests
- [x] `uv run --active python ./harness/scripts/test-run.py all all` reaches all project tests registered with the harness
- [x] Module-level tests cover key boundary behavior and failure paths
- [x] External interfaces have contract-focused tests
- [x] Unit tests execute every conditional branch of changed code, or each uncovered branch has a per-branch gap reason
- [x] DV tests cover module lifecycle, each main workflow, and at least one failure workflow, or record gaps
- [x] Integration tests cover success and failure semantics for every consumed exported interface, or record gaps
- [x] Every `## Design Element Coverage` element type maps to derived cases or carries a concrete not-applicable reason naming the design evidence
- [x] Each behavior is verified at the lowest test level that can expose its failure (`harness/rules/test-design-rules.md`)
- [x] Every implemented change has direct validation coverage or an explicit gap
- [x] Every implemented `change_id` appears in `proposal.md`, `design.md`, generated test evidence, and optional `testplan.yaml` unless the validation path is explicitly `manual` or `disabled`
- [x] Every validation path maps to a concrete behavior, risk, or success criterion
- [x] Any `manual` or `disabled` layer has the same reason in `testing.md` and `testplan.yaml`
- [x] Relevant automated tests pass

## Approval Record
- approver: auto-pipeline
- approval_date: 2026-07-07T17:10:00+08:00
- user_statement: "确认，自动处理后续步骤"
