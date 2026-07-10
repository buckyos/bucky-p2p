# Implementation Admission: TTP Client Multi Address Cache

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal read; `ttp_client_connection_lifecycle` requires exact maintained target handling, preserves other targets, and forbids expanding target deletion into deleting a peer or all same-peer tunnels. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Approved design read; `ttp_client_connection_lifecycle` maps implementation to `p2p-frame/src/ttp/client.rs` and describes exact `(remote_id, remote_ep)` target semantics plus local cache lifecycle metadata. |
| change_scope_matches_request | proposal `P-TTP-CLIENT-CONNECTION-LIFECYCLE-1` / design `ttp_client_connection_lifecycle` | pass | The current request asks `TtpClient` to support one node with multiple addresses; this is a local TTP client target/cache lifecycle correction so same-peer exact targets do not overwrite or remove each other. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | `p2p-frame/src/ttp/client.rs` belongs to the approved `p2p-frame` module packet and the `ttp` submodule boundary recorded in design. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved versioned documents; the chat request only triggers the covered TTP client exact-target/cache behavior. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 7673cdcd425fa6f4f8b67bba71ead6843ee0cc598da475f5c78fcdfe97a7d097 |
| docs/versions/v0.1/modules/p2p-frame/design.md | cb2a0a01fc60c51362c130be732a838f1f6e14b63b88a18840d83b03b0b6dbf9 |

## Coverage Quotes

### Quote: proposal.md ttp_client_connection_lifecycle
> | P-TTP-CLIENT-CONNECTION-LIFECYCLE-1 | ttp_client_connection_lifecycle | `TtpClient` 支持删除保持连接的 server target，删除后 maintain loop 不再自动重建该 target；非保持连接 target 的 cached tunnel 在无 active stream/control stream/datagram/pending open 且超过 idle 阈值后释放本地缓存。 | 不改变 `Tunnel` / `TunnelNetwork` trait、TCP/QUIC/PN/TTP 线协议、vport/purpose 编码、身份校验、tunnel publish 规则或 `TtpServer` lookup-only 语义；不把 target 删除扩展为删除 peer 或全局关闭所有同 peer tunnel；idle release 不得强制中断 active channel，且不得作用于仍在 maintained target 集合中的 server target。 | schema/admission 能以 `ttp_client_connection_lifecycle` 建立后续准入；design/testing 需要覆盖 maintained target add/remove 幂等、删除后 maintain loop 不重连、其他 maintained target 不受影响、非 maintained tunnel idle release、active/pending channel 阻止 release、保持 target 不被 idle release 清理、以及代码审查确认 wire/trait/TtpServer 行为不变。 |

### Quote: design.md ttp_client_connection_lifecycle
> | ttp_client_connection_lifecycle | P-TTP-CLIENT-CONNECTION-LIFECYCLE-1 | `TtpClient::remove_server(&TtpTarget)` 按 `(remote_id, remote_ep)` 精确幂等删除 maintained target；maintain loop 每轮读取当前集合快照，删除后的 target 不再自动重建；`TtpClient` cache 记录 maintained 标记、last_used 和 active/pending lease，`open_*` 期间持有 lease，non-maintained 且 lease 为 0 的 cache entry 超过默认 5 分钟 idle timeout 后只释放本地引用；maintained target 和 active/pending target 均保留。 | `p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/tests.rs` | 回滚时删除 `remove_server` 和 lease/idle metadata，恢复简单 `HashMap<P2pId, TunnelRef>`；不得改变 traits、wire、target matching、底层 close 或 `TtpServer` lookup-only 行为。 |
