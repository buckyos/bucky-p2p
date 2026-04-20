# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-04-17` 确认，进入自动流水线

## 验收基线
- 最终验收以下列文档为准：
  - `docs/versions/v0.1/modules/p2p-frame/proposal.md`

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|------|--------|--------|------|----------|
| P-1 | planning | 创建本轮 `pn_server` 流量统计/限速阶段图与退回路由 | `p2p-frame` / `pn/service` | root | proposal 已批准 | `harness/pipeline-plan.md` | 计划完成 |
| D-1 | design | 定义 relay 侧统计/限速的结构、身份归属和 `sfo-io` 接入边界 | `docs/versions/v0.1/modules/p2p-frame/design.md`、`design/pn-server.md` | root | proposal 已批准 | `design.md`、`design/pn-server.md` | 设计文档覆盖本轮目标且不越界 |
| T-1 | testing | 把 relay 侧统计/限速设计映射为 unit/DV/integration 验证面 | `testing.md`、`testing/pn-server.md`、`testplan.yaml` | root | proposal 已批准，design 已批准 | `testing.md`、`testing/pn-server.md`、`testplan.yaml` | 测试文档与设计一致 |
| I-1 | implementation | 在已批准边界内把 `sfo-io` 统计/限速能力装配进 `pn_server` bridge 路径 | `p2p-frame/src/pn/service/**` 及必要测试 | root | proposal 已批准，design 已批准，testing 已批准 | code + tests | 代码与测试完成并通过验证 |
| A-1 | acceptance | 审计 proposal 到实现的证据链，确认统计口径、限速行为和验证结果一致 | `p2p-frame` 本轮交付 | root | implementation 证据已就绪 | acceptance report | acceptance 通过 |

## 子模块任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|--------|--------|--------|------|----------|
| D-1.1 | design | 定义用户身份归属、统计口径和 bridge 计量挂点 | `pn/service/pn_server.rs` | D-1 | proposal 已批准 | `design/pn-server.md` 更新 | 统计边界清晰 |
| D-1.2 | design | 定义 `sfo-io` 限速器装配、配置输入和失败处理 | `pn/service/pn_server.rs` | D-1 | proposal 已批准 | `design/pn-server.md` 更新 | 限速边界清晰 |
| T-1.1 | testing | 补充 relay 统计准确性与串户防护验证 | `pn/service` unit | T-1 | D-1 已完成 | `testing/pn-server.md` 更新 | 已声明统计相关断言 |
| T-1.2 | testing | 补充 relay 限速、背压和 `sfo-io` 集成验证 | `pn/service` unit + DV | T-1 | D-1 已完成 | `testing/pn-server.md`、`testplan.yaml` 更新 | 已声明限速相关断言 |
| I-1.1 | implementation | 接入 `sfo-io` 统计实现并暴露最小可测观测点 | `pn/service` | I-1 | T-1 已完成 | code + tests | 统计行为通过验证 |
| I-1.2 | implementation | 接入 `sfo-io` 限速实现并保证 bridge 透明转发契约 | `pn/service` | I-1 | T-1 已完成 | code + tests | 限速行为通过验证 |

## 退回规则
- 如果 `sfo-io` 能力边界、依赖版本或配置来源无法从 proposal 支撑：
  - 退回 proposal 澄清任务
- 如果 relay 统计口径、身份归属或限速装配位置不明确：
  - 退回 design 任务
- 如果缺少统计准确性、限速背压或 DV/integration 覆盖：
  - 退回 testing 任务
- 如果实现未按 `sfo-io` 接入或改变了握手/bridge 契约：
  - 退回 implementation 任务

## 退出条件
- [ ] 所有阻塞问题已关闭
- [ ] 必需证据存在
- [ ] relay 侧 `pn_server` 已按 proposal 完成用户流量统计与限速
- [ ] 统计与限速均通过 `sfo-io` 接入实现
- [ ] 已基于 `proposal.md` 通过最终验收
