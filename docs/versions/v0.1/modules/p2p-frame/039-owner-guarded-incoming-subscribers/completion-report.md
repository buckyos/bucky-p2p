# Completion Report: 039-owner-guarded-incoming-subscribers

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/039-owner-guarded-incoming-subscribers.md

## Delivery Summary

- Outcome: `NetManager` 的入站 subscriber 注册/注销全部收敛为 owner guard 接口；`TunnelManager` 只有注册成功才持有 `IncomingTunnelSubscriptionGuard`，`Drop` 不再按 identity 裸删订阅。对同一 P2pId 的第二次 `TunnelManager::new` 失败后，incumbent 的 acceptance subscriber 保持存活，入站隧道不再被“无 subscriber”误拒；旧回调在替换竞态中也不能移除新 owner 的 entry。
- Handoff: 仅修改 `p2p-frame/src/networks/net_manager.rs`、`p2p-frame/src/tunnel/tunnel_manager.rs`、`p2p-frame/tests/net_manager/net_manager_post_accept_tests.rs`；TTP runtime、协议、wire、CLI、部署面未改动。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| incoming_subscriber_owner_guard_api | 全部注册入口返回 owner guard/token；删除无 owner register/unregister；唯一性与 `AlreadyExists` 保留 | proposal.md P-039-OWNER-API 行 | `net_manager.rs` 新增 `register_owned_incoming_tunnel_acceptance_subscriber` 返回 guard，删除两个旧接口；`rg` 确认仓库无旧接口调用 | 唯一性检查与错误码原样保留；全部调用点迁移为 guard 持有 | pass |
| tunnel_manager_holds_subscriber_guard | `TunnelManager` 注册成功才持有 guard；失败路径不注销；`drop` 不再按 identity 裸删 | proposal.md P-039-TMANAGER 行 | `incoming_subscription` 采用 Mutex 包裹 Option 的 guard 字段，构造函数注册成功后才赋值；`Drop` 删除裸 unregister 调用 | `failed_duplicate_construction_keeps_incumbent_subscriber` 证明失败构造后 incumbent 仍能 accept；首次直写 Option 被并发测试暴露的 `Arc::get_mut` 竞态已改用 Mutex | pass |
| owner_subscriber_stale_loser_regression | 测试迁移到 owner 接口；新增失败构造与替换竞态回归 | proposal.md P-039-TESTS 行 | `net_manager.rs`/`net_manager_post_accept_tests.rs` 迁移；两条新回归用例 | `stale_legacy_callback_cannot_remove_replacement_subscriber` 证明旧回调不能移除新 entry；相关旧用例全部保持通过 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | baseline diff：guard 语义、`Arc::ptr_eq` 匹配、`Drop` 字段析构顺序、`publish_tunnel` legacy 分支 | 审查“第二次构造失败->incumbent 存活”全链：注册入口在 `contains_key` 处返回 Err 时根本不产生 guard，`Drop` 无可删之物；替换竞态下旧 owner 指针不匹配 | 两条新回归红改绿的语义均落实；无按 identity 无条件删除路径残留 | pass |
| boundaries-and-failure-paths | 注册失败、成功、析构、guard 提前 drop、NetManager 先亡（Weak upgrade 失败）各分支 | 检查 guard 的 Weak 升级失败路径不 panic；确认 `incoming_subscription` 在注册返回 Err 时保持 None | 失败路径、弱引用失效路径均安全；旧接口删除后不可能再按 key 裸删 | pass |
| regression-and-side-effects | `networks::net_manager::tests`（10）、`tunnel::tunnel_manager::tests`（63）、完整 lib（427）、9 个集成测试目标 | 完整套件重跑确认无行为回归；TTP shared-runtime、真实 socket/SN 集成测试全过；`cargo check --all-features` 通过 | 首轮完整套件出现既有时序用例 `udp_punch_runtime_skips_missed_ticks_for_active_and_reverse` 抖动：该文件未在本次 diff 中，隔离 3/3 通过，重跑完整套件 427/427 通过，属既有时序 flake | pass |

## Verification

- Targeted check: 两条新回归 + `duplicate_control_rejection_preserves_original_data_connection_route`；`networks::net_manager::tests`；`tunnel::tunnel_manager::tests`；`cargo test -p p2p-frame --features x509 --lib`（427）；`cargo test -p p2p-frame --features x509 --tests`（9 个目标）；`cargo check -p p2p-frame --all-features`
- Result: pass
- Exception reason: n/a

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 039-F-001 | none | 完整套件首轮 426/427，失败用例为该文件未被改动的 `udp_punch_runtime_skips_missed_ticks_for_active_and_reverse` | 既有 QUIC punch 时序用例在完整并发下偶发 “active punch must not replay missed ticks back-to-back”；隔离运行 3/3、重跑完整套件 427/427 通过 | no |
| 039-F-002 | none | `rg` 全仓库无旧接口调用；`cargo check --all-features` 通过 | 注册函数由 `pub` 收敛为 `pub(crate)`，属有意破坏性签名变更，仓库内无消费方；若未来对外发布为库 API 需提升 guard 可见性 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 两条定向回归证明失败构造不再误删 incumbent、旧回调不能移除替换者；owner-matched 注销覆盖全部删除路径；完整 lib 427/427 与全部集成测试目标通过；独立缺陷发现的既有 flake 不涉及本次改动。
