# Task 20260612-quic-listener-socket-port Admission Evidence

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | docs/versions/v0.1/modules/p2p-frame/proposal.md | pass | Read P-SFO-QUIC-LISTENER-1 coverage for QUIC listener `serve_socket`, worker sockets, UDP punch socket source, and listener port evidence. |
| design_read | docs/versions/v0.1/modules/p2p-frame/design.md and docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md | pass | Read `networks_sfo_reuseport_quic_listener_socket` scope and the QUIC listener flow requiring `UDP punch 保存第一个 worker socket`. |
| change_scope_matches_request | proposal P-SFO-QUIC-LISTENER-1 / design networks_sfo_reuseport_quic_listener_socket | pass | The failing test checks that the UDP punch socket preserves the listener local port, which is explicit success evidence for this change_id. |
| active_module_resolved | docs/versions/v0.1/modules/p2p-frame | pass | The panic is in p2p-frame/src/networks/quic/listener.rs, so the active module is p2p-frame v0.1. |
| no_chat_only_evidence | versioned docs only | pass | Admission is based on approved proposal/design rows for networks_sfo_reuseport_quic_listener_socket, not on chat-only behavior. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 3276e5ab90a040d487dd00e11e3b569ed19384dbb48dfa766bfbf0c2f4fdb6e8 |
| docs/versions/v0.1/modules/p2p-frame/design.md | 3ae37756c97a595f48e0d255767005698a5a32d0439fc5e5869b124b1407bdf9 |

## Coverage Quotes

### Quote: proposal.md networks_sfo_reuseport_quic_listener_socket
> | P-SFO-QUIC-LISTENER-1 | networks_sfo_reuseport_quic_listener_socket | QUIC tunnel listener 基于 `sfo_reuseport::QuicServer::serve_socket(...)` 重构，使用内部 Quinn `AsyncUdpSocket` 适配器把每个 worker `sfo_reuseport::UdpSocket` 交给对应 `quinn::Endpoint`，并使用 worker-shard CID generator 保持连接路由稳定；主动 connect 可选择任一 worker endpoint，同源 UDP punch 使用首个可用 worker socket；`ServerRuntime` 可由外部显式设置，默认路径仍自动创建。 | 不新增 raw UDP tunnel、业务载荷解析、公共 `TunnelNetwork` NAT 参数或 `NetworkServerRuntime`；不改变 QUIC tunnel 线协议、TLS 身份校验、NAT punch candidate policy、50ms cadence、active/reverse 起发时机、截止规则或 heartbeat 语义。 | unit 能覆盖 worker socket `try_send_to` / `poll_recv_from` 被 Quinn `AsyncUdpSocket` 使用、worker CID generator 绑定对应 worker shard、UDP punch 本地端口与 QUIC listener 端口一致、listener close 后关闭 `QuicServer` 和所有 Quinn endpoint；integration 能覆盖 QUIC tunnel 仍可建立并传输。 |

### Quote: design.md networks_sfo_reuseport_quic_listener_socket
> | networks_sfo_reuseport_quic_listener_socket | P-SFO-QUIC-LISTENER-1 | QUIC listener 使用 `sfo_reuseport::QuicServer::serve_socket(...)` 注册服务；每个 `(UdpSocket, worker_id)` 回调创建一个 Quinn endpoint，`AsyncUdpSocket::poll_recv()` / `try_send()` / `UdpPoller` 分别委托给 `UdpSocket::poll_recv_from_vectored(...)`、`try_send_to(...)` 和 `poll_send_ready(...)`；endpoint CID generator 使用 `QuicCidGenerator::for_worker(worker_id)`；所有 endpoint accept 结果汇入同一 listener 队列；主动 connect 可选择任一 endpoint；UDP punch 保存第一个 worker socket；`QuicTunnelListener` close 同时关闭 `QuicServer` 和所有 Quinn endpoint。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/quic/listener.rs`、`p2p-frame/src/networks/quic/network.rs`、`docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md` | 回滚时恢复直接 UDP socket + `quinn::Endpoint::new(...)` 路径，但不得引入独立 punch socket、raw UDP tunnel 或改变 NAT punch/heartbeat 语义。 |
