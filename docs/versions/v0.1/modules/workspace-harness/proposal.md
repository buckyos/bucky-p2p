---
module: workspace-harness
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-07-07T16:58:00+08:00
approved_content_sha256: 0664def7647d2e078612ad86da30a878bc41a09553d7ad80c4bd78792c95cbed
---

# workspace-harness Proposal

## Background and Goal
- The workspace harness owns `harness/rules/**`, `harness/scripts/**`, and `harness/pipeline-plan.md`.
- `harness/scripts/test-run.py all all` currently executes repeated commands because direct `TEST_COMMANDS` entries and module `testplan.yaml` steps are both appended, and because several module integration levels register the same `cargo test --workspace` command.
- The goal is to keep the canonical unified test entrypoint and machine-written run artifacts while reducing unnecessary repeated command execution.
- Existing workspace-harness governance coverage for admission, acceptance, and pipeline plan remains part of this packet.

## Scope
### In scope
- Acceptance review rules for full worktree diff, untracked files, cross-module admission, and direct change mapping.
- Direct change mapping, schema validation, module document exceptions, task entry gate, and related admission scripts.
- `schema-check.py`, `admission-check.py`, `verify-workspace-harness.py`, and other governance scripts under `harness/scripts/**`.
- `harness/pipeline-plan.md` as the current auto-pipeline execution graph and return routing record.
- `test-run.py` command scheduling optimization that avoids repeated execution of identical commands inside one run while preserving module/level coverage evidence in the run artifact.
- `test-run.py` command source precedence so module `testplan.yaml` entries do not duplicate fallback registry entries for the same module and level.

### Out of scope
- Changing `p2p-frame`, `cyfs-p2p`, `sn-miner`, `desc-tool`, or other business module protocols, runtime behavior, or tests.
- Weakening `test-run.py all all`; it must still reach all registered project tests in deterministic order.
- Dropping machine-readable run artifact evidence or making acceptance rely on pasted terminal output.
- Silently bypassing failing tests, quality gates, schema checks, admission checks, or acceptance checks.
- Treating this governance change as approval for unrelated business module implementation changes.

### Boundary with neighboring modules
- Business modules own their own proposal/design/testing/implementation evidence chains and test content.
- `workspace-harness` owns the unified invocation surface, command discovery, dedupe policy, run artifact shape, and governance checkers.
- If runner optimization exposes stale or duplicate module testplan entries, the harness may report them or dedupe execution, but business module test semantics remain owned by the affected module packet.

## Assumptions and Ambiguities
- Assumptions:
  - Re-running the exact same command argv in the same `test-run.py` invocation is redundant when the command, working directory, and environment are identical.
  - The run artifact can represent reused command results without losing coverage traceability if each module/level step records the command status and whether it was executed or reused.
  - Deterministic command order remains required for `all all`.
- Open ambiguities:
  - Some repeated commands may be intentionally repeated to expose flaky behavior; this should remain possible through an explicit override.
- Decision needed before approval:
  - Approve adding deterministic in-run command deduplication with an opt-out such as `--no-dedupe`.

## Constraints
- Allowed libraries/components:
  - Python standard library only for `harness/scripts/test-run.py` changes.
  - Existing `test-results/test-runs/*.json` artifact directory and schema version unless design decides a compatible extension is required.
- Disallowed approaches:
  - Do not remove module/level coverage records from artifacts just because a command result is reused.
  - Do not make `all all` skip a registered module, level, or enabled testplan step.
  - Do not make dedupe depend on command string parsing that changes argv semantics.
- System constraints:
  - `test-run.py all all`, `<module> all`, and `<module> <level>` remain canonical supported entrypoints.
  - Acceptance checks must still be able to verify fresh passing whole-project artifacts.

## Requirement Challenge
| question | evaluation | risk_or_tradeoff | decision |
|----------|------------|------------------|----------|
| Is the stated requirement reasonable for the user's goal? | Yes. Dry-run evidence shows exact repeated commands such as duplicate `cargo test -p cyfs-p2p` and multiple `cargo test --workspace` executions in one `all all` run. | Optimizing execution must not hide module coverage or test failures. | revise: dedupe execution while preserving per-module evidence. |
| Is there a simpler or safer approach? | Removing fallback `TEST_COMMANDS` entries would reduce some duplicates, but not repeated workspace integration commands across modules. | Command removal alone risks losing coverage when a module lacks or breaks `testplan.yaml`. | choose command-result reuse with testplan precedence and fallback behavior. |
| Is scope ambiguous? | The optimization target is the unified runner, not business test content. | If treated as business testing work, module ownership and admission would be unclear. | proceed in workspace-harness with explicit boundary. |

## Large Module Submodule Decision
| Submodule | New or Existing | Responsibility | Proposal Packet | Reason |
|-----------|-----------------|----------------|-----------------|--------|
| unified-test-entry | existing | Unified harness test invocation, command discovery, execution scheduling, and run artifacts. | `not-applicable: retained in workspace-harness proposal.md` | The feature changes one existing harness script responsibility and does not need an independent submodule packet. |

## Trigger Matrix
| trigger_category | applies | evidence | required_checks | deferred_checks_and_reason |
|------------------|---------|----------|-----------------|----------------------------|
| contract/protocol | yes | `test-run.py` artifact semantics are consumed by acceptance-report checks. | design must define compatible artifact evidence for executed and reused commands; acceptance report check must still pass. | none |
| data/schema | yes | Run artifact step records may need a compatible field for deduped/reused commands. | schema/design review of artifact JSON shape; direct artifact inspection after runner tests. | none |
| security/privacy/permission | no | Runner executes local test commands and does not alter permission or secret handling. | none | none |
| runtime/integration | yes | `test-run.py all all` executes workspace, module, integration, and DV commands. | dry-run comparison, lightweight module all run, and one authoritative physical project run through the root shortcut. | none |
| build/dependency/config/deployment | yes | Runner invokes `cargo`, Python harness checks, and root shortcut wrappers. | `test-run.py --dry-run`, `test-run.py workspace-harness all`, and one `./test-run.sh all all` physical full run. Direct `test-run.py all all` is reserved for wrapper debugging. | none |
| ui/datamodel/workflow | no | No UI or application data model is affected. | none | none |
| harness/process | yes | This is a governance runner optimization under `harness/scripts/test-run.py`. | schema/admission checks, stage-scope checks, acceptance-report checks, and pipeline-plan checks as applicable. | none |

## High-Level Outcomes
- `test-run.py all all` avoids executing identical commands repeatedly within the same invocation.
- Run artifacts remain suitable acceptance evidence and show which module/level entries were covered by executed or reused command results.
- Module `testplan.yaml` entries take precedence over static fallback registry entries for the same module/level to avoid double registration.
- A debug opt-out exists for strict reruns or flaky investigation.
- Existing `test-run.py` entrypoint names and root shortcut behavior remain stable.

## Proposal Items
| proposal_id | change_id | Outcome | Scope Boundary | Success Evidence | Explicit Non-Goal |
|-------------|-----------|---------|----------------|------------------|-------------------|
| P-HARNESS-REVIEW-1 | workspace_harness_acceptance_review_gate | acceptance rules require review of full worktree diff, untracked files, cross-module admission, and direct change mapping. | Governance review rules only; no business module approval bypass. | design/testing cover acceptance rule, trigger rule, schema/admission checker, and workspace harness verifier. | Do not let passing tests alone mean acceptance passed. |
| P-HARNESS-ADMISSION-1 | workspace_harness_direct_admission_gate | task entry, schema-check, and admission-check form direct change_id admission gates that fail closed on missing mapping. | Admission and schema governance only; explicit module exceptions remain narrow. | schema/admission/check-implementation entries give consistent results for default and exempt modules. | Do not add implicit exemptions for default modules. |
| P-HARNESS-PIPELINE-1 | workspace_harness_pipeline_plan_current_change | `harness/pipeline-plan.md` records current change stage graph, dependencies, return routing, and exit conditions. | Current pipeline planning only; launch remains explicit. | pipeline plan matches approved proposal, design/testing state, and acceptance return routing. | Do not treat pipeline plan as approval by itself. |
| P-HARNESS-TEST-RUN-1 | workspace_harness_test_run_dedupe | `test-run.py` avoids duplicate command execution within one run while preserving module/level coverage evidence and deterministic ordering. | `harness/scripts/test-run.py`, related unified-test-entry rules if needed, and workspace-harness test metadata. | dry-run shows repeated commands collapsed or marked reused; lightweight module all passes; one root shortcut all/all artifact passes and includes reuse evidence. | Do not skip registered coverage, remove artifact evidence, change business module test commands, or require duplicate physical all/all runs by default. |

## Success Criteria
- Concrete user-visible or system-visible result:
  - Targeted dry-runs show repeated command reuse by default and repeated scheduling when dedupe is explicitly disabled.
  - The single required physical all/all run writes an artifact where reused command results remain traceable to the requesting module/level.
- Required evidence:
  - `doc-structure-check.py --docs proposal` passes before proposal handoff.
  - After downstream implementation, `test-run.py workspace-harness all` passes as focused harness coverage, and `./test-run.sh all all` passes as the single authoritative physical project-wide run.
  - Acceptance report check still accepts fresh passing whole-project artifacts.
- Explicit non-goals:
  - No change to business module protocols, runtime semantics, or test assertions.
  - No acceptance bypass and no deletion of required harness checks.

## Risks
- Over-aggressive dedupe could hide flaky tests or commands whose environment changes between module contexts.
- Artifact changes could break acceptance-report checks or downstream tooling that reads step records.
- Dedupe could make debugging harder if users expect every module/level command to execute physically.
- Keeping both fallback registry and testplan discovery without clear precedence can reintroduce duplicate execution.

## Downstream Follow-Up
| follow_up_id | Owning Stage | Reason | Triggering Proposal Item | Blocking |
|--------------|--------------|--------|--------------------------|----------|
| FU-HARNESS-TEST-RUN-DESIGN | design | Define command identity, testplan precedence, artifact representation for reused results, and `--no-dedupe` behavior. | P-HARNESS-TEST-RUN-1 | yes |
| FU-HARNESS-TEST-RUN-IMPLEMENTATION | implementation | Update `harness/scripts/test-run.py` only after approved design and admission. | P-HARNESS-TEST-RUN-1 | yes |
| FU-HARNESS-TEST-RUN-TESTING | testing | Add or update runnable evidence for dry-run, lightweight module all, and a single root shortcut project all run. | P-HARNESS-TEST-RUN-1 | yes |
| FU-HARNESS-TEST-RUN-ACCEPTANCE | acceptance | Audit that dedupe preserves registered coverage and fresh artifact evidence. | P-HARNESS-TEST-RUN-1 | yes |

## Proposal Guardrails
- Proposal-stage tasks modify only `proposal.md` unless the user explicitly requests a multi-stage update.
- If this proposal change requires design, implementation, testing, or acceptance updates, record the needed follow-up instead of editing downstream artifacts by default.
- If this is a large module with many independent submodules, classify whether the requested feature is a new direct submodule before design or testing starts.
- Put the split submodule's proposal and design files in a submodule directory under this module packet; optional post-implementation testing artifacts may live there when generated. Do not use `design/<submodule>/` or `testing/<submodule>/` for independent submodule docs.
- Keep human-authored proposal docs under 1000 lines where practical; if a doc would exceed 1000 lines, split it and update the relevant document index.
- If the request has multiple plausible meanings, record the ambiguity instead of silently choosing one.
- Proposal approval should not depend on chat-only context; task-critical assumptions belong in this file.
- Keep implementation strategy out of proposal except where a constraint is part of the requirement.
- Every implementation-ready requirement must have a stable `change_id`.
- A broad module-level statement is not enough for implementation admission; the relevant `change_id` must name the concrete behavior, contract, or implementation unit being admitted.

## Approval Record
- approver: auto-pipeline
- approval_date: 2026-07-07T16:58:00+08:00
- user_statement: "确认，自动处理后续步骤"
