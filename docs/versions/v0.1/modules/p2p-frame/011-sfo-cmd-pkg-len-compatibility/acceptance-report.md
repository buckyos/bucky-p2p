# sfo-cmd-server CmdPkgLen Compatibility Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-000 | none | acceptance | complete proposal-plan-admission-code-test evidence chain | no blocking or non-blocking correctness finding | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: p2p-frame now compiles against sfo-cmd-server 0.4 with one shared 10 MiB owner command length alias and a separate two-byte ordinary SN alias.
- What was verified: exact type aliases, all six admitted consumers, header generic agreement, fixed widths and limits, over-limit rejection, migrated consumer closure, all-target compile closure, scope evidence, and proposal-plan-code-test consistency.
- Evidence used: launch-confirmed proposal, validated pipeline plan/state, admission evidence and stamp, implementation and testing scope manifests, production diff, dedicated external test, testplan, and the passing task-run artifact.
- Blocking issues: no blocking issue remains.
- Next action: close task 011; preserve the documented mixed-version rollout and 10 MiB capacity risks during deployment.

## Object and Scope
- Module: p2p-frame
- Version: v0.1
- Task name: 011-sfo-cmd-pkg-len-compatibility
- change_id values reviewed: sfo_cmd_pkg_len_v04_compatibility
- Review date: 2026-07-21
- In scope: `p2p-frame/src/sn/types.rs`, SN client/service consumers, owner directory client/server consumers, inter-SN node consumers, the dedicated compatibility test, and task-local evidence.
- Out of scope: command bodies/codes, QA correlation, tunnels, timeout/dispatch behavior, PN behavior, dependency implementation, runtime version negotiation, and neighboring production crates.
- Task-relevant acceptance scope: proposal item P-CMD-PKG-LEN-1 and its single p2p-frame implementation binding, including the migration-required public/build surface and scoped consumer closure.
- Out-of-scope checks not run: direct module runtime suites, whole-workspace runtime suites, root shortcuts, documentation tests, and quality gates.

## Optional Diff / Status Evidence
- `git status --short` summary: used only to distinguish task paths from substantial unrelated worktree changes.
- `git diff --stat` summary: six production files contain 20 insertions and 15 deletions relative to HEAD; the dedicated new test is untracked discovery evidence until committed.
- `git diff --name-status` summary: task review was restricted to the six admitted production files, the dedicated test, and task evidence paths.
- `git diff --check` result: passed for all six production files and the dedicated test.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| Shared ordinary SN length type | proposal P-CMD-PKG-LEN-1; plan `SnCmdPkgLen` interface | `SnCmdPkgLen = sfo_cmd_server::U16`; client, service, and `SnCmdHeader` consume it | dedicated alias/header test plus all-target compile closure | implemented |
| Shared owner/inter-SN length type | proposal P-CMD-PKG-LEN-1; plan `OwnerCmdPkgLen` interface | one `OwnerCmdPkgLen = U24` 10 MiB instantiation; directory client/server and inter-SN node/handlers consume it | three-byte/exact-limit/next-byte rejection tests plus migrated consumer scan | implemented |
| Build restoration on sfo-cmd-server 0.4 | proposal success criteria; plan build impact | active primitive `CmdPkgLen` generics removed from all admitted consumers | focused `cargo check` passed during implementation and task artifact all-target compile step passed | implemented |
| Preserved non-goals | proposal Scope and Risks; plan failure flows | no body, command, tunnel, timeout, dispatch, lifecycle, lock, or neighboring-crate production behavior changed | scoped source review and task-local compile/test closure | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| Alias contract and exact capacity / `sfo_cmd_pkg_len_v04_compatibility` | normal, boundary, negative, error | testplan integration step covers trait implementation, 2/3-byte widths, u16 maximum, exact 10 MiB, and 10 MiB plus one rejection | `cmd_pkg_len_compatibility` runs three passing tests in the task artifact | adequate |
| Public/build migration closure / `sfo_cmd_pkg_len_v04_compatibility` | compatibility, cross-module | required external-positive, canonical removed-symbol-scan, and repository-compile-closure contract steps bind every production scope and migrated consumer | all three contract kinds pass with current scoped evidence hash `379e30d5edd8b6a300fb82945ca4cd3807e5e6da62491decc3cbbdc676d5ae2b` | adequate |
| Runtime lifecycle and red-green boundary | lifecycle, regression | lifecycle is concretely not applicable to compile-time aliases; pre-fix code cannot form a runnable test target because primitive types fail trait bounds at crate compilation | pre-implementation `cargo check` reproduced 15 E0277/E0599/E0631 errors; the post-fix task artifact supplies the green compile and tests | adequate |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | const-generic type selection and header construction | both alias declarations, six generic/header consumer migrations, boundary tests | exact aliases are selected consistently; no changed branch, loop, arithmetic control flow, or incorrect generic remains | none | pass |
| termination and progress | compile-time aliases and unchanged command runtime | production diff and plan non-goals | no loop, wait, retry, task, blocking operation, or progress mechanism changed | none | pass |
| concurrency and synchronization | unchanged SN client/server/node runtime | source diff, plan state ownership, task tests | no lock, shared runtime state, ordering, callback, or synchronization behavior changed | none | pass |
| resource lifetime and cleanup | fixed-width header types and command body boundary | aliases, dependency CmdPkgLen limit contract, unchanged body/tunnel code | no resource owner or cleanup path changed; fixed 10 MiB bound remains explicit rather than unbounded | none | pass |
| state and data integrity | wire length field type identity and limits | producer/consumer/handler closure and fixed-width tests | ordinary paths remain two bytes; owner paths are consistently three bytes with one const identity; no partial endpoint migration remains | none | pass |
| error handling and recovery | over-limit and malformed command framing boundary | plan failure flows, exact-limit conversion test, unchanged dependency error propagation | next-byte overflow is rejected; no swallowed error, fallback, retry, or new error category was introduced | none | pass |
| interface boundary and compatibility | public aliases, sfo-cmd-server 0.4 API, former-u32 wire width | consumer migration table, canonical scan, external-positive compile, all-target compile, header tests | repository consumers are migrated and compile; mixed 0.3/0.4 former-u32 wire compatibility is explicitly unsupported rather than silently claimed | none | pass |
| security and capacity safety | remotely supplied command body length | 10 MiB const bound, 3-byte width, over-limit test, proposal risk | size is bounded and 10 MiB plus one is rejected; larger allowed bodies increase bounded memory pressure but do not create an unbounded allocation contract | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-CMD-1 | proposal P-CMD-PKG-LEN-1 | one shared 10 MiB U24 alias is reused by every former-u32 owner/inter-SN endpoint and handler | source closure plus exact-limit/width test | pass |
| AR-CMD-2 | proposal non-goal and plan interface mapping | former-u16 SN paths use U16 and retain two-byte framing | shared alias/header source plus fixed-width test | pass |
| AR-CMD-3 | plan producer/consumer closure | every client/server/node generic agrees with its registered handler header concrete type | source inspection, external-positive compile, all-target compile | pass |
| AR-CMD-4 | proposal build success criterion | p2p-frame compiles against the locked 0.4 dependency without primitive trait-bound errors | repository-compile-closure artifact step | pass |
| AR-CMD-5 | migration-required build/API trigger | required contract checks and current scoped evidence binding exist | external-positive, canonical removed-symbol-scan, compile closure, evidence hash | pass |
| AR-CMD-6 | capacity/security trigger | exact 10 MiB succeeds and the next byte fails | dedicated boundary test | pass |

## Inputs
- launch-confirmed `proposal.md`
- task-local `pipeline/plan.md` and pre-acceptance `pipeline/state.json`
- `testplan.yaml` and `p2p-frame/tests/cmd_pkg_len_compatibility.rs`
- six admitted production source files
- admission evidence and generated stamp
- proposal, design, implementation, and testing stage-scope manifests/results
- machine-written task-run artifact
- `docs/modules/p2p-frame.md` and required workspace architecture documents
- `harness/rules/acceptance-review-rules.md` and `harness/rules/acceptance-task-rules.md`

## Review Order
1. Bound proposal and `sfo_cmd_pkg_len_v04_compatibility` acceptance boundary.
2. Validated pipeline interface, compatibility, failure, scope, and file-order mappings.
3. Admission and stage-scope evidence.
4. Six production files and their complete producer/consumer/header closure.
5. Post-implementation testplan, dedicated test, contract kinds, and task artifact.
6. Long-lived p2p-frame and architecture boundaries.
7. Eight-category implementation correctness audit and conclusion.

## Consistency Summary
- Proposal authority check: the explicit user launch binds the current proposal, and the admission stamp binds its hash without requiring manual approval metadata.
- Proposal vs design: the plan maps P-CMD-PKG-LEN-1 to exactly six production paths, retains the ordinary U16 boundary, and records the requested owner U24 limit and mixed-version risk without scope expansion.
- Design vs testing implementation: the testplan and dedicated external test cover the plan's widths, limits, compile-time identity, migration closure, and over-limit failure; runtime lifecycle is correctly marked not applicable.
- Design vs long-lived boundary doc: all production changes remain within `p2p-frame/src/sn/**`, matching the Tier-0 p2p-frame responsibility; neighboring crate production paths are unchanged.
- Design vs implementation: both shared aliases and all five consumer families match the file-level sequence and admitted scope; no direct primitive or bare/repeated U24 consumer remains.
- Test implementation vs test code vs results: testplan registration names the dedicated file and its exact command; all four artifact steps have non-empty sources and exit code zero.
- Test design adequacy: normal, boundary, negative, error, compatibility, and cross-module cases are covered at the real dependency boundary; disabled unit/DV and lifecycle rows have concrete lowest-level reasons.
- change_id traceability: `sfo_cmd_pkg_len_v04_compatibility` appears consistently in proposal, plan, admission stamp, testplan, pipeline state, stage evidence, and task artifact.
- Acceptance criteria traceability: proposal success criteria map to AR-CMD-1 through AR-CMD-6 with executable evidence.
- Cross-module admission: only p2p-frame contains production implementation evidence; the external dependency and workspace consumers are validation boundaries, not additional production target modules.
- Public API / codec / runtime semantics review: public concrete types are migration-required; former-u16 encoding is preserved, former-u32 encoding deliberately becomes bounded three-byte U24, and command runtime semantics remain unchanged.
- Document logic review: no ambiguity, contradiction, impossible state, silent narrowing, or unsupported success claim was found.
- Implementation logic review: no type drift, wrong const instantiation, primitive trait consumer, unbounded limit, or behavior change was found in the admitted paths.
- Implementation correctness audit completeness and routing: all eight required categories were reviewed and passed; no upstream return route is required.
- Document approval timing (approved_content_sha256 verified by schema-check): auto-pipeline launch confirms the draft proposal; schema and admission passed against current proposal and plan hashes before implementation.
- Implementation task paths bound to design Scope Paths (`stage-scope-check.py --stage implementation --change-id ... --changed-paths-file ...`): passed for all six admitted production files and current implementation evidence.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: exact red replay is infeasible because the old primitive generics prevent the crate/test target from compiling; the pre-implementation 15-error reproduction and current green compile/test artifact provide concrete compile-regression evidence.

## Validation Evidence
- Existing schema result (cite the owning-stage result; do not rerun unchanged input): passed for v0.1/p2p-frame/011 with launch-confirmed proposal and current plan before implementation.
- Existing admission stamp (cite the stamp; do not run `--verify-only` during acceptance unless an admission-owned input changed): `docs/versions/v0.1/evidence/admission/20260721-sfo-cmd-pkg-len-compatibility.p2p-frame.011-sfo-cmd-pkg-len-compatibility.stamp.json` binds the proposal, plan, target module, change ID, and six Scope Paths.
- Existing stage-scope result (cite the owning-stage result; do not rerun an unchanged manifest/scope): proposal, design, implementation, and testing checks passed using the four task manifests and sidecars; implementation covered 9 task paths and testing covered 4.
- Existing pipeline-plan result, when applicable (cite the latest result for current plan/state inputs): normal validation passed after the current pre-acceptance state update; final complete-mode validation is owned by pipeline finalization after this report.
- Existing testing-coverage result: passed for the current testplan, state coverage rows, migration contracts, and `sfo_cmd_pkg_len_v04_compatibility`.
- Task-relevant test run artifact: `test-results/test-runs/20260721T065710Z-p2p-frame+011-sfo-cmd-pkg-len-compatibility-all.json`; exact task scope/all, one change ID, four non-empty successful steps, current evidence hash `379e30d5edd8b6a300fb82945ca4cd3807e5e6da62491decc3cbbdc676d5ae2b`.
- Commands rerun because checker-owned inputs changed: none during acceptance; all admission, scope, coverage, plan, and test evidence was reused from its owning stage.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; the permitted p2p-frame compile-only consumer closure is contained inside the task artifact.
- Risk-triggered task-local contract kinds and assertions, when applicable: external-positive/new-path-compiles, removed-symbol-scan/no-unallowlisted-old-symbol-references, and repository-compile-closure/repository-consumers-compile all passed.
- Scoped evidence input hash current, when risk-triggered: yes, `379e30d5edd8b6a300fb82945ca4cd3807e5e6da62491decc3cbbdc676d5ae2b`, covering Cargo inputs, all six production files, the dedicated test, and testplan.
- Quality gates: not applicable; no explicit user request authorized the separate broad quality gate.
- Explicitly requested quality run artifact, if any: not applicable; no quality run was requested.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: required principles, workspace constraints, validation model, and p2p-frame long-lived boundaries were inspected; no global architecture change or mismatch exists.
- Acceptance report check after this report was created or modified: passed; the report was then finalized with the same evidence and conclusion.
- Targeted migration search, only when applicable to the reviewed task: canonical removed-symbol-scan in the task artifact passed; ad hoc discovery search is not claimed as acceptance evidence.

## Automated Test Exception
- Applies: no
- Reason: automated task-scoped evidence exists and passed.
- Owner: testing stage
- Risk: no automation gap remains.
- Acceptance impact: none.
- Alternative evidence: not needed because the task artifact contains four executed successful steps.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: the launch-confirmed requirement is implemented through one shared 10 MiB owner alias and one ordinary U16 alias, every admitted consumer and handler agrees on its concrete type, p2p-frame compiles against sfo-cmd-server 0.4, and current task-scoped boundary plus migration evidence passes with no correctness finding.
- Supporting task-relevant test evidence: `test-results/test-runs/20260721T065710Z-p2p-frame+011-sfo-cmd-pkg-len-compatibility-all.json` (four successful steps; evidence hash `379e30d5edd8b6a300fb82945ca4cd3807e5e6da62491decc3cbbdc676d5ae2b`).
- Residual risk: former-u32 0.3.x peers are not wire-compatible with the 0.4 U24 framing, and the requested 10 MiB cap allows higher bounded per-message memory pressure than the dependency default; coordinated rollout and operational limits remain necessary.

## Follow-Up Tasks
- Requirement task: no requirement correction is needed.
- User decision required for proposal issue: no proposal ambiguity or contradiction was found.
- Design task: no design return is needed.
- Implementation task: no implementation return is needed.
- Testing task: no testing return is needed.
- Testing return reason if coverage is incomplete: coverage is complete; unit/DV/lifecycle exclusions are concrete and nonblocking.
- Iteration count: 1
- Stop reason if more than 5 unsuccessful iterations: not applicable.
