# 验收任务规则

## 目标
- 定义验收如何评估证据并记录结果。

## 范围
- `acceptance.md`
- `docs/versions/<version>/reviews/` 下的评审报告

## 必需输入
- 对默认模块：
  - 已批准的 `proposal.md`
  - 已批准的 `design.md` 与 `design/`
  - 已批准的 `testing.md` 与 `testing/`
  - `testplan.yaml`
  - `acceptance.md`
  - 长期模块文档
  - 实现代码
  - 测试代码
  - 测试结果
  - 完整工作区 diff
  - `harness/rules/acceptance-review-rules.md`
- 对 `harness/rules/module-doc-exception-rules.md` 中列出的模块：
  - 长期模块文档
  - 实现代码
  - 测试代码
  - 测试结果
  - 任何命中的触发规则与实际执行的验证说明

## 验收规则
- 验收要评估整条证据链的一致性。
- 验收必须应用 `harness/rules/acceptance-review-rules.md`，审计完整工作区 diff，而不是只审计声明的 module 或 `change_id`。
- 验收必须检查阶段文档之间、文档与代码之间的一致性。
- 对默认模块，验收必须把交付结果与已批准的 proposal 进行对照。
- 对文档豁免模块，验收或完成判断必须把交付结果与用户意图、模块长期边界以及命中的触发规则进行对照。
- 验收必须写独立的评审报告，而不是去修改实现或阶段文档。
- 验收必须为每个阻塞性不一致项标明责任阶段。
- 对默认模块，验收应检查本轮改动是否能回溯到直接的 proposal、design 与 testing 条目，而不是只回溯到模块概览或历史背景说明。
- 对默认模块，验收必须检查实现改动是否通过稳定 `change_id` 映射到 proposal、design、testing 与 `testplan.yaml`。
- 验收必须检查每个发生变更的模块是否都有已批准且直接映射的 proposal、design、testing 与 testplan 覆盖。
- 缺失或模糊的 active module / `change_id` 证据属于阻塞性准入失败。
- 缺少必需证据时，验收不得标记为 passed。
- 测试通过本身不构成 acceptance passed。

## 失败处理
- proposal 不匹配：退回 proposal
- proposal 与 design 不匹配：退回 design；若 proposal 本身歧义或自相矛盾，退回 proposal
- design 与 testing 不匹配：退回 testing
- testing 存在缺口或测试元数据无效：退回 testing
- 文档与代码不匹配：退回 implementation/code
- implementation 存在缺陷：退回 implementation
- 完整 diff 评审 finding：退回 finding 标明的责任阶段

## 严格性规则
- 缺失证据即为失败。
- 对默认模块，上游制品仍是 draft 即为失败。
- 对默认模块，实现任务完成但没有验收报告，不算真正完成。
- 对文档豁免模块，若缺少实现结果与验证证据说明，同样不算真正完成。
