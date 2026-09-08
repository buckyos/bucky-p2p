---
task_manifest: task.yaml
status: approved
---

# SN service 子模块设计

scheduler、peer manager 和 query/called 路径继续发布/消费 `NatProfile`，但不再有 observed endpoint 字段。freshness 只由 `observed_at`/`valid_until` 判断；缓存 profile 仍作为分类/hint 参与 plan 选择，但不参与预测候选展开。

```rust
current_profile(peer_id, now).map(|p| p.observation)
```

兼容决策：breaking；旧 profile blob 不被新 decoder 接受。
