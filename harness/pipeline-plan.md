# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-04-25` 确认，进入自动流水线
- `2026-04-28` 发现 QUIC 同源 UDP punch cadence 需求超出已批准边界，按失败路由回退到 `proposal`、`design` 与 `testing`
- `2026-04-28` 再次发现 active/reverse punch 起始时机需要区分：active 从 `200ms` 开始，reverse 立即开始，按失败路由重新回退到 `proposal`、`design` 与 `testing`

## 验收基线
- 最终验收以下列文档为准：
  - `docs/versions/v0.1/modules/p2p-frame/proposal.md`

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|------|--------|--------|------|----------|
| P-1 | planning | 为 QUIC 同源 UDP punch cadence 扩边创建回退阶段图、依赖和退回路由 | `p2p-frame` / `networks/quic` / `tunnel` / `sn/client` | root | 原 proposal 已批准，发现实现需求越界 | `harness/pipeline-plan.md` | 回退计划完成 |
| P-2 | proposal | 把 QUIC 同源 UDP punch 从“统一立即起发的 50ms cadence”补充为“active 从 200ms 起发、reverse 立即起发，默认持续到 1 秒”的新基线与风险边界 | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | root | P-1 | `proposal.md` | proposal 草案覆盖扩边目标、约束、风险与验收锚点 |
| D-1 | design | 定义 active `200ms` 起发、reverse `0ms` 起发、之后固定 `50ms` cadence、`1s` burst 截止与 hedged window 裁剪边界 | `docs/versions/v0.1/modules/p2p-frame/design.md`、`design/tunnel-nat-traversal.md` | root | P-2 | `design.md`、`design/tunnel-nat-traversal.md` | 设计文档覆盖本轮目标且不越界 |
| T-1 | testing | 把新的 active/reverse 非对称 UDP punch cadence 设计映射为 unit、DV 和 integration 验证面 | `testing.md`、`testing/tunnel-nat-traversal.md`、`testplan.yaml` | root | D-1 | `testing.md`、`testing/tunnel-nat-traversal.md`、`testplan.yaml` | 测试文档与设计一致 |
| I-1 | implementation | 在重新批准的边界内实现 active `200ms` 起发、reverse 立即起发的 QUIC UDP punch 调度及必要测试 | `p2p-frame/src/networks/quic/**`、必要 `p2p-frame/src/tunnel/**` 和测试 | root | proposal 已批准，design 已批准，testing 已批准 | code + tests | 代码与测试完成并通过声明验证 |
| A-1 | acceptance | 审计 proposal、design、testing、implementation 和验证证据是否一致 | `p2p-frame` 本轮交付 | root | implementation 证据已就绪 | acceptance report | acceptance 通过 |

## 子模块任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|--------|--------|--------|------|----------|
| D-1.1 | design | 定义 QUIC UDP punch 的 active `200ms` 起发、reverse `0ms` 起发、之后固定 `50ms` cadence、默认 `1s` burst 截止和 hedged window 裁剪边界 | `networks/quic` | D-1 | P-2 已完成 | `design/tunnel-nat-traversal.md` 更新 | cadence、起发时机、截止时长和裁剪语义清晰 |
| D-1.2 | design | 定义 QUIC/UDP NAT 场景下 direct/reverse 竞速触发条件、短延迟窗口和 reverse waiter 边界 | `tunnel/TunnelManager` | D-1 | P-2 已完成 | `design/tunnel-nat-traversal.md` 更新 | 竞速策略清晰且保持统一 register/publish 生命周期 |
| D-1.3 | design | 定义 `SnCall` 新鲜 `reverse_endpoint_array` 候选来源、去重、新鲜度和转发边界 | `sn/client`、必要 `sn/service` | D-1 | P-2 已完成 | `design/tunnel-nat-traversal.md` 更新 | 候选刷新与 SN 语义不冲突 |
| T-1.1 | testing | 补充 QUIC UDP punch 的 active `200ms` 起发、reverse `0ms` 起发、固定 `50ms` cadence、默认 `1s` 截止和同源 socket 验证 | `networks/quic` unit | T-1 | D-1 已完成 | `testing/tunnel-nat-traversal.md`、`testplan.yaml` 更新 | 已声明 cadence、起发时机与截止断言 |
| T-1.2 | testing | 补充 direct/reverse 短延迟竞速与 `SnCall` 候选验证 | `tunnel`、`sn/client`、`sn/service` unit | T-1 | D-1 已完成 | `testing/tunnel-nat-traversal.md` 更新 | 已声明调度与候选断言 |
| I-1.1 | implementation | 实现 active `200ms` 起发、reverse `0ms` 起发、`50ms` cadence / `1s` 截止的 QUIC UDP punch 调度与必要 listener 测试 | `networks/quic` | I-1 | T-1 已完成 | code + tests | unit 证据覆盖 |
| I-1.2 | implementation | 保持 NAT-aware direct/reverse、proxy 脱代理和按协议 endpoint 评分与新 cadence 一致 | `tunnel`、`sn/client` | I-1 | T-1 已完成 | code + tests | unit/DV 证据覆盖 |

## 退回规则
- 如果单 SN NAT 打洞范围、非目标或验收锚点无法支撑 active `200ms` / reverse `0ms` 的 UDP punch 扩边需求：
  - 退回 proposal 澄清任务
- 如果 direct/reverse 竞速、active/reverse 起发时机、固定 cadence、hedged window 裁剪、候选刷新、proxy 脱代理或 endpoint 评分结构不明确：
  - 退回 design 任务
- 如果缺少 active/reverse 非对称起发、50ms cadence、1 秒截止、短延迟竞速、`SnCall` 候选、proxy 退避或评分隔离的验证覆盖：
  - 退回 testing 任务
- 如果实现引入多 SN fanout、改变 `SnCallResp` 语义、把 proxy 视为升级成功，或破坏统一 register/publish 生命周期：
  - 退回 implementation 任务

## 退出条件
- [ ] 所有阻塞问题已关闭
- [ ] 必需证据存在
- [ ] 单 SN NAT 打洞优化未引入多 SN fanout 或跨 SN NAT 类型推断
- [ ] QUIC/UDP NAT 候选下 active `200ms` 起发、reverse `0ms` 起发、固定 `50ms` cadence、默认 `1s` 截止、direct/reverse 竞速、`SnCall` 新鲜候选、proxy 短窗口脱代理和 endpoint 评分隔离均已按 proposal 完成
- [ ] 已基于 `proposal.md` 通过最终验收
