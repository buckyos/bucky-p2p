# Pipeline Submodule Task

## Task Identity
- Task ID:
- Stage:
- Responsibility:
- Version:
- Module:
- Submodule / Task Directory:
- Packet Module:
- Target Module:
- change_id:
- Parent Task:
- Depends On:
- Owner:
- Expected Impact Paths (traceability only):
- Parallel-Eligible Ready Tasks:
- Serialization Reason: none / dependency / edit-coordination / concurrency-capacity

## Goal
- Complete the named stage for this task packet or direct submodule only.

## Scope Boundary
- Primary scope:
- Related areas:
- Shared topics handled elsewhere:

## Inputs
- Proposal excerpts:
- Design references:
- Testing references or generated test evidence:
- Upstream outputs:

## Entry Checks
- [ ] Required upstream artifacts exist
- [ ] The launch-confirmed proposal and boundary-selected design source exist; approved `design.md` is required when design precedes the first automatic stage
- [ ] If per-stage user confirmation is skipped, the pipeline plan records explicit user auto-pipeline authorization
- [ ] Runtime `.harness/pipelines/.../state.json` status was updated to `confirmed` or `complete` before dependent tasks continue
- [ ] Shared artifact ownership is a merge convention, not a write prohibition
- [ ] Ready sibling tasks used practical edit coordination and available capacity
- [ ] Automatic design/testing produce no corresponding stage Markdown document; manual stages retain normal documents and approval semantics
- [ ] The primary submodule/stage and any cross-area synchronization are recorded
- [ ] For design: this submodule's same-level structure and child mappings are recorded in `pipeline/plan.md`, without generating `design.md` or task-local `design/`
- [ ] For design: file-level modules owned by this submodule are recorded in the pipeline-plan implementation sequence in dependency order before implementation starts
- [ ] Baselines, locks, manifests, and Scope Paths are not used as project file permissions
- [ ] For implementation: the proposal is confirmed by an explicit current user launch recorded verbatim as `User launch statement`, and dependency/interface/state/failure/alternative design inputs for this submodule are validated
- [ ] For implementation: active `version`, packet `module`, `target_module`, submodule, and `change_id` are explicit
- [ ] For implementation: a current schema result exists for the submodule packet; unchanged inputs were not rechecked
- [ ] For implementation: Scope Paths are treated as planned-impact hints only
- [ ] For implementation: this child task implements the next ready file-level module in the validated pipeline-plan dependency sequence
- [ ] For implementation: the task started with relevant context and read or changed additional files as needed

## Required Outputs
- Output file(s):
- Evidence:

## Expected Impact
- Likely files:
- Any additional project file may be read or changed as needed; shared-artifact edits are reported for integration.

## Done Condition
- [ ] Submodule output is complete
- [ ] For testing: submodule tests are reachable through `harness/scripts/test-run.py <module>/<submodule> all` or the repository's documented submodule equivalent
- [ ] For acceptance: independent submodule defect discovery covers every applicable correctness category before conclusion selection
- [ ] For acceptance: design/test correctness plus document consistency are recorded for every submodule source that exists
- [ ] Actual impact is recorded where useful; no path-based checker blocks completion
- [ ] Handover data for the next dependent task exists

## Failure Handling
- If the issue is shared or upstream, return it instead of solving it inside this submodule task.
- Record:
  - issue id
  - return stage
  - return target task
  - expected upstream fix
