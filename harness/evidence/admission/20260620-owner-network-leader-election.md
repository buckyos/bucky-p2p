## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | pass | Approved proposal directly covers ownerSN network voting, leader failover, and leader-driven Serving SN session replication. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | pass | Approved design maps `sn_owner_network_leader_election` and `sn_owner_leader_session_replication` to `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, and `sn-miner-rust/**`. |
| change_scope_matches_request | user request, proposal items, design mapping | pass | User requested network voting, leader election, leader failover, follower-to-leader communication, and leader synchronization of Serving SN online/offline/session state. |
| active_module_resolved | packet front matter and path | pass | version `v0.1`, module `p2p-frame`, submodule `sn-distributed-directory`. |
| no_chat_only_evidence | proposal/design direct rows | pass | Admission relies on approved proposal/design rows, not chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | e42ea0b6903c8a3b6d1948a590c0a9a2e2f8d56d2a84692b7374f6967b5d07d7 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | 66fa322b9e2887aa0dee0409279a63bbbb9878cec60364bc8a794beae0f39f72 |

## Coverage Quotes
### Quote: proposal.md sn_owner_network_leader_election
> | P-SN-DIST-CONTROL-2 | sn_owner_network_leader_election | Owner SN 之间通过网络投票选举 leader；leader 正常时 follower 与 leader 通信，leader 失效后 quorum 内重新选主。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、`p2p-frame/src/sn/mod.rs`、必要 `sn-miner` 配置装配。 | 多 ownerSN 进程或等价 DV 中可观察到投票选主；停止 leader 后可重新选出 leader；follower 不直接提交 session 变更。 | 不实现完整 etcd/Raft 持久化、snapshot、joint consensus 或 linearizable read。 |

### Quote: design.md sn_owner_network_leader_election
> | sn_owner_network_leader_election | P-SN-DIST-CONTROL-2 | Owner-to-owner vote request/response, candidate/follower/leader roles, leader heartbeat, heartbeat timeout, failover election, follower redirect/forward | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | owner peer/control command boundary gains election messages | runtime-only election; no persistent WAL/snapshot |

### Quote: proposal.md sn_owner_leader_session_replication
> | P-SN-DIST-CONTROL-3 | sn_owner_leader_session_replication | Serving SN online/offline/renew/revoke 事件由 leader 提交并同步给其它 ownerSN，Serving SN 异常离线事件也由 leader 复制到集群其它机器。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 owner directory serving-facing listener。 | Serving SN 状态上报到 follower 时被转发或 redirect 到 leader；leader commit 后其它 ownerSN 查询同一 Serving SN session/epoch 状态一致；leader 失效重选后可继续处理后续状态事件。 | 不复制用户级 `PeerRoute` 到所有 ownerSN；只复制 control/session 小状态。 |

### Quote: design.md sn_owner_leader_session_replication
> | sn_owner_leader_session_replication | P-SN-DIST-CONTROL-3 | leader-only Serving SN session mutation, follower forwarding, quorum replication, committed session apply on followers, offline/revoke propagation | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | session state replication across ownerSN control plane | does not replicate user-level `PeerRoute` |
