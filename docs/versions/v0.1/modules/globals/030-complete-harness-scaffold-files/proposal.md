---
task_manifest: task.yaml
status: approved
---

# 补齐 Harness 规则引用的基础脚手架文件

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 本任务新增规则路由器、unfinished-task 索引工具及其共享 manifest 解析器，会直接改变仓库 Harness 流程行为，命中 material Harness-process boundary。
- Proposal and tier confirmation: confirmed by user launch statement `确认，自动完成`

## Background and Goal

最新版规则已经引用 `harness/scripts/context.py`、`harness/scripts/task-index.py` 与 `docs/changes/_template.md`，但当前仓库尚未安装这些路径，导致 `harness-self-check.py` 报 `generated docs reference missing concrete paths`。其中最新版 `task-index.py` 直接依赖同目录的 `task_manifest.py`，因此仅补三个报错路径仍会留下不可运行的索引工具。

目标是用当前 `harness-engineering` bootstrap kit 补齐这四个必要文件，使规则路由、unfinished-task 索引和 standard change record 模板具备可运行、可验证的本地实现。

## Scope

### In scope

- 从当前 bootstrap kit 适配并新增 `harness/scripts/context.py`。
- 从当前 bootstrap kit 适配并新增 `harness/scripts/task-index.py`。
- 新增 `harness/scripts/task_manifest.py`，作为 `task-index.py` 的直接运行依赖和后续 task manifest 解析单一来源。
- 从当前 bootstrap kit 适配并新增 `docs/changes/_template.md`。
- 验证生成规则及自定义规则索引、context 路由、task-index 基本生命周期、workspace Harness 结构和本地 Harness 自检。

### Out of scope

- 不更新或补装 `harness-check.py`、`lifecycle-check.py`、`task-transition.py`、`lower-tier-check.py` 等其他脚手架工具。
- 不迁移或重写现有 legacy task packet、`docs/versions/v0.1/modules/tasks.md` 历史条目或既有运行态证据。
- 不修改 `harness/rules/`、`harness/custom-rules/`、产品代码、测试代码或 test runner。
- 不启动 auto-pipeline，不改变任何产品模块的 runtime、协议、构建或发布行为。

### Boundary with neighboring modules

新增文件属于工作区级 Harness 工具/模板。它们读取现有规则索引和 task packet，不拥有或修改 P2P 产品模块行为；现有自定义规则仍由 `harness/custom-rules/` 独立拥有。

## Requirement Review

用户要求补齐上一轮自检指出的缺失文件，方向合理。直接复制三个报错路径会遗漏 `task-index.py` 的共享解析依赖，因此最小可运行范围应为四个文件。采用 bootstrap kit 当前模板并做仓库路径适配，比重新实现解析和路由逻辑更能保持与最新版规则一致。主要风险是新旧 task packet 格式并存；本任务通过不迁移 legacy packet、只验证新格式路径来控制该风险。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | complete_harness_scaffold_files | 安装可运行的 context router、task index、共享 task manifest parser 和 standard change template，消除当前规则引用的缺失路径 | 仅新增四个文件，不重构其他 Harness 工具 | 保留 legacy packet/index 并存，不在本任务完成全量迁移 | `context.py --validate-index`、task-index 基本生命周期、`harness-self-check.py`、workspace verification 与 `git diff --check` 通过 | 不修改产品代码、规则、自定义规则、测试 runner 或其他 Harness 工具 |
| P-002 | validate_cross_module_harness_routing | 证明工作区级 router/index 对至少两个具体项目模块保持一致可用 | 仅验证 `p2p-frame` 与 `cyfs-p2p` 路由/绑定，不改变模块行为 | 增加一个跨模块验证维度，避免把 globals 工具错误绑定为单模块工具 | 两个模块的 context 路由均成功，globals task index 绑定保持有效 | 不新增模块专用 Harness 分支 |

## Success Criteria

- Concrete user-visible or system-visible result: 四个缺失文件存在且可由仓库本地命令直接调用；原缺失路径自检错误消失。
- Required evidence: context 索引及 `p2p-frame`/`cyfs-p2p` manual/auto 路由验证通过；task-index init/add/list/contains/validate 验证通过；workspace Harness verification、harness self-check 和 diff check 通过。
- Explicit non-goals: 不声称整个最新版 Harness 脚手架已全部同步，也不把 legacy packet 自动迁移为新 manifest。

## Risks

- Harness-process risk: 新增 router 和 task index 会成为后续任务的流程入口，错误解析或索引行为可能影响任务选择与关闭。
- Compatibility risk: 当前仓库仍有无 `task.yaml` 的 legacy unfinished packet；本任务不将它们伪装为已经迁移的新索引记录。
- Scope risk: bootstrap kit 还包含其他新版工具；本任务刻意不顺带安装，后续完整 scaffold 审计仍可能发现独立缺口。
