# PN Tunnel Idle Close 测试补充

本补充文档定义 `PnTunnel` idle timeout 生命周期关闭的验证重点。它把 [docs/versions/v0.1/modules/p2p-frame/design/pn-tunnel-idle-close.md](/mnt/f/work/p2p/docs/versions/v0.1/modules/p2p-frame/design/pn-tunnel-idle-close.md) 中的状态机、channel lease、idle sweeper 和关闭后重新创建语义落地为可运行断言。

## 范围

- channel 数量归零后持续超过 idle timeout，`PnTunnel` 进入 closed/error 终态
- idle close 与手动 `close()` 共享本地效果，包括 close 幂等、accept 唤醒和后续 open 立即失败
- active、pending、queued channel 全部纳入 idle 判定
- 已返回给上层的 stream/datagram 通过 lease wrapper 正确归还 active 计数
- idle close 后，同一 `(remote_id, tunnel_id)` 的后续 inbound open 必须创建新的 passive tunnel
- 后续 inbound open 不得投递到已关闭对象，也不得等待 `PN_OPEN_TIMEOUT`

## 规范入口

- Unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`
- DV: `python3 ./harness/scripts/test-run.py p2p-frame dv`
- Integration: `python3 ./harness/scripts/test-run.py p2p-frame integration`

## Unit 覆盖矩阵

| 行为 | 建议测试名 | 预期断言 |
|------|------------|----------|
| 空闲超过 timeout 触发关闭 | `pn_tunnel_idle_timeout_closes_when_no_channels_remain` | 所有计数为 0 且超过测试 timeout 后，`state()` 为 `Closed` 或等价错误终态，`is_closed()` 为 true |
| active channel 未归零不触发 idle close | `pn_tunnel_idle_timeout_waits_for_active_channel_drop` | 已返回给上层的 stream/datagram lease 未 drop 前，即使超过 timeout 也不关闭；最后一个 lease drop 后重新开始 idle 计时 |
| pending open 计入活动 | `pn_tunnel_idle_timeout_does_not_close_pending_open` | open 握手未完成或 TLS wrapper 未完成时，sweeper 不关闭 tunnel；失败或成功后按 pending 归还/转 active |
| queued inbound 计入活动 | `pn_tunnel_idle_timeout_does_not_close_queued_inbound_channel` | `ProxyOpenReq` 已入 inbound queue 但尚未 accept 时，idle sweeper 不关闭 tunnel |
| idle close 唤醒 accept | `pn_tunnel_idle_close_wakes_pending_accept` | `accept_stream()` / `accept_datagram()` 在无 channel 等待时，idle close 后立即返回 `Interrupted` 或等价关闭错误 |
| idle close 后 open 立即失败 | `pn_tunnel_idle_close_rejects_later_open` | 同一 `PnTunnel` 上后续 `open_stream()` / `open_datagram()` 不发起 PN open，也不等待 `PN_OPEN_TIMEOUT`，直接返回关闭错误 |
| close 幂等 | `pn_tunnel_idle_close_is_idempotent_with_manual_close` | idle close 与手动 close 任意顺序重复调用，都不会重复递减计数、panic 或重新打开状态 |
| 关闭后重新创建 | `pn_tunnel_idle_close_allows_recreate_for_same_key` | idle close 后，同 `(remote_id, tunnel_id)` 的后续 inbound open 创建新的 passive tunnel，而不是投递到旧对象 |
| 新对象继续接收同 key channel | `pn_tunnel_recreated_tunnel_receives_followup_channels` | 重新创建后的 tunnel 被注册到 registry，后续同 key channel 投递到新对象的队列 |
| TLS wrapper 不绕过 lease | `pn_tunnel_idle_close_tracks_tls_stream_lease` | `TlsRequired` stream 交给上层后，TLS wrapper 存活期间 active 计数不归零；drop 后才允许 idle close |
| datagram wrapper 归还 lease | `pn_tunnel_idle_close_tracks_datagram_lease` | `open_datagram()` / `accept_datagram()` 返回的 write/read drop 后正确归还 active 计数 |

## 失败覆盖

| 条件 | 预期结果 |
|------|----------|
| idle sweeper 与 inbound open 并发 | 要么 inbound 在 `Open` 状态内成功入队并阻止 idle close，要么 close 先赢并使 inbound 走重新创建路径；不得出现 closed 后入队 |
| idle sweeper 与 local open 并发 | 要么 local open 在 `Open` 状态内登记 pending 并阻止 idle close，要么 close 先赢且 local open 立即失败 |
| queued channel 在 close 时被 drain | 已排队但未 accept 的 channel 尽力收到失败 `ProxyOpenResp`，opener 不应依赖 5 秒 timeout 才失败 |
| lease drop 多次发生 | active 计数只递减一次，不得下溢 |
| 已关闭 tunnel 仍在 registry 中 | `get_tunnel()` 必须清理旧 entry 并返回 `None`，让 listener 创建新的 passive `PnTunnel` |

## DV 与 Integration 继承

- DV 仍通过 all-in-one 场景验证 PN client/server 与 stack 组合路径没有被 idle close 配置破坏。默认 30 分钟 idle timeout 不应影响短时 DV 场景。
- Integration 仍通过 workspace 测试确认新增 PN client 内部状态与 wrapper 不破坏下游编译和既有测试。
- idle close 的精确时间、竞态和重新创建行为必须由 unit 测试承担，不依赖 DV 的长时间等待。

## 完成定义

- idle timeout 触发关闭、accept 唤醒、后续 open 立即失败都有 unit 断言。
- active、pending、queued 三类 channel 都被证明会阻止 idle close。
- stream、datagram、TLS wrapper 的 lease drop 都被证明会归还计数。
- 关闭后同 key 重新创建以及后续同 key channel 投递到新对象都有 unit 断言。
- 测试使用短 timeout 或可控时钟，不把默认 30 分钟等待带入测试运行。
