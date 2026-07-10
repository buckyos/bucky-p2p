## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | pass | Approved proposal covers owner control/session small-state sync and partitioned peer route requirements. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | pass | Approved design maps `sn_owner_control_plane_sessions` and `sn_partitioned_peer_route` to concrete scope paths and implementation order. |
| change_scope_matches_request | user request, proposal items, design mapping | pass | User requested ownerSN Raft implementation; admitted scope implements the approved first-version owner control plane/session and partitioned route behavior. |
| active_module_resolved | packet front matter and path | pass | version `v0.1`, module `p2p-frame`, submodule `sn-distributed-directory`. |
| no_chat_only_evidence | proposal/design direct rows | pass | Admission relies on approved proposal/design rows, not chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | cfe9f6f0ac5dd6d37f1035dc46a125da8a7a5e3a76bd3ae87ed8881a4d2f4d75 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | f155663558df81b2b41dab3cbfa128325f496214ea60a7130fb78c08fab0b447 |

## Coverage Quotes
### Quote: proposal.md sn_owner_control_plane_sessions
> | P-SN-DIST-CONTROL-1 | sn_owner_control_plane_sessions | Owner SN 集群同步 owner/control/session 小状态，并用 Serving SN session/epoch 批量判定 Serving SN 可用性。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 `sn-miner` 配置装配。 | Serving SN 正常续约时可继续返回其 route；revoke 或异常过期后，其旧 route 不再被 query 返回。 | 不同步用户级 `PeerRoute` 到所有 ownerSN；不在 proposal 固定日志和字段结构。 |

### Quote: design.md sn_owner_control_plane_sessions
> | sn_owner_control_plane_sessions | P-SN-DIST-CONTROL-1 | Owner control plane, Serving SN session/epoch, leader/quorum commit, revoke/expiry filtering | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | new owner control/session APIs inside directory domain | first version in-memory Raft-style state machine |

### Quote: proposal.md sn_partitioned_peer_route
> | P-SN-DIST-ROUTE-1 | sn_partitioned_peer_route | 用户级 peer route 由固定分区逻辑管理，Serving SN 根据 ownerSN 列表本地计算 route owner set 并批量写入/query。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 `sn/service` 接入。 | route 只写入分区 owner set；query fanout 到分区 owner set；route 返回前必须校验 Serving SN session/epoch。 | 不做 route 全 ownerSN 广播；第一版不做 membership 变更期迁移。 |

### Quote: design.md sn_partitioned_peer_route
> | sn_partitioned_peer_route | P-SN-DIST-ROUTE-1 | Fixed partition route owner set, route publish/query, session/epoch filter, route miss behavior | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs` | replaces per-peer lease as primary route model | no membership migration first version |
