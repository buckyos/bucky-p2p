---
task_manifest: task.yaml
status: approved
---

# 入站订阅统一为 owner guard，失败构造不再误删 incumbent subscriber

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 改动集中在 p2p-frame 内部 `NetManager` subscriber 注册/注销接口与 `TunnelManager` 构造/析构生命周期，涉及并发与生命周期正确性，并改变 crate 内公开注册函数的签名，不满足 trivial 的“无并发/生命周期/runtime 集成影响、无接口变更”条件；不涉及协议/wire/数据模型/依赖/部署/安全面，无 high-risk 触发，故按 bounded single-project refactor 归 standard。
- Proposal and tier confirmation: 用户于 2026-09-02 以“确认”确认该提案与 standard tier。后续按 standard 流程：pre-edit baseline -> `docs/changes/039-owner-guarded-incoming-subscribers.md` -> 实现与定向验证 -> 独立缺陷发现 -> `completion-report.md` -> 收尾移除索引。

## Background and Goal

`TunnelManager::new` 先 `Arc::new_cyclic` 构造 manager 并启动后台任务，之后才通过 `register_incoming_tunnel_acceptance_subscriber` 注册入站订阅；注册失败时局部 Arc 析构，`Drop for TunnelManager` 无条件按 identity 调用 `unregister_incoming_tunnel_subscriber`。

对同一 P2pId 的第二次构造在 `contains_key` 处返回 `AlreadyExists`，随后析构仍按 identity 删除订阅 entry——删掉的是第一个正常 manager 的订阅。之后入站隧道因无 subscriber 被 `publish_tunnel` 直接 `Rejected` 并关闭。根因是 map 的“一个 P2pId 一个 subscriber”唯一性只保证同 key 单 entry，不保证“删除者 == 插入者”；注册入口已生成 `Arc<IncomingTunnelSubscriptionOwner>`，但 acceptance 包装函数把它丢弃，`TunnelManager::drop` 只能退回裸 identity 注销。

目标：所有 subscriber 注册路径统一返回 owner token/guard 并由注册方持有；注销只允许 owner-matched（`Arc::ptr_eq`）；注册失败路径因未持有 token 而完全不注销；不再保留任何无 owner 的注册/注销接口。

## Scope

### In scope

- `p2p-frame/src/networks/net_manager.rs`：
  - 保留 crate 内部 `register_incoming_tunnel_subscription` 返回 `Arc<IncomingTunnelSubscriptionOwner>`；保留“一个 P2pId 一个 subscriber”的 `contains_key` 唯一性检查与 `AlreadyExists` 语义。
  - 删除无 owner 的 `register_incoming_tunnel_subscriber` 与 `unregister_incoming_tunnel_subscriber`。
  - acceptance 注册改为返回 `IncomingTunnelSubscriptionGuard`（统一走 owner-matched drop）；legacy 借 `register_owned_incoming_tunnel_subscriber` 继续返回 guard。
  - 保留 `unregister_owned_incoming_tunnel_subscriber` 的 `Arc::ptr_eq` 语义与 `publish_tunnel` legacy 分支的现有 owner 检查。
- `p2p-frame/src/tunnel/tunnel_manager.rs`：
  - `TunnelManager` 新增持订阅 owner 的字段（`Option<IncomingTunnelSubscriptionGuard>` 或等价 token），注册成功才持有，失败保持空。
  - `Drop for TunnelManager` 不再直接按 identity 注销；注销由 guard/owner-matched 路径完成。
- 测试迁移与回归：
  - 更新 `net_manager` 单元测试 helper 与 `p2p-frame/tests/net_manager/net_manager_post_accept_tests.rs` 全部旧接口调用。
  - 新增 stale-loser 回归：第二次 `TunnelManager::new` 因重复 identity 失败后，第一个 manager 的订阅仍存活并能 accept 入站隧道；并覆盖 guard 替换/旧回调不能移除新 owner entry 的竞态。

### Out of scope

- 不修改协议、wire、CLI、配置、依赖、部署面；不建旧接口兼容层或别名。
- 不改 map 唯一性语义（每 P2pId 仍至多一个 subscriber）。
- 不改 `publish_tunnel` 的 accept/reject 判定逻辑；Acceptance 分支现不在 reject 时删除 subscriber，维持不变。
- 不改 TTP runtime 已采用的 owner 行为本身，仅验证其不受影响。
- 不修改 `cyfs-p2p-test/**`、其它 crate 代码与模块长文档。

### Boundary with neighboring modules

- `p2p-frame/src/ttp/runtime/handle.rs` 已使用 `register_owned_incoming_tunnel_subscriber` 与 `IncomingTunnelSubscriptionGuard`，作为本任务的目标形态模板；本任务不改变其字段与流程。
- 当前仓库内无 owner 接口的调用点仅 `TunnelManager::new`、`TunnelManager::drop`、`net_manager.rs` 单元测试与 `tests/net_manager/net_manager_post_accept_tests.rs`；全员迁移后不再存在裸 identity 注册/注销调用。

## Requirement Review

合理。备选方向一：只让失败构造路径跳过注销并保留裸 identity 接口——仍遗留“同 key 仅凭身份匹配”的 dispatch 替换竞态，且保留旧接口的误删风险面；方向二（本提案）：全量 owner 接口，与 legacy guard/TTP 既有模式统一，删除路径天然携带所有权证据。用户已明确“不要兼容老接口，都按 owner 的新接口来”，故选择方向二。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-039-OWNER-API | incoming_subscriber_owner_guard_api | NetManager 全部注册入口返回 owner guard/token，删除无 owner 的 register/unregister 接口，唯一性与 `AlreadyExists` 保留 | `net_manager.rs` 及其全部调用点迁移 | 破坏性签名变更、不兼容旧接口（用户明确选择） | crate 内 `rg` 无旧接口残余调用；`cargo check/test -p p2p-frame` 通过 | 不改唯一性语义、不加兼容层 |
| P-039-TMANAGER | tunnel_manager_holds_subscriber_guard | `TunnelManager` 持有订阅 guard/token；注册失败不持有，`drop` 不再按 identity 裸删 | `tunnel_manager.rs` 构造与析构路径 | 增加一个所有权字段换取失败路径与竞态安全 | stale-loser 回归测试证明第二次构造失败后第一个 manager 仍能 accept 入站隧道 | 不改变 manager 其它生命周期与回调行为 |
| P-039-TESTS | owner_subscriber_stale_loser_regression | 既有测试迁移到 owner 接口；新增失败构造不误删 incumbent 与 guard 替换竞态回归 | `net_manager.rs` 单元测试与 `tests/net_manager/net_manager_post_accept_tests.rs` | 测试需持有 guard 并显式模拟重复构造/替换时序 | 定向回归用例 red->green；相关测试组通过 | 不做全量套件无差别改造、不引入 mock/直连 handler 替代真实语义 |

## Success Criteria

- Concrete system-visible result: 对同一 P2pId 的第二次 `TunnelManager::new` 返回 `AlreadyExists`，且失败析构后第一个 manager 的入站订阅仍然存活；后续入站隧道不再因“无 subscriber”被误拒。全部订阅注册/注销调用点不再出现裸 identity 接口。
- Required evidence: `rg` 调用点迁移清单；新增 stale-loser 回归用例 red->green；`cargo test -p p2p-frame`（含 x509 feature 定向组）通过；最终变更清单与 `completion-report.md` 中的独立缺陷发现记录。
- Explicit non-goals: 不宣称跨进程/多节点/公共 NAT/部署环境证据；不改协议与公开 wire/CLI 契约；不保证本任务外其它生命周期缺陷。

## Risks

- 破坏性签名变更可能影响 crate 之外调用方：提交前做全 workspace `rg` 盘点并跑编译确认；当前盘点仅上述调用点。
- `IncomingTunnelSubscriptionGuard` 当前为 `pub(crate)`，改签名后仍只在 crate 内使用；若未来需跨 crate 暴露，再单独提升可见性，本任务不改。
- `TunnelManager` 持有 guard 的字段 drop 时序：guards 在 struct 字段销毁时执行 owner-matched 注销，TTP runtime 已有同模式实证；回归测试覆盖失败构造与替换竞态两条路径。

## Approval Record

- approver: user
- approval_date: 2026-09-02
- user_statement: 确认（确认提案按 standard 执行）
