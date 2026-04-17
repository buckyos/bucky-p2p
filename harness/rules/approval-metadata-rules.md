# 审批元数据规则

## 目标
- 让阶段审批状态可被机器读取并可审计。

## 范围
- `proposal.md`
- `design.md`
- `testing.md`

## 必需 Front Matter
- `module`
- `version`
- `status`
- `approved_by`
- `approved_at`

## 允许的状态值
- `draft`
- `approved`

## 规则
- `status: approved` 要求 `approved_by` 和 `approved_at` 都非空。
- `status: draft` 是新建阶段文档的默认值。
- 审批元数据只能由对应的评审或审批任务更新，不能由 implementation 阶段更新。
- 如果审批元数据缺失或不一致，实现准入必须失败。

## 审计规则
- 工作区级状态报告必须从 front matter 读取审批元数据，而不是根据评论或提交历史推断审批结果。
