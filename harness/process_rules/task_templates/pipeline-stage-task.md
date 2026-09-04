# Pipeline Stage Task

## Task Identity
- Task ID:
- Stage: design / implementation / testing / acceptance
- Execution Mode: manual / auto-pipeline
- First Auto Stage:
- Responsibility:
- Scope:
- Version:
- Module:
- Task Packet: `docs/versions/<version>/modules/<project-or-globals>/<task-seq>-<task-slug>/`
- Packet Module: `<project-or-globals>`
- Target Module: `<project>`
- Submodule:
- change_id:
- Parent Task:
- Depends On:
- Owner:
- Expected Impact Paths (traceability only):
- Parallel-Eligible Ready Tasks:
- Serialization Reason: none / dependency / edit-coordination / concurrency-capacity

## Goal
- Describe the single stage outcome this task must complete.

## Inputs
- Proposal inputs:
- Upstream task outputs:
- Relevant docs:
- Relevant code:
- Constraints:

## Entry Checks
- [ ] For implementation-like work: `harness/rules/task-entry-gate-rules.md` was used for classification, not file authorization
- [ ] Required upstream artifacts exist
- [ ] Required upstream approvals exist
- [ ] If per-stage user confirmation is skipped, the pipeline plan records explicit user auto-pipeline authorization
- [ ] Runtime `.harness/pipelines/.../state.json` status was updated to `confirmed` or `complete` before dependent tasks continue
- [ ] Shared-artifact ownership is treated as a merge convention, not a write prohibition
- [ ] Dependency-ready work was scheduled with practical edit coordination and available capacity
- [ ] The launch stage and predecessors use manual rules; only stages at/after First Auto Stage use automatic artifact rules
- [ ] Automatic design/testing did not generate their corresponding Markdown artifacts, and automatic testing generated `testplan.yaml`
- [ ] If a repository-local extension produces a stage document and auto-confirmation is enabled, the document front matter was updated to `status: approved`
- [ ] Changed paths belong to this stage; cross-stage needs were returned or split and synchronization is recorded
- [ ] For design: the design decomposes top-down from the whole affected module to child submodules, nested submodules, and file-level modules where applicable
- [ ] For design: automatic design records child mappings in `pipeline/plan.md`; manual design records them in normal `design.md` / `design/`
- [ ] For design: `pipeline/plan.md` `## File-Level Implementation Sequence` lists concrete source files to create or modify in dependency order
- [ ] Baselines, locks, manifests, and Scope Paths are not used as project file permissions
- [ ] For implementation: task-local `pipeline/plan.md` `User launch statement` copies the user's explicit current instruction verbatim, and validated dependency/interface/state/failure/alternative evidence plus scope bindings cover current `change_id` values
- [ ] For implementation: active `version`, packet `module`, `target_module`, and `change_id` are explicit
- [ ] For implementation in a direct submodule packet: active `submodule` is explicit
- [ ] For implementation: a current schema result exists for the active packet; unchanged inputs were not rechecked
- [ ] For implementation in a direct submodule packet: schema and scope-binding checks passed with `--submodule <submodule>`
- [ ] For implementation: this child task corresponds to the next ready item in the pipeline-plan file-level implementation sequence
- [ ] For implementation: the task started with relevant context and read or changed any additional files needed
- [ ] For a `globals` packet: each affected project passed independently with `--module globals --submodule <task-name> --target-module <project>`
- [ ] For cross-submodule implementation: each affected submodule packet has an independent design Scope Path binding
- [ ] For implementation: preparation checks are evidence rather than edit authorization

## Expected Impact
- Likely files:
- Any additional project file may be read or changed as needed; shared-artifact edits are reported for integration.

## Required Outputs
- Output 1:
- Output 2:

## Done Condition
- [ ] Required output exists
- [ ] For testing: every generated or changed automated test is reachable through `harness/scripts/test-run.py`
- [ ] For acceptance: independent defect discovery covers every required correctness category before conclusion selection
- [ ] For acceptance: every `change_id` is reviewed and design/test correctness plus document consistency are recorded
- [ ] Actual impact is recorded where useful; no path-based checker blocks completion
- [ ] Dependencies satisfied
- [ ] Evidence attached

## Failure Handling
- If an upstream issue is found, either patch it when appropriate or record a return route; Harness does not prohibit the necessary files.
- Record:
  - blocking issue
  - suspected owning stage
  - return target
  - evidence
