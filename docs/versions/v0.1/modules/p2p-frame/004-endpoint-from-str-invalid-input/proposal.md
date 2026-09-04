---
module: p2p-frame
task_name: 004-endpoint-from-str-invalid-input
submodule: 004-endpoint-from-str-invalid-input
version: v0.1
status: approved
approved_by: user
approved_at: 2026-07-14T10:31:51+08:00
approved_content_sha256: 36c1ec8a4a270ddcb1be6b8af3eaccc38653b696d05c2a746ed1137608fa1634
---

# Endpoint FromStr Invalid-Input Safety Proposal

## Background and Goal

`Endpoint::from_str` currently slices fixed byte ranges such as `s[0..1]` and `s[2..5]` before establishing that the input is long enough and that those offsets are UTF-8 character boundaries. Empty or short strings therefore panic on an out-of-range slice, while non-ASCII input can panic when a byte offset falls inside a multi-byte character.

The goal is to make parsing total for every Rust `&str`: malformed, short, or non-ASCII endpoint text must return a `P2pError` whose code is `InvalidInput`, never panic. Existing valid endpoint text and its decoded `Endpoint` value must remain compatible.

## Scope

### In scope

- Make `Endpoint::from_str` reject every input that cannot safely provide the complete area, address-family, protocol, and socket-address components with `P2pErrorCode::InvalidInput`.
- Cover empty input, every truncated fixed-prefix length, invalid ASCII prefixes, and non-ASCII text whose UTF-8 encoding crosses any currently assumed byte boundary.
- Preserve parsing of valid `Lan`, `Wan`, `Mapped`, and `ServerReflexive` endpoint strings, IPv4 and IPv6 addresses, TCP, QUIC (`qic` and legacy accepted `udp`), and supported extension protocols.
- Preserve existing validation of address-family markers, extension protocol range, and socket-address syntax.
- Require post-implementation red-green regression evidence that the affected inputs panic before the production fix and return `InvalidInput` afterward.

### Out of scope

- Changing the canonical `Endpoint` display encoding, including the `qic` spelling emitted for QUIC.
- Adding Unicode endpoint syntax, trimming whitespace, accepting lowercase area markers, or otherwise broadening the accepted grammar.
- Changing raw binary endpoint codec behavior, `Endpoint` data layout, public signatures, or error-code definitions.
- Refactoring unrelated parsing or endpoint behavior.
- Modifying production code, tests, design, or testing artifacts during this proposal stage.

### Boundary with neighboring modules

- The behavior boundary is the public `FromStr for Endpoint` implementation in `p2p-frame/src/endpoint.rs`.
- Callers in `cyfs-p2p`, `cyfs-p2p-test`, `sn-miner-rust`, and `desc-tool` continue using the same `FromStr` contract; only malformed-input failure changes from an unwind to the already-declared `P2pError` result path.
- The textual encoder and raw binary codec remain unchanged, so no wire-format, persisted-data, or migration work is introduced.

## Requirement Review

- The request is reasonable and necessary. `FromStr` exposes a fallible `Result` contract, so caller-controlled malformed text must be represented by `Err`, not by an indexing panic.
- Returning the existing `InvalidInput` category is preferable to adding an error variant: it matches the parser's other malformed area, protocol, address, and version failures and avoids a public error-contract expansion.
- The accepted grammar should stay ASCII and byte-oriented because every valid fixed prefix is ASCII and `SocketAddr` also uses ASCII syntax. Non-ASCII text is malformed input, not a reason to introduce Unicode-aware endpoint tokens.
- A length-only check would stop short-input panics but would not by itself make arbitrary UTF-8 byte slicing safe. The downstream design must eliminate all unchecked string indexing at fixed byte offsets, or first establish both length and ASCII/character-boundary safety before any slice.
- Wrapping the parser in `catch_unwind` would hide unsafe indexing and impose the wrong control-flow contract, so it is not an acceptable fix.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-EFSI-1 | endpoint_from_str_invalid_input_no_panic | `Endpoint::from_str` must be panic-free for every `&str`; empty, truncated, malformed, and non-ASCII inputs return `P2pErrorCode::InvalidInput`, while all previously valid endpoint strings retain the same decoded value | Limited to endpoint text parsing and its directly bound regression coverage; no signature, display codec, raw codec, grammar expansion, or downstream caller change | Adds explicit prefix validation and rejection before decoding; prioritizes a total parser and stable error semantics over accepting new textual forms | Red-green regression demonstrates pre-fix panic and post-fix `InvalidInput` for empty/short/UTF-8 boundary cases; positive cases cover current area, address-family, protocol, and IPv4/IPv6 forms through the task-scoped test entry | No Unicode grammar, whitespace normalization, new error type, raw codec change, or unrelated parser refactor |

## Success Criteria

- Concrete user-visible or system-visible result: no Rust `&str` supplied to `Endpoint::from_str` can trigger a panic through fixed-range string indexing; invalid inputs return `InvalidInput`, and valid endpoint strings still parse as before.
- Required evidence: an approved design maps `endpoint_from_str_invalid_input_no_panic` to the parsing boundary and concrete scope paths; post-implementation testing supplies red-green cases for `""`, all prefix lengths below the complete fixed prefix, representative invalid ASCII, and multi-byte UTF-8 at/before fixed offsets, plus positive compatibility cases for existing formats.
- Explicit non-goals: no accepted-grammar expansion, display/raw encoding change, public signature change, new error code, downstream API migration, or broad endpoint refactor.

## Risks

- Checking only `s.len() >= 5` still permits a panic if byte offset 1, 2, or 5 is not a UTF-8 boundary; the design and implementation must address both range and boundary safety.
- Overly broad ASCII rejection must not accidentally reject valid IPv6 punctuation; validation should distinguish the fixed ASCII grammar from subsequent socket-address parsing without changing accepted valid forms.
- Rewriting the parser more broadly could accidentally drop the currently accepted legacy `udp` spelling or extension protocol validation; compatibility assertions are required.
- A regression test that checks only `is_err()` could miss the required error category; testing must assert `P2pErrorCode::InvalidInput` as well as the absence of panic.

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/src/endpoint.rs` implements public `FromStr for Endpoint`; this task changes malformed-input failure from panic to the declared `Result::Err` path while preserving valid text grammar | Design records compatibility and exact failure semantics; testing covers positive current formats and negative `InvalidInput` behavior | Proposal inspection identified the public parser, current fixed slices, accepted `qic`/`udp`, extension range, and address-family checks | owner: design/testing; reason: file-level shape and executable cases belong to later stages; acceptance impact: missing compatibility or error-code evidence blocks acceptance | Parser restructuring could unintentionally narrow a valid legacy form |
| data/schema | no | `p2p-frame/src/endpoint.rs` display/raw codec paths remain out of scope; no persistence, schema, migration, cache key, or serialized output changes are requested | Source and design scope review confirm encoder/raw codec paths are unchanged | Proposal scope explicitly excludes textual output and raw codec changes | owner: none; reason: not applicable; acceptance impact: none | none |
| security/privacy/permission | yes | `Endpoint::from_str` is an input-validation boundary and currently lets malformed caller input cause process unwind/denial of service | Design eliminates unchecked fixed-offset indexing; testing includes malformed and multi-byte abuse cases and confirms `InvalidInput` without sensitive logging | Proposal inspection confirmed panic-capable indexing occurs before validation | owner: design/testing; reason: implementation and negative evidence do not yet exist; acceptance impact: any remaining panic-capable malformed input blocks acceptance | Uncovered UTF-8 placement could leave another unwind path |
| runtime/integration | no | The change is synchronous, stateless text parsing in `p2p-frame/src/endpoint.rs`; it does not alter startup, shutdown, concurrency, retries, network calls, background tasks, or external services | Task-scoped unit/contract evidence is sufficient unless design discovers a runtime consumer impact | Proposal inspection found no runtime lifecycle or integration behavior in the affected function | owner: design; reason: re-evaluate only if caller impact is discovered; acceptance impact: newly discovered runtime impact must return to design/testing | Low; downstream callers may rely on valid forms, covered under contract compatibility |
| build/dependency/config/deployment | no | Scope contains no Cargo, feature, dependency, configuration, packaging, deployment, or generated-resource paths | Scope review confirms no such files change | Proposal path boundary is limited to endpoint parsing and later regression artifacts | owner: none; reason: not applicable; acceptance impact: none | none |
| ui/datamodel/workflow | no | `p2p-frame/src/endpoint.rs` has no UI presentation, accessibility, navigation, or frontend/backend workflow surface | Confirm implementation stays inside parser behavior | Proposal inspection found no UI surface | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | This task follows existing packet, admission, testing, and acceptance machinery and does not change `harness/**`, templates, CI, or checker schemas | Run only the existing stage-owned checks when their inputs change | Proposal task uses existing task sequence, doc structure, and stage-scope mechanisms | owner: downstream stages; reason: later checks belong to their owning stages; acceptance impact: missing required existing evidence blocks acceptance | none |

## Approval Record

- approver: user
- approval_date: 2026-07-14
- user_statement: "批准该 proposal，并启动 auto-pipeline 自动完成后续 design、implementation、testing 和 acceptance。"
