# 直接改动映射规则

## 目标
- 让 implementation-ready 的模块数据包能够把“本轮改动”直接锚定到 proposal 和 design；post-implementation testing 生成后应继续映射同一 `change_id`。

## 范围
- `docs/versions/<version>/modules/<module>/proposal.md`
- `docs/versions/<version>/modules/<module>/design.md`
- 可选 `docs/versions/<version>/modules/<module>/testing.md`
- `docs/versions/<version>/modules/_template/*`

## 必需章节
- `proposal.md`：`## Proposal Items`
- `design.md`：`## Directly Mapped Change Items`
- 可选 `testing.md`：`## Direct Change Coverage`

## 规则
- proposal 必须为当前改动列出稳定 `change_id`、`proposal_id`、可验收结果和成功证据，而不是只写泛化目标。
- design 必须把同一 `change_id` 映射到 proposal 条目、设计覆盖以及真实代码路径或接口。
- testing 生成后必须把同一 `change_id` 映射到验证 ID、`testplan_level`、步骤 ID 和规范入口或明确缺口；同一 `change_id` 可以有多个验证行，但每一行都必须能映射到 `testplan.yaml`。
- `testplan.yaml` 生成后，对应自动化步骤或 manual/disabled 层级必须以 `change_ids` 字段包含同一 `change_id`。
- 模块概览、历史说明、外部设计索引或聊天上下文都不能替代这些直接映射章节。
- `harness/rules/module-doc-exception-rules.md` 中列出的模块，不强制要求本模块 packet 提供这些直接映射章节。

## 渐进收紧策略
- `_template` 和所有新的或仍为 `draft` 的模块 proposal/design 数据包，必须具备必需章节；testing/testplan 章节在生成对应制品时必须具备。
- 当前仓库中的 legacy 已批准数据包可以在下一次正式 doc-stage 更新前暂时豁免旧结构检查，但新的 implementation/bugfix 准入仍必须提供明确 `change_id`；缺失时退回对应文档阶段补齐。
- 一旦某个 legacy 数据包重新进入 proposal/design/testing 审批循环，对应阶段的豁免应被移除。

## 失败处理
- proposal 缺少 `Proposal Items` 或 `change_id`：退回 proposal
- design 缺少 `Directly Mapped Change Items` 或 `change_id`：退回 design
- 已生成的 testing 缺少 `Direct Change Coverage`、`change_id` 或 testplan 映射：退回 testing
