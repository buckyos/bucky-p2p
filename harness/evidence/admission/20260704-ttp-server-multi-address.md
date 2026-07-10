# Implementation Admission: TTP Server Multi Address Cache

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal read; `ttp_server_tunnel_accept_validator` requires accepted incoming tunnels to attach and be remembered in the TtpServer local cache without changing wire, traits, publish rules, or lookup-only open semantics. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Approved design read; `ttp_server_tunnel_accept_validator` maps implementation to `p2p-frame/src/ttp/server.rs` and defines accept-before-attach/cache behavior for TtpServer. |
| change_scope_matches_request | proposal `P-TTP-SERVER-TUNNEL-ACCEPT-VALIDATOR-1` / design `ttp_server_tunnel_accept_validator` | pass | The current request asks `TtpServer` to support one node with multiple addresses; this is a TtpServer local incoming tunnel cache correction so same-peer endpoint-specific tunnels do not overwrite each other. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | `p2p-frame/src/ttp/server.rs` belongs to the approved `p2p-frame` module packet and the `ttp` submodule boundary recorded in design. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved versioned documents; the chat request only triggers the covered TtpServer attach/cache behavior. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 7673cdcd425fa6f4f8b67bba71ead6843ee0cc598da475f5c78fcdfe97a7d097 |
| docs/versions/v0.1/modules/p2p-frame/design.md | cb2a0a01fc60c51362c130be732a838f1f6e14b63b88a18840d83b03b0b6dbf9 |

## Coverage Quotes

### Quote: proposal.md ttp_server_tunnel_accept_validator
> | P-TTP-SERVER-TUNNEL-ACCEPT-VALIDATOR-1 | ttp_server_tunnel_accept_validator | `TtpServer` 增加 incoming tunnel validator 或等价策略装配点；server 收到 `NetManager` 投递的新 tunnel 后，必须先执行 validator，只有 accept 后才能调用 `TtpRuntime::attach_tunnel(...)` 并记入本地 tunnel cache；默认构造路径保持显式 allow-all 兼容行为。 | 不改变底层 `NetManager` / `TunnelNetwork` publish 规则、TCP/QUIC/PN/TTP wire、公共 `Tunnel` / `TunnelNetwork` trait、vport/purpose 编码、身份校验、`TtpServer` lookup-only open 语义、`TtpNode` active-open 行为或 `TtpClient` 生命周期语义；不把 validator 扩展为传输层认证协议、计费、限速或全局 tunnel 策略。 | schema/admission 能以 `ttp_server_tunnel_accept_validator` 建立后续准入；design/testing 需要定义 validator 上下文、默认 allow-all 构造路径、显式 reject 行为、错误/close 处理和 API 命名；unit 覆盖默认 allow-all 会 attach/remember，新 validator reject 不 attach、不 remember、后续 open 无法复用被拒 tunnel，并确认 wire/trait/TtpNode/TtpClient 行为不变。 |

### Quote: design.md ttp_server_tunnel_accept_validator
> | ttp_server_tunnel_accept_validator | P-TTP-SERVER-TUNNEL-ACCEPT-VALIDATOR-1 | `TtpServer` 新增 incoming tunnel validator trait/context、allow-all helper 和显式 validator 构造路径；default `TtpServer::new(...)` 使用 allow-all 保持兼容；incoming callback 在 attach/cache 前执行 validator；accept 后 attach/remember；reject 或 validator error 不 attach、不 remember，并 best-effort close 被拒 tunnel。 | `p2p-frame/src/ttp/server.rs`、`p2p-frame/src/ttp/mod.rs` | 回滚时删除 validator trait/context/helper 和显式构造路径，恢复 `TtpServer` 默认 attach/remember 所有 incoming tunnel；不得改变 `NetManager` publish、wire、public tunnel traits、`TtpNode` active-open 或 `TtpClient` lifecycle。 |
