# Implementation Admission: TCP Claim Retry Contention

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal read; `tunnel_stream_datagram_listen_callback` requires TCP/QUIC/PN tunnel channel delivery to preserve listen filtering, close/error behavior, and stream/datagram establishment under the callback model. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Approved design read; `tunnel_stream_datagram_listen_callback` maps implementation scope to `p2p-frame/src/networks/tcp/tunnel.rs` and requires TCP tunnel dispatch/open behavior to respect state, listen rules, and bounded delivery semantics. |
| change_scope_matches_request | proposal `P-TUNNEL-CHANNEL-CALLBACK-1` / design `tunnel_stream_datagram_listen_callback` | pass | The current request reports intermittent TCP tunnel `connection not claimable` / `claim retries exhausted` failures while opening stream/datagram channels; the fix is limited to TCP tunnel local claim retry contention without changing wire protocol or public APIs. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | `p2p-frame/src/networks/tcp/tunnel.rs` belongs to the approved `p2p-frame` module packet and the `networks/tcp` boundary recorded in design. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved versioned documents for the writable scope and invariants; the chat report only identifies the observed intermittent failure. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 7673cdcd425fa6f4f8b67bba71ead6843ee0cc598da475f5c78fcdfe97a7d097 |
| docs/versions/v0.1/modules/p2p-frame/design.md | cb2a0a01fc60c51362c130be732a838f1f6e14b63b88a18840d83b03b0b6dbf9 |

## Coverage Quotes

### Quote: proposal.md tunnel_stream_datagram_listen_callback
> | P-TUNNEL-CHANNEL-CALLBACK-1 | tunnel_stream_datagram_listen_callback | `Tunnel` trait 移除 `accept_stream()` / `accept_datagram()`，`listen_stream(...)` / `listen_datagram(...)` 改为接收可克隆、线程安全的异步回调；Tunnel 内部在入站 stream/datagram channel 到达时按 vport/purpose listen 规则触发回调。 | 不改变 TCP/QUIC/PN/TTP 线协议、TLS 身份校验、PN proxy channel 协议、业务 payload 格式、vport/purpose 编解码、tunnel publish 规则或 `TunnelNetwork` listener 回调语义；不保留公共 `accept_*` 兼容旁路；不要求调用方同时注册回调又轮询队列。 | schema/admission 能以 `tunnel_stream_datagram_listen_callback` 建立后续准入；unit 或编译覆盖公共 `Tunnel` trait 不再包含 `accept_*`，TCP/QUIC/PN tunnel 以及 TTP/stream/datagram manager 调用点均迁移到 listen 回调；unit 覆盖关闭后不再调用回调、未 listen 的 purpose 仍拒绝或报错、回调满载/背压/错误路径按 design 收敛；integration 覆盖 workspace 调用方迁移后 stream/datagram 仍可建立并传输。 |

### Quote: design.md tunnel_stream_datagram_listen_callback
> | tunnel_stream_datagram_listen_callback | P-TUNNEL-CHANNEL-CALLBACK-1 | `Tunnel` trait 删除 `accept_stream()` / `accept_datagram()`；新增或调整 `IncomingStreamCallback`、`IncomingDatagramCallback` 等等价回调类型；`listen_stream(...)` / `listen_datagram(...)` 保存 listen 规则和回调；TCP/QUIC/PN tunnel 入站 stream/datagram dispatch 按状态、listen 规则和 bounded capacity 触发回调；TTP、stream manager、datagram manager 和 PN server/client 迁移到回调消费模型。 | `p2p-frame/src/networks/tunnel.rs`、`p2p-frame/src/networks/tcp/tunnel.rs`、`p2p-frame/src/networks/quic/tunnel.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/ttp/runtime.rs`、`p2p-frame/src/stream/stream_manager.rs`、`p2p-frame/src/datagram/datagram_manager.rs`、`p2p-frame/src/pn/service/pn_server.rs`、`docs/versions/v0.1/modules/p2p-frame/design/tunnel-channel-listen-callback.md`、相关 tests | 回滚时恢复公共 `accept_*` 和旧 accept loops，并同步回滚 TTP/manager 调用点；不得留下既有回调又有公共 accept 的双入口状态。 |
