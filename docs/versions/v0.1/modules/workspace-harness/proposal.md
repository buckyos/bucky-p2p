---
module: workspace-harness
version: v0.1
status: approved
approved_by: user
approved_at: 2026-05-13
---

# workspace-harness 提案

## 背景与目标
- 当前工作树包含 `harness/rules/**`、`harness/scripts/**` 和 `harness/pipeline-plan.md` 的治理层变更。
- 这些变更影响 implementation admission、acceptance review、trigger 判断和工作区结构检查，不属于 `p2p-frame`、`cyfs-p2p` 或其他业务模块的代码实现范围。
- 本提案目标是为工作区 Harness 治理变更建立独立的 proposal-first 准入范围，避免治理规则改动绕过自身审计。

## 范围
### 范围内
- acceptance review 对完整工作树 diff、未跟踪文件和跨模块准入的审计规则。
- direct change mapping、schema validation、module doc exception 和 task entry gate 等准入规则。
- `schema-check.py`、`admission-check.py` 和 `verify-workspace-harness.py` 等治理脚本。
- `harness/pipeline-plan.md` 对当前自动流水线计划的描述和退回路径。

### 范围外
- 修改 `p2p-frame`、`cyfs-p2p`、`sn-miner` 或其他业务模块的协议、运行时或测试实现。
- 通过治理规则变更追认未审批的业务实现改动。
- 将本模块作为用户自动审批的替代品；后续 design/testing 仍需独立审批。

### 与相邻模块的边界
- `p2p-frame` 等业务模块负责自身 proposal/design/testing/implementation 证据链。
- `workspace-harness` 只负责工作区门禁、触发规则、结构检查、准入检查和验收规则。
- 任何治理规则导致业务模块准入失败时，业务变更必须回到对应业务模块的数据包，而不是在治理模块中直接放行。

## 约束
- 治理规则必须 fail closed：缺失模块、缺失映射、缺失审批或结构错误不得被静默放行。
- 新增规则必须有脚本或结构检查路径，不能只增加建议性 prose。
- 工作区结构检查必须确认新增规则文件路径真实存在。
- 自动流水线规则保持默认不进入；只有用户显式启动时才可作为后续阶段前置条件。

## 高层结果
- 治理层变更拥有独立可审计范围。
- acceptance 可以把 harness 规则和脚本 diff 归因到 `workspace-harness`，而不是误归入业务模块。
- 后续 implementation admission 和 acceptance review 的失败原因更明确。

## Proposal Items
| proposal_id | change_id | Outcome | Constraints / Non-goals | Success Evidence |
|-------------|-----------|---------|--------------------------|------------------|
| P-HARNESS-REVIEW-1 | workspace_harness_acceptance_review_gate | acceptance 规则要求审计完整工作树 diff、未跟踪文件、跨模块准入和直接 change mapping。 | 不替代业务模块审批；不让测试通过本身构成 acceptance passed。 | design/testing 需覆盖 acceptance rule、trigger rule、schema/admission checker 和 workspace harness verifier。 |
| P-HARNESS-ADMISSION-1 | workspace_harness_direct_admission_gate | task entry、schema-check 和 admission-check 形成直接 change_id 准入门禁，缺失 proposal/design/testing/testplan 映射时失败关闭。 | 不为默认模块提供隐式豁免；仅允许规则文件中显式登记的模块豁免。 | schema/admission/check-implementation 入口能对默认模块和豁免模块给出一致结果。 |
| P-HARNESS-PIPELINE-1 | workspace_harness_pipeline_plan_current_change | `harness/pipeline-plan.md` 记录当前 change_id 的阶段图、依赖、退回路径和退出条件。 | 不自动进入 pipeline；不把 pipeline 计划当作审批本身。 | pipeline plan 与 approved proposal、design/testing 状态和 acceptance 退回路径一致。 |

## 风险
- 治理规则过宽会让未准入实现通过验收。
- 治理规则过窄会阻塞已有合法豁免或 legacy 数据包。
- 脚本与规则 prose 不一致会导致人工评审和机器检查结果分叉。
- 将治理变更与业务实现混在同一 acceptance 中，会使责任阶段不清晰。
