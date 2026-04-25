# TunnelManager Tunnel 发布生命周期测试补充

本补充文档把 [docs/versions/v0.1/modules/p2p-frame/design/tunnel-publish-lifecycle.md](/mnt/f/work/p2p/docs/versions/v0.1/modules/p2p-frame/design/tunnel-publish-lifecycle.md) 中定义的 `TunnelManager` register/publish 生命周期，落地为 `p2p-frame/src/tunnel/tunnel_manager.rs` 的 unit 验证要求。

## 范围

### 范围内
- 新建 direct、proxy、普通 incoming 和 reverse incoming tunnel 的 register/publish 路径
- `reverse_waiter` 命中时的延后 publish 行为
- reverse waiter 被消费、超时、取消或失败后的可见性转换
- 已 published candidate 与 hidden reverse candidate 的选择优先级
- proxy tunnel 进入候选管理并触发后续脱代理升级后的 publish 行为

### 范围外
- `StreamManager` / `DatagramManager` 的 accept-loop 行为本身
- transport 握手细节和 PN/TLS 协议语义
- 运行时级的 all-in-one 场景时序验证

## Unit 验证矩阵

| 行为 | 建议或现有测试 | 预期结果 |
|------|----------------|----------|
| 非 reverse candidate register 后立即 publish | 现有 `register_tunnel_publishes_all_non_reverse_candidates` | active/passive 非 reverse tunnel 一旦登记成功，就会进入订阅流，且被标记为 published |
| reverse incoming 命中 waiter 时先隐藏 | 现有 `incoming_tunnel_notifies_reverse_waiter` | tunnel 已登记但不会出现在订阅流；waiter 能拿到 tunnel |
| reverse waiter 被消费后，使用统一入口把 tunnel 提升为 published | 待补 reverse open 完成路径直测 | waiter 返回的 reverse tunnel 随后会进入订阅流，且后续 `get_tunnel()` 优先返回 published candidate |
| reverse incoming 在本地无 waiter 时立即 publish | 现有 `register_local_reverse_tunnel_without_waiter_is_published`、`reverse_incoming_tunnel_publishes_after_waiter_consumed` 的第二阶段 | reverse tunnel 不再长期隐藏，而是按普通新 tunnel 进入 publish 流程 |
| published candidate 优先于 hidden reverse candidate | 现有 `get_tunnel_prefers_published_candidate_over_hidden_reverse` | `get_tunnel()` 默认优先返回已 published candidate |
| publish 入口幂等 | 待补统一 publish 入口幂等测试 | 同一 candidate 重复提升为 published 时，不重复广播，也不破坏状态 |
| proxy candidate 发布后触发后续脱代理升级 | 现有 proxy upgrade 相关测试集 | proxy 作为立即可用候选被 publish；升级成功后新的 non-proxy candidate 进入同一 publish 路径 |
| proxy 升级失败退避封顶 | 现有 proxy upgrade backoff 相关测试集 | 重试间隔指数增长但不超过 2 小时 |

## reverse waiter 专项验证

| 场景 | 预期结果 | 验证层级 |
|------|----------|----------|
| reverse open 等待期间收到匹配 incoming tunnel | waiter 拿到 tunnel；订阅者在 waiter 释放前看不到该 tunnel | unit |
| waiter 正常完成后再提升为 published | tunnel 通过统一 publish 入口进入订阅流 | unit |
| waiter 超时或显式取消后，再到达同 `(remote_id, tunnel_id)` reverse tunnel | 不再隐藏，直接按普通新 tunnel publish | unit |
| hedged direct/reverse 竞争中 reverse 分支被取消 | 不遗留悬空 waiter；后续同 key reverse tunnel 不会被错误隐藏 | unit |

## 现有测试映射

- [p2p-frame/src/tunnel/tunnel_manager.rs](/mnt/f/work/p2p/p2p-frame/src/tunnel/tunnel_manager.rs) 中的 `incoming_tunnel_notifies_reverse_waiter` 继续承担“命中 waiter 时先隐藏”的核心证据。
- 同文件中的 `register_local_reverse_tunnel_without_waiter_is_published` 继续承担“无 waiter 时 reverse 立即 publish”的证据。
- 同文件中的 `reverse_incoming_tunnel_publishes_after_waiter_consumed` 继续承担“waiter 消费后，后续 reverse candidate 不再隐藏”的证据。
- 同文件中的 `get_tunnel_prefers_published_candidate_over_hidden_reverse` 继续承担 published candidate 选择优先级的证据。
- 同文件中的 proxy upgrade 测试继续承担 proxy candidate 可见性、升级成功后切换 publish 路径，以及失败退避封顶的证据。

## 缺口

- 需要一条更直接的 unit 测试覆盖“`open_reverse_path()` 成功返回后，命中的 reverse tunnel 会通过统一 publish 入口变为 published”，而不是只靠第二个 incoming reverse candidate 间接证明。
- 需要一条 unit 测试覆盖“统一 publish 入口的幂等性”，避免实现收敛后重复广播。
- 需要一条 unit 测试覆盖“hedged direct/reverse 竞争导致 reverse future 被取消时 waiter 会被清理”，防止收敛 publish 逻辑后引入隐藏 candidate 泄漏。
