---
task_manifest: task.yaml
status: approved
---

# tests 子模块设计

调整所有直接构造/断言 `NatProfile.observed_endpoint` 的单元、wire、scheduler、inter-SN 与策略矩阵测试。新增验证：无 profile endpoint 时 freshness 仍正确；fallback `Predicted` 不使用缓存 hint；live prediction 端点和 hint last_observed 必须来自同一次探测。

兼容决策：测试与 production 同步迁移到新结构。
