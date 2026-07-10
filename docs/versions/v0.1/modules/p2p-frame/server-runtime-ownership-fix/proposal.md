---
module: p2p-frame
submodule: server-runtime-ownership-fix
version: v0.1
status: approved
approved_by: user
approved_at: 2026-07-10T15:27:16+08:00
approved_content_sha256: c5f4a7a3650d45467ce8b8d1e31dacae7b53fdf15bbb3be9bbc274987d00227c
---

# Server Runtime Ownership Fix Proposal

## Background and Goal
The current `ServerRuntime` migration removed the fallback from `p2p-frame`, but the delivered implementation and evidence chain are not yet coherent. `SnServiceConfig::new(...)` already requires a runtime at compile time while the approved design and acceptance report still describe a missing-runtime error. `cyfs-p2p::create_cyfs_p2p_config(...)` still starts a hidden runtime and panics on startup failure, so a process combining SN service and a P2P stack can start two worker pools. The targeted runtime test is gated by `x509` but the registered unit command does not enable that feature. The current `sn-miner` process tests prove only that a child remains alive for three seconds, not that the requested owner or serving role reached a usable ready state.

This sibling fix task aligns the versioned contract with the compile-time-required implementation, makes runtime ownership explicit at the `cyfs-p2p` caller boundary, reuses one process-owned runtime across combined SN/stack startup, registers the feature-gated test correctly, and replaces liveness-only process evidence with role-specific readiness evidence.

## Scope
### In scope
- Record `SnServiceConfig::new(..., ServerRuntime)` as the selected compile-time-required API; absence of a runtime is a compile error rather than a runtime `InvalidParam` branch.
- Keep `create_sn_service(...)` free of runtime creation and remove stale documentation or acceptance claims that it reports a missing-runtime configuration error.
- Change the CYFS P2P configuration construction boundary so the caller supplies an existing `ServerRuntime`; configuration helpers must not start a hidden runtime or panic while doing so.
- Make combined `cyfs-p2p-test all-in-one` startup create one process-owned runtime and clone/pass that same runtime to both `SnServiceConfig` and `P2pConfig`.
- Register the runtime-injection test with the `x509` feature so the canonical unit entry executes a real test instead of reporting zero matching tests.
- Strengthen `sn-miner-rust/tests/real_process.rs` so owner and serving cases wait for role-specific readiness after successful startup, fail on early exit or timeout, retain actionable child output, and clean up the child process.
- Regenerate downstream testing and acceptance evidence after implementation; stale accepted reports must not be treated as evidence for the corrected behavior.

### Out of scope
- Changing `sfo-reuseport` worker scheduling, socket distribution, TCP/QUIC wire behavior, TLS identity validation, SN command semantics, endpoint classification, or owner-directory protocol formats.
- Adding a global singleton, lazy global runtime, service-local fallback, or default runtime inside `p2p-frame` or `cyfs-p2p`.
- Replacing the existing `ServerRuntime` type with a repository-specific wrapper.
- Treating a log line emitted before listener startup, a fixed sleep, or mere process survival as readiness evidence.
- Expanding the real-process test into the full multi-owner/multi-serving distributed-directory DV matrix.

### Boundary with neighboring modules
- `p2p-frame` owns the required-runtime constructor contracts and consumes the supplied runtime without creating it.
- `cyfs-p2p` adapts identity/config types but requires its caller to provide the process-owned runtime.
- `cyfs-p2p-test` owns the combined startup assembly and must share one runtime across SN and stack construction.
- `sn-miner-rust` owns role startup and the observable point at which owner or serving listeners/services are ready; its integration test owns the readiness probe and child cleanup.
- Harness testing metadata owns feature registration and must make the targeted test reachable through the canonical entrypoint.

## Assumptions and Ambiguities
- Assumptions:
  - `ServerRuntime` clones share the same underlying worker pool and are the supported way to pass one process-owned runtime to multiple consumers.
  - Returning successfully from the role's listener/service `start()` call is the earliest valid point for emitting an internal readiness signal.
  - Readiness evidence must combine successful role startup with an externally observable role-specific signal; a child being alive is insufficient.
- Open ambiguities:
  - The exact readiness transport (structured stdout marker plus bound-port probe, or an equivalent deterministic probe) is left to design, but it must be observable by the parent test and emitted only after the relevant start calls succeed.
  - Backward compatibility for the old zero-runtime `create_cyfs_p2p_config(endpoints)` helper must be decided in design; retaining a hidden runtime is not allowed.
- Decision needed before approval:
  - Approve compile-time-required runtime injection as the canonical SN and stack configuration contract.
  - Approve a caller-supplied CYFS helper API even if it requires migrating current workspace callers.
  - Approve role-specific readiness evidence rather than the current liveness-only process assertion.

## Constraints
- Allowed libraries/components:
  - Existing `sfo_reuseport::ServerRuntime` clone semantics.
  - Existing `SnServer::start`, `OwnerDirectoryServer::start`, listener endpoints, process stdout/stderr, and standard-library socket/process primitives.
  - Existing Cargo feature and testplan wiring.
- Disallowed approaches:
  - Hidden `ServerRuntime::start(...)` calls in library configuration helpers.
  - `.expect(...)` or `.unwrap()` as the public-library runtime creation error boundary.
  - Readiness based only on sleep duration, process liveness, or discarded output.
  - Acceptance reports that cite a filtered-out or deleted test.
- System constraints:
  - One process-owned runtime may be cloned, but the combined startup path must not create multiple independent worker pools.
  - Existing network/protocol behavior must remain unchanged.
  - Child processes must be terminated and waited on on both success and failure paths.

## Requirement Challenge
| question | evaluation | risk_or_tradeoff | decision |
|----------|------------|------------------|----------|
| Is aligning documentation to the compile-time-required SN runtime API reasonable? | Yes. The proposal already allowed a mandatory constructor parameter, and compile-time exclusion of the missing state is simpler than retaining an impossible runtime error branch. | This is a migration-required public API and invalidates the old design/acceptance wording. | keep; create sibling correction rather than editing the approved packet in place |
| Should `cyfs-p2p` continue to create a default runtime for convenience? | No. It hides ownership, can create duplicate CPU-sized worker pools, and changes a recoverable startup failure into a panic. | Callers must create and retain/pass the runtime explicitly, requiring workspace caller migration. | require caller-supplied runtime |
| Is a global singleton a simpler way to ensure one runtime? | No. It would hide lifecycle and shutdown ownership and is explicitly outside the existing proposal boundary. | Explicit plumbing changes signatures but keeps ownership auditable. | reject singleton; clone an explicitly owned runtime |
| Is a readiness log marker alone sufficient? | Only if emitted strictly after all role-specific start calls, but it still does not independently prove listener reachability. A deterministic external probe is stronger where an endpoint is available. | Port probes must avoid consuming or corrupting protocol state and need bounded timeout/cleanup. | design a post-start marker plus role-appropriate external readiness check or an equivalently strong deterministic signal |
| Is enabling `x509` only for the targeted test sufficient? | It makes the test reachable, but the canonical module unit entry must also execute it so acceptance cannot accidentally cite a zero-test command. | Enabling `x509` increases unit build time. | update canonical testplan command or add a dedicated registered feature-enabled step bound to the change id |
| Is scope ambiguous? | The user explicitly requested all five corrections. The only remaining choice is the exact backward-compatible API/readiness mechanism, which design can resolve without changing the requested outcomes. | A design that preserves the old helper by secretly starting a runtime would violate the goal. | proceed after explicit proposal approval |

## Large Module Submodule Decision
| Submodule | New or Existing | Responsibility | Proposal Packet | Reason |
|-----------|-----------------|----------------|-----------------|--------|
| `server-runtime-ownership-fix` | new task packet over existing `stack_runtime`, `sn/service`, and startup assembly responsibilities | Correct runtime ownership/API evidence and role-readiness validation across existing modules | `docs/versions/v0.1/modules/p2p-frame/server-runtime-ownership-fix/proposal.md` | The work is a bounded correction to an approved cross-module migration and needs an independent approval/evidence chain; it does not create a new production submodule. |

## Trigger Matrix
| trigger_category | applies | evidence | required_checks | deferred_checks_and_reason |
|------------------|---------|----------|-----------------|----------------------------|
| contract/protocol | yes | `SnServiceConfig::new` and `create_cyfs_p2p_config` are public Rust API boundaries consumed by workspace crates | design compatibility decision; compile migration checks; positive and negative/error contract coverage | none |
| data/schema | no | No persisted data, codec, descriptor schema, or wire field changes are requested | confirm no serialization diff | none |
| security/privacy/permission | no | Runtime ownership and readiness probing do not change identity, authorization, TLS, or secret handling | retain existing TLS/identity tests through integration entry | none |
| runtime/integration | yes | Worker-pool ownership, startup failure, listener readiness, process cleanup, and combined SN/stack startup are runtime lifecycle behavior | single-runtime regression evidence; startup failure propagation; owner/serving readiness and timeout/early-exit cases; cleanup verification | none |
| build/dependency/config/deployment | yes | Cargo `x509` feature selection and constructor signatures change build/configuration behavior | feature-enabled unit execution; relevant crate checks; caller migration coverage | none |
| ui/datamodel/workflow | no | No UI, presentation model, or user interaction workflow exists in the affected crates | not applicable beyond CLI process readiness | none |
| harness/process | yes | `testplan.yaml` must register a feature-gated test and acceptance evidence must stop citing zero-test/deleted cases | doc/testing structure checks; unified test entry; test artifact inspection; acceptance report check | none |

## High-Level Outcomes
- One explicitly created runtime backs both SN service and P2P stack in a combined process.
- Library configuration APIs do not start hidden worker pools or panic on runtime startup.
- SN runtime injection documentation matches the compile-time-required implementation.
- Canonical test execution actually runs the runtime injection case with `x509` enabled.
- Owner and serving real-process tests prove bounded, role-specific readiness rather than mere survival.

## Proposal Items
| proposal_id | change_id | Outcome | Scope Boundary | Success Evidence | Explicit Non-Goal |
|-------------|-----------|---------|----------------|------------------|-------------------|
| P-RUNTIME-FIX-1 | server_runtime_required_api_alignment | Versioned design/testing/acceptance describe `SnServiceConfig::new(..., ServerRuntime)` as compile-time required and remove the stale missing-runtime `InvalidParam` claim. | p2p-frame task docs and affected evidence only; production SN protocol behavior unchanged. | schema/doc checks pass; code and docs expose the same constructor/error contract; new acceptance does not cite the deleted missing-runtime test. | Do not restore an optional runtime or service-local fallback. |
| P-RUNTIME-FIX-2 | process_runtime_single_owner | Combined startup creates one `ServerRuntime` and clones it to SN and stack consumers. | `cyfs-p2p-test` assembly and required runtime-plumbing API only. | regression evidence demonstrates a single creation point in the combined path and successful SN/stack startup using clones of the same runtime. | Do not introduce a singleton or change worker scheduling. |
| P-RUNTIME-FIX-3 | cyfs_config_runtime_injection | CYFS P2P config construction receives a caller-owned runtime and never starts or panics on a hidden runtime. | `cyfs-p2p` config helper and its workspace callers. | source/API checks show no `ServerRuntime::start` in the helper; compile/integration covers migrated callers; runtime startup errors remain caller-handleable. | Do not add a default-runtime compatibility overload that hides ownership. |
| P-RUNTIME-FIX-4 | runtime_x509_test_registration | Canonical test metadata executes the external-runtime test with `x509` enabled. | p2p-frame testing metadata and feature-gated test registration. | targeted command reports exactly one or more executed matching tests; unified unit entry reaches the feature-enabled step and passes. | A zero-test successful exit is not evidence. |
| P-RUNTIME-FIX-5 | sn_miner_role_readiness_evidence | Real-process owner and serving tests wait for deterministic role readiness, diagnose early exit/timeout, and always clean up. | `sn-miner-rust` startup observability and integration tests; no distributed-protocol expansion. | owner readiness proves both required owner listener surfaces are started/reachable; serving readiness proves SN service startup/listener readiness; timeout and early-exit paths fail with retained output; children are reaped. | Do not use a fixed sleep or liveness alone as readiness. |

## Success Criteria
- Concrete user-visible or system-visible result:
  - Combined SN/stack startup uses one process-owned runtime.
  - Callers explicitly supply runtime to CYFS config construction.
  - Owner and serving process tests fail unless their role is genuinely ready within a bounded timeout.
- Required evidence:
  - Approved sibling design with exact API compatibility and readiness mechanism.
  - Passing admission evidence for production changes.
  - Feature-enabled unit output showing the runtime test actually executed.
  - Real-process readiness success plus early-exit/timeout failure semantics.
  - Fresh acceptance report replacing stale claims.
- Explicit non-goals:
  - No protocol, TLS, endpoint, distributed-directory, or worker implementation changes.

## Risks
- Public helper signature migration may affect out-of-tree callers; design must record compatibility and migration guidance.
- A raw TCP connect probe may be accepted as an incomplete protocol connection; design must ensure it cannot destabilize the service or must choose a safer readiness seam.
- Child-output capture can deadlock if pipes are not drained; design must define bounded concurrent capture or a small structured readiness channel.
- Feature-enabled tests may increase build time; test registration should avoid duplicate full-suite execution while still producing real coverage.
- Readiness endpoints using port `0` require the parent to learn the actual bound address; design must use deterministic reserved ports or expose the bound endpoint safely.

## Downstream Follow-Up
| follow_up_id | Owning Stage | Reason | Triggering Proposal Item | Blocking |
|--------------|--------------|--------|--------------------------|----------|
| FU-RUNTIME-DESIGN | design | Define caller-supplied helper signature, one-runtime flow, error boundary, readiness signal/probe, output capture, and migration. | P-RUNTIME-FIX-1 through P-RUNTIME-FIX-5 | yes |
| FU-RUNTIME-IMPLEMENTATION | implementation | Change runtime plumbing and startup observability only after approved design and admission. | P-RUNTIME-FIX-2, P-RUNTIME-FIX-3, P-RUNTIME-FIX-5 | yes |
| FU-RUNTIME-TESTING | testing | Generate feature-enabled unit and role-readiness tests after implementation. | P-RUNTIME-FIX-4, P-RUNTIME-FIX-5 | yes |
| FU-RUNTIME-ACCEPTANCE | acceptance | Re-audit the corrected proposal/design/code/tests and supersede stale acceptance conclusions. | P-RUNTIME-FIX-1 through P-RUNTIME-FIX-5 | yes |

## Proposal Guardrails
- This sibling proposal does not modify the approved parent packet.
- Production and test edits remain blocked until this proposal and its design are explicitly approved and implementation admission passes.
- Testing is post-implementation and must be derived from the delivered code plus the approved proposal/design.
- Each changed line must map to one of the five change ids above.

## Approval Record
- approver: user
- approval_date: 2026-07-10
- user_statement: "批准，自动处理后续步骤"
