# 入站订阅统一为 owner guard，失败构造不再误删 incumbent subscriber

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/039-owner-guarded-incoming-subscribers/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/039-owner-guarded-incoming-subscribers/proposal.md
- Affected paths: p2p-frame/src/networks/net_manager.rs, p2p-frame/src/tunnel/tunnel_manager.rs, p2p-frame/tests/net_manager/net_manager_post_accept_tests.rs
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

把所有入站 subscriber 注册收敛到 owner-guard 接口：

- `NetManager` 删除无 owner 的 `register_incoming_tunnel_subscriber`/`unregister_incoming_tunnel_subscriber`，新增 `register_owned_incoming_tunnel_acceptance_subscriber` 返回 `IncomingTunnelSubscriptionGuard`；legacy 沿用现有 `register_owned_incoming_tunnel_subscriber`。内部 `register_incoming_tunnel_subscription` 仍返回 owner Arc，`contains_key` 唯一性与 `AlreadyExists` 语义保留。
- `TunnelManager` 新增 `incoming_subscription: Mutex<Option<IncomingTunnelSubscriptionGuard>>`，仅在注册成功后持有；注册失败路径（重复 identity）不持有任何 token，`Drop` 不再按 identity 裸删，注销统一由 guard 的 owner-matched `Arc::ptr_eq` 路径完成。字段用 `Mutex` 而非 `Option` 直写，因为后台 proxy-upgrade 任务会立即通过 `weak.upgrade()` 持有临时强引用，构造期 `Arc::get_mut` 不可用（首次实现被现有并发测试暴露并修正）。
- TTP runtime 已使用的 owner 路径不变；`publish_tunnel` 的 legacy 分支原有 owner 检查保留。
- 测试迁移到 owner 接口，并新增两条定向回归：失败的重复 `TunnelManager::new` 不删除 incumbent 订阅；旧 legacy 回调在替换竞态中不能移除新 owner 的 entry。

## Risk Screen

- Public contract, protocol, or CLI change: yes（有意破坏旧注册/注销签名，统一为 owner 接口；仓库内 `rg` 盘点无旧接口调用方，函数改为 `pub(crate)`。该签名变更是已确认提案的明确范围，不改 wire/protocol/CLI）
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: yes（失败构造析构不再删除 incumbent 订阅；删除必须携带注册所有权。定向回归 + 完整套件验证）
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no（仅新增/迁移 p2p-frame 测试）
- Cross-project or architectural boundary change: no（改动仅限 p2p-frame 内部三文件）

## Verification

- Targeted check: `failed_duplicate_construction_keeps_incumbent_subscriber`、`stale_legacy_callback_cannot_remove_replacement_subscriber`、`duplicate_control_rejection_preserves_original_data_connection_route`；`networks::net_manager::tests` 10 个；`tunnel::tunnel_manager::tests` 63 个；完整 `cargo test -p p2p-frame --features x509 --lib` 427 个；全部集成测试目标（9 个）通过。
- Result: pass
- Residual risk or follow-up: 完整套件首轮曾出现既有时序用例 `udp_punch_runtime_skips_missed_ticks_for_active_and_reverse` 抖动（三次隔离运行全过，重跑完整套件 427/427 通过，该文件未被本次改动触碰）。注册函数由 `pub` 收敛为 `pub(crate)`，仓库内无调用方；若未来以库形式对外发布 p2p-frame，需把 guard 提为 `pub`。
