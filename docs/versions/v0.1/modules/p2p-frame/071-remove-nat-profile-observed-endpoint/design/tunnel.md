---
task_manifest: task.yaml
status: approved
---

# tunnel 子模块设计

`nat_candidates(Predicted)` 不再使用缓存 remote profile/hint 展开候选；需要预测时必须先获得 live `predict_traversal_endpoints` 结果。若预测结果不可用则 fail-closed，不得回到缓存 profile 展开。

```rust
// Predicted 候选只来自 live prediction endpoints；
// Base 候选继续来自 remote_endpoints。
```

兼容决策：不影响 `NatCandidateMode::Base`；`Predicted` 行为从缓存 fallback 改为 live-only。
