---
module: p2p-frame
task_name: 015-callback-result-scope-path-amendment
submodule: 015-callback-result-scope-path-amendment
version: v0.1
status: approved
approved_by: user
approved_at: 2026-08-26
approved_content_sha256: 0cb29c7e8af554e9f62baa58a949d3ba0e050bb53012de792304e97eb63bcfd9
---

# Callback Result Scope Path Amendment Proposal

## Background and Goal

This packet is a sibling amendment to `docs/versions/v0.1/modules/p2p-frame/014-callback-result-published-release-migration/`.

Task 014 correctly approved migration from the local callback-result patch to crates.io 0.2.5, but its design recorded the removal boundary as the glob `third-party/callback-result/**`. Implementation admission accepted that mapping, while the required implementation `stage-scope-check.py` rejected it because admitted Scope Paths must be concrete paths rather than globs.

The goal is to preserve the complete approved 014 behavior and implementation while correcting only the machine-readable scope spelling to the concrete directory prefix `third-party/callback-result`.

## Scope

### In scope

- Amend task 014's implementation admission through this sibling packet.
- Bind the unchanged registry migration to concrete Scope Paths: `Cargo.toml`, `p2p-frame/Cargo.toml`, `Cargo.lock`, and `third-party/callback-result`.
- Reuse the already delivered minimal migration: require 0.2.5, remove the root path patch, resolve the registry lock entry, and delete the local package.
- Continue the required stage checks, post-implementation verification, and acceptance against the unchanged 014 success baseline.

### Out of scope

- Editing the approved 014 proposal or design.
- Adding, removing, or changing any production behavior beyond the migration already approved by 014.
- Expanding deletion above `third-party/callback-result` or upgrading unrelated dependencies.
- Changing Harness rules or weakening `stage-scope-check.py`.

### Boundary with neighboring modules

- Task 014 remains the requirement baseline for the dependency migration.
- Task 015 owns only the corrected concrete admission mapping required to validate the same p2p-frame build-resource changes.
- Unrelated p2p-frame production source, downstream modules, and unfinished task 001 remain excluded.

## Requirement Review

- A sibling amendment is required because task 014's approved design is immutable and the checker rejects its glob syntax.
- Changing the checker would broaden the task and weaken a fail-closed rule; correcting the task-local mapping is the smaller and safer route.
- Using `third-party/callback-result` as a directory prefix retains exactly the original deletion boundary: descendants match, while sibling paths under `third-party/` do not.
- The amendment does not retroactively claim new implementation behavior. Task 014 admission passed before the migration edits; task 015 supplies the corrected scope mapping needed for completion evidence.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-CRSPA-1 | callback_result_scope_path_amendment | Validate the unchanged task 014 registry migration under concrete implementation Scope Paths, replacing only the rejected `third-party/callback-result/**` mapping with `third-party/callback-result` | Task-local admission mapping for `Cargo.toml`, `p2p-frame/Cargo.toml`, `Cargo.lock`, and the `third-party/callback-result` directory prefix | Adds a sibling governance packet for a one-token scope syntax correction, accepted because modifying frozen task 014 or weakening the checker is forbidden | schema/admission pass for task 015; implementation scope check accepts every migrated/deleted path and rejects paths outside the concrete prefix; task 014 functional success criteria remain satisfied | No new production behavior, no Harness change, no broader third-party deletion, no unrelated dependency update |

## Success Criteria

- Concrete result: the existing callback-result 0.2.5 migration passes implementation scope validation using concrete non-glob paths.
- Required evidence: approved amendment proposal/design, task 015 admission stamp, passing implementation scope check, and downstream task verification/acceptance tied back to task 014.
- Explicit non-goals: no change to the delivered Cargo diff or deleted package boundary and no mutation of approved task 014 documents.

## Risks

- A prefix broader than `third-party/callback-result` could admit unrelated deletion; the design must use the exact directory name.
- Omitting one of the three Cargo files would leave the delivered atomic migration partly outside admission.
- Treating this as a new runtime fix could duplicate or widen test obligations; downstream evidence must describe it as an admission correction for the unchanged task 014 implementation.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | no | This amendment changes only task-local Scope Path syntax and preserves task 014's approved dependency contract | Confirm no new interface path enters design | Proposal maps only existing migration paths | owner: none; reason: not applicable beyond task 014; acceptance impact: task 014 contract evidence still required | none |
| data/schema | no | No persisted data, serialization, cache, or migration path is changed | Scope review | Exact Cargo/vendor paths recorded | owner: none; reason: not applicable; acceptance impact: none | none |
| security/privacy/permission | no | No trust, identity, secret, permission, TLS, or input boundary changes | Scope review | Exact task-local correction recorded | owner: none; reason: not applicable; acceptance impact: none | none |
| runtime/integration | no | No runtime code or behavior changes beyond already-approved task 014 | Confirm implementation diff is unchanged | Proposal explicitly forbids new runtime changes | owner: acceptance; reason: task 014 runtime evidence remains authoritative; acceptance impact: unexplained runtime diff blocks completion | none |
| build/dependency/config/deployment | yes | The corrected Scope Paths admit the existing `Cargo.toml`, `p2p-frame/Cargo.toml`, `Cargo.lock`, and local dependency deletion | Run schema, admission, concrete implementation scope check, then reuse/produce task 014 dependency verification | Proposal enumerates all four exact boundaries | owner: implementation/testing; reason: scope and dependency evidence follow approval; acceptance impact: failed concrete scope or registry migration blocks completion | Incorrect prefix could under- or over-admit deletion |
| ui/datamodel/workflow | no | No UI or presentation path is named | Scope review | No UI paths admitted | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | yes | The amendment exists because `stage-scope-check.py` rejects the glob stored in frozen task 014 design; checker source remains unchanged | Validate proposal/design structure, admission, and stage scope with concrete paths | Failure was reproduced with the exact checker message | owner: implementation/acceptance; reason: passing result requires approved design and new admission; acceptance impact: unresolved checker failure blocks completion | Additional checker/document wording inconsistency remains a governance follow-up, not this task's implementation |

## Approval Record

- approver: user
- approval_date: 2026-08-26
- user_statement: "确认"
