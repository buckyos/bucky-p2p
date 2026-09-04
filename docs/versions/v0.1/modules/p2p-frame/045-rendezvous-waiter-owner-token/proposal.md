---
task_manifest: task.yaml
status: approved
---

# Rendezvous Waiter Owner Token 生命周期修复 Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment

- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 修复会改变 `TunnelManager` 内入站 rendezvous 的并发线性化与取消/替换生命周期。当前 tuple-keyed waiter 可被重复通知覆盖，并可被失败 contender 或 displaced stale owner 删除；这直接触发 concurrency/lifecycle/runtime integration 高风险边界。
- Proposal and tier confirmation: confirmed by user statement `确认，自动完成`; auto-pipeline launched from design.

## Background and Goal

`on_sn_rendezvous` 当前在 owner 安装前把入站 waiter 写入只由 `(remote_id, tunnel_id, expected_reverse)` 标识的 `pending_reverse_waiters`。相同 tuple 的重复或碰撞通知会先覆盖合法 owner 的 notifier，随后 `install_rendezvous_owner` 返回 `AlreadyExists`，错误清理再按 tuple 删除刚覆盖的槽；合法 owner 的 waiter 因而失去通知入口并只能超时。owner 替换时，`abort_rendezvous_owner` 同样按 tuple 清理，可能删除 replacement winner 的 waiter。

目标是让 rendezvous waiter 与本地唯一 owner token/generation 绑定，并让 owner 与其 waiter 在同一状态锁下完成发布/替换决策；任何未成为 owner、已被替换或晚到的旧 owner 都不能覆盖、消费或删除当前 owner 的 waiter。

## Scope

### In scope

- 为 rendezvous-owned incoming waiter 增加唯一 owner token/generation 绑定；普通非 rendezvous reverse waiter 的既有语义保持可用。
- 将 rendezvous owner 安装与其 waiter 发布放到同一个 `ManagerState` 临界区中，消除“先覆盖 waiter、后发现 owner 冲突”的窗口。
- 所有 rendezvous completion/cancel/abort/displacement 清理仅在 token 仍匹配时移除 waiter；stale owner 操作不得影响 replacement owner。
- 重复同 `seq/tunnel_id/direction` 通知必须返回现有冲突结果，同时保留合法 owner 与 notifier。
- 稳定 peer ordering 导致 owner 替换或 contender 让步时，winner 的 waiter 必须继续可通知，loser 的 waiter 不得遗留或误删 winner。
- 增加确定性回归测试，直接覆盖重复通知覆盖/错误清理以及 stale displaced owner 删除 replacement waiter 两条竞态。

### Out of scope

- 不改变 SN rendezvous wire 字段、command code、认证、协议版本或 response/ack 语义。
- 不改变 NAT plan、prediction、endpoint eligibility、single-query/legacy fallback 或 PN fallback 策略。
- 不把一般 reverse path waiter 全面重构为新的公开抽象；只做保证 rendezvous owner 安全所需的最小内部调整。
- 不整理、覆盖或归属工作区中既有的其他未提交修改。

### Boundary with neighboring modules

- 修改限定在 `p2p-frame/src/tunnel/tunnel_manager.rs` 的私有 owner/waiter 状态、生命周期 helper 与相邻单元测试。
- `p2p-frame/src/sn/**` 仍只负责通知与协议处理，不改变 wire contract。
- `p2p-frame/src/networks/**` 的 incoming tunnel 交付继续按逻辑 tuple 查找 waiter；只有 rendezvous owner 对该槽的发布和清理增加 token 所有权校验。

## Requirement Review

该修复要求合理且必要。现有 owner 自身已有 `Arc<()>` token，并在 attach/complete/cancel 时校验，但 waiter 仍脱离 token 生命周期，形成不一致的所有权模型。建议复用同一 owner token，不引入 wire generation；在持有 `ManagerState` 锁时同时决定 owner 冲突/替换和 waiter 槽的安装，使 owner record 与 waiter record 共享一个线性化点。清理路径必须携带 token 做 compare-and-remove，而不是按 tuple 无条件删除。

主要权衡是 incoming tunnel 的消费路径只持有 tuple，无法预知 token；因此 waiter entry 应保存 token，而 incoming 成功仍可按 tuple 消费当前 entry，只有 owner 发起的管理/清理操作要求 token 匹配。这样既保护 replacement winner，又不改变网络交付接口。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-RWOT-1 | rendezvous_waiter_owner_token_lifecycle | rendezvous owner 与 waiter 使用同一唯一 token，并在一个状态锁临界区内原子安装；owner 的 abort/complete/cancel/displacement 只能删除 token 匹配的 waiter | 仅限 `TunnelManager` 私有状态与 helper；incoming tunnel 仍按 tuple 消费当前 waiter | waiter entry 需要区分普通 registration 与 owner-bound registration，但避免扩大网络层接口 | 状态级测试证明重复 contender 不能覆盖/删除 incumbent waiter，stale displaced owner 不能删除 replacement waiter；相关现有 owner lifecycle tests 通过 | 不新增 wire generation，不改 SN/NAT 策略 |
| P-RWOT-2 | rendezvous_waiter_collision_regression_tests | 为重复入站通知和 owner replacement 清理竞态增加可确定复现的 red-green 回归覆盖 | 测试应走生产 owner/waiter helper 或 `on_sn_rendezvous` 可控路径，不以仅验证新辅助函数替代真实生命周期 | 完整网络通知路径可能依赖昂贵身份/预测环境；允许用同模块 production state helper 精确控制交错，并补跑现有 rendezvous 集成测试 | 修复前测试分别表现为 incumbent waiter 丢失和 replacement waiter 被 stale abort 删除；修复后两者可被对应 incoming tunnel/消费路径通知 | 不声称本地测试覆盖公网 NAT、多 SN 部署或不可控调度的所有交错 |

## Success Criteria

- Concrete user-visible or system-visible result: 重复或碰撞的入站 rendezvous 不再使合法 owner 的 incoming waiter 丢失；owner replacement 后，旧 owner 的取消/完成/abort 不会破坏新 owner 的 waiter。
- Required evidence: 两条缺陷的 red-green 回归测试；现有 rendezvous collision、stale attach/complete/cancel、incoming waiter 与相关 p2p-frame targeted tests 通过；检查所有无条件 tuple 删除路径，证明 owner 管理清理均使用 token compare-and-remove。
- Explicit non-goals: 不改 wire、NAT 决策、endpoint/prediction、fallback；不将动作已布置误报为 tunnel 已建立；本地测试不冒充公网/多 SN 部署证据。

## Risks

- owner/waiter 若未共享同一锁和 token，任何部分修复仍可能保留 check-then-insert 或 stale cleanup 窗口。
- incoming 成功消费与 owner completion/cancel 存在正常竞争；实现需保证 notifier 至多被一个路径取走，同时未取到不误伤 replacement entry。
- outbound rendezvous、普通 reverse waiter 与 NAT-plan waiter 复用同一 map；token 化不得破坏这些非入站 owner 路径的现有 RAII cleanup。
- 目标文件已有大量未提交修改，实施必须以最小增量补丁工作，并用 task baseline/stage evidence 区分本任务改动与既有内容。

## Approval Record

- approver: user
- approval_date: 2026-09-03
- user_statement: `确认，自动完成`
