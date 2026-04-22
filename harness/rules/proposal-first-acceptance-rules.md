# 提案优先验收规则

## 目标
- 让已批准的 proposal 成为每个受治理模块的最终验收基线。

## 规则
- 对默认模块，已批准的 `proposal.md` 是最终的意图声明。
- 对 `harness/rules/module-doc-exception-rules.md` 中列出的模块，最终意图声明退回到用户明确要求、长期模块边界和触发规则，不再强制依赖模块 packet proposal。
- `design.md`、`testing.md` 与 `acceptance.md` 负责把该意图落地，但不能覆盖它。
- 如果下游制品与 proposal 冲突，验收必须失败并把工作退回上游。

## 验收必须回答的问题
- 最终交付是否仍在已批准范围内？
- 下游文档是否保留了 proposal 中的非目标与约束？
- 任何宣称的成功，是否建立在比已批准 proposal 更窄的解释之上？

## 失败规则
- 出于实现便利而缩窄 proposal 意图，属于 proposal/design 失败，而不是 implementation 成功。
