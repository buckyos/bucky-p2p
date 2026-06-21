# Task 20260614-default-pn-client-route Admission Evidence

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | docs/versions/v0.1/modules/p2p-frame/proposal.md | pass | Read approved proposal coverage for `pn_multi_server_assigned_target`, including upper-layer specified PN server selection, no library directory, no automatic PN switching, and no cross-PN bridge. |
| design_read | docs/versions/v0.1/modules/p2p-frame/design.md | pass | Read approved design coverage for `pn_multi_server_assigned_target`, including stack not using implicit latest PN selection, explicit PN client construction inputs, and scope paths including necessary `p2p-frame/src/stack.rs`. |
| change_scope_matches_request | P-PN-MULTI-SERVER-ASSIGNED-TARGET-1 / pn_multi_server_assigned_target | pass | User requested default `PnClient` construction to support the current stack actively connecting to its configured `PnServer` address so other peers can connect back through that PN; if PN is SN, reuse the SN TTP client, otherwise create a separate TTP client. |
| active_module_resolved | docs/versions/v0.1/modules/p2p-frame | pass | Request points at `p2p-frame/src/stack.rs`, which belongs to the approved `p2p-frame` stack/runtime and PN client assembly boundary. |
| no_chat_only_evidence | versioned docs only | pass | Admission relies on approved proposal/design rows and their bound hashes, not on chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 40781e1709b82117f8ae9ffaa40d636612056910458d2e29430f33dfe616aef5 |
| docs/versions/v0.1/modules/p2p-frame/design.md | f1dafd6a84aec0b7193242add91ca13c470399359583584c3bb1c994d9eac0ef |

## Coverage Quotes

### Quote: proposal.md pn_multi_server_assigned_target
> | P-PN-MULTI-SERVER-ASSIGNED-TARGET-1 | pn_multi_server_assigned_target | 支持多 PN server 部署下由上层指定目标 PN server 建立 proxy tunnel；每个用户到自身 assigned PN server 的保持连接、断线重连、分配和目录查询由库使用者负责；`PnServer` 通过 assigned target 准入限制本 server 可作为新建 proxy tunnel target 的用户。 | 不做库内 `peer_id -> PN` 全局目录、不自动发现或切换 PN server、不做用户迁移/重平衡/多副本在线策略、不做 PN server 间目录同步、不做 `A -> PN-A -> PN-B -> B` 跨 PN 二跳业务 bridge；不改变 PN wire 协议、TLS-over-proxy、统计限速口径；不要求保留默认单 PN / allow-all PN server 构造兼容路径，design/implementation 可删除或替换与显式 PN server 选择冲突的旧逻辑。 | schema/admission 能以 `pn_multi_server_assigned_target` 建立后续准入；design/testing 需要覆盖上层指定正确 PN 成功、指定错误 PN 在打开目标侧 stream 前失败且不自动切换、未配置 assigned target 或未指定目标 PN server 时按配置/参数错误失败、统计/限速保持本 PN server 本地视图且不要求跨 PN 聚合。 |

### Quote: design.md pn_multi_server_assigned_target
> | pn_multi_server_assigned_target | P-PN-MULTI-SERVER-ASSIGNED-TARGET-1 | PN client active open 必须在既有 `TunnelNetwork::create_tunnel_with_intent(...)` 实现内部使用自身 resolver 得到上层指定的 relay PN server id/route，并通过既有 `TtpConnector::open_stream(...)` target 接口连接该 relay 的 `PROXY_SERVICE`；`TunnelManager` 不持有 PN route resolver，stack 不通过 latest tunnel 隐式选择 PN；`PnServer` 构造要求 assigned target admission，拒绝非本 PN assigned target 的新建 logical tunnel；成功建立后记录 relay session，后续相同 `(tunnel_id, endpoint pair)` 的双向 channel 可复用该 session 而不重新按 target 分配拒绝。 | `p2p-frame/src/pn/client/mod.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/pn/client/pn_listener.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/pn/service/pn_server.rs`、`p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/server.rs`、`p2p-frame/src/tunnel/tunnel_manager.rs`、必要 `p2p-frame/src/stack.rs`、`docs/versions/v0.1/modules/p2p-frame/design/pn-server.md` | 回滚时恢复旧单 PN / allow-all PN server 构造和 implicit proxy client 选择；本变更的正常失败路径是缺少 route 或指定错误 PN 直接失败，不得通过回滚残留的 latest tunnel fallback 掩盖错误 PN。 |
