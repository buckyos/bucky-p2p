# Pipeline Plan

Workflow tier: high-risk

Risk profile: ../risk-profile.yaml

## Trigger

- Proposal: docs/versions/v0.1/modules/p2p-frame/033-real-p2p-tunnel-socket-tests/proposal.md
- User launch confirmed: yes
- User launch statement: “确认，自动完成”
- Launch stage: design
- First auto stage: implementation
- Design source: design.md
- Per-stage user confirmation: skipped by explicit user auto-pipeline authorization
- Auto-confirm completed document stages: Design remains the approved manual artifact; Implementation, Testing, and Acceptance are pipeline-owned
- Auto-pipeline document policy: stage-selective; automatic testing uses runtime state; testplan.yaml required for automatic testing
- Version: v0.1
- Packet module: p2p-frame
- Task name: 033-real-p2p-tunnel-socket-tests
- Target module(s): p2p-frame
- change_id values: real_socket_tunnel_strategy_matrix, real_socket_legacy_and_proxy_fallbacks, real_socket_collision_and_cross_sn_paths

## Acceptance Baseline

- Final acceptance is judged against `proposal.md`, the approved manual `design.md`, the delivered test source, `testplan.yaml`, and task-bound runtime evidence.
- A rendezvous acknowledgement is only action-armed evidence; every successful tunnel case must prove bidirectional unique-payload transfer over the resulting peer tunnel.
- `cyfs-p2p-test` is excluded from implementation and formal evidence.

## Stage Graph

| Task ID | Stage | Execution Mode | Responsibility | Scope | Parent Task | Depends On | Output | Done Condition |
|---------|-------|----------------|----------------|-------|-------------|------------|--------|----------------|
| D-1 | design | manual | freeze unchanged production relationships and zero-product-change boundary | approved task design | root | none | `design.md` and lifecycle receipt | approved design passes manual completion checks |
| I-1 | implementation | auto-pipeline | audit production call paths and confirm no production prerequisite is missing | existing `p2p-frame/src/**` tunnel, SN, network, and proxy boundaries | root | D-1 | runtime implementation audit with zero product changed paths | all three change ids are implementable entirely in Testing without production mutation |
| T-1 | testing | auto-pipeline | integrate the dedicated test surface, testplan, runner wiring, and executable evidence | task test root and parent-owned shared artifacts | root | T-MATRIX, T-FALLBACK, T-COLLISION | root integration test, module wiring, `testplan.yaml`, and test-run artifacts | every required level is reachable through the task runner and task-bound evidence is recorded |
| A-1 | acceptance | auto-pipeline | independently audit proposal, design, implementation audit, tests, and runtime evidence | complete task packet and delivered test surface | root | T-1 | `acceptance-report.md` | accepted report passes the acceptance report checker with no blocking issue |

## Submodule Tasks

| Task ID | Stage | Execution Mode | Responsibility | Submodule | Parent Task | Depends On | Output | Done Condition |
|---------|-------|----------------|----------------|-----------|-------------|------------|--------|----------------|
| T-FIXTURE | testing | auto-pipeline | build real-socket nodes, readiness, deadlines, payload assertions, and teardown substrate | `p2p-frame/tests/real_p2p_tunnel_socket/fixture.rs` | T-1 | I-1 | exclusive fixture source | fixture exposes only real socket and production-entry helpers needed by child cases |
| T-MATRIX | testing | auto-pipeline | implement public, NAT strategy, and TCP/QUIC representative cases | `p2p-frame/tests/real_p2p_tunnel_socket/strategy_matrix.rs` | T-1 | T-FIXTURE | exclusive strategy test source | required conditions assert branch evidence and bidirectional payload |
| T-FALLBACK | testing | auto-pipeline | implement missing/unknown/stale profile, rendezvous-to-legacy, and PN proxy cases | `p2p-frame/tests/real_p2p_tunnel_socket/fallback.rs` | T-1 | T-FIXTURE | exclusive fallback test source | wire-visible path evidence and final real-tunnel payload assertions cover fallback boundaries |
| T-COLLISION | testing | auto-pipeline | implement simultaneous-open owner lifecycle and production TTP inter-SN case | `p2p-frame/tests/real_p2p_tunnel_socket/collision_cross_sn.rs` | T-1 | T-FIXTURE | exclusive collision and cross-SN test source | stable winner, bounded cleanup, real inter-SN control transport, and payload closure are proven |

## Parallel Scheduling

- Strategy: dependency-ready-set
- Concurrency: use all runtime-available child-agent slots
- Shared artifact owner: parent-orchestrator
- Lock directory: `.harness/locks/`
- Dispatch rule: launch dependency-ready work with practical edit coordination and available capacity
- Serialization reasons: explicit dependency, edit coordination, or exhausted concurrency capacity
- Evidence: record launched task ids and serialization reasons in `.harness/pipelines/v0.1/p2p-frame/033-real-p2p-tunnel-socket-tests/state.json` scheduler waves

## Implementation Merge Decision

Implementation remains one task because the approved design requires zero product modification. Splitting read-only audits by production submodule would create handoffs without separable code ownership; I-1 instead verifies the complete production dependency closure and returns upstream if any test-only implementation would require a new production contract.

## Testing Ownership

- `T-FIXTURE`, `T-MATRIX`, `T-FALLBACK`, and `T-COLLISION` own only their exclusive source paths.
- The parent orchestrator alone owns `p2p-frame/tests/real_p2p_tunnel_socket.rs`, `p2p-frame/tests/real_p2p_tunnel_socket/mod.rs`, `testplan.yaml`, runner registration, scope manifests, and pipeline `state.json`.
- Feature case tasks become dependency-ready only after `T-FIXTURE`; their exclusive paths allow the three case groups to run concurrently.

## Return Rules

- If acceptance finds proposal ambiguity, stop the pipeline and ask the user to decide; do not infer the requirement or create an automatic proposal return task.
- If a production contract is required, return to Design because the approved design explicitly freezes production interfaces.
- If test implementation or evidence is defective, return to the owning Testing task and rerun the dependent integration and acceptance tasks.
- If the same unresolved issue remains after more than 5 unsuccessful iterations, stop and report the issue to the user.

Execution status, testing evidence, return records, and final acceptance are stored in `.harness/pipelines/v0.1/p2p-frame/033-real-p2p-tunnel-socket-tests/state.json`.
