# Task 20260710-listenerless-quic-client-endpoint Admission Evidence

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal item `P-SN-CLIENT-PROTOCOL-PRIORITY-1` requires supported outbound QUIC candidates even when no local protocol listener exists, while preserving QUIC-first/TCP-fallback behavior. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | User-approved design defines one network-owned outbound-only Quinn endpoint per IP version, listener-first selection, endpoint lifetime/error/local-metadata behavior, and exact QUIC network Scope Paths. |
| change_scope_matches_request | proposal `P-SN-CLIENT-PROTOCOL-PRIORITY-1` / design `sn_client_protocol_priority` | pass | The request fixes the exact gap between listener-optional SN candidate generation and `QuicTunnelNetwork::create_tunnel(...)`, which currently returns `NotFound` without attempting a client endpoint. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | Active version is `v0.1`, module is `p2p-frame`, and concrete change id is `sn_client_protocol_priority`; no direct submodule packet owns this existing `networks/quic` correction. |
| no_chat_only_evidence | approved proposal/design plus validated `harness/pipeline-plan.md` | pass | Implementation behavior, non-goals, state ownership, error handling, and Scope Paths are fully recorded in versioned artifacts; chat is used only as approval and explicit auto-pipeline launch provenance. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 99703f9bd69b266f2680a6d70cebd9f3dd5ad624801fede3fc056f3204c93f7a |
| docs/versions/v0.1/modules/p2p-frame/design.md | f93dbdd755bb3b5249322ed507886b9dc9bf84b8f66ab52e1c6ec9bdfc364723 |

## Coverage Quotes

### Quote: proposal.md sn_client_protocol_priority
> | P-SN-CLIENT-PROTOCOL-PRIORITY-1 | sn_client_protocol_priority | SN 客户端连接同一 SN 服务端时，对同时可用的 QUIC 与 TCP endpoint 按 QUIC 优先、TCP fallback 的顺序尝试；候选是否可尝试取决于本地是否支持对应出站连接协议，不要求本地已经监听该协议端口；如果本地已监听匹配协议端口，QUIC 候选继续使用该 listener 的 `local_ep`，TCP 候选必须使用 ephemeral local port 或 `local_ep = None`，不得复用已监听端口进行出站 bind；QUIC SN command tunnel 建立并 `ReportSn` 成功后停止该 SN 的 TCP 尝试，只有 QUIC 建链或 report 失败后才尝试 TCP。 | 不改变 SN command wire、control-stream-only 信令、不新增客户端选择 Serving SN API、不改变 TCP/QUIC tunnel 线协议、TLS 身份校验、TTP target 语义、`TtpClient::connect_server(...)` 的 maintained target 语义或入站 listener/上报候选语义；不把 TCP 失败用于降低同一远端 QUIC/UDP 候选优先级；不把“未监听本地端口”解释为“不支持出站连接协议”；不让 TCP fallback 复用已监听端口导致本地 bind 冲突。 | schema/admission 能以 `sn_client_protocol_priority` 建立后续准入；design/testing 需要覆盖 SN client 候选排序、支持协议但无匹配本地 listener 时仍尝试该 SN endpoint、有匹配 QUIC listener 时仍使用 listener `local_ep`、有匹配 TCP listener 时 TCP fallback 使用 ephemeral/no local_ep、QUIC 成功后不触发 TCP fallback、不重复 active SN、QUIC 建链失败或 `ReportSn` 失败后尝试 TCP、以及 `connect_server` 不因同一成功 SN 被协议循环重复调用。 |

### Quote: design.md sn_client_protocol_priority
> | sn_client_protocol_priority | P-SN-CLIENT-PROTOCOL-PRIORITY-1 | `SNClientService::ping_proc` 或等价 `sn/client` helper 对同一 SN 的 report 候选执行 QUIC-first/TCP-fallback 顺序；候选尝试以本地支持出站协议为前提，不以匹配 listener 存在为前提；有匹配 QUIC listener 时继续把 listener `local_ep` 传给 command tunnel 分类；无匹配 QUIC listener 时，`QuicTunnelNetwork::create_tunnel(...)` 按远端 IP 版本懒创建并复用仅出站 Quinn client endpoint，绑定 wildcard ephemeral port，使用既有每连接 identity/TLS config 建链，成功后记录实际 local IP 与 OS-assigned port；client endpoint 不进入 listener 信息、不接收入站 tunnel、不随 `close_all_listener()` 关闭、不触发 listener 同源 UDP punch；有匹配 TCP listener 时把 unspecified IP 转成 `None` 或把具体 IP 的 port 转成 `0` 后再传给 command tunnel 分类，避免 outbound socket bind 复用监听端口；无匹配 listener 时用 `local_ep = None` 尝试出站连接；候选建链/report 失败必须输出包含 SN id、protocol、local_ep、remote endpoint 和错误的日志；QUIC command tunnel 建立并 `ReportSn` 成功后停止 TCP fallback；QUIC 建链/control-stream/report 失败后继续 TCP；写入 active SN 前按 `sn_peer_id` 去重，避免同一 SN 因多 listener/endpoint 或 QUIC/TCP 双成功而重复 active。 | `p2p-frame/src/sn/client/sn_service.rs`、`p2p-frame/src/networks/quic/network.rs`、必要 `p2p-frame/src/sn/**` 与 `p2p-frame/src/networks/quic/**` 测试 | 回滚 listenerless client endpoint 时会恢复无 listener QUIC `NotFound` 并违反 proposal，因此只能在同步退回 proposal/design 时撤销；不得改变 SN command wire、control-stream-only 信令、TCP/QUIC tunnel wire、TLS 身份校验、TTP target matching、`TtpClient::connect_server(...)` maintained target 语义或入站 listener/上报候选语义。 |
