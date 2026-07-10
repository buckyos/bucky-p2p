# Implementation Admission: SN Client Protocol Priority

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal read; `sn_client_protocol_priority` requires SN client QUIC-first/TCP-fallback ordering, QUIC report success stopping TCP fallback, and no duplicate active SN records. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Approved design read; `sn_client_protocol_priority` maps implementation to `p2p-frame/src/sn/client/sn_service.rs` and necessary SN tests, while keeping TTP target matching and `connect_server(...)` maintained-target semantics unchanged. |
| change_scope_matches_request | proposal `P-SN-CLIENT-PROTOCOL-PRIORITY-1` / design `sn_client_protocol_priority` | pass | The current request asks to change the client to prefer QUIC and use TCP only after QUIC failure, and to avoid repeated successful protocol attempts for the same SN. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | `p2p-frame/src/sn/client/sn_service.rs` belongs to the approved `p2p-frame` module packet and the `sn` submodule boundary recorded in design. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved versioned documents; the chat request supplied the concrete user confirmation and launch instruction. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | f81178b9e6d09bb6f87408a084f95670083f336b7dd6e672effec5c506906421 |
| docs/versions/v0.1/modules/p2p-frame/design.md | e268025eba90a72f73abef80b9f4345cb6fbc41a9fe7585b65c91b62dcb194a4 |

## Coverage Quotes

### Quote: proposal.md sn_client_protocol_priority
> | P-SN-CLIENT-PROTOCOL-PRIORITY-1 | sn_client_protocol_priority | SN 客户端连接同一 SN 服务端时，对同时可用的 QUIC 与 TCP endpoint 按 QUIC 优先、TCP fallback 的顺序尝试；QUIC SN command tunnel 建立并 `ReportSn` 成功后停止该 SN 的 TCP 尝试，只有 QUIC 建链或 report 失败后才尝试 TCP。 | 不改变 SN command wire、control-stream-only 信令、不新增客户端选择 Serving SN API、不改变 TCP/QUIC tunnel 线协议、TLS 身份校验、TTP target 语义或 `TtpClient::connect_server(...)` 的 maintained target 语义；不把 TCP 失败用于降低同一远端 QUIC/UDP 候选优先级。 | schema/admission 能以 `sn_client_protocol_priority` 建立后续准入；design/testing 需要覆盖 SN client 候选排序、QUIC 成功后不触发 TCP fallback、不重复 active SN、QUIC 建链失败或 `ReportSn` 失败后尝试 TCP、以及 `connect_server` 不因同一成功 SN 被协议循环重复调用。 |

### Quote: design.md sn_client_protocol_priority
> | sn_client_protocol_priority | P-SN-CLIENT-PROTOCOL-PRIORITY-1 | `SNClientService::ping_proc` 或等价 `sn/client` helper 对同一 SN 的 report 候选执行 QUIC-first/TCP-fallback 顺序；QUIC command tunnel 建立并 `ReportSn` 成功后停止 TCP fallback；QUIC 建链/control-stream/report 失败后继续 TCP；写入 active SN 前按 `sn_peer_id` 去重，避免同一 SN 因多 listener/endpoint 或 QUIC/TCP 双成功而重复 active。 | `p2p-frame/src/sn/client/sn_service.rs`、必要 `p2p-frame/src/sn/**` 测试 | 回滚时恢复按 `NetManager::listener_info_entries()` 原始顺序扫描所有协议候选的旧行为；不得改变 SN command wire、control-stream-only 信令、TCP/QUIC tunnel wire、TLS 身份校验、TTP target matching 或 `TtpClient::connect_server(...)` maintained target 语义。 |
