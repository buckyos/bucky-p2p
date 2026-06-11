---
module: workspace-harness
version: v0.1
status: approved
approved_by: user
approved_at: 2026-05-13
approved_content_sha256: a56c434ef7d630f8893ded967ce3fe1fe5a207640edcbd63d173bc85c8e9192e
---

# workspace-harness 设计

## 设计范围
### 目标
- 为工作区治理规则、准入脚本、结构检查和 pipeline plan 建立独立模块边界。
- 让 acceptance review 审计完整工作树 diff，并能识别未跟踪文件、跨模块改动和直接 change mapping 缺口。
- 让 implementation admission 以 `version`、`module`、`change_id` 和已批准 proposal/design/testing/testplan 映射为准。

### 非目标
- 修改业务模块代码、协议或测试实现。
- 通过治理规则变更追认缺少业务模块文档覆盖的实现。
- 让 pipeline plan 自动等价于审批记录。

## 总体方案
- `harness/rules/**` 保存人类和 agent 都必须遵守的治理规则。
- `harness/scripts/**` 保存机器检查入口，优先把硬门禁编码为失败关闭的脚本。
- `harness/pipeline-plan.md` 只记录当前自动流水线计划、阶段依赖和退回路径；是否进入流水线仍由用户显式触发。
- `docs/versions/v0.1/modules/workspace-harness/**` 作为治理层变更的版本化数据包。

## 实现布局
| 路径 | 类型 | 职责 |
|------|------|------|
| `harness/rules/acceptance-task-rules.md` | rule | 定义验收输入、完整 diff 审计和失败退回规则 |
| `harness/rules/trigger-rules.md` | rule | 定义契约、运行时、安全、配置、harness 等触发类别和额外检查 |
| `harness/rules/acceptance-review-rules.md` | rule | 定义 acceptance review 对完整工作树、未跟踪文件和跨模块准入的审计方式 |
| `harness/rules/direct-change-mapping-rules.md` | rule | 定义 proposal/design/testing/testplan 的 `change_id` 直接映射规则 |
| `harness/rules/module-doc-exception-rules.md` | rule | 定义少数模块文档豁免路径及边界 |
| `harness/rules/schema-validation-rules.md` | rule | 定义结构和审批元数据校验边界 |
| `harness/rules/task-entry-gate-rules.md` | rule | 定义 implementation-like 请求进入文档准入的入口规则 |
| `harness/scripts/schema-check.py` | script | 校验模块数据包结构、审批元数据和 testplan 形状 |
| `harness/scripts/admission-check.py` | script | 校验指定 `change_id` 是否被已批准 proposal/design/testing/testplan 直接覆盖 |
| `harness/scripts/verify-workspace-harness.py` | script | 校验工作区治理文件和必需规则路径存在 |
| `harness/pipeline-plan.md` | plan | 记录当前 change pipeline 的阶段图、依赖和退回路径 |

## Directly Mapped Change Items
| change_id | proposal_id | Design Coverage | Scope Paths | Risk / Rollback Notes |
|-----------|-------------|-----------------|-------------|-----------------------|
| workspace_harness_acceptance_review_gate | P-HARNESS-REVIEW-1 | acceptance 必须审计完整工作树 diff、未跟踪文件、跨模块准入和直接 change mapping；缺失证据不得标记 passed。 | `harness/rules/acceptance-task-rules.md`、`harness/rules/acceptance-review-rules.md`、`harness/rules/trigger-rules.md` | 若规则过宽导致业务实现绕过准入，应回滚规则并重新设计失败关闭条件。 |
| workspace_harness_direct_admission_gate | P-HARNESS-ADMISSION-1 | schema/admission/check-implementation 入口按 `version`、`module`、`change_id`、审批状态和 testplan 映射失败关闭；模块豁免只允许显式登记。 | `harness/rules/task-entry-gate-rules.md`、`harness/rules/direct-change-mapping-rules.md`、`harness/rules/module-doc-exception-rules.md`、`harness/rules/schema-validation-rules.md`、`harness/scripts/schema-check.py`、`harness/scripts/admission-check.py`、`harness/scripts/verify-workspace-harness.py` | 若 legacy 数据包被过度阻塞，应在规则中显式记录窄豁免，而不是在脚本中静默通过。 |
| workspace_harness_pipeline_plan_current_change | P-HARNESS-PIPELINE-1 | pipeline plan 记录当前 change 的 planning/design/testing/implementation/acceptance 节点、依赖、输出和退回规则。 | `harness/pipeline-plan.md` | 若当前 change 切换，pipeline plan 必须随新的已批准 proposal 更新；计划本身不是审批。 |

## 风险与回滚
- 治理规则与脚本不一致会造成准入结果不可预测。
- 新增规则路径如果未纳入 workspace verifier，可能在验收中被遗漏。
- 回滚治理变更时必须同时回滚规则、脚本和 pipeline plan 中的对应假设。

## Approval Record
- approver: user
- approval_date: 2026-06-11
- user_statement: 将已有文档都迁移到新的要求吧
