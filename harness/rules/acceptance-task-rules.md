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
  - 长期模块文档
  - 实现代码
  - 测试代码
  - 测试结果
- 对 `harness/rules/module-doc-exception-rules.md` 中列出的模块：
  - 长期模块文档
  - 实现代码
  - 测试代码
  - 测试结果
  - 任何命中的触发规则与实际执行的验证说明

## 验收规则
- 验收要评估整条证据链的一致性。
- 对默认模块，验收必须把交付结果与已批准的 proposal 进行对照。
- 对文档豁免模块，验收或完成判断必须把交付结果与用户意图、模块长期边界以及命中的触发规则进行对照。
- 验收必须写独立的评审报告，而不是去修改实现或阶段文档。
- 验收必须为每个阻塞性不一致项标明责任阶段。
- 对默认模块，验收应检查本轮改动是否能回溯到直接的 proposal、design 与 testing 条目，而不是只回溯到模块概览或历史背景说明。

## 失败处理
- proposal 不匹配：退回 proposal
- design 不匹配：退回 design
- testing 存在缺口：退回 testing
- implementation 存在缺陷：退回 implementation

## 严格性规则
- 缺失证据即为失败。
- 对默认模块，上游制品仍是 draft 即为失败。
- 对默认模块，实现任务完成但没有验收报告，不算真正完成。
- 对文档豁免模块，若缺少实现结果与验证证据说明，同样不算真正完成。
