# 模块分级矩阵

| 模块 | 级别 | 原因 | 最低必需证据 |
|------|------|------|--------------|
| `p2p-frame` | Tier 0 | 核心传输/协议库，影响面最大 | 已批准的 proposal/design/testing，unit，DV，integration，触发项审查，验收报告 |
| `cyfs-p2p` | Tier 1 | 适配层，会影响下游运行时行为 | 已批准的 proposal/design/testing，unit，integration，验收报告 |
| `cyfs-p2p-test` | Tier 1 | 对验证至关重要的运行时 harness，但已发布模块级文档豁免 | DV，语义变化时的 integration；若影响相邻模块契约，则仍需回到受影响模块补齐文档与验收 |
| `sn-miner` | Tier 1 | 负责运行时启动的二进制，具有操作副作用 | 已批准的 proposal/design/testing，DV，验收报告 |
| `desc-tool` | Tier 2 | 相对独立的工具，但输出具备安全敏感性 | 已批准的 proposal/design/testing，unit，行为变化时面向操作员的 DV，验收报告 |

## 评审路由
- Tier 0：触发规则执行最严格，除非验收明确说明，否则不得跳过任何证据
- Tier 1：执行标准硬门禁，并要求评审更多关注运行时行为
- Tier 2：沿用相同阶段模型；若不存在跨模块影响，则 integration 期望可更窄
