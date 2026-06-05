---
module: p2p-frame
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-05-29
---

# sfo-reuseport Listener 验证补充

## 覆盖目标
- `networks_sfo_reuseport_tcp_listener`：TCP listener 必须由 `sfo_reuseport::TcpServer` 负责 bind、worker accept 和 close，`TcpTunnelListener` 只保留协议握手和 tunnel 分发逻辑。
- `networks_sfo_reuseport_quic_listener_socket`：QUIC listener 必须由 `sfo_reuseport::QuicServer::serve_socket(...)` 负责 bind、QUIC packet routing 和 worker socket 交付；Quinn 通过每个 worker 一个自定义 `AsyncUdpSocket` / `Endpoint` 消费该 worker socket。
- `tunnel_network_listen_callback`：`TunnelNetwork::listen(...)` 必须接收入站 tunnel 回调并返回 `P2pResult<()>`，公共 trait 不再返回或查询 `TunnelListenerRef`；`NetManager` 通过回调继续执行 incoming validator、订阅发布和 reject close。
- `P2pConfig::set_server_runtime(...)` 必须允许外部注入 `sfo_reuseport::ServerRuntime`；未注入时 stack 创建默认 runtime 并共享给 TCP/QUIC network。

## 验证入口
| validation_id | change_id | 命令 | 覆盖断言 |
|---------------|-----------|------|----------|
| V-SFO-TCP-LISTENER-UNIT | networks_sfo_reuseport_tcp_listener | `python3 ./harness/scripts/test-run.py p2p-frame unit` | crate unit 编译和运行覆盖 TCP network/listener 构造、stack runtime 注入路径和现有 `TunnelNetwork` 调用方兼容性。 |
| V-SFO-QUIC-LISTENER-UNIT | networks_sfo_reuseport_quic_listener_socket | `python3 ./harness/scripts/test-run.py p2p-frame unit` | crate unit 编译和运行覆盖 QUIC network/listener 构造、自定义 Quinn `AsyncUdpSocket` trait 实现、`serve_socket` worker callback、per-worker CID generator、`UdpSocket` Quinn helper 和 UDP punch 发送 helper。 |
| V-TUNNEL-NETWORK-CALLBACK-UNIT | tunnel_network_listen_callback | `python3 ./harness/scripts/test-run.py p2p-frame unit` | crate unit 编译和运行覆盖 `TunnelNetwork::listen(...)` 回调签名、`NetManager::listen(...)` 回调分发、validator reject close、订阅发布、PN listener 幂等 listen，以及公共 trait 移除 `listeners()` 后的调用点迁移。 |
| V-SFO-LISTENER-CHECK | networks_sfo_reuseport_tcp_listener, networks_sfo_reuseport_quic_listener_socket | `cargo check -p p2p-frame` | 验证生产代码和测试外调用方在当前 feature 组合下均满足类型约束。 |
| V-TUNNEL-NETWORK-CALLBACK-CHECK | tunnel_network_listen_callback | `cargo check -p p2p-frame` | 验证生产代码和测试外调用方不再依赖 `TunnelNetwork::listeners()` 或 `listen(...) -> TunnelListenerRef`。 |

## 回归关注
- `TcpServer` 当前不暴露 TCP listener socket；当配置端口为 `0` 时，listener 在交给 `TcpServer` 前先分配具体端口，以维持 `listener_infos()` 返回非零实际端口的既有调用语义。
- QUIC `poll_recv` 必须只向 Quinn 暴露 QUIC packet；非 QUIC UDP punch payload 不进入业务解析路径。
- `try_send` 使用当前 worker `UdpSocket::try_send_to(...)`；发送失败必须作为 I/O 错误返回给 Quinn，UDP punch 仍按既有 best-effort 处理。
- per-worker endpoint 的 CID generator 必须使用对应 worker shard；主动 connect 可选择任一 endpoint，UDP punch 使用第一个可用 worker socket且只要求同源端口一致。
- listener 回调化后，关闭或 accept 错误不得绕过 `NetManager` 原有错误处理；关闭后的迟到入站 tunnel 不得继续调用外部回调。
