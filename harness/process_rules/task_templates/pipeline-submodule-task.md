# 流水线子模块任务

## 任务标识
- 任务 ID：
- 阶段：
- 职责：
- Version：
- 模块：
- 子模块：
- change_id：
- 父任务：
- 依赖项：
- 负责人：

## 目标
- 仅为一个直接子模块完成指定阶段。

## 范围边界
- 范围内：
- 范围外：
- 由其他地方处理的共享主题：

## 输入
- Proposal 摘录：
- Design 参考：
- Testing 参考：
- 上游输出：
- 当前子模块映射到的 proposal 条目：
- 当前子模块映射到的 design 条目：
- 当前子模块映射到的 testing 条目：

## 准入检查
- [ ] 必需的上游制品存在
- [ ] 必需的上游批准存在
- [ ] 范围保持在该直接子模块内
- [ ] 范围保持在命名阶段内，除非用户已显式要求跨阶段同步
- [ ] 若为 implementation：该子模块的 proposal、design 输入均已批准
- [ ] 若为 implementation：active `version`、`module`、子模块与 `change_id` 已明确
- [ ] 若为 implementation：module packet 的 `schema-check.py` 已通过
- [ ] 若为 implementation：每个已准入 `change_id` 的 `admission-check.py` 已通过
- [ ] 若为 implementation：当前子模块改动已能映射到直接的 proposal/design 条目

## 必需输出
- 输出文件：
- 证据：
- 若为 implementation：验证是否执行，以及依据是什么

## 允许的改动
- 可以修改：
- 不可修改：

阶段任务默认值：
- Proposal 可以修改：仅 `proposal.md`
- Design 可以修改：仅 `design.md`、`design/` 和必需的长期边界同步
- Testing 可以修改：测试代码、测试夹具、测试入口，以及可选的 `testing.md`、`testing/` 和 `testplan.yaml`
- Implementation 可以修改：仅生产代码和必需的非测试运行时/构建资源
- Acceptance 可以修改：仅评审报告
- 跨阶段编辑必须由用户显式命名额外阶段，或显式要求跨阶段同步

## 完成条件
- [ ] 子模块输出完整
- [ ] 未修改范围外文件
- [ ] 已存在交接给下游依赖任务的数据

## 失败处理
- 如果问题属于共享问题或上游问题，应退回而不是在该子模块任务内直接解决。
- 记录：
  - 问题 ID
  - 退回阶段
  - 退回目标任务
  - 期望的上游修复
