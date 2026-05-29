# 流水线阶段任务

## 任务标识
- 任务 ID：
- 阶段：proposal / planning / design / testing / implementation / acceptance
- 职责：
- 范围：
- Version：
- Module：
- change_id：
- 父任务：
- 依赖项：
- 负责人：

## 目标
- 仅为一个模块或一个边界明确的范围完成恰好一个阶段产出。

## 输入
- Proposal 输入：
- 上游任务输出：
- 相关文档：
- 相关代码：
- 约束：
- 当前改动映射到的 proposal 条目：
- 当前改动映射到的 design 条目：
- 当前改动映射到的 testing 条目：

## 准入检查
- [ ] 对 implementation-like 范围：在任何代码、测试、构建或资源编辑前已应用 `harness/rules/task-entry-gate-rules.md`
- [ ] 必需的上游制品存在
- [ ] 必需的上游批准存在
- [ ] 范围没有跨入其他阶段
- [ ] 如果范围跨入其他阶段，用户已显式要求这些阶段或跨阶段同步
- [ ] 若为 implementation：`proposal.md` 与 `design.md` 全部为 `approved`
- [ ] 若为 implementation：active `version`、`module` 与 `change_id` 已明确
- [ ] 若为 implementation：active module packet 的 `schema-check.py` 已通过
- [ ] 若为 implementation：每个已准入 `change_id` 的 `admission-check.py` 已通过
- [ ] 若为 implementation：当前改动已能映射到直接的 proposal/design 条目
- [ ] 若为跨模块 implementation：每个受影响模块都已独立通过准入
- [ ] 若为 implementation：已在代码编辑前明确输出 `实现准入通过` (`Implementation admission passed`)

## 允许的改动
- 可以修改：
- 不可修改：

阶段任务默认值：
- Proposal 可以修改：仅 `proposal.md`
- Design 可以修改：仅 `design.md`、`design/` 和必需的长期边界同步
- Testing 可以修改：测试代码、测试夹具、测试入口，以及可选的 `testing.md`、`testing/` 和 `testplan.yaml`
- Implementation 可以修改：仅生产代码和必需的非测试运行时/构建资源
- Acceptance 可以修改：仅评审报告
- 上游变更产生的下游后续工作应记录为退回路由，除非用户显式要求跨阶段同步

## 必需输出
- 输出 1：
- 输出 2：
- 若为 testing：当前改动的验证覆盖或明确缺口记录
- 若为 implementation：是否运行验证，以及为何运行或未运行

## 完成条件
- [ ] 必需输出存在
- [ ] 遵守范围边界
- [ ] 依赖已满足
- [ ] 证据已附上

## 失败处理
- 如果被上游问题阻塞，不要越过范围边界打补丁。
- 记录：
  - 阻塞问题
  - 疑似责任阶段
  - 退回目标
  - 证据
