---
module: example-module
task_name: 001-example-task
submodule: 001-example-task
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# [Module Name] Testing

> This optional file records post-implementation test design. Generate test cases from `proposal.md`, `design.md`, and the delivered code, then keep this file aligned with the actual test implementation.

## Test Document Index
| Document | Topic | Scope |
|----------|-------|-------|
| none | no split testing docs yet | full module |

<!-- Keep testing artifacts in this task packet. If a task spans multiple project-level modules, use the globals task packet. Do not put a new task's testing docs under an older task's testing/<task-seq>-<task-slug>/ directory. Keep each human-authored testing doc under 1000 lines; split oversized docs and update this index. -->

## Unified Test Entry
- Machine-readable task plan: `docs/versions/<version>/modules/<module>/<task-name>/testplan.yaml`
- Task all: `uv run --active python ./harness/scripts/test-run.py <module>/<task-name> all`
- Canonical module level: `uv run --active python ./harness/scripts/test-run.py <module> unit|dv|integration`
- Canonical module regression: `uv run --active python ./harness/scripts/test-run.py <module> all`
- Canonical project regression: `uv run --active python ./harness/scripts/test-run.py all all`
- Registration: every generated or changed automated test is reachable through the unified entrypoint.

## Submodule Tests
| Submodule | Responsibility | Detailed Test Doc | Required Behaviors | Edge/Failure Cases | Test Type | Test Files | Status | Gap / Manual Reason |
|-----------|----------------|-------------------|--------------------|--------------------|-----------|------------|--------|---------------------|
| | | | | | | | ready / gap / manual / disabled | |

## Module-Level Tests
| Test Item | Covered Boundary | Entry | Expected Result | Test Type | Test File/Script | Status | Gap / Manual Reason |
|-----------|------------------|-------|-----------------|-----------|------------------|--------|---------------------|
| | | | | | | ready / gap / manual / disabled | |

## External Interface Tests
| Interface | Responsibility | Success Cases | Failure/Edge Cases | Test Type | Test Doc/File | Status | Gap / Manual Reason |
|-----------|----------------|---------------|--------------------|-----------|---------------|--------|---------------------|
| | | | | | | ready / gap / manual / disabled | |

## Direct Change Coverage
| change_id | design_source | validation_id | testplan_level | testplan_step_id | Gap? | Gap / Manual Reason |
|-----------|---------------|---------------|----------------|------------------|------|---------------------|
| CHG-example | `design.md` section / `design/...` doc + code path | VAL-example | unit / dv / integration | example-unit | no | |

## Case-Type Coverage
| change_id | case_type | required | validation_id | level | status | gap_manual_reason |
|-----------|-----------|----------|---------------|-------|--------|-------------------|
| CHG-example | normal | yes/no | VAL-example | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |
| CHG-example | boundary | yes/no | VAL-example | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |
| CHG-example | negative | yes/no | VAL-example | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |
| CHG-example | error | yes/no | VAL-example | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |
| CHG-example | compatibility | yes/no | VAL-example | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |
| CHG-example | lifecycle | yes/no | VAL-example | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |
| CHG-example | cross-module | yes/no | VAL-example | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |

## Design Element Coverage
<!-- Derive cases mechanically from design artifacts per `harness/rules/test-design-rules.md`. Every element_type row is required; use `not-applicable` only with a concrete reason naming the design evidence. -->
| element_type | design_source | derived_cases | level | status | gap_manual_reason |
|--------------|---------------|---------------|-------|--------|-------------------|
| parameter-domain | design `## File-Level Interfaces` section / doc path | case ids or test names | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |
| state-transition | design `## State and Ownership` state transitions | case ids or test names | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |
| failure-path | design `## Key Call Flows` failure handling | case ids or test names | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |
| error-handling | error categories in changed code | case ids or test names | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |
| invariant | design `## Invariants to Preserve` | case ids or test names | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |
| concurrency | concurrency / reentrancy / ordering declarations in design | case ids or test names | unit / dv / integration | covered / gap / manual / disabled / not-applicable | reason if not covered |

## Validation Rationale
| Behavior or Risk | Validation Signal | Why This Is Sufficient | Gap / Manual Reason |
|------------------|-------------------|------------------------|---------------------|
| | | | |

## Unit Tests
<!-- Function/branch granularity per `harness/rules/test-design-rules.md`: every conditional branch of changed code is executed by a case or has a per-branch gap reason. -->
| Function or Unit | Branch or Condition | Covered Behavior | Test File | Status | Gap / Manual Reason |
|------------------|---------------------|------------------|-----------|--------|---------------------|
| | | | | covered / gap / manual / disabled / not-applicable | |

## DV Tests
<!-- Single-module runnable verification: module lifecycle, each main workflow, at least one failure workflow, behavior-changing config variants, and persisted-state recovery when applicable. -->
| Workflow | Kind | Entry | Expected Result | Test File or Script | Status | Gap / Manual Reason |
|----------|------|-------|-----------------|---------------------|--------|---------------------|
| | lifecycle / main / failure / config / persistence | | | | covered / gap / manual / disabled / not-applicable | |

## Integration Tests
<!-- Neighbor-module contracts: every consumed exported interface has a success case and a failure-semantics case (error propagation, timeout, retry, partial completion). -->
| Contract or Flow | Modules Involved | Success Case | Failure Case | Test File | Status | Gap / Manual Reason |
|------------------|------------------|--------------|--------------|-----------|--------|---------------------|
| | | | | | covered / gap / manual / disabled / not-applicable | |

## Regression Focus
<!-- Historical bugs and high-risk boundary cases -->

## Definition of Done
- [ ] Testing docs cover all direct submodules or explain why they do not exist
- [ ] Large-module testing docs are split into direct submodule packets when proposal/design uses direct submodules
- [ ] Human-authored testing docs stay under 1000 lines, or oversized docs are split and indexed
- [ ] `testplan.yaml` matches the declared test entrypoints
- [ ] `testplan.yaml` exists for completed testing work, unless a repo-local versioned exception explicitly permits missing machine-readable test metadata and records reason, owner, risk, and acceptance impact
- [ ] Generated tests are registered with `harness/scripts/test-run.py`
- [ ] New unit tests live in dedicated test files, test directories, or test-only crates/packages; no new inline test bodies were added to production source files
- [ ] `uv run --active python ./harness/scripts/test-run.py <module>/<task-name> all` reaches only this task's plan
- [ ] `uv run --active python ./harness/scripts/test-run.py <module> all` runs only this module's canonical `all` suite
- [ ] `uv run --active python ./harness/scripts/test-run.py all all` runs each module's canonical `all` suite once without appending task plans
- [ ] Module-level tests cover key boundary behavior and failure paths
- [ ] External interfaces have contract-focused tests
- [ ] Unit tests execute every conditional branch of changed code, or each uncovered branch has a per-branch gap reason
- [ ] DV tests cover module lifecycle, each main workflow, and at least one failure workflow, or record gaps
- [ ] Integration tests cover success and failure semantics for every consumed exported interface, or record gaps
- [ ] Every `## Design Element Coverage` element type maps to derived cases or carries a concrete not-applicable reason naming the design evidence
- [ ] Each behavior is verified at the lowest test level that can expose its failure (`harness/rules/test-design-rules.md`)
- [ ] Every implemented change has direct validation coverage or an explicit gap
- [ ] Every implemented `change_id` appears in `proposal.md`, `design.md`, generated test evidence, and optional `testplan.yaml` unless the validation path is explicitly `manual` or `disabled`
- [ ] Every validation path maps to a concrete behavior, risk, or success criterion
- [ ] Any `manual` or `disabled` layer has the same reason in `testing.md` and `testplan.yaml`
- [ ] Relevant automated tests pass

## Approval Record
<!-- Fill only when the user explicitly approves this document. Agents MUST NOT fill this section or set `status: approved` on their own initiative. `approver` must match front matter `approved_by`; `user_statement` must quote the user's approval instruction verbatim. The same edit must record front matter `approved_content_sha256` (generate via `schema-check.py --print-approval-hash <this-file>`); any later content edit invalidates approved evidence and MUST NOT be repaired by refreshing the hash. Use a sibling amendment/fix task for approved-document corrections. Auto-pipeline approvals use front matter plus `pipeline/plan.md` launch evidence instead of this section. -->
- approver:
- approval_date:
- user_statement: ""
