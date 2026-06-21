# Admission Evidence: cyfs-p2p-test document exemption admission

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | docs/versions/v0.1/modules/workspace-harness/proposal.md | pass | Read workspace_harness_direct_admission_gate coverage for schema/admission/check-implementation behavior and explicit module exemptions. |
| design_read | docs/versions/v0.1/modules/workspace-harness/design.md | pass | Read workspace_harness_direct_admission_gate design scope including harness/scripts/admission-check.py and module-doc-exception rules. |
| change_scope_matches_request | proposal P-HARNESS-ADMISSION-1 / design workspace_harness_direct_admission_gate | pass | The task fixes admission-check behavior so explicit document_exempt_modules are honored before the cyfs-p2p-test harness implementation proceeds. |
| active_module_resolved | docs/versions/v0.1/modules/workspace-harness | pass | Active module is workspace-harness because the first required edit is the governance admission script. |
| no_chat_only_evidence | versioned docs and governance yaml | pass | Admission relies on approved workspace-harness mapping plus harness/workspace-governance.yaml and module-doc-exception-rules.md, not chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/workspace-harness/proposal.md | fdf702240fbbf4f8712f80df2a94adfce2a16c350531593391576fe465c84996 |
| docs/versions/v0.1/modules/workspace-harness/design.md | 63773a33db279ca2f766fe88065a4525af2fd7bcedd8420a643e909d7f1e5130 |

## Coverage Quotes
### Quote: proposal.md workspace_harness_direct_admission_gate
> | P-HARNESS-ADMISSION-1 | workspace_harness_direct_admission_gate | task entry、schema-check 和 admission-check 形成直接 change_id 准入门禁，缺失 proposal/design/testing/testplan 映射时失败关闭。 | 不为默认模块提供隐式豁免；仅允许规则文件中显式登记的模块豁免。 | schema/admission/check-implementation 入口能对默认模块和豁免模块给出一致结果。 |

### Quote: design.md workspace_harness_direct_admission_gate
> | workspace_harness_direct_admission_gate | P-HARNESS-ADMISSION-1 | schema/admission/check-implementation 入口按 `version`、`module`、`change_id`、审批状态和 testplan 映射失败关闭；模块豁免只允许显式登记。 | `harness/rules/task-entry-gate-rules.md`、`harness/rules/direct-change-mapping-rules.md`、`harness/rules/module-doc-exception-rules.md`、`harness/rules/schema-validation-rules.md`、`harness/scripts/schema-check.py`、`harness/scripts/admission-check.py`、`harness/scripts/verify-workspace-harness.py` | 若 legacy 数据包被过度阻塞，应在规则中显式记录窄豁免，而不是在脚本中静默通过。 |
