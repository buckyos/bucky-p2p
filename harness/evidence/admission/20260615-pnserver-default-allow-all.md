# Task 20260615-pnserver-default-allow-all Admission Evidence

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | docs/versions/v0.1/modules/p2p-frame/proposal.md | pass | Read approved proposal coverage for `pn_multi_server_assigned_target`, including the revised requirement that `PnServer` default `PnConnectionValidator` is explicit allow-all while multi-PN assigned target admission is enabled only through an explicit validator / policy path. |
| design_read | docs/versions/v0.1/modules/p2p-frame/design.md and docs/versions/v0.1/modules/p2p-frame/design/pn-server.md | pass | Read approved design coverage for `PnServer::new(...)` default allow-all compatibility, explicit assigned target validator / policy construction, and scope paths including `p2p-frame/src/pn/service/pn_server.rs`. |
| change_scope_matches_request | P-PN-MULTI-SERVER-ASSIGNED-TARGET-1 / pn_multi_server_assigned_target | pass | User requested `PnServer` default `PnConnectionValidator` should allow all connections; this is directly covered by the revised proposal/design and does not change PN route selection, wire protocol, or library-owned directory scope. |
| active_module_resolved | docs/versions/v0.1/modules/p2p-frame | pass | `PnServer` and `PnConnectionValidator` live under `p2p-frame/src/pn/service/pn_server.rs`, inside the `p2p-frame` PN service boundary. |
| no_chat_only_evidence | versioned docs only | pass | Implementation admission relies on approved proposal/design rows and document hashes, not on chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 50a0afc03075dd5b917b352d1f9999cae20e388f3445bb5e7a4ee2b051f3eb77 |
| docs/versions/v0.1/modules/p2p-frame/design.md | 5f4c07f452a1ae72d597255f5ce26f7057d08646cf724abc560d7dfc82403ae1 |

## Coverage Quotes

### Quote: proposal.md pn_multi_server_assigned_target
> | P-PN-MULTI-SERVER-ASSIGNED-TARGET-1 | pn_multi_server_assigned_target | 支持多 PN server 部署下由上层指定目标 PN server 建立 proxy tunnel；每个用户到自身 assigned PN server 的保持连接、断线重连、分配和目录查询由库使用者负责；`PnServer` 默认 `PnConnectionValidator` 为显式 allow-all，显式 assigned target validator / policy 路径限制本 server 可作为新建 proxy tunnel target 的用户。 | 不做库内 `peer_id -> PN` 全局目录、不自动发现或切换 PN server、不做用户迁移/重平衡/多副本在线策略、不做 PN server 间目录同步、不做 `A -> PN-A -> PN-B -> B` 跨 PN 二跳业务 bridge；不改变 PN wire 协议、TLS-over-proxy、统计限速口径；不允许多 PN 新路径依赖默认 allow-all 掩盖错误 PN；不删除 `PnServer::new(...)` 默认 allow-all 兼容路径。 | schema/admission 能以 `pn_multi_server_assigned_target` 建立后续准入；design/testing 需要覆盖默认 `PnServer::new(...)` allow-all 兼容、显式 assigned target 策略下上层指定正确 PN 成功、指定错误 PN 在打开目标侧 stream 前失败且不自动切换、未指定目标 PN server 时按配置/参数错误失败、统计/限速保持本 PN server 本地视图且不要求跨 PN 聚合。 |

### Quote: design.md pn_multi_server_assigned_target
> | pn_multi_server_assigned_target | P-PN-MULTI-SERVER-ASSIGNED-TARGET-1 | PN client active open 必须在既有 `TunnelNetwork::create_tunnel_with_intent(...)` 实现内部使用自身 resolver 得到上层指定的 relay PN server id/route，并通过既有 `TtpConnector::open_stream(...)` target 接口连接该 relay 的 `PROXY_SERVICE`；`TunnelManager` 不持有 PN route resolver，stack 不通过 latest tunnel 隐式选择 PN；`PnServer::new(...)` 默认安装显式 allow-all validator；显式 assigned target validator / policy 路径拒绝非本 PN assigned target 的新建 logical tunnel；成功建立后记录 relay session，后续相同 `(tunnel_id, endpoint pair)` 的双向 channel 可复用该 session 而不重新按 target 分配拒绝。 | `p2p-frame/src/pn/client/mod.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/pn/client/pn_listener.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/pn/service/pn_server.rs`、`p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/server.rs`、`p2p-frame/src/tunnel/tunnel_manager.rs`、必要 `p2p-frame/src/stack.rs`、`docs/versions/v0.1/modules/p2p-frame/design/pn-server.md` | 回滚时恢复 explicit route / admission 前的 implicit proxy client 选择；不得删除默认 allow-all PN server 构造，也不得让默认 allow-all 掩盖显式策略下的错误 PN 失败。 |
