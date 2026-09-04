---
module: p2p-frame
task_name: 011-sfo-cmd-pkg-len-compatibility
submodule: 011-sfo-cmd-pkg-len-compatibility
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# sfo-cmd-server CmdPkgLen Compatibility Proposal

## Background and Goal

`p2p-frame` is being updated from `sfo-cmd-server` 0.3.x to 0.4.0. The new release no longer implements `CmdPkgLen` for primitive `u16` or `u32`; command clients, servers, nodes, and handler headers must use the dependency's fixed-width length types. The current partial migration leaves raw primitive types and inconsistent `U24` const limits in the SN command paths, causing `cargo check -p p2p-frame` to fail.

The goal is to restore compilation with one centrally defined 10 MiB `U24<{ 10 * 1024 * 1024 }>` alias for every SN owner-serving/inter-SN command path that previously used `u32`, and to migrate remaining primitive length parameters to the matching 0.4 wrapper without unintentionally widening their wire format or package limit.

## Scope

### In scope

- Define the 10 MiB `U24` package-length alias once in the shared SN command type boundary and reference that alias from owner-serving client/server, inter-SN node service, and their command handler headers.
- Replace remaining SN command-runtime primitive length parameters that no longer implement `CmdPkgLen` with the matching `sfo-cmd-server` 0.4 fixed-width wrapper.
- Keep each client, server, node, and registered handler on exactly the same concrete length type and const limit so generic handler signatures agree.
- Retain the requested 10 MiB upper bound for the former `u32` owner-serving/inter-SN command family.
- Validate the focused crate build and the existing p2p-frame verification surfaces required by the later testing stage.

### Out of scope

- Raising legacy `u16` SN command channels to the 10 MiB `U24` format when a matching `U16` wrapper preserves their existing two-byte framing.
- Changing command codes, command bodies, QA correlation, tunnel selection, timeout behavior, SN business semantics, or PN behavior.
- Modifying `sfo-cmd-server`, adding a local dependency patch, or upgrading unrelated dependencies.
- Broadly refactoring SN directory, service, or inter-SN responsibilities.

### Boundary with neighboring modules

- The compatibility adaptation is owned by `p2p-frame` and remains within its `sfo-cmd-server` consumption paths.
- `cyfs-p2p`, `cyfs-p2p-test`, and `sn-miner-rust` remain unchanged consumers; later integration evidence must show they still compile against the corrected `p2p-frame` surface where the canonical test entry requires it.
- The dependency owns `CmdPkgLen`, `U16`, `U24`, encoding, and length validation. This task only selects and consistently aliases those public types.

## Requirement Review

- The requested change is necessary: `sfo-cmd-server` 0.4 deliberately replaces primitive length parameters with bounded fixed-width wrapper types, and the reproduced build reports 15 trait-bound or handler-signature errors from incomplete migration.
- Centralizing the former-`u32` replacement is preferable to repeating `U24<{ 10 * 1024 * 1024 }>` at each generic and handler site. It prevents the observed mismatch where a bare `U24` uses the dependency's default 1 MiB limit while adjacent services use 10 MiB.
- Primitive `u16` sites require a separate compatibility adaptation because `u16` also lost the trait implementation. Using the matching `U16` wrapper preserves the prior two-byte encoding and representable maximum; changing those channels to `U24` would be an unrelated wire-protocol expansion.
- Replacing `u32` with a three-byte `U24` changes the former owner command length field width. This is required by the user-selected dependency API, but mixed 0.3.x/0.4.x peers on those command paths are not assumed wire-compatible and must be called out in design and testing.
- The 10 MiB limit is below `U24`'s representable maximum and gives the dependency a concrete bound. Later design must ensure all producer/consumer and handler types share the exact same const instantiation.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-CMD-PKG-LEN-1 | sfo_cmd_pkg_len_v04_compatibility | Replace raw primitive `CmdPkgLen` parameters with `sfo-cmd-server` 0.4 fixed-width types; define one shared `U24<{ 10 * 1024 * 1024 }>` alias for all former-`u32` SN owner-serving/inter-SN clients, servers, nodes, and handler headers; adapt former-`u16` SN runtime sites with the matching wrapper while preserving their framing | Limited to `p2p-frame` dependency consumption and SN command type/generic/header sites needed for consistent compilation | The former `u32` field becomes a bounded three-byte wire field and is not assumed mixed-version compatible; centralized typing removes const-limit drift and keeps the requested 10 MiB cap | Approved design maps every affected generic/header consumer to the shared aliases and exact scope paths; post-implementation evidence shows no raw primitive remains in active `CmdPkgLen` positions, `cargo check -p p2p-frame` passes, focused boundary/compatibility checks pass, and required canonical p2p-frame validation succeeds | No command-body/API redesign, no unrelated protocol widening, no sfo-cmd-server patch, no timeout/tunnel/business behavior change, and no neighboring-crate production edits |

## Success Criteria

- Concrete user-visible or system-visible result: `p2p-frame` compiles with `sfo-cmd-server` 0.4.0; all former-`u32` owner-serving/inter-SN command paths use one shared 10 MiB `U24` alias, and no handler/service pair disagrees on the concrete length type.
- Required evidence: approved design with exact consumer and scope-path mapping; post-implementation source inspection for primitive/mismatched length types; focused compile evidence; boundary evidence for the 10 MiB limit and preserved `U16` framing; canonical p2p-frame verification required by the testing design.
- Explicit non-goals: no mixed-version compatibility guarantee for former-`u32` owner command frames, no 10 MiB expansion for existing `u16` channels, no dependency implementation change, and no unrelated SN/PN behavior refactor.

## Risks

- A bare `U24` silently selects the dependency's default 1 MiB const limit, so any direct use outside the shared alias can recreate a compile-time handler mismatch or a runtime limit mismatch.
- Client and server peers must agree on the exact fixed-width encoding. Former `u32` peers compiled with 0.3.x use a different length-field width from `U24` peers compiled with 0.4.x.
- A 10 MiB accepted command body increases per-message resource exposure relative to the dependency's 1 MiB default. The dependency-enforced bound must remain explicit, and later testing must include rejection above the limit rather than relying only on compilation.
- Fixing only the currently first-reported `u32` sites leaves `u16: CmdPkgLen` failures in the SN service path; the design must enumerate all active consumers instead of stopping at the first successful local substitution.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/src/sn/directory/{client,server}.rs` and `p2p-frame/src/sn/inter_sn/mod.rs` place the length type in command client/server/node generics and handler `CmdHeader`; `sfo-cmd-server` 0.4 `U24` encodes three bytes instead of the former `u32` four bytes | Design enumerates caller/callee/header types and mixed-version impact; testing covers matching endpoints, exact boundary, over-limit rejection, and preserved former-`u16` width | Proposal inspection and the reproduced compiler diagnostics identify mismatched concrete const types and remaining primitive consumers | owner: design/testing; reason: exact file mapping and executable protocol checks belong downstream; acceptance impact: missing producer/consumer or boundary evidence blocks acceptance | Mixed 0.3.x/0.4.x owner command peers may not interoperate |
| data/schema | no | The affected `CmdHeader` is transient network framing; no persisted data, database schema, cache key, migration, import/export, or retention path is in scope | Confirm design contains no durable-data path and implementation changes no persisted representation | Proposal scope restricts work to command runtime types and network framing | owner: none; reason: not applicable; acceptance impact: none | none |
| security/privacy/permission | yes | The const generic on `U24<{ 10 * 1024 * 1024 }>` is an input-size trust boundary for remote command bodies | Design records where dependency length validation occurs; testing checks exact-limit acceptance and above-limit rejection without allocation or dispatch past the boundary | Dependency source inspection confirms `CmdPkgLen::MAX_PKG_LEN` drives encode/decode validation | owner: design/testing; reason: validation call flow and negative executable evidence belong downstream; acceptance impact: unbounded or incorrectly accepted oversized bodies block acceptance | The requested 10 MiB cap permits larger individual allocations than the dependency default |
| runtime/integration | yes | SN service, directory, and inter-SN command peers exchange these headers over live tunnels, and compile diagnostics show client/server/handler type agreement is required | Design maps each runtime peer pair and failure behavior; testing runs focused command exchange or canonical DV/integration coverage in addition to compile | Proposal inspection enumerates the affected runtime families; no lifecycle or retry behavior change is requested | owner: design/testing; reason: runnable peer evidence belongs downstream; acceptance impact: compile-only evidence cannot establish peer agreement | An unenumerated runtime consumer could retain the wrong concrete type |
| build/dependency/config/deployment | yes | `p2p-frame/Cargo.toml` and `Cargo.lock` currently select `sfo-cmd-server` 0.4.0, whose public `CmdPkgLen` bound rejects primitives | Design records dependency/API surface and rollback to the prior locked release; testing includes a clean or equivalent reproducible `p2p-frame` build and lockfile review | `cargo check -p p2p-frame` reproduced 15 errors against downloaded 0.4.0 | owner: design/testing; reason: admitted implementation and clean validation are downstream; acceptance impact: unresolved lock drift or non-reproducible build blocks acceptance | Transitive dependency changes in the lockfile could exceed the intended upgrade scope |
| ui/datamodel/workflow | no | No frontend, presentation, navigation, accessibility, or UI data model path consumes the Rust command length generic | Confirm no UI paths enter design or implementation scope | Repository/module inspection confines the request to `p2p-frame` Rust networking code | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | The task uses the existing packet, stage-scope, admission, testing, and acceptance machinery without changing `AGENTS.md`, `harness/**`, templates, CI, or schemas | Run only existing stage-owned checks when their governed inputs change | Proposal packet follows the current task sequence and proposal-stage requirements | owner: none; reason: not applicable; acceptance impact: none | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
