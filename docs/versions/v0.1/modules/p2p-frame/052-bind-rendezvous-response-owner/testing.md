---
task_manifest: task.yaml
status: approved
---

# Bind Rendezvous Response Endpoints to the Target Owner Testing

Risk profile: ./risk-profile.yaml

## Test Document Index

| Document | Topic | Scope |
|----------|-------|-------|
| `testing.md` | target serving-SN response ownership validation | full task |

## Unified Test Entry

- Machine-readable task plan: `docs/versions/v0.1/modules/p2p-frame/052-bind-rendezvous-response-owner/testplan.yaml`
- Task all: `UV_CACHE_DIR=.harness/uv-cache uv run --active python ./harness/scripts/test-run.py p2p-frame/052-bind-rendezvous-response-owner all`
- Single-task boundary: only task-plan steps are selected; no package/module runtime suite, `all all`, quality gate, or `cyfs-p2p-test` command is used.
- Registration: both dedicated `p2p-frame` test files are reached through the task plan.

## Repository Consumer Closure

Not applicable: the task changes no public symbol, crate-root export, build surface, or documentation example.

## Submodule Tests

| Submodule | Responsibility | Detailed Test Doc | Required Behaviors | Edge/Failure Cases | Test Type | Test Files | Status | Gap / Manual Reason |
|-----------|----------------|-------------------|--------------------|--------------------|-----------|------------|--------|---------------------|
| SN service | bind target response endpoints to live authenticated serving-SN observations | `testing.md` | same-IP/different-port acceptance and response enforcement | empty observation, target-reported-only IP, mixed owned/unowned list | unit and DV | `p2p-frame/tests/unit/sn_tests/service/distributed_nat_profile_tests.rs`; `p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs` | ready | none |

## Module-Level Tests

| Test Item | Covered Boundary | Entry | Expected Result | Test Type | Test File/Script | Status | Gap / Manual Reason |
|-----------|------------------|-------|-----------------|-----------|------------------|--------|---------------------|
| same-SN owner validation | real SN plus two authenticated clients | `same_sn_rendezvous_response_owner_` tests | observed IP with different port succeeds; any unowned IP returns generic rendezvous failure | DV | `p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs` | ready | none |

## External Interface Tests

| Interface | Responsibility | Success Cases | Failure/Edge Cases | Test Type | Test Doc/File | Status | Gap / Manual Reason |
|-----------|----------------|---------------|--------------------|-----------|---------------|--------|---------------------|
| authenticated inter-SN `relay_rendezvous_from_sn` to target serving SN | enforce ownership before returning an inter-SN response | existing cross-SN no-prediction success remains covered | malicious target prediction returns `PermissionDenied` at serving SN | integration | `p2p-frame/tests/unit/sn_tests/service/distributed_nat_profile_tests.rs` | ready | none |

## Direct Change Coverage

| change_id | design_source | validation_id | testplan_level | testplan_step_id | Gap? | Gap / Manual Reason |
|-----------|---------------|---------------|----------------|------------------|------|---------------------|
| CHG-bind-rendezvous-response-owner | `design.md` File-Level Interfaces, Key Flows, State and Ownership; `design/sn-service.md`; delivered `SnService::deliver_rendezvous_to_local_peer` | VAL-owner-complete | unit | response-owner-unit | no | The mapped unit step covers the empty/no-observation branch; the same change_id is additionally exercised by the DV and integration steps in `testplan.yaml`. |

## Case-Type Coverage

| change_id | case_type | required | validation_id | level | status | gap_manual_reason |
|-----------|-----------|----------|---------------|-------|--------|-------------------|
| CHG-bind-rendezvous-response-owner | normal | yes | VAL-owner-same-sn | dv | covered | observed target IP with a deliberately different predicted port is returned |
| CHG-bind-rendezvous-response-owner | boundary | yes | VAL-owner-empty | unit | covered | self-reported target IP with zero live observations is rejected |
| CHG-bind-rendezvous-response-owner | negative | yes | VAL-owner-same-sn | dv | covered | a mixed list containing one unowned IP rejects the whole response |
| CHG-bind-rendezvous-response-owner | error | yes | VAL-owner-cross-sn | integration | covered | target serving SN returns `PermissionDenied` instead of forwarding the malicious response |
| CHG-bind-rendezvous-response-owner | compatibility | yes | VAL-owner-same-sn | dv | covered | response wire remains unchanged and valid observed-IP prediction succeeds |
| CHG-bind-rendezvous-response-owner | lifecycle | yes | VAL-owner-empty | unit | covered | no current command-tunnel observation fails closed without new cached state |
| CHG-bind-rendezvous-response-owner | cross-module | yes | VAL-owner-cross-sn | integration | covered | authenticated inter-SN entry, serving SN, and real target command transport are exercised |

## Design Element Coverage

| element_type | design_source | derived_cases | level | status | gap_manual_reason |
|--------------|---------------|---------------|-------|--------|-------------------|
| parameter-domain | `design.md` File-Level Interfaces and ownership inputs | empty observed set; one owned endpoint; mixed owned/unowned endpoints; different predicted port | dv | covered | none |
| state-transition | `design.md` State and Ownership specifies snapshot-only observation with no new state | current observation permits response; absent observation rejects response | dv | covered | none |
| failure-path | `design.md` Key Flows unowned/missing observation branch | same-SN generic failure and cross-SN `PermissionDenied` | dv | covered | none |
| error-handling | delivered helper's `PermissionDenied` category and outer generic rendezvous failure | direct serving-SN error plus caller-visible failure | integration | covered | none |
| invariant | `design/sn-service.md` Ownership Invariants | port difference accepted; any unowned IP rejects the entire list; self-report is ignored | dv | covered | none |
| concurrency | design adds no state, task, lock, waiter, or transition and reads one current tunnel snapshot per completed response | no race-specific case | unit | not-applicable | No concurrency declaration or new shared state exists; fail-closed disappearance is represented by the empty-observation branch. |

## Validation Rationale

| Behavior or Risk | Validation Signal | Why This Is Sufficient | Gap / Manual Reason |
|------------------|-------------------|------------------------|---------------------|
| legitimate prediction remains usable | real SN/client response with observed IP and different port succeeds | distinguishes IP binding from erroneous full socket-address equality | none |
| arbitrary third-party traffic steering is blocked | mixed owned/unowned response fails after the target callback executes | proves a single valid endpoint cannot mask a malicious endpoint | none |
| target-controlled report data is not trusted | service cache contains the asserted IP but no live command-tunnel observation and validation fails | directly distinguishes report cache from network observation | certificate-only assertion is inspected statically because the new helper does not read `peer_mgr` at all |
| cross-SN trust boundary is correct | `relay_rendezvous_from_sn` reaches a real target command tunnel and returns `PermissionDenied` | proves enforcement happens on B's serving SN before relay completion | deployed multi-SN/public-NAT infrastructure remains outside local evidence |

## Unit Tests

| Function or Unit | Branch or Condition | Covered Behavior | Test File | Status | Gap / Manual Reason |
|------------------|---------------------|------------------|-----------|--------|---------------------|
| `validate_rendezvous_response_owner` | prediction list empty | existing no-prediction same-SN and cross-SN successes remain valid | existing rendezvous tests | covered | none |
| `validate_rendezvous_response_owner` | observed IP set empty | self-reported cached IP is rejected without a live observation | `p2p-frame/tests/unit/sn_tests/service/distributed_nat_profile_tests.rs` | covered | none |
| `validate_rendezvous_response_owner` | all endpoint IPs present | observed IP with a different port is accepted | `p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs` | covered | real command tunnel is required, so the lowest exposing level is DV |
| `validate_rendezvous_response_owner` | any endpoint IP absent | mixed response is rejected atomically | `p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs` | covered | real command tunnel is required, so the lowest exposing level is DV |

## DV Tests

| Workflow | Kind | Entry | Expected Result | Test File or Script | Status | Gap / Manual Reason |
|----------|------|-------|-----------------|---------------------|--------|---------------------|
| same-SN prediction response | main | authenticated caller -> SN -> authenticated target -> SN response | same observed IP/different port succeeds | `p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs` | covered | none |
| same-SN malicious response | failure | same production command path with mixed endpoint list | caller receives failed rendezvous and no endpoint list | `p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs` | covered | none |
| service lifecycle | lifecycle | no new lifecycle owner exists | not applicable to this snapshot-only validator | none | not-applicable | The task adds no task, timer, socket owner, or cleanup transition. |
| service configuration | config | no behavior-changing configuration exists | not applicable to this unconditional security boundary | none | not-applicable | The ownership check has no configuration branch. |
| service persistence | persistence | no persistent datum exists | not applicable to a live command-tunnel snapshot | none | not-applicable | The task neither reads nor writes persistent state. |

## Integration Tests

| Contract or Flow | Modules Involved | Success Case | Failure Case | Test File | Status | Gap / Manual Reason |
|------------------|------------------|--------------|--------------|-----------|--------|---------------------|
| inter-SN rendezvous delivery | inter-SN trait, SN service, SN client command transport | existing cross-SN `WaitIncoming` response succeeds | predicted third-party IP is rejected by target serving SN | `p2p-frame/tests/unit/sn_tests/service/distributed_nat_profile_tests.rs` | covered | none |

## Regression Focus

- Red evidence before the production fix: the exact same-SN unowned prediction case failed because `unwrap_err()` received `Ok(SnTunnelRendezvousResp { predicted_endpoint_array: [S4qic192.0.2.99:42500] })`.
- Green evidence is produced only through the task-scoped unified runner after all cases are registered.
- Local loopback sockets prove authenticated routing and enforcement order, not public NAT, deployed owner-directory routing, or per-port ownership.

## Definition of Done

- [x] Direct submodule behavior, branch coverage, and cross-SN boundary are recorded.
- [x] `testplan.yaml` maps every validation to `CHG-bind-rendezvous-response-owner`.
- [x] New tests live in dedicated test files already included by the crate test modules.
- [x] Task-scoped runner selects only this task's unit, DV, and integration steps.
- [x] No `cyfs-p2p-test` file, command, scenario, or artifact is used.
- [x] Task-scoped automated tests pass: 4/4 selected cases in `.harness/test-results/test-runs/20260904T050049Z-p2p-frame+052-bind-rendezvous-response-owner-all.json`.
