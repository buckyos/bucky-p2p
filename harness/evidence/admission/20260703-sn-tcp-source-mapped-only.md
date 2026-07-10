# Implementation Admission: SN TCP Source Mapped Only

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal read; `sn_tcp_source_mapped_only` requires SN service to avoid returning or storing raw TCP tunnel source socket addresses, while allowing mapped candidates from source IP plus reported `map_ports`. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Approved design read; `sn_tcp_source_mapped_only` maps implementation to `p2p-frame/src/sn/service/service.rs`, with optional SN service tests, and separates direct observed endpoint return from mapped endpoint construction. |
| change_scope_matches_request | proposal `P-SN-TCP-SOURCE-MAPPED-ONLY-1` / design `sn_tcp_source_mapped_only` | pass | The current request asks that TCP source addresses not be returned to the client or kept by SN server, except that source IP plus client `map_port` may construct an external mapped address. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | `p2p-frame/src/sn/service/service.rs` belongs to the approved `p2p-frame` module packet and the existing `sn` submodule boundary recorded in design. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved versioned documents; the chat request supplied the concrete user confirmation and launch instruction. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 7673cdcd425fa6f4f8b67bba71ead6843ee0cc598da475f5c78fcdfe97a7d097 |
| docs/versions/v0.1/modules/p2p-frame/design.md | cb2a0a01fc60c51362c130be732a838f1f6e14b63b88a18840d83b03b0b6dbf9 |

## Coverage Quotes

### Quote: proposal.md sn_tcp_source_mapped_only
> | P-SN-TCP-SOURCE-MAPPED-ONLY-1 | sn_tcp_source_mapped_only | SN 服务端不把 TCP tunnel 来源 socket address 本身作为候选 endpoint 返回给客户端或保存为 peer endpoint 状态；客户端上报 `map_ports` 时，SN 可使用 TCP 来源 IP 与映射端口构造 `Mapped` 外网候选。 | 不改变 SN command wire、control-stream-only 信令、TCP tunnel wire、TLS 身份校验、QUIC `ServerReflexive` NAT punch 语义或 `EndpointArea` 编解码；不把原始 TCP 来源端口作为候选返回；不删除客户端自报 endpoint 或 desc endpoint 的既有处理。 | schema/admission 能以 `sn_tcp_source_mapped_only` 建立后续准入；unit 覆盖无 `map_ports` 时 report/query/called 不包含 TCP 来源 socket address，有 `map_ports` 时返回来源 IP + 上报端口的 `Mapped` endpoint，并确认 SN peer cache 不保存原始 TCP 来源 endpoint。 |

### Quote: design.md sn_tcp_source_mapped_only
> | sn_tcp_source_mapped_only | P-SN-TCP-SOURCE-MAPPED-ONLY-1 | SN service 从 command tunnel 观察到的 TCP remote socket address 不作为普通候选返回、不进入 peer endpoint 状态；`ReportSnResp`、`SnQueryResp` 和 `SnCalled.reverse_endpoint_array` 只能返回非 TCP observed candidate 或由 `map_ports` 构造出的 `Mapped` candidate；构造 `Mapped` 时使用 observed TCP 来源 IP 与上报协议/端口，不能使用 raw TCP 来源端口。 | `p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/service/**`、相关 tests | 回滚时恢复 TCP observed endpoint 可直接进入 SN 响应候选的旧行为；不得改变 SN command wire、control-stream-only 信令、TCP tunnel wire、TLS 身份校验、QUIC `ServerReflexive` 语义或 endpoint codec。 |
