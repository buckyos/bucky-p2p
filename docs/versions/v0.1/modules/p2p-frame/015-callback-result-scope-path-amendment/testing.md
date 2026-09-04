---
module: p2p-frame
task_name: 015-callback-result-scope-path-amendment
submodule: 015-callback-result-scope-path-amendment
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# Callback Result Scope Path Amendment Testing

## Test Document Index

| level | document | responsibility |
|-------|----------|----------------|
| root | `testing.md` | Maps the amendment and unchanged task 014 migration to task-local automated evidence; no child test document is needed |

## Unified Test Entry

- Canonical command: `python3 ./harness/scripts/test-run.py p2p-frame/015-callback-result-scope-path-amendment all`.
- Machine plan: `testplan.yaml` in this packet.
- Generated run evidence: `test-results/test-runs/*.json` for the exact task scope.

## Submodule Tests

The task-local Python verifier owns dependency-state assertions. Its self-test covers both the accepted registry shape and rejected stale configurations without modifying repository files.

## Module-Level Tests

The verifier reads the delivered root and p2p-frame manifests plus lockfile and checks that the local package directory is absent. This is the lowest runnable module workflow for the metadata-only migration.

## External Interface Tests

Cargo dependency-tree inspection proves the real p2p-frame and `sfo-cmd-server` closure resolves callback-result 0.2.5. A compile-only all-targets contract step checks repository consumer compatibility without running broad runtime suites.

## Direct Change Coverage

| change_id | design_source | validation_id | testplan_level | testplan_step_id | gap | gap_manual_reason |
|-----------|---------------|---------------|----------------|------------------|-----|-------------------|
| callback_result_scope_path_amendment | task 015 Overall Approach and Directly Mapped Change Items; task 014 migration success criteria | VAL-CRSPA-WORKSPACE | dv | callback-result-registry-workspace-dv | no | Automated workspace validation plus unit, integration, and compile-contract evidence covers the unchanged migration and corrected scope boundary |

## Case-Type Coverage

| change_id | case_type | required | validation_id | level | status | gap_manual_reason |
|-----------|-----------|----------|---------------|-------|--------|-------------------|
| callback_result_scope_path_amendment | normal | yes | VAL-CRSPA-CLEAN | dv | covered | clean manifests, registry lock entry, checksum, and absent vendor are validated |
| callback_result_scope_path_amendment | boundary | yes | VAL-CRSPA-PREFIX | unit | covered | exact callback-result directory presence is rejected without treating sibling third-party paths as input |
| callback_result_scope_path_amendment | negative | yes | VAL-CRSPA-STALE | unit | covered | stale root patch and old direct requirement fixtures are rejected |
| callback_result_scope_path_amendment | error | yes | VAL-CRSPA-LOCK | unit | covered | a local lock entry without registry source/checksum is rejected |
| callback_result_scope_path_amendment | compatibility | yes | VAL-CRSPA-COMPILE | integration | covered | all p2p-frame targets compile against the published dependency |
| callback_result_scope_path_amendment | lifecycle | yes | VAL-CRSPA-REMOVE | dv | covered | final migration state requires all local package files and directory to be absent |
| callback_result_scope_path_amendment | cross-module | yes | VAL-CRSPA-TREE | integration | covered | reverse dependency tree includes p2p-frame and sfo-cmd-server consumers |

## Design Element Coverage

| element_type | design_source | derived_cases | level | status | gap_manual_reason |
|--------------|---------------|---------------|-------|--------|-------------------|
| parameter-domain | task 015 concrete Scope Paths | clean prefix, stale path patch, old version, local lock, and remaining vendor inputs | unit | covered | Dependency-state representations form the relevant input domain |
| state-transition | task 014 atomic migration order | final registry state and forbidden partial local states | dv | covered | Workspace check observes the delivered terminal state |
| failure-path | task 014 Key Flows registry/resolve failure and task 015 fail-closed scope flow | missing registry source/checksum and stale local source are rejected | unit | covered | Self-test injects both failure shapes |
| error-handling | verifier nonzero exit contract | every validation error is accumulated, printed, and returns failure | unit | covered | Self-test proves invalid fixtures are not accepted |
| invariant | task 014 Success Criteria and task 015 exact prefix | exactly one registry 0.2.5 lock entry, exact direct requirement, no patch, no vendor | dv | covered | Current workspace validation checks all invariants together |
| concurrency | no concurrency is declared by either design; Cargo metadata inspection is single-process and read-only | no concurrent case applies | unit | not-applicable | Concrete design evidence contains no shared runtime state, ordering, or reentrancy behavior |

## Validation Rationale

- The generated verifier is preferable to a raw text search because it parses TOML, checks uniqueness, validates registry source/checksum shape, and has executable negative fixtures.
- The dependency tree validates actual Cargo resolution rather than only declared text.
- Compile-only all-target closure is proportionate to the build-resource change and avoids running unrelated runtime suites.
- Pre-fix red/green is not applicable as a runtime bug reproduction: the task removes a temporary dependency source after upstream publication. Negative fixtures reproduce every stale metadata state the migration must reject.

## Unit Tests

| function_or_unit | branch_or_condition | covered_behavior | test_file | status | gap_manual_reason |
|------------------|---------------------|------------------|-----------|--------|-------------------|
| `validate_contents` | clean metadata | accepts exact 0.2.5 registry state | `testing/verify_callback_result_registry.py` | covered | Automated self-test |
| `validate_contents` | root patch present | rejects remaining local override | `testing/verify_callback_result_registry.py` | covered | Automated self-test |
| `validate_contents` | old direct version | rejects dependency lower than required release | `testing/verify_callback_result_registry.py` | covered | Automated self-test |
| `validate_contents` | missing source/checksum | rejects old local lock shape | `testing/verify_callback_result_registry.py` | covered | Automated self-test |
| `validate_contents` | vendor directory exists | rejects incomplete cleanup | `testing/verify_callback_result_registry.py` | covered | Automated self-test |

## DV Tests

| workflow | kind | entry | expected_result | test_file_or_script | status | gap_manual_reason |
|----------|------|-------|-----------------|---------------------|--------|-------------------|
| delivered dependency lifecycle | lifecycle | task-local workspace verifier | terminal state contains registry release and no local package | `testing/verify_callback_result_registry.py` | covered | Automated DV step |
| registry migration main workflow | main | task-local workspace verifier | manifests and lock agree on 0.2.5 | `testing/verify_callback_result_registry.py` | covered | Automated DV step |
| partial migration rejection | failure | verifier self-test plus workspace validation | stale path, version, local lock, or vendor state returns failure | `testing/verify_callback_result_registry.py` | covered | Automated unit fixtures exercise failure states; DV applies the same validator to delivered files |

## Integration Tests

| contract_or_flow | modules_involved | success_case | failure_case | test_file | status | gap_manual_reason |
|------------------|------------------|--------------|--------------|-----------|--------|-------------------|
| callback-result dependency resolution | p2p-frame, sfo-cmd-server, callback-result | reverse tree resolves registry 0.2.5 and all p2p-frame targets compile | missing/incompatible dependency causes Cargo tree or compile command to fail nonzero | task-local `testplan.yaml` integration and contract steps | covered | Automated unified-entry commands |

## Definition of Done

- Task-local self-test, workspace validation, dependency tree, and compile-only consumer closure all execute through the unified entry and pass.
- `testing-coverage-check.py` accepts direct change, case-type, design-element, API-impact, and testplan mappings.
- Testing stage scope includes only the two removed vendored test files, task-local testing artifacts, and generated run evidence.
- Acceptance can trace the corrected scope evidence back to task 014 without claiming new runtime behavior.
