# 模块验收标准

## 用途
- 本文件声明模块的验收基线。
- 它是标准，不是运行日志。

## 必需输入
- 已批准的 `proposal.md`
- 已批准的 `design.md`
- 已批准的 `testing.md`
- `testplan.yaml`
- 长期模块边界文档
- 实现代码与测试
- 测试结果

## 验收标准
- Proposal 意图仍被满足，且没有静默的范围漂移。
- Design 决策已体现在实现布局和接口中。
- Testing 覆盖与直接子模块和暴露契约保持一致。
- 必需的 unit、DV 和 integration 证据存在，或已被显式且可接受地延后。
- 高风险面发生变化时，已记录触发的额外检查。

## 失败条件
- 缺失必需的上游制品
- 必需上游制品缺少审批元数据
- design、testing、implementation 或结果之间不一致
- 无法解释地偏离了触发规则或模块级别要求
- 某项被宣称的行为缺少验收证据

## 输出规则
- 每次验收都在 `docs/versions/<version>/reviews/` 下写独立报告。
- Acceptance 任务不修复代码，也不修复阶段文档。
