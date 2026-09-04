---
module: p2p-frame
task_name: 006-ttp-attach-failure-close
submodule: 006-ttp-attach-failure-close
version: v0.1
status: approved
approved_by: user
approved_at: 2026-07-14T17:53:25+08:00
approved_content_sha256: 60f03cb451cff91270ca27984f41d14d107a1f4784798ab20d658e2868787db6
---

# TTP Attach Failure Tunnel Close Proposal

## Background and Goal

`TtpRuntime::attach_tunnel(...)` marks a tunnel instance as attached before registering its stream, control-stream, and datagram listeners. A non-`NotSupport` registration failure returns immediately, but the current function does not close the tunnel. If an earlier registration already succeeded, the failed tunnel can retain a partially installed set of TTP callbacks while remaining unusable as a completely attached tunnel.

The intended lifecycle is fail-closed: one tunnel instance is attached at most once; if any supported listener registration fails, that tunnel instance is no longer reusable and must be closed before `attach_tunnel(...)` returns the registration error. A later retry creates a new tunnel instance. Listener results with `P2pErrorCode::NotSupport` continue to mean that the corresponding channel capability is absent and do not invalidate the tunnel.

## Scope

### In scope

- On every non-`NotSupport` error returned by `Tunnel::listen_stream(...)`, `Tunnel::listen_control_stream(...)`, or `Tunnel::listen_datagram(...)`, invoke `Tunnel::close()` before returning from `TtpRuntime::attach_tunnel(...)`.
- Preserve the listener-registration error as the result of `attach_tunnel(...)`; a close failure is diagnostic cleanup information and must not replace the original attach failure.
- Cover failures at all three registration positions, including partial attachment where one or two earlier listeners succeeded.
- Confirm that successful registration and `NotSupport` results do not close the tunnel.
- Keep retry semantics based on creating a new `TunnelRef`; the failed tunnel instance is terminal and is not reattached.

### Out of scope

- Adding listener rollback or new `unlisten_*` methods to the public `Tunnel` trait.
- Introducing `Attaching`/`Attached` state machines, waiters, or concurrent attach coordination.
- Removing or redesigning `attached_tunnels`, changing its pointer-identity deduplication, or making a closed tunnel instance attachable again.
- Changing TCP, QUIC, PN, SN, or TTP wire protocols; changing tunnel creation, selection, caching, reconnection, or publish policy.
- Changing the meaning of `P2pErrorCode::NotSupport` or masking the original listener-registration error with a cleanup error.
- Modifying production code, tests, design, or testing artifacts during this proposal stage.

### Boundary with neighboring modules

- The behavior change is owned by `p2p-frame/src/ttp/runtime.rs`, at the point where TTP installs tunnel callbacks.
- Existing TCP, QUIC, and PN tunnel implementations remain responsible for their established `Tunnel::close()` behavior, including clearing stored callbacks and terminating their receive/control paths.
- TTP client, server, and node callers continue to receive the original attach error. They do not gain a requirement to close the tunnel separately after that error.
- Downstream crates and public APIs remain unchanged.

## Requirement Review

- The requested fail-closed lifecycle is reasonable because a tunnel with only a subset of its TTP listeners installed cannot satisfy the successful `attach_tunnel(...)` contract.
- Closing the entire tunnel is preferable to listener-by-listener rollback: the `Tunnel` trait has no `unlisten_*` contract, while the supported production tunnel implementations already own complete close cleanup.
- The failed instance must be terminal. Retrying listener registration on the same instance would require new rollback and concurrency semantics and is unnecessary when the connection layer can create a new tunnel.
- The original registration error is the most actionable cause for callers. Cleanup failure should be logged with tunnel context but must not change the returned error.
- `NotSupport` is not an attach failure in the current capability model. Closing on it would incorrectly reject tunnels that intentionally lack control-stream or another optional channel type.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-TAFC-1 | ttp_attach_failure_closes_tunnel | `TtpRuntime::attach_tunnel(...)` closes the current tunnel exactly once on each non-`NotSupport` stream, control-stream, or datagram listener-registration failure, then returns the original registration error; successful and `NotSupport` registrations do not trigger close | Limited to TTP attach failure cleanup and directly bound regression coverage in `p2p-frame`; no public API, tunnel trait, transport implementation, wire behavior, cache, retry, or publish change | Treats a partially attached tunnel as terminal and relies on a newly created tunnel for retry, avoiding unsupported listener rollback while sacrificing reuse of the failed instance | Task-scoped tests inject a distinct error at each registration position, assert the original error is returned and close count is one, and assert success/`NotSupport` paths leave close count zero; implementation review confirms callers need no duplicate cleanup | No same-instance retry, attach state machine, listener rollback API, `attached_tunnels` redesign, transport close refactor, or broad reconnection test |

## Success Criteria

- Concrete user-visible or system-visible result: a real listener-registration error cannot leave the affected tunnel open with a partially installed TTP callback set; `attach_tunnel(...)` closes it and reports the original error.
- Required evidence: an approved design maps `ttp_attach_failure_closes_tunnel` to the exact runtime failure branches and test seams; post-implementation testing covers stream-first failure, control failure after stream success, datagram failure after earlier successes, successful attachment, and `NotSupport` capability absence.
- Explicit non-goals: no public API or wire change, no rollback API, no same-instance retry guarantee, no attach concurrency state machine, no transport implementation rewrite, and no change to tunnel selection or reconnection policy.

## Risks

- A helper or branch rewrite could accidentally close on `NotSupport`, rejecting valid tunnels with optional unsupported capabilities.
- Returning a close error instead of the listener error would hide the actual attach failure and change caller-visible diagnostics.
- Cleanup must occur for all three listener registration stages; covering only the later partial-registration cases would leave first-registration failure inconsistent.
- If a future `Tunnel` implementation relies on the trait's default no-op `close()`, it will not satisfy the fail-closed cleanup expectation. This task validates the TTP call contract and existing supported production implementations; changing the public trait contract is out of scope.
- The existing attached marker remains pointer-based and is not removed immediately. This is acceptable only under the explicit invariant that a failed tunnel instance is terminal and retry creates a new instance.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/src/ttp/runtime.rs` defines the crate-private `attach_tunnel(...)` result/lifecycle contract consumed by TTP client, server, and node; the change adds fail-closed cleanup while preserving the returned registration error and all public signatures | Design records exact error precedence, caller impact, compatibility, and `NotSupport` behavior; testing includes positive and negative contract cases | Proposal inspection traced all three listener registrations and the TTP client/server/node call sites | owner: design/testing; reason: executable mapping and regression cases belong to later stages; acceptance impact: missing error-preservation or `NotSupport` evidence blocks acceptance | Internal callers could rely on an errored tunnel remaining open, although current callers either propagate or log the error and do not cache it as successfully attached |
| data/schema | no | `p2p-frame/src/ttp/runtime.rs` attachment state is process-local and non-persistent; no serialized data, schema, migration, cache key, or durable state changes are in scope | Scope review confirms no codec or persistence path changes | Proposal inspection found only runtime callback registration and close behavior | owner: none; reason: not applicable; acceptance impact: none | none |
| security/privacy/permission | no | The change does not alter identity, TLS, authentication, authorization, encryption, secrets, permissions, or input trust boundaries; transport close implementations remain unchanged | Confirm implementation stays within TTP attach cleanup | Proposal scope explicitly excludes transport and security behavior | owner: none; reason: not applicable; acceptance impact: none | none |
| runtime/integration | yes | `p2p-frame/src/ttp/runtime.rs` registers stream/control/datagram callbacks across asynchronous tunnel operations; failures currently can leave partial runtime state, and TCP/QUIC/PN close paths clear their callbacks/control runtime | Design specifies lifecycle and failure ordering; testing injects every failure position, verifies close count/error identity, and maps an appropriate task-scoped unit plus DV/integration decision | Proposal inspection confirmed registration order, partial-failure exits, supported close cleanup, and caller error propagation | owner: design/testing; reason: implementation and runnable evidence do not yet exist; acceptance impact: any listener failure path that leaves the tunnel open or any success path that closes it blocks acceptance | An already executing callback may race with close; no new callback cancellation contract is introduced |
| build/dependency/config/deployment | no | No Cargo, feature, dependency, configuration, packaging, deployment, or generated-resource path is in scope | Scope review confirms the change is source/test behavior only | Proposal identifies no build-surface change | owner: none; reason: not applicable; acceptance impact: none | none |
| ui/datamodel/workflow | no | TTP tunnel attachment has no UI presentation, navigation, accessibility, frontend data model, or user-facing workflow surface | Confirm no UI paths change | Proposal inspection found no UI surface | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | The task follows existing packet, admission, testplan, runner, and acceptance rules without changing `harness/**`, templates, CI, or checker behavior | Run existing stage-owned checks only when their owned inputs change | Proposal uses the existing sibling task and stage-scope mechanisms | owner: downstream stages; reason: later checks belong to their owning stages; acceptance impact: missing required existing evidence blocks acceptance | none |

## Approval Record

- approver: user
- approval_date: 2026-07-14
- user_statement: "批准该 proposal，并启动 auto-pipeline 自动完成后续 design、implementation、testing 和 acceptance。"
