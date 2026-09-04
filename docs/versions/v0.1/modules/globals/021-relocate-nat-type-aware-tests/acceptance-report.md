# Globals 021 Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Problem | Fail Condition Hit |
|----|----------|-------|----------|---------|--------------------|
| F-021-001 | none | design | `.gitignore:18`; `pipeline/plan.md:83-100,144-152`; fresh artifact static step | Closed: the unanchored `sn/` rule hid the initial `tests/unit/sn/**` design; every SN-owned target now uses visible `tests/unit/sn_tests/**`, with common wire under `protocol/common` | none |
| F-021-002 | none | design | `pipeline/plan.md:100,152`; `p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs:112-515`; fresh artifact TunnelManager step | Closed: the validation-only TunnelManager inventory was corrected from seven to eight and the whole eight-test module executes | none |
| F-021-003 | none | acceptance | current p2p-frame/sn-miner stamps created at `2026-08-29T11:49:09Z`; fresh artifact started after them | Closed process disclosure: an earlier reviewer-side stamp timestamp refresh was superseded by the parent's formal post-return admission for the corrected plan | none |

## Result Summary
- Overall result: accepted
- Plain-language outcome: All 020-owned test implementations and fixtures in scope now live under their owning crate's `tests/**` tree, while source files retain only `cfg(test)` loaders and production behavior remains unchanged.
- What was verified: 11 exact visible destinations, eight removed standalone source paths, three extracted inline tests/fixtures, unchanged common-wire bytes, 18 moved p2p-frame names, one moved sn-miner name, one public API test, two SN profile-flow tests, eight TunnelManager tests, private access, release compilation, and task evidence.
- Evidence used: current proposal/plan/state/testplan, testing baseline, source loaders and moved files, two target admission stamps, stage manifests, independent source review, and `test-results/test-runs/20260829T115417Z-globals+021-relocate-nat-type-aware-tests-all.json`.
- Blocking issues: none; both design returns are closed.
- Next action: record accepted completion and remove 021 from unfinished-task bookkeeping.

## Object and Scope
- Module: globals
- Target modules: p2p-frame, sn-miner
- Version: v0.1
- Task name: 021-relocate-nat-type-aware-tests
- change_id values reviewed: nat_type_aware_test_file_layout, nat_type_aware_test_registration_parity
- Review date: 2026-08-29
- In scope: only 020-owned test files, inline test bodies, test-only fixtures, existing `cfg(test)` loaders, exact registration, Git visibility, release isolation, and 021 evidence.
- Out of scope: production NAT/SN/tunnel behavior, immutable 020 artifacts, historical 017–019 QUIC tests, unrelated source tests, Harness changes, `.gitignore` changes, public-network NAT validation, quality gates, and root-wide suites.
- Task-relevant acceptance scope: the exact relocation map, p2p-frame and sn-miner private test assembly, current task packet, two target admissions, testing baseline/manifests, and the fresh task-scoped artifact.
- Out-of-scope checks not run: quality gates, root `all all`, broad module suites, unrelated dirty-worktree checks, and public-network deployment validation.

## Optional Diff / Status Evidence
- `git status --short` summary: the worktree contains pre-existing 020 and unrelated task changes; explicit 021 stage manifests define the reviewed boundary.
- `git diff --stat` summary: not used as the completion standard because several moved source files began untracked under 020.
- `git diff --name-status` summary: targeted source/baseline comparison shows only test loaders and extraction within 021 source paths; all new implementations are under crate `tests/unit/**`.
- `git diff --check` result: scoped source and relocated files passed; package-wide formatting remains affected by pre-existing out-of-scope drift.
- Note: diff/status output is a discovery aid only, not the acceptance standard.

## Evidence Coverage
| Documented Item | Source Document | Implementation Evidence | Test / Result Evidence | Status |
|-----------------|-----------------|-------------------------|------------------------|--------|
| nat_type_aware_test_file_layout / P-NTATL-1 | proposal layout and visibility requirement; plan exact relocation map | ten p2p-frame and one sn-miner destinations under `tests/unit/**`; source files retain only `cfg(test)` loaders; old standalone paths and inline bodies are absent | fresh artifact static step proves exact paths, Git visibility, no accidental integration target and common-wire SHA; release checks pass | implemented |
| nat_type_aware_test_registration_parity / P-NTATL-2 | proposal parity requirement; plan 18+1 moved and 1/2/8 validation-only inventory | loaders preserve original Rust module identities; no public seam or test assertion relaxation was added | fresh artifact mechanically compares 18+1 moved names, executes every relocated filter, the eight-test TunnelManager module, public API, two SN flows and distributed query | implemented |

## Test Design Adequacy
| Behavior / Risk / change_id | Required Case Types | Test Design Evidence | Runnable Test Evidence | Status |
|-----------------------------|---------------------|----------------------|------------------------|--------|
| physical layout and Git visibility / nat_type_aware_test_file_layout | normal / boundary / negative / compatibility / cross-module | exact new/old allowlists, ignore checks, inline-body absence, common-wire hash, no `tests/unit.rs` or `main.rs`, and two release checks | artifact steps 1 and 15–16 pass for both crates; every exact target is visible and every old path is absent | adequate |
| registration parity / nat_type_aware_test_registration_parity | normal / boundary / negative / lifecycle / compatibility / cross-module | exact pre/post 18+1 name-set checks plus original unit, DV and integration filters | artifact 19/19 steps pass, including three real UDP probe tests, eight TunnelManager cases, public API, two SN flows and distributed query | adequate |

## Defect-Discovery Categories
| Category | Status | Concrete Evidence and Falsification Result |
|----------|--------|--------------------------------------------|
| requirement-and-behavior | pass | Both change_ids are satisfied: all scoped test bodies/fixtures reside under owning crate tests trees, registration remains equivalent, and unrelated historical tests and production behavior remain outside the change. |
| logic-and-control-flow | pass | Each loader stays under its original `cfg(test)` module and retains the old module identity; exact name-set and executable filters reject path or nesting mistakes. |
| boundary-and-input | pass | The static step checks 11 exact destinations, eight old paths, nested ignore behavior, three extracted inline bodies and both accidental Cargo integration-target names. |
| state-and-data-integrity | pass | Common-wire bytes retain SHA-256 `5110c42e07e74c3ef47ac64ff3199d6ed6b99f7c2e8698d838d1e0af00f18cf2`; moved name sets contain neither missing nor extra matching tests. |
| error-handling-and-recovery | not-applicable | The relocation adds no production error or recovery path; missing includes, ignored files and registration mismatches fail compilation or explicit static/name assertions. |
| resource-lifetime-and-cleanup | not-applicable | File placement owns no production resource lifetime; existing probe and tunnel owner-lifecycle tests remain registered and pass. |
| concurrency-and-ordering | not-applicable | No production concurrency or ordering code changed; existing async SN and TunnelManager tests execute unchanged. |
| interface-and-compatibility | pass | p2p-frame and sn-miner release checks pass with test loaders disabled; public API, SN flow and distributed-query tests pass without API/build/wire expansion. |
| security-and-capacity | not-applicable | No external input, authentication, permission, allocation or runtime-capacity path changed; all destinations are fixed repository-local paths. |
| test-adequacy | pass | The fresh task artifact has 19 successful steps, no non-executed level, exact 18+1 parity, eight TunnelManager cases, unit/DV/integration execution and release checks. |

## Implementation Correctness Audit
| Category | Applicable Scope | Evidence Reviewed | Finding / Reason Not Applicable | Owning Stage | Status |
|----------|------------------|-------------------|---------------------------------|--------------|--------|
| logic and control flow | Rust module assembly and filter identity | source/baseline loader diffs, exact Cargo lists and focused executions | No extra nesting, zero-test filter or private-access workaround remains. | none | pass |
| termination and progress | test runner execution | fresh artifact 19-step completion | All task steps terminate successfully; no new runtime loop or wait was introduced. | none | pass |
| concurrency and synchronization | existing async tests only | probe, TunnelManager and SN-flow steps | Not applicable to relocation code; the existing async coverage remains runnable. | none | not applicable |
| resource lifetime and cleanup | existing owned probe/tunnel cases | real UDP and TunnelManager module results | No resource-owning production code changed; preserved lifecycle cases pass. | none | not applicable |
| state and data integrity | file bytes, names and ownership | common hash, path allowlists, inline extraction and exact name sets | No duplicate source copy, ignored target, missing fixture or name drift remains. | none | pass |
| error handling and recovery | include/registration failure surfaces | static assertions, Rust compilation and Cargo execution | All layout failures are fail-fast; no production recovery contract changed. | none | pass |
| interface boundary and compatibility | private visibility and release build | loader placement, no `pub` seam, two release checks and integration steps | Crate-private tests compile from `tests/unit/**` and production surfaces are unchanged. | none | pass |
| security and capacity safety | repository path resolution only | fixed `CARGO_MANIFEST_DIR` includes and ignore checks | Includes remain inside owning crates with no dynamic or unbounded input. | none | pass |

## Generated Acceptance Rules
| Rule ID | Source | Expected Result | Evidence Required | Status |
|---------|--------|-----------------|-------------------|--------|
| AR-021-1 | P-NTATL-1 and exact relocation map | every scoped test body/fixture is under owning crate `tests/**`, source retains only test-only assembly, and every target is Git-visible | exact path scan, old-path absence, source/baseline comparison, common SHA and release checks | pass |
| AR-021-2 | P-NTATL-2 and corrected inventory | 18 moved p2p-frame and one moved sn-miner name are identical before/after, while 1 public, 2 SN-flow and 8 TunnelManager validation-only tests execute | Cargo list comparison, nonzero unit filters, DV and integration steps in fresh artifact | pass |

## Inputs
- `docs/versions/v0.1/modules/globals/021-relocate-nat-type-aware-tests/proposal.md`
- `docs/versions/v0.1/modules/globals/021-relocate-nat-type-aware-tests/pipeline/plan.md`
- `docs/versions/v0.1/modules/globals/021-relocate-nat-type-aware-tests/pipeline/state.json`
- `docs/versions/v0.1/modules/globals/021-relocate-nat-type-aware-tests/testplan.yaml`
- p2p-frame and sn-miner source loaders plus all 11 relocated test destinations
- `.harness/baselines/021-relocate-nat-type-aware-tests-testing/manifest.json`
- current p2p-frame and sn-miner admission stamps and implementation/testing scope manifests
- `test-results/test-runs/20260829T115417Z-globals+021-relocate-nat-type-aware-tests-all.json`
- `harness/rules/acceptance-review-rules.md`

## Review Order
1. Re-read the proposal and current plan without adopting prior completion claims.
2. Inspect the source/baseline differences, moved test files, exact mapping, Git ignore behavior and private-access assembly.
3. Falsify missing, ignored, duplicated, renamed, zero-test, accidental integration-target, public-seam and release-build failure hypotheses.
4. Audit both change_ids, every test-design case type and all ten defect-discovery categories.
5. Accept only after both design returns, fresh target admissions, current evidence hash and 19 task steps were independently verified.

## Consistency Summary
- Proposal authority check: the auto-pipeline plan records the user's explicit launch and preserves the proposal's test-only, two-crate and historical-exclusion boundaries.
- Proposal vs design: the exact visible destination map, private loader strategy, 18+1 moved inventory and corrected 1/2/8 validation inventory implement both proposal items.
- Design vs testing implementation: testplan static and name-set steps directly encode the map/count invariants; focused unit/DV/integration commands execute the unchanged behaviors.
- Design vs long-lived boundary doc: no long-lived module boundary or production crate ownership changes; tests remain owned by p2p-frame and sn-miner.
- Design vs implementation: implementation is intentionally no-op; all delivery changes are test files or existing `cfg(test)` assembly.
- Test implementation vs test code vs results: current code and testplan hash to the fresh 28-input artifact; all 19 steps exit zero.
- Test design adequacy: adequate for repository layout, private assembly, registration, compatibility and preserved runtime-test execution.
- change_id traceability: both change_ids map from proposal through plan, two-target admission, testplan/state rows, fresh artifact and accepted review evidence.
- Acceptance criteria traceability: exact locations, Git visibility, no production seam, common hash, 18+1 names, eight TunnelManager cases and all enabled levels have direct evidence.
- Cross-module admission: current p2p-frame and sn-miner stamps bind plan hash `1504319147dbf9703ac8c774d561c79c19d326e7e54b6c2857281156fdd42387`, both change_ids and target-specific scopes.
- Public API / codec / runtime semantics review: no public API, codec or runtime source behavior changed; release checks and integration consumers pass.
- Document logic review: the ignore-path and seven-to-eight inventory defects are explicitly recorded and closed; current proposal, plan, testplan and evidence agree.
- Implementation logic review: baseline comparison finds only test-only loaders/extraction within 021 source paths and no release-visible change.
- Implementation correctness audit completeness and routing: all applicable categories pass; no requirement, design, implementation or testing return remains.
- Document approval timing: auto-pipeline launch evidence binds the draft proposal without manual approval metadata as allowed by repository rules.
- Implementation task paths bound to design Scope Paths: both target modules and both change_ids passed current implementation scope checks.
- Bugfix red-green regression evidence, when the reviewed work contains a bugfix: the layout defect is reproduced by the old ignored path and closed by the visible-path static step; this task changes no production behavior.

## Validation Evidence
- Existing schema result: current globals/021 schema passed after the corrected plan binding and unchanged testplan were present.
- Existing admission stamp: both current target stamps were formally regenerated after the final plan correction and bind the proposal hash, plan hash, two change_ids and exact target scopes.
- Existing stage-scope result: proposal and design passed; implementation passed independently for both targets/change_ids; testing passed for the current 33-path manifest with the captured baseline.
- Existing pipeline-plan result, when applicable: current plan/state passed with D/I/T complete and A running immediately before final acceptance.
- Task-relevant test run artifact(s): `test-results/test-runs/20260829T115417Z-globals+021-relocate-nat-type-aware-tests-all.json` records 19 successful steps and both change_ids.
- Commands rerun because checker-owned inputs changed after their previous pass: target admission and the task-scoped `all` run were regenerated after the 7-to-8 design correction; the independent reviewer recomputed the fresh artifact hash.
- Direct package/module runtime suites, whole-project suites, and root shortcuts: not run; only the task-scoped unit/DV/integration and release checks were executed.
- Risk-triggered task-local contract kinds and assertions, when applicable: not applicable because public API, crate-root export, build surface and documentation examples are unchanged.
- Scoped evidence input hash current, when risk-triggered: the fresh artifact's 28-input hash is `bc04b9e4a47a64b7c3024d319c711e6006fc5196e353a4a7d6e242ae9a1bf398`, independently recomputed against current inputs.
- Quality gates: not applicable; no quality run was requested for this task.
- Explicitly requested quality run artifact, if any: none.
- Architecture doc check, only when `docs/architecture/` evidence is relevant: not run because 021 changes no architecture document or production boundary.
- Acceptance report check after this report was created or modified: run during final closeout; any failure blocks completion.
- Targeted migration search, only when applicable: all 11 destinations are visible, all eight standalone old paths are absent, and all three inline test bodies/fixtures are absent from source.

## Automated Test Exception
- Applies: no
- Reason: a fresh machine-written task artifact covers every enabled level and both change_ids.
- Owner: acceptance
- Risk: no automation waiver is used.
- Acceptance impact: acceptance relies on the task artifact plus independent source and evidence review.
- Alternative evidence: not needed.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: every in-scope 020 test implementation is owned by the correct crate tests tree, exact registration and compatibility are preserved, both design returns are closed, and no blocking defect remains across the ten audit categories.
- Supporting task-relevant test evidence: `test-results/test-runs/20260829T115417Z-globals+021-relocate-nat-type-aware-tests-all.json`, 19/19 successful steps, both change_ids, and independently matched evidence hash `bc04b9e4a47a64b7c3024d319c711e6006fc5196e353a4a7d6e242ae9a1bf398`.
- Residual risk: none specific to the relocation; package-wide formatting and public-network NAT behavior remain outside this task and were not claimed.

## Follow-Up Tasks
- Requirement task: none.
- User decision required for proposal issue: none.
- Design task: none.
- Implementation task: none.
- Testing task: none.
- Testing return reason if coverage is incomplete: not applicable; current coverage is complete.
- Iteration count: 2
- Stop reason if more than 5 unsuccessful iterations: not applicable.
