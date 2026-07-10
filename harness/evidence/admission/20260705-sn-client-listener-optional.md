# Implementation Admission: SN Client Listener Optional Protocol Priority

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal row `sn_client_protocol_priority` requires supported outbound protocol gating, no local-listener prerequisite, listener `local_ep` preservation when present, QUIC-first ordering, TCP fallback, and no duplicate active SN. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Approved design row `sn_client_protocol_priority` scopes implementation to `p2p-frame/src/sn/client/sn_service.rs` and necessary SN tests. |
| change_scope_matches_request | user request and mapped docs | pass | User requested SN client should connect by supported protocol without requiring local listener, while preserving listener port as `local_ep` when present. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Active version is `v0.1`; active module is `p2p-frame`; change_id is `sn_client_protocol_priority`. |
| no_chat_only_evidence | proposal/design coverage quotes below | pass | Implementation admission is based on approved proposal/design rows, not chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 2ec0750cb0aa0613be87482aa0b9e255ab3617993684fe050c87eef90658cf78 |
| docs/versions/v0.1/modules/p2p-frame/design.md | cab128711e1d2fd2f02d78d7b89e1ea9ee8316be454d9d124e01fd577fe0d6a1 |

## Coverage Quotes
### Quote: proposal.md sn_client_protocol_priority
> | P-SN-CLIENT-PROTOCOL-PRIORITY-1 | sn_client_protocol_priority | SN 客户端连接同一 SN 服务端时，对同时可用的 QUIC 与 TCP endpoint 按 QUIC 优先、TCP fallback 的顺序尝试；候选是否可尝试取决于本地是否支持对应出站连接协议，不要求本地已经监听该协议端口；如果本地已监听匹配协议端口，建链分类必须继续使用该 listener 的 `local_ep`；QUIC SN command tunnel 建立并 `ReportSn` 成功后停止该 SN 的 TCP 尝试，只有 QUIC 建链或 report 失败后才尝试 TCP。 | 不改变 SN command wire、control-stream-only 信令、不新增客户端选择 Serving SN API、不改变 TCP/QUIC tunnel 线协议、TLS 身份校验、TTP target 语义、`TtpClient::connect_server(...)` 的 maintained target 语义或入站 listener/上报候选语义；不把 TCP 失败用于降低同一远端 QUIC/UDP 候选优先级；不把“未监听本地端口”解释为“不支持出站连接协议”；不在已有匹配 listener 时丢弃或改写其 `local_ep`。 | schema/admission 能以 `sn_client_protocol_priority` 建立后续准入；design/testing 需要覆盖 SN client 候选排序、支持协议但无匹配本地 listener 时仍尝试该 SN endpoint、有匹配本地 listener 时仍使用 listener `local_ep`、QUIC 成功后不触发 TCP fallback、不重复 active SN、QUIC 建链失败或 `ReportSn` 失败后尝试 TCP、以及 `connect_server` 不因同一成功 SN 被协议循环重复调用。 |

### Quote: design.md sn_client_protocol_priority
> | sn_client_protocol_priority | P-SN-CLIENT-PROTOCOL-PRIORITY-1 | `SNClientService::ping_proc` 或等价 `sn/client` helper 对同一 SN 的 report 候选执行 QUIC-first/TCP-fallback 顺序；候选尝试以本地支持出站协议为前提，不以匹配 listener 存在为前提；有匹配 listener 时继续把 listener `local_ep` 传给 command tunnel 分类，无匹配 listener 时用 `local_ep = None` 尝试出站连接；QUIC command tunnel 建立并 `ReportSn` 成功后停止 TCP fallback；QUIC 建链/control-stream/report 失败后继续 TCP；写入 active SN 前按 `sn_peer_id` 去重，避免同一 SN 因多 listener/endpoint 或 QUIC/TCP 双成功而重复 active。 | `p2p-frame/src/sn/client/sn_service.rs`、必要 `p2p-frame/src/sn/**` 测试 | 回滚时恢复只按 `NetManager::listener_info_entries()` 扫描 listener 派生协议候选的旧行为；不得改变 SN command wire、control-stream-only 信令、TCP/QUIC tunnel wire、TLS 身份校验、TTP target matching、`TtpClient::connect_server(...)` maintained target 语义或入站 listener/上报候选语义。 |
