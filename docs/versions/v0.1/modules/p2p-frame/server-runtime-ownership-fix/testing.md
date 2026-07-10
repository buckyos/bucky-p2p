---
module: p2p-frame
submodule: server-runtime-ownership-fix
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-07-10T15:44:18+08:00
approved_content_sha256: 83d4610b25b5ee970fde4608ff7575a3c1e3f3ca66808114241e22f11f9f2a73
---

# Server Runtime Ownership Fix Testing

## Test Document Index
| Document | Topic | Scope |
|----------|-------|-------|
| `testing.md` | Derived runtime ownership, feature and readiness cases | all five change ids |
| `testplan.yaml` | Unified feature-enabled unit, DV and integration commands | sibling task entry |

## Unified Test Entry
- Machine-readable plan: `docs/versions/v0.1/modules/p2p-frame/server-runtime-ownership-fix/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py p2p-frame/server-runtime-ownership-fix unit`
- DV: `python3 ./harness/scripts/test-run.py p2p-frame/server-runtime-ownership-fix dv`
- Integration: `python3 ./harness/scripts/test-run.py p2p-frame/server-runtime-ownership-fix integration`
- Module all: `python3 ./harness/scripts/test-run.py p2p-frame/server-runtime-ownership-fix all`
- Project all: `python3 ./harness/scripts/test-run.py all all`
- Registration: the sibling testplan is discovered as `p2p-frame/server-runtime-ownership-fix`; unit explicitly enables `x509` and DV executes the real-process test target.

## Submodule Tests
| Submodule | Responsibility | Detailed Test Doc | Required Behaviors | Edge/Failure Cases | Test Type | Test Files | Status | Gap / Manual Reason |
|-----------|----------------|-------------------|--------------------|--------------------|-----------|------------|--------|---------------------|
| `stack_runtime` / `cyfs_config_adapter` | Required runtime injection and shared handle use | this file | SN config and stack env accept clones of one runtime; CYFS helper accepts runtime and does not create one | missing argument rejected by compiler; runtime start error remains caller-owned | unit + integration | `p2p-frame/src/sn/tests.rs`, compile of `cyfs-p2p/src/stack_builder.rs` and callers | ready | not-applicable: automated |
| `process_startup` / `readiness_validation` | Role start completion and external reachability | this file | owner marker plus two listeners; serving marker plus peer listener; cleanup | invalid config early exit, wrong marker timeout, ready marker with unreachable probe | DV | `sn-miner-rust/tests/real_process.rs` | ready | not-applicable: automated |

## Module-Level Tests
| Test Item | Covered Boundary | Entry | Expected Result | Test Type | Test File/Script | Status | Gap / Manual Reason |
|-----------|------------------|-------|-----------------|-----------|------------------|--------|---------------------|
| Feature-enabled runtime unit | p2p-frame SN/stack required runtime | sibling unit entry | two matching runtime tests execute and pass; output is not zero tests | unit | `p2p-frame/src/sn/tests.rs` | ready | not-applicable: automated |
| SN miner role readiness | real owner/serving binary lifecycle | sibling DV entry | five real-process cases pass with real listener probes and cleanup | DV | `sn-miner-rust/tests/real_process.rs` | ready | not-applicable: automated |
| Workspace caller migration | CYFS helper and callers | sibling integration entry | relevant crates and all targets compile with required runtime parameter | integration | Cargo check step | ready | not-applicable: automated |

## External Interface Tests
| Interface | Responsibility | Success Cases | Failure/Edge Cases | Test Type | Test Doc/File | Status | Gap / Manual Reason |
|-----------|----------------|---------------|--------------------|-----------|---------------|--------|---------------------|
| `SnServiceConfig::new(..., ServerRuntime)` | Required SN runtime | external runtime accepted; one handle clone also used by stack env | omitted argument is compile-time invalid | unit | `p2p-frame/src/sn/tests.rs` | ready | compile-time negative is verified by required signature/source review rather than compile-fail fixture |
| `create_cyfs_p2p_config(endpoints, runtime)` | CYFS adapter pass-through | all workspace callers compile and combined path shares one runtime | no hidden default overload/start/expect; caller owns start error | integration | `cyfs-p2p/src/stack_builder.rs`, Cargo check | ready | OS thread creation failure is not deterministically injectable; source/API evidence covers ownership |
| `SN_MINER_READY role=owner` | Owner startup completion | marker followed by both TCP listener probes | early exit, wrong role marker and unreachable probe fail | DV | `sn-miner-rust/tests/real_process.rs` | ready | not-applicable: automated |
| `SN_MINER_READY role=serving` | Serving startup completion | marker followed by peer TCP listener probe | invalid config exits before marker | DV | `sn-miner-rust/tests/real_process.rs` | ready | not-applicable: automated |

## Direct Change Coverage
| change_id | design_source | validation_id | testplan_level | testplan_step_id | Gap? | Gap / Manual Reason |
|-----------|---------------|---------------|----------------|------------------|------|---------------------|
| server_runtime_required_api_alignment | `design.md` Overall Approach and Interfaces | VAL-RUNTIME-API-COMPILE | integration | runtime-caller-compile | no | compile and source evidence match required constructor contract |
| process_runtime_single_owner | `design.md` Combined SN/stack startup | VAL-RUNTIME-SHARED-X509 | unit | runtime-shared-x509 | no | same runtime is cloned into SN config and stack env; combined source has one creation point |
| cyfs_config_runtime_injection | `design.md` CYFS config construction | VAL-CYFS-RUNTIME-CALLER | integration | runtime-caller-compile | no | helper and all workspace call sites compile with explicit parameter |
| runtime_x509_test_registration | `design.md` Feature-enabled regression | VAL-RUNTIME-X509-EXECUTED | unit | runtime-shared-x509 | no | command enables x509 and test filter executes two cases |
| sn_miner_role_readiness_evidence | `design.md` Owner/Serving readiness key flows | VAL-SN-MINER-READY | dv | sn-miner-role-readiness | no | five real-process cases cover success, early exit, marker timeout, probe failure and cleanup |

## Case-Type Coverage
| change_id | case_type | required | validation_id | level | status | gap_manual_reason |
|-----------|-----------|----------|---------------|-------|--------|-------------------|
| server_runtime_required_api_alignment | normal | yes | VAL-RUNTIME-API-COMPILE | integration | covered | required constructor compiles at all callers |
| server_runtime_required_api_alignment | boundary | yes | VAL-RUNTIME-SHARED-X509 | unit | covered | same handle crosses SN/stack boundary |
| server_runtime_required_api_alignment | negative | yes | VAL-RUNTIME-API-COMPILE | unit | covered | omitted parameter is unrepresentable by function signature |
| server_runtime_required_api_alignment | error | no | VAL-RUNTIME-API-COMPILE | unit | not-applicable | not-applicable: missing runtime has no runtime error state after compile-time alignment |
| server_runtime_required_api_alignment | compatibility | yes | VAL-RUNTIME-API-COMPILE | integration | covered | existing result-returning service creation remains compatible |
| server_runtime_required_api_alignment | lifecycle | yes | VAL-RUNTIME-SHARED-X509 | unit | covered | runtime handle remains alive across both constructed consumers |
| server_runtime_required_api_alignment | cross-module | yes | VAL-RUNTIME-API-COMPILE | integration | covered | p2p-frame, CYFS and binaries compile together |
| process_runtime_single_owner | normal | yes | VAL-RUNTIME-SHARED-X509 | unit | covered | one runtime constructs SN and stack consumers |
| process_runtime_single_owner | boundary | yes | VAL-RUNTIME-SHARED-X509 | unit | covered | clone crosses subsystem ownership boundary |
| process_runtime_single_owner | negative | yes | VAL-CYFS-RUNTIME-CALLER | integration | covered | source audit rejects second hidden start in helper/combined flow |
| process_runtime_single_owner | error | yes | VAL-CYFS-RUNTIME-CALLER | integration | covered | runtime startup remains before partial combined assembly |
| process_runtime_single_owner | compatibility | yes | VAL-RUNTIME-API-COMPILE | integration | covered | all direct callers migrate and compile |
| process_runtime_single_owner | lifecycle | yes | VAL-RUNTIME-SHARED-X509 | unit | covered | clones share lifetime through consumer construction |
| process_runtime_single_owner | cross-module | yes | VAL-RUNTIME-API-COMPILE | integration | covered | SN and CYFS stack consumers share the same dependency type |
| cyfs_config_runtime_injection | normal | yes | VAL-CYFS-RUNTIME-CALLER | integration | covered | supplied runtime produces P2pConfig |
| cyfs_config_runtime_injection | boundary | yes | VAL-RUNTIME-SHARED-X509 | unit | covered | runtime is accepted at adapter boundary |
| cyfs_config_runtime_injection | negative | yes | VAL-CYFS-RUNTIME-CALLER | integration | covered | one-argument caller no longer compiles; no hidden overload exists |
| cyfs_config_runtime_injection | error | yes | VAL-CYFS-RUNTIME-CALLER | integration | covered | fallible start occurs at application boundary and cyfs_perf propagates it |
| cyfs_config_runtime_injection | compatibility | yes | VAL-RUNTIME-API-COMPILE | integration | covered | all workspace callers migrated |
| cyfs_config_runtime_injection | lifecycle | yes | VAL-RUNTIME-SHARED-X509 | unit | covered | caller retains/propagates runtime handle |
| cyfs_config_runtime_injection | cross-module | yes | VAL-RUNTIME-API-COMPILE | integration | covered | CYFS adapter and p2p-frame constructor agree |
| runtime_x509_test_registration | normal | yes | VAL-RUNTIME-X509-EXECUTED | unit | covered | two targeted tests execute |
| runtime_x509_test_registration | boundary | yes | VAL-RUNTIME-X509-EXECUTED | unit | covered | x509 feature gate is enabled explicitly |
| runtime_x509_test_registration | negative | yes | VAL-RUNTIME-X509-EXECUTED | unit | covered | zero-test output is rejected by inspecting executed test count |
| runtime_x509_test_registration | error | yes | VAL-RUNTIME-X509-EXECUTED | unit | covered | Cargo nonzero exit fails unified step |
| runtime_x509_test_registration | compatibility | yes | VAL-RUNTIME-API-COMPILE | integration | covered | feature-enabled test target compiles with workspace callers |
| runtime_x509_test_registration | lifecycle | no | VAL-RUNTIME-X509-EXECUTED | unit | not-applicable | not-applicable: feature registration has no independent runtime lifecycle |
| runtime_x509_test_registration | cross-module | yes | VAL-RUNTIME-API-COMPILE | integration | covered | feature-enabled p2p test and downstream compile coexist |
| sn_miner_role_readiness_evidence | normal | yes | VAL-SN-MINER-READY | dv | covered | owner and serving reach real listeners |
| sn_miner_role_readiness_evidence | boundary | yes | VAL-SN-MINER-READY | dv | covered | owner requires two surfaces; serving requires one known peer listener |
| sn_miner_role_readiness_evidence | negative | yes | VAL-SN-MINER-READY | dv | covered | invalid config exits before ready and wrong role marker times out |
| sn_miner_role_readiness_evidence | error | yes | VAL-SN-MINER-READY | dv | covered | unreachable probe after marker reports timeout diagnostics |
| sn_miner_role_readiness_evidence | compatibility | yes | VAL-SN-MINER-READY | dv | covered | marker is additive stdout behavior and role startup remains unchanged |
| sn_miner_role_readiness_evidence | lifecycle | yes | VAL-SN-MINER-READY | dv | covered | spawn, ready/error, kill if active, wait/reap paths execute |
| sn_miner_role_readiness_evidence | cross-module | yes | VAL-SN-MINER-READY | integration | covered | built sn-miner consumes p2p-frame/CYFS listener APIs |

## Design Element Coverage
| element_type | design_source | derived_cases | level | status | gap_manual_reason |
|--------------|---------------|---------------|-------|--------|-------------------|
| parameter-domain | `design.md` Interfaces and Dependencies | required runtime and clone cases | unit | covered | not-applicable: automated |
| parameter-domain | `design.md` Interfaces and Dependencies | owner marker, serving marker and wrong marker cases | dv | covered | not-applicable: automated |
| state-transition | `design.md` Data and State | start success -> marker -> reachable -> reaped; early exit; timeout; probe failure | dv | covered | not-applicable: automated |
| failure-path | `design.md` Key Call Flows | invalid config, wrong marker and unreachable listener | dv | covered | not-applicable: automated |
| failure-path | `design.md` Key Call Flows | caller-owned runtime startup mapping | integration | covered | OS thread-creation failure uses source/API evidence because deterministic injection is unavailable |
| error-handling | delivered code and design failure handling | required constructor and stdout flush success | unit | covered | stdout flush failure is not safely injectable and is recorded in Unit Tests |
| error-handling | delivered code and design failure handling | child exit, timeout and probe diagnostics | dv | covered | not-applicable: automated |
| error-handling | delivered code and design failure handling | runtime error mapping at caller boundary | integration | covered | not-applicable: compile/source evidence |
| invariant | `design.md` Invariants to Preserve | one runtime can serve both required consumers | unit | covered | not-applicable: automated |
| invariant | `design.md` Invariants to Preserve | marker after start and child reaped | dv | covered | not-applicable: automated |
| invariant | `design.md` Invariants to Preserve | no hidden start and no protocol/API caller drift | integration | covered | not-applicable: compile plus source audit |
| concurrency | `design.md` Risks and Rollback | bounded polling while child workers run; file capture avoids pipe deadlock | dv | covered | test polls output files and owns cleanup; no shared mutable production state added |

## Validation Rationale
| Behavior or Risk | Validation Signal | Why This Is Sufficient | Gap / Manual Reason |
|------------------|-------------------|------------------------|---------------------|
| Required/shared runtime contract | two x509 tests execute plus relevant callers compile | Exercises both constructors with one runtime and catches signature drift | identity equality is private; one creation point is also source-audited |
| Hidden CYFS runtime/panic removal | source search plus all-target compile | Directly checks forbidden helper behavior and migrated consumers | deterministic OS thread-start failure unavailable |
| Owner readiness | marker after `start()` plus TCP connect to both configured endpoints | Proves internal start completion and external listener reachability | raw connect intentionally does not complete TLS protocol |
| Serving readiness | marker after PN/SN start plus TCP connect to desc endpoint | Proves peer listener is actually bound and reachable | owner remote availability is outside this bounded correction |
| Cleanup and diagnostics | active and already-exited children both pass through `stop_and_wait`; Drop is fallback | Prevents leaked children and preserves stdout/stderr on failures | not-applicable: automated |

## Unit Tests
| Function or Unit | Branch or Condition | Covered Behavior | Test File | Status | Gap / Manual Reason |
|------------------|---------------------|------------------|-----------|--------|---------------------|
| `SnServiceConfig::new` / `create_sn_service` | supplied runtime | service construction accepts required runtime | `p2p-frame/src/sn/tests.rs` | covered | not-applicable: automated |
| shared runtime construction | runtime cloned to SN and moved to stack | both consumers construct successfully | `p2p-frame/src/sn/tests.rs` | covered | not-applicable: automated |
| `create_cyfs_p2p_config` | straight-line supplied-runtime path | returns P2pConfig without start/expect | compile/source evidence | covered | function has no conditional branch |
| `announce_ready` | stdout flush succeeds | marker is observable before parent probes | `sn-miner-rust/tests/real_process.rs` | covered | exercised by owner/serving processes |
| `announce_ready` | stdout flush fails | returns contextual startup error | no direct test | gap | standard stdout flush failure is not safely injectable without changing production abstraction; owner: testing, risk: very low, acceptance impact: review error mapping |
| `ManagedChild::wait_ready` | child exits before marker | returns output-rich error | `sn-miner-rust/tests/real_process.rs` | covered | invalid config case |
| `ManagedChild::wait_ready` | matching marker and all probes connect | returns ready | `sn-miner-rust/tests/real_process.rs` | covered | owner and serving cases |
| `ManagedChild::wait_ready` | wrong marker until deadline | returns timeout with captured marker | `sn-miner-rust/tests/real_process.rs` | covered | marker mismatch case |
| `ManagedChild::wait_ready` | marker seen but probe unreachable | returns timeout with `marker_seen=true` | `sn-miner-rust/tests/real_process.rs` | covered | probe failure case |
| `ManagedChild::stop_and_wait` | active child | kill then wait/reap | `sn-miner-rust/tests/real_process.rs` | covered | readiness success/timeout cases |
| `ManagedChild::stop_and_wait` | already exited child | wait/reap without kill | `sn-miner-rust/tests/real_process.rs` | covered | invalid config case |

## DV Tests
| Workflow | Kind | Entry | Expected Result | Test File or Script | Status | Gap / Manual Reason |
|----------|------|-------|-----------------|---------------------|--------|---------------------|
| Owner startup/readiness/shutdown | lifecycle | `sn-miner-role-readiness` | marker, both TCP probes, kill/wait pass | `sn-miner-rust/tests/real_process.rs` | covered | not-applicable: automated |
| Serving startup/readiness/shutdown | lifecycle | `sn-miner-role-readiness` | marker, peer TCP probe, kill/wait pass | `sn-miner-rust/tests/real_process.rs` | covered | not-applicable: automated |
| Role-specific ready workflow | main | `sn-miner-role-readiness` | selected role emits only its post-start marker and exposes its required listener surface | `sn-miner-rust/tests/real_process.rs` | covered | owner and serving cases implement the two main variants |
| Invalid mixed config | failure | `sn-miner-role-readiness` | exits nonzero before marker with `ERROR:` output | `sn-miner-rust/tests/real_process.rs` | covered | not-applicable: automated |
| Wrong role marker | failure | `sn-miner-role-readiness` | bounded timeout retains actual owner marker and reaps child | `sn-miner-rust/tests/real_process.rs` | covered | not-applicable: automated |
| Unreachable post-marker listener | failure | `sn-miner-role-readiness` | bounded timeout records `marker_seen=true` and reaps child | `sn-miner-rust/tests/real_process.rs` | covered | not-applicable: automated |

## Integration Tests
| Contract or Flow | Modules Involved | Success Case | Failure Case | Test File | Status | Gap / Manual Reason |
|------------------|------------------|--------------|--------------|-----------|--------|---------------------|
| CYFS helper consumes p2p-frame runtime | cyfs-p2p, p2p-frame, cyfs-p2p-test, cyfs_perf | all targets compile with explicit runtime | omitted runtime rejected by Rust compiler; no hidden overload | Cargo integration step | covered | compile-time negative does not need a runtime fixture |
| Combined runtime feeds SN and stack | p2p-frame, cyfs-p2p-test | shared-runtime unit and all-in-one source use one start plus clones | hidden second start rejected by source audit | p2p unit + source inspection | covered | all-in-one is non-terminating and remains excluded as automated DV |
| sn-miner listener startup | sn-miner, cyfs-p2p, p2p-frame | owner/serving real binaries expose reachable listeners | invalid config, marker mismatch and probe failure are diagnostic | real-process test target | covered | full distributed query/call remains in existing distributed-directory task |

## Regression Focus
- Previous targeted command exited zero with `running 0 tests`; the sibling unit command must report two executed tests.
- Previous CYFS helper started and `expect`ed its own runtime; source search must return no such call in `stack_builder.rs`.
- Previous all-in-one path started two CPU-sized worker pools; the delivered path has one explicit start and clones it.
- Previous process tests accepted three seconds of liveness with discarded output; new tests require marker plus real TCP connect and retain diagnostics.
- Pre-fix red artifact is unavailable because the new helper call and readiness tests do not compile/run against the old signature/absent marker in this dirty shared worktree. The concrete pre-fix infeasibility is recorded here; review evidence established zero-test execution and liveness-only logic before implementation.

Execution evidence: `test-results/test-runs/20260710T074336Z-p2p-frame+server-runtime-ownership-fix-all.json` records exit code 0 for the feature-enabled two-test unit step, five-case real-process readiness step, and relevant all-target compile step.

## Definition of Done
- [x] Testing docs cover all affected existing submodules without creating a production submodule.
- [x] Human-authored testing docs remain below 1000 lines.
- [x] `testplan.yaml` matches declared entrypoints.
- [x] Tests are reachable through the unified sibling entry.
- [x] Unit tests execute the changed test/helper branches or record a per-branch gap.
- [x] DV covers owner/serving lifecycle and three failure workflows.
- [x] Integration covers the consumed CYFS helper contract and caller migration.
- [x] All five change ids map to validation and testplan steps.
- [x] Required unified commands and fresh artifacts pass.

## Approval Record
- approver:
- approval_date:
- user_statement: ""
