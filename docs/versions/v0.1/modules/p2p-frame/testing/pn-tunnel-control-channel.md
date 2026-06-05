# PN Tunnel Control Channel 测试补充

本补充文档定义 `PnTunnel` tunnel 级控制通道的验证重点。它把 `design/pn-tunnel-control-channel.md` 中的 control open/ready、`Ping` / `Pong` / `Close`、heartbeat、远端关闭感知和统一 close path 落地为可运行断言。

## 范围

- active `PnTunnel` 创建时必须等待 control channel ready 成功后才返回可用 tunnel
- passive `PnTunnel` 由 control open 创建、注册并投递给 listener
- control open/ready 与业务 `ProxyOpenReq` / `ProxyOpenResp` 分离
- control `Close`、EOF、decode 失败、写失败和 heartbeat timeout 都关闭本地 tunnel
- control close、manual close 与 idle close 共享幂等关闭路径
- close 后后续 `open_*` 立即失败，pending `accept_*` 被唤醒
- close 后同一 `(remote_id, tunnel_id)` 的后续 inbound business open 不投递到旧对象

## 规范入口

- Unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`
- DV: 当前 disabled；`cyfs-p2p-test all-in-one` 不作为 p2p-frame 或其他模块的 DV 证据
- Integration: `python3 ./harness/scripts/test-run.py p2p-frame integration`

## Unit 覆盖矩阵

| 行为 | 建议测试名 | 预期断言 |
|------|------------|----------|
| active 等待 control ready | `pn_tunnel_create_waits_for_control_ready` | `create_tunnel*` 在收到 control ready success 前不返回可用 `PnTunnel` |
| control ready 失败 | `pn_tunnel_control_ready_failure_fails_create` | ready result 非 success 时创建失败，当前 `tunnel_id` 不被复用，registry 不保留半开 tunnel |
| passive 由 control open 创建 | `pn_tunnel_control_open_creates_passive_tunnel` | passive listener 收到 tunnel 前已保存 control channel，初始 business queued 计数为 0 |
| control 与业务 channel 分离 | `pn_tunnel_control_open_does_not_consume_business_accept` | control open 不出现在 `accept_stream()` / `accept_datagram()` 业务队列中，业务 `ProxyOpenReq` 不被解释为 control ready |
| remote close 关闭本地 tunnel | `pn_tunnel_remote_control_close_closes_local_tunnel` | 收到 `Close` 后本地 `is_closed()` 为 true，pending accept 被唤醒，后续 open 立即失败 |
| control EOF 关闭本地 tunnel | `pn_tunnel_control_eof_closes_local_tunnel` | control read loop 读到 EOF 后进入 closed/error 终态 |
| control decode 失败关闭本地 tunnel | `pn_tunnel_invalid_control_frame_closes_tunnel` | 无效 control frame 或未知 control command 触发 protocol/error close |
| control write 失败关闭本地 tunnel | `pn_tunnel_control_send_failure_closes_tunnel` | 发送 `Ping` / `Pong` / `Close` 失败后 tunnel 关闭且 close path 幂等 |
| heartbeat timeout 关闭本地 tunnel | `pn_tunnel_control_heartbeat_timeout_closes_tunnel` | 测试短 timeout 下未收到控制面流量或匹配 `Pong` 时关闭 tunnel |
| manual close 通知对端 | `pn_tunnel_manual_close_sends_control_close` | 本地 `close()` 尽力发送 `Close`，对端收到后关闭；发送失败不阻塞本地 close |
| control close 与 idle close 幂等 | `pn_tunnel_control_close_is_idempotent_with_idle_close` | remote close、idle close、manual close 任意重复组合不重复 drain、不重复递减计数、不重新打开 |
| close 后 registry 清理 | `pn_tunnel_control_close_unregisters_tunnel` | close 后 `PnShared` live registry 不返回旧对象；后续同 key inbound open 走重新创建路径 |

## DV 与 Integration 继承

- DV 当前 disabled；默认 `PnClient` / `PnServer` 构造路径不能依赖 `cyfs-p2p-test all-in-one` 作为自动 DV 证据。
- heartbeat 精确时序由 unit 使用短 timeout 或注入点覆盖。
- Integration 使用 workspace 测试验证新增 PN protocol 命令和 `PnTunnel` ready gate 不要求下游修改通用 `Tunnel` / `TunnelNetwork` trait 调用签名。

## 完成定义

- control ready gate、ready 失败、passive control open 创建都有 unit 断言。
- remote close、control EOF、decode 失败、write 失败和 heartbeat timeout 都有 unit 断言。
- manual close 通知对端和发送失败本地收敛都有 unit 断言。
- control close、manual close、idle close 的幂等性和 registry 清理都有 unit 断言。
- unit/integration 证明默认构造路径和下游调用签名兼容。
