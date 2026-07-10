# Task 20260711-ttp-client-idle-loop-lifecycle Admission Evidence

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | docs/versions/v0.1/modules/p2p-frame/proposal.md | pass | `P-TTP-CLIENT-CONNECTION-LIFECYCLE-1` requires idle release for non-maintained cached tunnels after active and pending use reaches zero. |
| design_read | docs/versions/v0.1/modules/p2p-frame/design.md | pass | `ttp_client_connection_lifecycle` assigns the idle cache lifecycle to `TtpClient` and scopes implementation to `p2p-frame/src/ttp/client.rs` plus later testing-stage coverage. |
| change_scope_matches_request | proposal P-TTP-CLIENT-CONNECTION-LIFECYCLE-1 / design ttp_client_connection_lifecycle | pass | Starting the idle sweeper for every `TtpClient`, including one-shot-only clients, and stopping its owned task at client teardown makes the already-designed idle release effective without changing tunnel or wire semantics. |
| active_module_resolved | docs/versions/v0.1/modules/p2p-frame | pass | The reported implementation and approved scope path are both under `p2p-frame/src/ttp/`. |
| no_chat_only_evidence | versioned docs only | pass | Admission relies on the approved proposal/design mapping; the user report identifies the implementation defect but is not used as the design authority. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 99703f9bd69b266f2680a6d70cebd9f3dd5ad624801fede3fc056f3204c93f7a |
| docs/versions/v0.1/modules/p2p-frame/design.md | f93dbdd755bb3b5249322ed507886b9dc9bf84b8f66ab52e1c6ec9bdfc364723 |

## Coverage Quotes

### Quote: proposal.md ttp_client_connection_lifecycle
> | P-TTP-CLIENT-CONNECTION-LIFECYCLE-1 | ttp_client_connection_lifecycle | `TtpClient` 支持删除保持连接的 server target，删除后 maintain loop 不再自动重建该 target；非保持连接 target 的 cached tunnel 在无 active stream/control stream/datagram/pending open 且超过 idle 阈值后释放本地缓存。 | 不改变 `Tunnel` / `TunnelNetwork` trait、TCP/QUIC/PN/TTP 线协议、vport/purpose 编码、身份校验、tunnel publish 规则或 `TtpServer` lookup-only 语义；不把 target 删除扩展为删除 peer 或全局关闭所有同 peer tunnel；idle release 不得强制中断 active channel，且不得作用于仍在 maintained target 集合中的 server target。 | schema/admission 能以 `ttp_client_connection_lifecycle` 建立后续准入；design/testing 需要覆盖 maintained target add/remove 幂等、删除后 maintain loop 不重连、其他 maintained target 不受影响、非 maintained tunnel idle release、active/pending channel 阻止 release、保持 target 不被 idle release 清理、以及代码审查确认 wire/trait/TtpServer 行为不变。 |

### Quote: design.md ttp_client_connection_lifecycle
> | ttp_client_connection_lifecycle | P-TTP-CLIENT-CONNECTION-LIFECYCLE-1 | `TtpClient::remove_server(&TtpTarget)` 按 `(remote_id, remote_ep)` 精确幂等删除 maintained target；maintain loop 每轮读取当前集合快照，删除后的 target 不再自动重建；`TtpClient` cache 记录 maintained 标记、last_used 和 active/pending lease，`open_*` 期间持有 lease，non-maintained 且 lease 为 0 的 cache entry 超过默认 5 分钟 idle timeout 后只释放本地引用；maintained target 和 active/pending target 均保留。 | `p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/tests.rs` | 回滚时删除 `remove_server` 和 lease/idle metadata，恢复简单 `HashMap<P2pId, TunnelRef>`；不得改变 traits、wire、target matching、底层 close 或 `TtpServer` lookup-only 行为。 |
