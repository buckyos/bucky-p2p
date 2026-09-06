# Completion Report: 062-fix-nat-probe-reconcile-race

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/062-fix-nat-probe-reconcile-race.md

## Delivery Summary

- Outcome: `SnService::reconcile_nat_probe_authority`（p2p-frame/src/sn/service/service.rs:1456）在跨 `get_peer_tunnels().await` 后只按 `peer_id` 清理旧 authority，曾误删 await 期间重连后建立的新注册并抹掉其新画像。修复：快照 `(authority_tunnel_id, registration_generation)`，删除改为 `remove_peer_if_authority` 在 scheduler 锁内重比对快照，并新增 `finish_nat_probe_authority_reconcile` 使"删除判定 + `peer_mgr.invalidate_net_profile`"共享一把锁的作用域；同时把 `apply_nat_probe_transition`、`maintain_nat_probe_state` 过期失效、`set_nat_probe_ports` 失效都收口到同一 scheduler 锁内，保证任一 profile 写都必须与该删除判定的临界区串行。
- Handoff: 生产改动限 `p2p-frame/src/sn/service/nat_probe_scheduler.rs` 与 `p2p-frame/src/sn/service/service.rs`；新增确定性交错回归在 `tests/unit/sn_tests/service/service/nat_probe_scheduler_tests.rs`。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-nat-probe-reconcile-race | 删除时重比对 authority 与注册 generation；保护画像失效；被删注册按快照匹配才删，否则放弃 | proposal.md Scope/In scope | `remove_peer_if_authority` + 锁内删除/失效收口 + 确定性交错测试 | 交错测试证明新注册与新画像幸存；genuine-missing 回归证明清理仍生效 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | `remove_peer_if_authority` 快照匹配、`finish_nat_probe_authority_reconcile` 锁内删除+失效 | tunnel 匹配但 generation 失配、generation 匹配但 tunnel 失配、匹配删除、已缺失 no-op 四类断言 | 仅快照完全匹配才删除，任何重注册（含同 tunnel 换 generation）均被拒绝 | pass |
| boundaries-and-failure-paths | snapshot 时无注册、隧道列表为空/无该 tunnel、断线清理与 reconcile 竞争、同 tunnel 地址变更重注册 | 交错测试走 authority_present=false 的激进路径；逻辑审查各 return 分支 | 无注册即提前返回；重注册排在删除/失效之前则整段放弃不移除 | pass |
| regression-and-side-effects | 全部 profile 写点（apply/expire/set_ports）锁序、`on_peer_disconnected` 独立路径、`authority_tunnel` 兼容保留、新增 debug 日志 | 锁序单向 scheduler→peer_mgr 且无 await 在锁内；`cargo check` 无告警；`nat_probe_logging_contract` 全过 | 新事件为 debug 级，无 info/warn endpoint 泄漏；未能识别到回归 | pass |

## Verification

- Targeted check: 新增 `remove_peer_if_authority_only_removes_a_matching_registration`、`stale_reconcile_does_not_delete_a_registration_rebuilt_during_tunnel_scan`、`stale_reconcile_still_removes_a_genuinely_missing_authority`；nat-probe 套件 33/33；`nat_probe_logging_contract` 3/3；`cargo test -p p2p-frame --features x509 --lib` 490/490；`cargo check -p p2p-frame` clean
- Result: pass
- Exception reason: n/a

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 062-F-001 | none | `on_peer_disconnected`（service.rs:1731）仅按 `peer_id` 删除，其事件不携带被断开的 tunnel id，因此若断线事件晚于另一连接上的并发重注册到达，无法在此处重比对快照 | 属独立、超出本任务范围（本任务针对 `reconcile_nat_probe_authority` 路径）的观察，已在 change record 记录为 follow-up | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 快照重比对 + 锁内失效收口使跨 await 的清理竞态确定性消除；受控交错测试证明新注册/画像幸存而 genuine 清理仍生效；完整 lib 套件与日志契约无回归；独立缺陷发现未发现阻塞项。