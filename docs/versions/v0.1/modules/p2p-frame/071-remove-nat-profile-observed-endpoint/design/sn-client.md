---
task_manifest: task.yaml
status: approved
---

# SN client 子模块设计

客户端本地 probe 结果仍以 `NatProfile` 保存和上报，但 profile 不再包含 observed endpoint。`ActiveSN.net_profile` 只记录分类、hint 和时效。

```rust
report(..., Some(&profile), Some(&result))
```

上报 wire 使用新 `NatProfile` 编码；不保留旧 blob 兼容。
