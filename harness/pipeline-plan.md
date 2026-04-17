# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：待定

## 验收基线
- 最终验收以下列文档为准：
  - `docs/versions/v0.1/modules/p2p-frame/proposal.md`

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|------|--------|--------|------|----------|
| P-1 | planning | 创建阶段图与退回路由 | `p2p-frame` 数据包 | root | proposal 已批准 | `harness/pipeline-plan.md` | 计划完成 |
| D-1 | design | 定义可执行结构与索引化设计证据 | 完整 `p2p-frame` 模块 | root | proposal 已批准 | `design.md` | design 完成 |
| T-1 | testing | 定义验证面与机器可读计划 | 完整 `p2p-frame` 模块 | root | proposal 已批准，design 已批准 | `testing.md`、`testplan.yaml` | testing 计划完成 |
| I-1 | implementation | 在已批准边界内交付代码与测试 | 完整 `p2p-frame` 模块 | root | proposal 已批准，design 已批准，testing 已批准 | code + tests | implementation 完成 |
| A-1 | acceptance | 审计证据链并判断 proposal 是否满足 | 最终模块评审 | root | implementation 证据已就绪 | acceptance report | acceptance 通过 |

## 子模块任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|--------|--------|--------|------|----------|
| D-1.1 | design | 细化传输与监听器边界 | `networks` | D-1 | proposal 已批准 | 设计说明索引更新 | 子模块设计清晰 |
| D-1.2 | design | 细化 tunnel 生命周期边界 | `tunnel` | D-1 | proposal 已批准 | 设计说明索引更新 | 子模块设计清晰 |
| D-1.3 | design | 细化协议/服务边界 | `ttp`/`sn`/`pn` | D-1 | proposal 已批准 | 设计说明索引更新 | 子模块设计清晰 |
| T-1.1 | testing | 映射传输与 tunnel 覆盖 | `networks`/`tunnel` | T-1 | D-1 已完成 | 测试覆盖更新 | 已声明子模块测试 |
| T-1.2 | testing | 映射协议/服务覆盖 | `ttp`/`sn`/`pn` | T-1 | D-1 已完成 | 测试覆盖更新 | 已声明子模块测试 |
| I-1.1 | implementation | 仅在确有需要时修改传输/tunnel 代码 | `networks`/`tunnel` | I-1 | T-1 已完成 | code + tests | 验证通过 |
| I-1.2 | implementation | 仅在确有需要时修改协议/服务代码 | `ttp`/`sn`/`pn` | I-1 | T-1 已完成 | code + tests | 验证通过 |

## 退回规则
- 如果 acceptance 发现 proposal 存在歧义：
  - 退回 proposal 澄清任务
- 如果 acceptance 发现 design 不匹配：
  - 退回 design 任务
- 如果 acceptance 发现 testing 存在缺口：
  - 退回 testing 任务
- 如果 acceptance 发现 implementation 缺陷：
  - 退回 implementation 任务

## 退出条件
- [ ] 所有阻塞问题已关闭
- [ ] 必需证据存在
- [ ] 已基于 `proposal.md` 通过最终验收
