# Implementation Admission: Stack Server Runtime Required

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal read; stack-level `ServerRuntime` injection and default runtime creation are covered by the TCP and QUIC `sfo-reuseport` listener change items. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Approved design read; both mapped listener change items include `p2p-frame/src/stack.rs` in Scope Paths and define `P2pStackConfig` / stack runtime assembly behavior. |
| change_scope_matches_request | proposal `P-SFO-TCP-LISTENER-1` / `P-SFO-QUIC-LISTENER-1` and design `networks_sfo_reuseport_tcp_listener` / `networks_sfo_reuseport_quic_listener_socket` | pass | The request targets `p2p-frame/src/stack.rs` `server_runtime` type, which is the stack-level runtime configuration used by TCP and QUIC listener construction. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | `p2p-frame/src/stack.rs` belongs to the approved `p2p-frame` module packet. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved versioned documents; the user request selects the concrete stack field to adjust but is not used as coverage evidence. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 0d44a287897110fd067f277fb96fdc28eaa79347864ff3c5634c97337ee0635a |
| docs/versions/v0.1/modules/p2p-frame/design.md | b5687d64b861287f47249d5103dc7bc6457c6196ff3553b9f4b795624a249a42 |

## Coverage Quotes

### Quote: proposal.md networks_sfo_reuseport_tcp_listener
> | P-SFO-TCP-LISTENER-1 | networks_sfo_reuseport_tcp_listener | TCP tunnel listener 基于 `sfo_reuseport::TcpServer` 重构，入站 `sfo_reuseport::TcpStream` 接入现有 TLS accept、TCP control/data connection 分流、registry 和 tunnel publish 流程；`ServerRuntime` 可由外部显式设置，默认路径仍自动创建。 | 不新增 `NetworkServerRuntime` 或 socket factory trait；不改变 TCP tunnel 线协议、TLS 身份校验或 tunnel publish 语义；不把 close 后的旧服务继续暴露为当前 listener 的入站 tunnel。 | unit 能覆盖外部 `ServerRuntime` 被 TCP listener 使用、默认 runtime 可用、`TcpServer` handler 的 stream 进入现有 control/data 分流路径、listener close 后不再发布新 tunnel；integration 能覆盖 TCP tunnel 仍可建立并传输。 |

### Quote: design.md networks_sfo_reuseport_tcp_listener
> | networks_sfo_reuseport_tcp_listener | P-SFO-TCP-LISTENER-1 | TCP listener 使用 `sfo_reuseport::TcpServer` 注册服务，handler 接收 `sfo_reuseport::TcpStream` 后调用现有 TLS accept 与 TCP control/data 分流；`TcpTunnelListener` 保存 `TcpServer` handle 并在 close 时停止服务；`P2pStackConfig` 可显式注入 `sfo_reuseport::ServerRuntime`，默认路径自动创建 runtime。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/tcp/listener.rs`、`p2p-frame/src/networks/tcp/connection.rs`、`p2p-frame/src/networks/tcp/network.rs`、`docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md` | 回滚时恢复本地 `TcpListener` bind/accept 路径和旧 `reuse_address` 设置，但不得改变 TCP tunnel 线协议或 publish 语义。 |

### Quote: proposal.md networks_sfo_reuseport_quic_listener_socket
> | P-SFO-QUIC-LISTENER-1 | networks_sfo_reuseport_quic_listener_socket | QUIC tunnel listener 基于 `sfo_reuseport::QuicServer::serve_socket(...)` 重构，使用内部 Quinn `AsyncUdpSocket` 适配器把每个 worker `sfo_reuseport::UdpSocket` 交给对应 `quinn::Endpoint`，并使用 worker-shard CID generator 保持连接路由稳定；主动 connect 可选择任一 worker endpoint，同源 UDP punch 使用首个可用 worker socket；`ServerRuntime` 可由外部显式设置，默认路径仍自动创建。 | 不新增 raw UDP tunnel、业务载荷解析、公共 `TunnelNetwork` NAT 参数或 `NetworkServerRuntime`；不改变 QUIC tunnel 线协议、TLS 身份校验、NAT punch candidate policy、50ms cadence、active/reverse 起发时机、截止规则或 heartbeat 语义。 | unit 能覆盖 worker socket `try_send_to` / `poll_recv_from` 被 Quinn `AsyncUdpSocket` 使用、worker CID generator 绑定对应 worker shard、UDP punch 本地端口与 QUIC listener 端口一致、listener close 后关闭 `QuicServer` 和所有 Quinn endpoint；integration 能覆盖 QUIC tunnel 仍可建立并传输。 |

### Quote: design.md networks_sfo_reuseport_quic_listener_socket
> | networks_sfo_reuseport_quic_listener_socket | P-SFO-QUIC-LISTENER-1 | QUIC listener 使用 `sfo_reuseport::QuicServer::serve_socket(...)` 注册服务；每个 `(UdpSocket, worker_id)` 回调创建一个 Quinn endpoint，`AsyncUdpSocket::poll_recv()` / `try_send()` / `UdpPoller` 分别委托给 `UdpSocket::poll_recv_from_vectored(...)`、`try_send_to(...)` 和 `poll_send_ready(...)`；endpoint CID generator 使用 `QuicCidGenerator::for_worker(worker_id)`；所有 endpoint accept 结果汇入同一 listener 队列；主动 connect 可选择任一 endpoint；UDP punch 保存第一个 worker socket；`QuicTunnelListener` close 同时关闭 `QuicServer` 和所有 Quinn endpoint。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/quic/listener.rs`、`p2p-frame/src/networks/quic/network.rs`、`docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md` | 回滚时恢复直接 UDP socket + `quinn::Endpoint::new(...)` 路径，但不得引入独立 punch socket、raw UDP tunnel 或改变 NAT punch/heartbeat 语义。 |
