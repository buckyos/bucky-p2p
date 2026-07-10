# Implementation Admission: TCP Listener Control Hello Test

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal read; `networks_sfo_reuseport_tcp_listener` requires inbound `sfo_reuseport::TcpStream` to enter existing TLS accept, control/data split, registry, and tunnel publish flow without changing TCP wire protocol or TLS identity verification. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Approved design read; `networks_sfo_reuseport_tcp_listener` maps implementation scope to TCP listener, connection, network, and stack paths, and preserves existing TLS accept and control/data split semantics. |
| change_scope_matches_request | proposal `P-SFO-TCP-LISTENER-1` / design `networks_sfo_reuseport_tcp_listener` | pass | The current request reports a panic in the TCP listener control-hello unit test caused by constructing a full listener and TLS acceptor for pure control/data hello validation; the fix is limited to TCP listener validation structure and does not change wire behavior. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | `p2p-frame/src/networks/tcp/listener.rs` belongs to the approved `p2p-frame` module packet and the `networks/tcp` boundary recorded in design. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved versioned documents for writable scope and invariants; the chat report only identifies the observed unit-test panic. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 7673cdcd425fa6f4f8b67bba71ead6843ee0cc598da475f5c78fcdfe97a7d097 |
| docs/versions/v0.1/modules/p2p-frame/design.md | cb2a0a01fc60c51362c130be732a838f1f6e14b63b88a18840d83b03b0b6dbf9 |

## Coverage Quotes

### Quote: proposal.md networks_sfo_reuseport_tcp_listener
> | P-SFO-TCP-LISTENER-1 | networks_sfo_reuseport_tcp_listener | TCP tunnel listener 基于 `sfo_reuseport::TcpServer` 重构，入站 `sfo_reuseport::TcpStream` 接入现有 TLS accept、TCP control/data connection 分流、registry 和 tunnel publish 流程；`ServerRuntime` 可由外部显式设置，默认路径仍自动创建。 | 不新增 `NetworkServerRuntime` 或 socket factory trait；不改变 TCP tunnel 线协议、TLS 身份校验或 tunnel publish 语义；不把 close 后的旧服务继续暴露为当前 listener 的入站 tunnel。 | unit 能覆盖外部 `ServerRuntime` 被 TCP listener 使用、默认 runtime 可用、`TcpServer` handler 的 stream 进入现有 control/data 分流路径、listener close 后不再发布新 tunnel；integration 能覆盖 TCP tunnel 仍可建立并传输。 |

### Quote: design.md networks_sfo_reuseport_tcp_listener
> | networks_sfo_reuseport_tcp_listener | P-SFO-TCP-LISTENER-1 | TCP listener 使用 `sfo_reuseport::TcpServer` 注册服务，handler 接收 `sfo_reuseport::TcpStream` 后调用现有 TLS accept 与 TCP control/data 分流；`TcpTunnelListener` 保存 `TcpServer` handle 并在 close 时停止服务；`P2pStackConfig` 可显式注入 `sfo_reuseport::ServerRuntime`，默认路径自动创建 runtime。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/tcp/listener.rs`、`p2p-frame/src/networks/tcp/connection.rs`、`p2p-frame/src/networks/tcp/network.rs`、`docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md` | 回滚时恢复本地 `TcpListener` bind/accept 路径和旧 `reuse_address` 设置，但不得改变 TCP tunnel 线协议或 publish 语义。 |
