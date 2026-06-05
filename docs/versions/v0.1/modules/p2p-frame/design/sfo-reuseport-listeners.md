---
module: p2p-frame
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-05-29
---

# sfo-reuseport listener 重构设计

## 目标
- `networks/tcp` 的 listener 使用 `sfo_reuseport::TcpServer` 接收入站 TCP stream。
- `networks/quic` 的 listener 使用 `sfo_reuseport::QuicServer::serve_socket(...)` 接收每个 worker 的 UDP socket，并通过 Quinn `AsyncUdpSocket` 适配器为每个 worker socket 创建一个 `quinn::Endpoint`。
- `ServerRuntime` 可由外部显式设置；未设置时 `p2p-frame` 创建默认 runtime。
- `TunnelNetwork::listen(...)` 改为接收入站 tunnel 回调并返回 `P2pResult<()>`；`TunnelNetwork` 不再导出 listener 对象或提供 `listeners()` 查询。
- 不新增 `NetworkServerRuntime`、socket factory trait 或 NAT 专用公共 `TunnelNetwork` 参数。

## 子模块职责
| 子模块 | 职责 | 新增责任 | 依赖 |
|--------|------|----------|------|
| `stack_runtime` | `P2pStackConfig` 和默认 stack 装配 | 持有可选 `sfo_reuseport::ServerRuntime`；构建默认 TCP/QUIC network 时传入 runtime | `sfo-reuseport` |
| `networks/tcp` | TCP listener、TLS accept、control/data 分流 | `TcpTunnelListener::bind` 注册 `TcpServer`；handler 把 `sfo_reuseport::TcpStream` 交给现有 accept 分流；内部 accept loop 调用入站 tunnel 回调 | `sfo_reuseport::TcpServer` |
| `networks/quic` | QUIC listener、Quinn endpoint、UDP punch | `QuicTunnelListener::bind` 注册 `QuicServer::serve_socket`；内部适配器实现 Quinn `AsyncUdpSocket`；每个 worker socket 对应一个 Quinn endpoint；endpoint accept loop 调用入站 tunnel 回调 | `sfo_reuseport::QuicServer`、`sfo_reuseport::UdpSocket`、`sfo_reuseport::QuicCidGenerator`、`quinn` |

## ServerRuntime 装配
- `P2pStackConfig` 增加 `server_runtime: Option<sfo_reuseport::ServerRuntime>`。
- 对外提供 `set_server_runtime(runtime)` 和只读访问入口。
- `open_p2p_stack` 组装默认 `TcpTunnelNetwork` / `QuicTunnelNetwork` 时：
  - 若配置提供 runtime，clone 该 runtime。
  - 若未提供 runtime，调用 `ServerRuntime::start(ServerRuntimeConfig::new())` 创建默认 runtime，并把同一个 runtime 传给 TCP 与 QUIC network。
- `ServerRuntime` 生命周期由持有它的 network/listener 保持；外部 runtime 的 shutdown 由外部 owner 决定。

## TCP listener flow
1. `TcpTunnelNetwork::listen(local, out, mapping_port, on_incoming_tunnel)` 创建 `TcpTunnelListener`，传入 `ServerRuntime` 和入站 tunnel 回调。
2. `TcpTunnelListener::bind(...)` 构造 `ServiceConfig::new(*local.addr())`，并把 `reuse_address` 映射到 `SocketOptions::reuse_address`。
3. 调用 `sfo_reuseport::TcpServer::serve(&runtime, config, handler)` 并保存返回的 `TcpServer` handle。
4. handler 收到 `sfo_reuseport::TcpStream` 后调用现有 TLS accept 逻辑：
   - 解析 local/remote endpoint。
   - 执行 `TlsAcceptor` 握手和 peer identity 校验。
   - 读取 `TcpConnectionHello`。
   - `Control` 进入 `on_control_connection`，成功后调用 `on_incoming_tunnel(Ok(tunnel))`。
   - `Data` 进入 `on_data_connection`，只绑定已有 tunnel。
5. `close()` 设置本地 closed 标记、调用 `TcpServer::close()`，并防止 handler 继续向当前 listener 的回调发布 tunnel。

该设计不改变 TCP tunnel frame、hello、ready、claim 或 data connection 协议。

## QUIC listener flow
1. `QuicTunnelNetwork::listen(local, out, mapping_port, on_incoming_tunnel)` 创建 `QuicTunnelListener`，传入 `ServerRuntime` 和入站 tunnel 回调。
2. `QuicTunnelListener::bind(...)` 调用 `sfo_reuseport::QuicServer::serve_socket(&runtime, config, callback)`。
3. callback 收到 `(socket, worker_id)` 后：
   - 若 listener 未关闭，构造 `Arc<SfoQuicUdpSocket>` 直接包裹该 worker socket。
   - 使用 `sfo_reuseport::QuicCidGenerator::for_worker(worker_id)` 创建 Quinn `ConnectionIdGenerator` 适配器，确保本 endpoint 生成的后续 CID 前 2 字节为对应 worker shard。
   - 创建一个 `quinn::Endpoint::new_with_abstract_socket(...)`：
   - `socket` 使用 `Arc<SfoQuicUdpSocket>`。
   - `runtime` 继续使用 `Arc<quinn::TokioRuntime>`。
   - `server_config` 沿用现有 TLS、transport 和 congestion 配置。
4. callback 把 endpoint 注册到 listener 的 endpoint 集合；第一个成功注册的 worker socket 同时保存为 UDP punch socket。
5. callback 内运行该 endpoint 的 `accept()` loop，后续 `accept_connection` 流程不变，所有 worker endpoint 的 accepted tunnel 调用同一个入站 tunnel 回调。
6. 主动 connect 从 endpoint 集合中选择一个 endpoint 发起；选择策略可为随机、轮询或第一个可用 endpoint，只要保持同一 QUIC listener 端口。
7. `close()` 设置 closed、调用 `QuicServer::close()`、关闭所有 Quinn endpoint，并清理 punch socket；关闭后不再调用入站 tunnel 回调。

## TunnelNetwork 回调契约
- `IncomingTunnelCallback` 或等价类型定义在 `networks/network.rs`，必须能被 TCP、QUIC、PN listener 持有并从异步任务中调用。
- 回调输入为 `P2pResult<TunnelRef>`。listener accept 到 tunnel 时传入 `Ok(tunnel)`；accept 过程中发生非终止性错误时可传入 `Err(err)` 让 `NetManager` 统一记录；listener 正常关闭不需要通过回调发送终止信号。
- `TunnelNetwork::listen(...)` 返回 `P2pResult<()>`。调用方不能再通过返回值取得 `TunnelListenerRef`，也不能通过 `listeners()` 轮询 listener。
- `listener_infos()` 继续是公共监听元数据入口，返回 local endpoint 和 mapping port。
- `NetManager::listen(...)` 为每个 endpoint 调用 network listen 时创建回调，回调复用原有 `dispatch_tunnel`、incoming validator、订阅发布、reject close 和错误日志逻辑。
- `NetManager::get_listener(...)` 与 `listener_entries(...)` 应移除或降为内部不可用路径；现有内部逻辑改为使用 `listener_infos()`。

## Quinn AsyncUdpSocket 适配器
`SfoQuicUdpSocket` 是 `networks/quic/listener.rs` 内部结构，职责如下：

| 方法 | 行为 |
|------|------|
| `poll_recv(cx, bufs, meta)` | 调用 worker `UdpSocket::poll_recv_from_vectored(cx, bufs)` 并填充 `RecvMeta { addr, len, stride, ecn: None, dst_ip: None }`；关闭或 socket 错误按 I/O error 返回。 |
| `try_send(transmit)` | 调用 worker `UdpSocket::try_send_to(...)` 发送到 `transmit.destination`；`WouldBlock` 保持 `WouldBlock`，其他错误透传给 Quinn；返回长度小于 datagram 长度时按 short send I/O error 处理。 |
| `local_addr()` | 返回 worker `UdpSocket::local_addr()`，该地址也是对应 endpoint 的 listener 本地端口来源。 |
| `create_io_poller()` | 返回调用 worker `UdpSocket::poll_send_ready(...)` 的 poller。 |
| `max_transmit_segments()` / `max_receive_segments()` | 返回 `1`，避免 GSO/GRO 语义错配。 |
| `may_fragment()` | 返回保守值，除非 `sfo-reuseport` socket 明确暴露等价能力。 |

## UDP punch 同源约束
- `QuicTunnelListener::start_udp_punch_burst(...)` 使用 listener 保存的第一个 worker `UdpSocket`。该 socket 与所有 worker socket 绑定同一 QUIC listener 本地端口，满足同源端口约束。
- punch policy、payload、cadence 和 deadline 完全沿用 `server_reflexive_quic_nat_keepalive` 与 NAT traversal design：
  - 只对 `ServerReflexive` QUIC 非 LAN IPv4、非 0 端口 candidate。
  - active `250ms` 起发，reverse `0ms` 起发。
  - 固定 `50ms` cadence，默认 `1s` 截止或被更短 window 裁剪。
  - payload 不解析、不确认、不上送。

## 关闭和错误语义
- TCP/QUIC listener close 必须先设置本地 closed 标记，再关闭 server handle，避免迟到 handler 调用入站 tunnel 回调。
- `serve_socket` callback 在 closed 后必须快速返回错误或 no-op。
- `ServerRuntime` drop 或 server close 导致的 accept/recv 结束，不应转化为正常 incoming tunnel。
- worker endpoint 注册失败时，该 worker callback 返回错误；若 bind 阶段没有任何 endpoint 或 punch socket 可用，应使 listen 失败。
- 后续 `try_send_to` 失败按 Quinn I/O error 处理。
- punch send 失败保持 best-effort，不改变 QUIC connect 结果。

## 实现顺序
1. 升级 `sfo-reuseport` 到提供 `TcpServer`、`QuicServer::serve_socket(...)`、`UdpSocket` Quinn helper 和 `QuicCidGenerator` 的版本。
2. 在 stack/network 构造中传递 `ServerRuntime`。
3. 引入 `IncomingTunnelCallback`，调整 `TunnelNetwork::listen(...)` 签名、移除公共 `listeners()`，并迁移 `NetManager`、stack、PN client 和测试调用点。
4. 重构 TCP listener bind/close 和 stream accept adapter，使其通过回调交付 accepted tunnel。
5. 实现 QUIC `SfoQuicUdpSocket`、worker CID generator 适配和 per-worker endpoint 注册。
6. 切换 QUIC listener bind/close、主动 connect endpoint 选择和 punch socket 来源，并通过回调交付 accepted tunnel。
7. 补齐 post-implementation tests。

## 回滚
- TCP 可回滚到旧 `bind_listener` + `TcpListener::accept()`，但必须保留 proposal/design 中的 `sfo-reuseport` 需求为未实现状态并退回 implementation。
- QUIC 可回滚到旧 `socket2` UDP socket + `quinn::Endpoint::new(...)`，但不得引入独立 punch socket 或 raw UDP tunnel。
- 若 `QuicCidGenerator` worker shard 与 `QuicServer` 路由规则无法保证同一 connection 稳定进入同一 worker endpoint，必须退回 design 重新定义 CID 和 endpoint 模型，而不是改变 NAT punch 或 QUIC tunnel 语义。
