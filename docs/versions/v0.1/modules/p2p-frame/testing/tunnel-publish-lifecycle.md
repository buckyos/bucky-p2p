# TunnelManager Tunnel 发布生命周期测试补充

本补充文档把 [docs/versions/v0.1/modules/p2p-frame/design/tunnel-publish-lifecycle.md](/mnt/f/work/p2p/docs/versions/v0.1/modules/p2p-frame/design/tunnel-publish-lifecycle.md) 中定义的 `TunnelManager` register/publish 生命周期，落地为 `p2p-frame/src/tunnel/tunnel_manager.rs` 的 unit 验证要求。

## 范围

### 范围内
- 新建 direct、proxy、普通 incoming 和 reverse incoming tunnel 的 register/publish 路径
- `reverse_waiter` 命中时的延后 publish 行为
- reverse waiter 被消费后的可见性转换，以及无 waiter reverse incoming close
- 已 published candidate 与 hidden reverse candidate 的选择优先级
- 已 published 非 proxy candidate 与 published proxy candidate 同时存在时的选择优先级
- proxy tunnel 进入候选管理并触发后续脱代理升级后的 publish 行为

### 范围外
- `StreamManager` / `DatagramManager` 的 accept-loop 行为本身
- transport 握手细节和 PN/TLS 协议语义
- 运行时级的 all-in-one 场景时序验证

## Unit 验证矩阵

| 行为 | 建议或现有测试 | 预期结果 |
|------|----------------|----------|
| 非 reverse candidate register 后立即 publish | 现有 `register_tunnel_publishes_all_non_reverse_candidates` | active/passive 非 reverse tunnel 一旦登记成功，就会进入订阅流，且被标记为 published |
| reverse incoming 命中 waiter 时只交付 | 现有 `incoming_tunnel_notifies_reverse_waiter` | tunnel 不登记、不出现在订阅流，waiter 能拿到 tunnel |
| reverse waiter 接收方完成 register/publish | `reverse_tunnel_is_published_after_waiter_completion` | waiter 返回的 reverse tunnel 由接收方 register 后进入订阅流，且后续 `get_tunnel()` 优先返回 published candidate |
| reverse incoming 在本地无 waiter 时关闭 | `reverse_without_waiter_closes_without_publish`、`reverse_without_waiter_closes_fresh_tunnel_id_without_x509` | tunnel 被 close，不进入候选表，不进入订阅流，`get_tunnel()` 不返回 |
| reverse incoming 在 waiter 已被消费后再次到达 | `reverse_incoming_tunnel_closes_without_waiter_after_waiter_consumed` | 后续 reverse tunnel 被 close；先前命中 waiter 的 tunnel 仍未登记，直到 reverse open 完成路径 register/publish |
| published candidate 优先于 hidden reverse candidate | 现有 `get_tunnel_prefers_published_candidate_over_hidden_reverse` | `get_tunnel()` 默认优先返回已 published candidate |
| published 非 proxy candidate 优先于 published proxy candidate | 现有 `select_preferred_tunnel_entry_prefers_non_proxy_over_newer_proxy`；`x509` feature 下现有 `get_tunnel_prefers_non_proxy_over_newer_proxy` | 同一远端已有 direct/passive/reverse 等非 proxy candidate 时，即使后续 proxy candidate 更新时间更晚，`get_tunnel()` 仍返回非 proxy；只有没有可用非 proxy candidate 时才返回 proxy |
| publish 入口幂等 | 待补统一 publish 入口幂等测试 | 同一 candidate 重复提升为 published 时，不重复广播，也不破坏状态 |
| proxy candidate 发布后触发后续脱代理升级 | 现有 proxy upgrade 相关测试集 | proxy 作为立即可用候选被 publish；升级成功后新的 non-proxy candidate 进入同一 publish 路径 |
| proxy 升级失败退避封顶 | 现有 proxy upgrade backoff 相关测试集 | 重试间隔指数增长但不超过 2 小时 |

## reverse waiter 专项验证

| 场景 | 预期结果 | 验证层级 |
|------|----------|----------|
| reverse open 等待期间收到匹配 incoming tunnel | waiter 拿到 tunnel；订阅者在 waiter 释放前看不到该 tunnel | unit |
| waiter 正常完成后再提升为 published | tunnel 通过统一 publish 入口进入订阅流 | unit |
| waiter 超时或显式取消后，再到达任意 reverse tunnel | 只要没有同 `(remote_id, tunnel_id)` waiter，就直接关闭，不 register、不 publish、不进入订阅流 | unit |
| hedged direct/reverse 竞争中 reverse 分支被取消 | 不遗留悬空 waiter；后续同 key reverse tunnel 不会被错误隐藏 | unit |

## 现有测试映射

- [p2p-frame/src/tunnel/tunnel_manager.rs](/mnt/f/work/p2p/p2p-frame/src/tunnel/tunnel_manager.rs) 中的 `incoming_tunnel_notifies_reverse_waiter` 继续承担“命中 waiter 时只交付、不登记、不发布”的核心证据。
- 同文件中的 `register_local_reverse_tunnel_without_waiter_is_published` 只覆盖内部注册入口 `register_tunnel_and_publish(...)`，不代表 incoming reverse 无 waiter 可发布。
- 新增 `reverse_without_waiter_closes_without_publish` 和 `reverse_without_waiter_closes_fresh_tunnel_id_without_x509` 承担“incoming reverse 无 waiter 关闭且不发布”的默认 unit 证据。
- 新增 `reverse_incoming_tunnel_closes_without_waiter_after_waiter_consumed` 承担“waiter 被消费后，后续 reverse incoming 因无 waiter 被关闭”的 `x509` feature 证据。
- 同文件中的 `get_tunnel_prefers_published_candidate_over_hidden_reverse` 继续承担 published candidate 选择优先级的证据。
- 同文件中的 `select_preferred_tunnel_entry_prefers_non_proxy_over_newer_proxy` 继续承担默认 unit 入口下“非 proxy 优先于更新时间更晚的 proxy”的选择策略证据；`x509` feature 下的 `get_tunnel_prefers_non_proxy_over_newer_proxy` 继续承担完整 `get_tunnel()` 路径证据。
- 同文件中的 proxy upgrade 测试继续承担 proxy candidate 可见性、升级成功后切换 publish 路径，以及失败退避封顶的证据。

## 缺口

- 需要继续保持一条直接 unit 测试覆盖“waiter 接收方拿到 reverse tunnel 后先 register 再通过统一 publish 入口变为 published”。
- 需要一条 unit 测试覆盖“统一 publish 入口的幂等性”，避免实现收敛后重复广播。
- 需要一条 unit 测试覆盖“hedged direct/reverse 竞争导致 reverse future 被取消时 waiter 会被清理，并让后续无 waiter reverse 被关闭”，防止收敛 publish 逻辑后引入隐藏 candidate 泄漏或失败后发布。
