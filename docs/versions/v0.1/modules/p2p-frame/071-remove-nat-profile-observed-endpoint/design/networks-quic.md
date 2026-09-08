---
task_manifest: task.yaml
status: approved
---

# networks/quic 子模块设计

`QuicTunnelListener::probe_nat_profile` 不再把 observed endpoint 放入 `NatProfile`。`predict_traversal_endpoints` 现场探测后从同一轮 `NatPredictionHint.last_observed` 推导预测端口；非对称 profile 无法预测时返回 `NotFound`。

```rust
let profile = listener.probe_nat_profile(...).await?;
let base = profile.prediction_hint.as_ref().map(|h| h.last_observed);
// base 必须来自本轮 probe，否则 prediction 失败。
```

兼容决策：breaking；`NatProfile` wire 不再携带 endpoint。
