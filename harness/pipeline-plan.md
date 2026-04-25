# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-04-25` 确认，进入自动流水线

## 验收基线
- 最终验收以下列文档为准：
  - `docs/versions/v0.1/modules/p2p-frame/proposal.md`

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|------|--------|--------|------|----------|
| P-1 | planning | 创建本轮单 SN NAT 打洞优化的阶段图、依赖和退回路由 | `p2p-frame` / `tunnel` / `sn/client` / 必要 SN 服务端候选转发 | root | proposal 已批准 | `harness/pipeline-plan.md` | 计划完成 |
| D-1 | design | 定义 NAT-aware tunnel 建立策略、候选刷新、direct/reverse 竞速和 proxy 脱代理升级边界 | `docs/versions/v0.1/modules/p2p-frame/design.md`、`design/tunnel-nat-traversal.md` | root | proposal 已批准 | `design.md`、`design/tunnel-nat-traversal.md` | 设计文档覆盖本轮目标且不越界 |
| T-1 | testing | 把 NAT-aware tunnel 建立设计映射为 unit、DV 和 integration 验证面 | `testing.md`、`testing/tunnel-nat-traversal.md`、`testplan.yaml` | root | proposal 已批准，design 已批准 | `testing.md`、`testing/tunnel-nat-traversal.md`、`testplan.yaml` | 测试文档与设计一致 |
| I-1 | implementation | 在已批准边界内实现单 SN NAT 打洞优化及必要测试 | `p2p-frame/src/tunnel/**`、`p2p-frame/src/sn/client/**`、必要 `p2p-frame/src/sn/service/**` 和测试 | root | proposal 已批准，design 已批准，testing 已批准 | code + tests | 代码与测试完成并通过声明验证 |
| A-1 | acceptance | 审计 proposal、design、testing、implementation 和验证证据是否一致 | `p2p-frame` 本轮交付 | root | implementation 证据已就绪 | acceptance report | acceptance 通过 |

## 子模块任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|--------|--------|--------|------|----------|
| D-1.1 | design | 定义 QUIC/UDP NAT 场景下 direct/reverse 竞速触发条件、短延迟窗口和 reverse waiter 边界 | `tunnel/TunnelManager` | D-1 | proposal 已批准 | `design/tunnel-nat-traversal.md` 更新 | 竞速策略清晰且保持统一 register/publish 生命周期 |
| D-1.2 | design | 定义 `SnCall` 新鲜 `reverse_endpoint_array` 候选来源、去重、新鲜度和转发边界 | `sn/client`、必要 `sn/service` | D-1 | proposal 已批准 | `design/tunnel-nat-traversal.md` 更新 | 候选刷新与 SN 语义不冲突 |
| D-1.3 | design | 定义 proxy 连通后的短窗口脱代理升级、退避上限和失败保护 | `tunnel/TunnelManager` | D-1 | proposal 已批准 | `design/tunnel-nat-traversal.md` 更新 | proxy 仍为兜底且升级路径不把 proxy 视为成功 |
| D-1.4 | design | 定义 endpoint 评分按协议、来源、新鲜度和历史结果拆分的边界 | `tunnel/TunnelManager` | D-1 | proposal 已批准 | `design/tunnel-nat-traversal.md` 更新 | TCP 与 QUIC/UDP 失败统计不互相污染 |
| T-1.1 | testing | 补充 direct/reverse 短延迟竞速和 waiter 清理验证 | `tunnel` unit | T-1 | D-1 已完成 | `testing/tunnel-nat-traversal.md` 更新 | 已声明调度断言 |
| T-1.2 | testing | 补充 `SnCall` 携带新鲜候选与 SN 转发保留验证 | `sn/client`、`sn/service` unit | T-1 | D-1 已完成 | `testing/tunnel-nat-traversal.md` 更新 | 已声明候选断言 |
| T-1.3 | testing | 补充 proxy 短窗口脱代理和 endpoint 评分验证 | `tunnel` unit + DV | T-1 | D-1 已完成 | `testing/tunnel-nat-traversal.md`、`testplan.yaml` 更新 | 已声明退避和评分断言 |
| I-1.1 | implementation | 实现 NAT-aware direct/reverse 竞速与新鲜候选构造 | `tunnel`、`sn/client` | I-1 | T-1 已完成 | code + tests | unit 证据覆盖 |
| I-1.2 | implementation | 实现 proxy 短窗口脱代理与按协议 endpoint 评分 | `tunnel` | I-1 | T-1 已完成 | code + tests | unit/DV 证据覆盖 |

## 退回规则
- 如果单 SN NAT 打洞范围、非目标或验收锚点无法支撑某个实现需求：
  - 退回 proposal 澄清任务
- 如果 direct/reverse 竞速、候选刷新、proxy 脱代理或 endpoint 评分结构不明确：
  - 退回 design 任务
- 如果缺少短延迟竞速、`SnCall` 候选、proxy 退避或评分隔离的验证覆盖：
  - 退回 testing 任务
- 如果实现引入多 SN fanout、改变 `SnCallResp` 语义、把 proxy 视为升级成功，或破坏统一 register/publish 生命周期：
  - 退回 implementation 任务

## 退出条件
- [ ] 所有阻塞问题已关闭
- [ ] 必需证据存在
- [ ] 单 SN NAT 打洞优化未引入多 SN fanout 或跨 SN NAT 类型推断
- [ ] QUIC/UDP NAT 候选下 direct/reverse 竞速、`SnCall` 新鲜候选、proxy 短窗口脱代理和 endpoint 评分隔离均已按 proposal 完成
- [ ] 已基于 `proposal.md` 通过最终验收
