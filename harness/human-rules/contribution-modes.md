# 协作模式

## 默认模式
- `agent-human loop`
- 人类负责批准 proposal、design 和 testing。
- Agent 在已批准边界内执行规划、实现以及证据准备。
- 人类或受委派的审阅者执行验收，或批准验收结果。

## 允许的模式
- `pure-human`
  - 适用于工作仍处于探索阶段，或法律/政策审批尚未明确的情况
- `human-agent loop`
  - 由人类主导任务，agent 仅辅助完成边界明确的步骤
- `agent-human loop`
  - 在人类完成审批后，由 agent 按阶段推进执行

## 严格性策略
- Tier 0 和 Tier 1 模块默认使用 `agent-human loop`，并且在实现前必须完成 proposal/design/testing 批准。
- 对受治理的模块数据包，任何模式都不得跳过验收报告。
