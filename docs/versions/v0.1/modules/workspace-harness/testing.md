---
module: workspace-harness
version: v0.1
status: approved
approved_by: user
approved_at: 2026-05-13
---

# workspace-harness 测试

## 统一测试入口
- 机器可读计划：`docs/versions/v0.1/modules/workspace-harness/testplan.yaml`
- Unit: `python3 ./harness/scripts/verify-workspace-harness.py v0.1`
- DV: `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`
- Integration: `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive`

## 验证矩阵
| 行为 | 入口 | 通过标准 |
|------|------|----------|
| 工作区治理结构完整 | `verify-workspace-harness.py v0.1` | 必需规则、脚本、架构文档和治理元数据路径存在 |
| schema 结构检查 | `schema-check.py --version v0.1 --module p2p-frame` | 已批准模块数据包结构、审批元数据和 testplan 结构可被校验 |
| implementation admission 失败关闭 | `admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive` | 在 p2p-frame design/testing approved 后通过；缺少审批或映射时失败 |
| 跨模块未审批失败关闭 | `admission-check.py --version v0.1 --module cyfs-p2p --change-id endpoint_area_server_reflexive` | 在 cyfs-p2p design/testing 未审批时失败；审批完成后通过 |

## Direct Change Coverage
| change_id | validation_id | testplan_level | testplan_step_id | Coverage | gap | gap_manual_reason |
|-----------|---------------|----------------|------------------|----------|-----|-------------------|
| workspace_harness_acceptance_review_gate | V-HARNESS-STRUCTURE | unit | workspace-harness-structure | 验证 acceptance review 相关规则文件被 workspace verifier 纳入必需路径。 | no | |
| workspace_harness_direct_admission_gate | V-HARNESS-SCHEMA | dv | workspace-harness-schema-p2p-frame | 验证 schema checker 能识别 p2p-frame 数据包结构和 approval/testplan 元数据。 | no | |
| workspace_harness_direct_admission_gate | V-HARNESS-ADMISSION-P2P | integration | workspace-harness-admission-p2p-frame | 验证 p2p-frame 的 `endpoint_area_server_reflexive` 可在审批和映射齐备后通过 admission。 | no | |
| workspace_harness_direct_admission_gate | V-HARNESS-ADMISSION-CYFS | integration | workspace-harness-admission-cyfs-p2p | 验证 cyfs-p2p 的同名 change 在 design/testing 未审批时失败关闭。 | no | |
| workspace_harness_pipeline_plan_current_change | V-HARNESS-PIPELINE | unit | workspace-harness-structure | 通过结构检查和人工评审确认 pipeline plan 对当前 change_id、阶段图和退回规则保持一致。 | no | |

## 完成定义
- [ ] workspace verifier 通过。
- [ ] schema checker 对代表模块通过。
- [ ] admission checker 对已审批路径通过，对未审批路径失败关闭。
- [ ] pipeline plan 与当前 change_id 和审批状态一致。
