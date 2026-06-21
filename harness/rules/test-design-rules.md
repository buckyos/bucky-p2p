# Test Design Rules

## Goal
- Define how test cases are designed, not only how they are recorded.
- Give each test level an explicit design contract: what it tests, how deep, and what "complete" means.
- Make case completeness mechanically derivable from approved design documents instead of relying on tester judgment.

## Scope
- test case design performed by the post-implementation testing stage
- generated test implementation at the `unit`, `dv`, and `integration` levels
- `testing.md` sections `## Unit Tests`, `## DV Tests`, `## Integration Tests`, `## Design Element Coverage`, and the `level` column of `## Case-Type Coverage`
- acceptance test design adequacy audits that cite this rule

## Level Contracts

### Unit
- Object under test: functions, methods, and branches of the changed production code; external dependencies are stubbed or mocked.
- MUST cover: every public function touched by the implemented `change_id` values has at least one test, and every conditional branch in changed code — if/else arms, match/switch arms, early returns, error returns, loop zero/one/many iterations, and boundary comparisons — is executed by at least one case.
- Uncovered branches MUST be recorded per branch with a concrete reason in `## Unit Tests`, not summarized per function or per file.
- Not responsible for: cross-module behavior, real external I/O, or full workflows.
- Done when: every changed-code branch is executed by a unit case or has a recorded per-branch gap reason.

### DV (single-module runnable verification)
- Object under test: the module running as a whole inside its own boundary, with real internal wiring between its submodules.
- MUST cover: module lifecycle (start, shutdown, and restart where applicable); each main workflow the module exposes; at least one failure workflow (invalid input, dependency failure, or interrupted operation); configuration variants that change behavior; and persisted-state recovery after restart when the module persists state.
- Not responsible for: line- or branch-level coverage of internal logic, or neighbor-module contracts.
- Done when: each main workflow and at least one failure workflow have runnable DV steps, or recorded gaps.

### Integration
- Object under test: contracts between this module and its neighbor modules.
- MUST cover: every exported interface in design `## Interfaces and Dependencies` that another module consumes has at least one success case and at least one failure-semantics case (error propagation, timeout, retry, or partial completion as recorded in the design failure handling); plus cross-module data flow and side-effect ordering for the design `## Key Call Flows` that cross the module boundary.
- Not responsible for: re-proving logic already proven at the unit or dv level.
- Done when: every consumed exported interface has success and failure coverage, or recorded gaps.

## Lowest-Level Placement Rule
- Verify each behavior at the lowest level that can expose its failure: pure logic and branching at `unit`, single-module runtime behavior at `dv`, cross-module contracts at `integration`.
- Higher levels MUST NOT compensate for missing lower-level coverage: a workflow test does not replace branch tests for changed logic, and a mocked unit test does not replace contract verification for an exported interface.
- Duplicate verification of the same behavior across levels MUST record a reason (for example, a regression that only reproduces in integration).

## Case Derivation Rules
Test cases MUST be derived mechanically from design artifacts. For each derivation source below, the testing stage MUST produce concrete cases or record an explicit row in `## Design Element Coverage`:

| element_type | Derivation source | Required cases |
|--------------|-------------------|----------------|
| parameter-domain | parameter and input domains of changed interfaces in design `## Interfaces and Dependencies` | equivalence classes plus boundary values: min, max, empty, just-outside-range, malformed input |
| state-transition | design `## Data and State` state transitions | one case per legal transition plus rejection cases for illegal transitions |
| failure-path | design `## Key Call Flows` failure handling | one fault-injection case per recorded failure path (error, timeout, retry, partial completion) |
| error-handling | error categories returned or raised by changed code | at least one case triggering each error category |
| invariant | design `## Invariants to Preserve` | one case verifying each listed invariant still holds |
| concurrency | concurrency, reentrancy, or ordering declarations in design | race, reentry, or ordering cases, or a recorded manual reason |

- Every `element_type` MUST appear in `## Design Element Coverage` at least once; an element type with no matching design content uses `status: not-applicable` with a concrete reason naming the design evidence.
- Each derived case MUST name its design source (section or doc path) so acceptance can audit authenticity; template-like, copy-pasted, or unverifiable derivation evidence is a finding against the testing stage.
- New failure paths, error categories, or state transitions discovered in the delivered code but missing from design MUST be returned to the design stage, not silently tested without a design source.

## Required Recording
- `## Unit Tests` records rows at function/branch granularity: tested function, branch or condition, covered behavior, test file, status, and per-branch gap reasons.
- `## DV Tests` records one row per workflow or runtime behavior with its kind (`lifecycle` / `main` / `failure` / `config` / `persistence`), entry, expected result, and status.
- `## Integration Tests` records one row per cross-module contract or flow with explicit success and failure cases.
- `## Design Element Coverage` records one row per derivation source with the derived cases and their level.
- `## Case-Type Coverage` records in its `level` column which test level implements each covered case.
- Status vocabulary for these tables is `covered` / `gap` / `manual` / `disabled` / `not-applicable`; any non-covered status carries a concrete reason.

## Guardrails
- `harness/scripts/doc-structure-check.py --docs testing` validates the level tables, the `## Design Element Coverage` element types, and the `level` column; placeholder-only rows fail.
- Acceptance MUST audit per-level depth against these contracts, not only the presence of case-type rows; violations return work to the testing stage.
- These contracts define design completeness. Execution evidence and unified-entrypoint registration remain governed by `harness/rules/testing-doc-rules.md` and `harness/rules/unified-test-entry-rules.md`.
