---
task_manifest: task.yaml
status: approved
---

# nat_type 子模块设计

`NatProfile` 删除 `observed_endpoint` 字段。`from_observations` 仍生成分类和 hint；`is_fresh` 去除 endpoint 条件。`usable_prediction_hint` 与 `predicted_ports` 使用 `NatPredictionHint.last_observed` 作为预测锚点。

```rust
pub struct NatProfile {
    pub version: u8,
    pub observation: NatMappingObservation,
    pub observed_at: Timestamp,
    pub valid_until: Timestamp,
    pub prediction_hint: Option<NatPredictionHint>,
}
```

兼容决策：breaking，不保留旧 wire。
