---
module: p2p-frame
task_name: 005-move-net-manager-post-accept-tests
submodule: 005-move-net-manager-post-accept-tests
version: v0.1
status: approved
approved_by: user
approved_at: 2026-07-14T16:34:16+08:00
approved_content_sha256: 45f9604e52b6f5c634c14b58ead45d08052e0623bf371788ec6dd413fefd357f
---

# NetManager Post-Accept Test Location Amendment Proposal

## Background and Goal

The approved and completed `003-tcp-control-tunnel-register-if-absent` task added `p2p-frame/src/networks/net_manager_post_accept_tests.rs` and `p2p-frame/src/networks/net_manager_tcp_post_accept_tests.rs`. Although both files contain only test code and are included from the existing `#[cfg(test)] mod tests` in `net_manager.rs`, their physical location under `src/networks/` does not satisfy the requested repository layout for newly added test implementations.

This sibling amendment/fix packet corrects that layout without editing the frozen 003 packet: move both files under the crate's `p2p-frame/tests/` tree while preserving their unit-test compilation context, names, coverage, feature gates, and behavior.

## Scope

### In scope

- Relocate `net_manager_post_accept_tests.rs` and `net_manager_tcp_post_accept_tests.rs` from `p2p-frame/src/networks/` to a dedicated nested directory under `p2p-frame/tests/net_manager/`.
- Update only the existing test-only include wiring inside `p2p-frame/src/networks/net_manager.rs` so the files continue compiling inside `net_manager::tests`, where they can access the private test fixtures and crate-private implementation boundaries they already use.
- Keep the current `post_accept_tests` and feature-gated `tcp_post_accept_registry_tests` module names and focused test filters stable.
- Ensure the nested layout does not cause Cargo to treat either included file as an independent top-level integration-test crate.
- Remove the obsolete source-tree copies after the new include paths are active.
- Generate task-local testing metadata and runnable evidence for this amendment instead of modifying the approved 003 packet or its frozen evidence.

### Out of scope

- Moving the tests to top-level `p2p-frame/tests/*.rs` files that compile as independent integration crates.
- Making `NetManager`, its private fixtures, crate-private tunnel internals, or helper types public solely to support the relocation.
- Changing any production behavior, public API, TCP wire/TLS behavior, registry semantics, test assertions, timeouts, synchronization, or coverage intent from task 003.
- Refactoring, merging, splitting, renaming, or otherwise rewriting the test bodies beyond path/module adjustments required by the relocation.
- Editing the approved 003 proposal, pipeline plan, testplan, state, acceptance report, or historical run artifacts.
- Modifying production code or test files during this proposal stage.

### Boundary with neighboring modules

- `p2p-frame/src/networks/net_manager.rs` remains the owner of the existing private `#[cfg(test)]` module and changes only its test-only `include!` paths.
- `p2p-frame/tests/net_manager/` owns the relocated test source files, but they remain compiled as child modules of `net_manager::tests`; they are not new public integration consumers.
- The production modules exercised by the tests and downstream crates remain unchanged.
- The completed 003 packet remains immutable historical evidence; this new packet owns all relocation-specific scope and validation.

## Requirement Review

- The requested layout correction is reasonable: newly added test implementations should be visibly separated from production source files.
- A direct move to `p2p-frame/tests/net_manager_post_accept_tests.rs` would be incorrect because Cargo auto-discovers top-level files there as independent integration test crates. These files currently rely on `super::*`, private test fixtures, and crate-private network/tunnel APIs, so that shape would either fail compilation or pressure the production API to become broader.
- A nested `p2p-frame/tests/net_manager/` location combined with `include!` from the pre-existing `#[cfg(test)]` module satisfies the physical-layout request while preserving the intended unit/private-boundary semantics.
- The include-path changes are test-only changes inside an existing exact `#[cfg(test)]` item. Later stage-scope evidence must mechanically prove no production portion of `net_manager.rs` changed.
- Rewriting these cases as true external integration tests would be materially larger, would require public seams not justified by the requested move, and could change test behavior; it is rejected for this correction.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-MNMPAT-1 | relocate_net_manager_post_accept_tests | Move both task-003 NetManager post-accept test source files into `p2p-frame/tests/net_manager/`, update only existing `#[cfg(test)]` include wiring, remove the old source copies, and preserve module names, feature gates, test behavior, filters, and private compilation context | Test layout and test-only include paths only; the approved 003 packet and every production/public/runtime contract remain unchanged | The files live under the crate tests tree but remain included unit-test modules rather than independently compiled integration targets, preserving private access at the cost of retaining explicit include wiring | Task-scoped commands compile and run `post_accept_tests` plus x509-gated `tcp_post_accept_registry_tests` from the new paths; path review shows no old copies, no independent Cargo target, and no production diff outside the existing cfg(test) item | No API visibility expansion, test rewrite, behavior/timeout/assertion change, production fix, 003 packet edit, or broad test-suite execution |

## Success Criteria

- Concrete user-visible or system-visible result: neither `p2p-frame/src/networks/net_manager_post_accept_tests.rs` nor `p2p-frame/src/networks/net_manager_tcp_post_accept_tests.rs` exists; equivalent files exist under `p2p-frame/tests/net_manager/` and the existing focused tests still compile and pass under their original module/filter names.
- Required evidence: downstream design records the nested test location, include ownership, Cargo discovery boundary, exact move/delete paths, and the invariant that `net_manager.rs` changes are confined to its existing `#[cfg(test)]` item; task-scoped testing runs both the non-x509 and x509-gated focused modules from a new task testplan.
- Explicit non-goals: no production/API/runtime behavior change, no external integration-test conversion, no private symbol export, no test logic rewrite, no modification of task 003 artifacts, and no package/workspace-wide runtime suite.

## Risks

- Placing the files directly under `p2p-frame/tests/` would make Cargo compile them as standalone crates and break `super::*` plus private/crate-private access; the nested directory is required.
- An incorrect relative `include!` path may compile on one assumed working directory but fail because paths resolve relative to the including source file; design and testing must verify the actual compiler resolution.
- A move implemented as copy-only could leave duplicate or stale source-tree tests; success requires removal of both old files and a single active include for each module.
- Editing outside the existing `#[cfg(test)] mod tests` block in `net_manager.rs` would cross into production scope and must fail the testing-stage scope check.
- Test filters are evidence contracts from the completed 003 task; renaming modules or cases could silently make focused commands execute zero tests, so the machine run must contain non-empty successful steps and observed test counts.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | no | `p2p-frame/src/networks/net_manager.rs` includes both files only inside `#[cfg(test)] mod tests`; proposal excludes production/public/wire/runtime semantics | Source and scope review confirm no production or public contract change | Proposal inspection identified the exact test-only include boundary | owner: downstream design/testing; reason: relocation has not occurred; acceptance impact: any production/public diff blocks acceptance | none if the cfg(test) boundary is preserved |
| data/schema | no | The two files contain transient test fixtures and assertions; no persistence, serialization, schema, migration, or cache-key path is in scope | Confirm relocated bodies are unchanged | Proposal inspection found no durable-data surface | owner: none; reason: not applicable; acceptance impact: none | none |
| security/privacy/permission | no | The relocation neither changes validator behavior nor exposes private APIs; visibility expansion is explicitly forbidden | Source review confirms no new `pub` surface or trust-boundary behavior | Proposal records the private-access constraint | owner: downstream design/testing; reason: visibility diff is checked after move; acceptance impact: any API expansion blocks acceptance | none |
| runtime/integration | no | Existing tests exercise runtime behavior, but this task changes only their physical source path and test-only include wiring; production runtime code and test logic are frozen | Focused commands prove the same cases still execute without changing their behavior | Proposal inspection identified existing module names and x509 gate | owner: testing; reason: runnable evidence belongs after relocation; acceptance impact: zero-test or renamed-filter execution blocks acceptance | Incorrect include wiring could make tests disappear, addressed by non-empty focused runs |
| build/dependency/config/deployment | yes | Cargo auto-discovers top-level `p2p-frame/tests/*.rs`, while nested included files avoid unintended standalone integration targets; include paths affect test compilation | Design fixes nested paths; testing compiles both focused modules, verifies no unintended standalone target, and changes no Cargo/dependency/config files | Proposal selected the nested include shape after inspecting current Cargo/test layout | owner: design/testing; reason: compiler evidence is not yet available; acceptance impact: discovery or include-path failure blocks acceptance | Relative-path or Cargo discovery mistakes could prevent/duplicate compilation |
| ui/datamodel/workflow | no | p2p-frame NetManager tests have no UI, presentation, navigation, accessibility, or frontend data-model surface | Confirm no UI paths change | Proposal inspection found no UI surface | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | The task uses existing packets, stage-scope checks, testplan schema, and unified runner without changing `harness/**`, templates, CI, or checker behavior | Run existing stage-owned checks only when their inputs change | Proposal uses the existing sibling amendment mechanism | owner: downstream stages; reason: later checks belong to their owning stages; acceptance impact: missing existing evidence blocks acceptance | none |

## Approval Record

- approver: user
- approval_date: 2026-07-14
- user_statement: "批准该 proposal，并启动 auto-pipeline 自动完成后续步骤"
