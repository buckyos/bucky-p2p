# Task 20260618-owner-directory-peer-transport Admission Evidence

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | pass | Read approved proposal boundary for independent OwnerDirectoryServer peer transport. |
| design_read | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | pass | Read approved design requiring owner peer `NetManager + TtpNode + DefaultCmdNodeService` transport and command dispatch. |
| change_scope_matches_request | proposal P-SN-DIST-OWNER-PEER-TRANSPORT-1 / design sn_owner_directory_peer_transport | pass | The request asks to continue implementation after confirming OwnerDirectoryServer transport and command-node design. |
| active_module_resolved | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory | pass | The touched code belongs to the p2p-frame SN distributed directory direct submodule. |
| no_chat_only_evidence | versioned docs only | pass | Implementation admission uses approved proposal/design content and not chat-only assumptions. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | 44b62063d7c6b01fd8bc7857a580717f37598174004b1e1a6514aae9916ae496 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | 355e4d1dbbf59006338215dd5d070a18afd668be8a3cd37fccf6479dad67f874 |

## Coverage Quotes

### Quote: proposal.md sn_owner_directory_peer_transport
> | P-SN-DIST-OWNER-PEER-TRANSPORT-1 | sn_owner_directory_peer_transport | OwnerDirectoryServer 提供独立 listener 和 owner peer-to-peer transport，承载 owner 间心跳、lease publish/query、detail/relay 协作等双向内部命令。 | `p2p-frame/src/sn/directory/**`、必要 peer transport 和命令节点接入、`sn-miner-rust` owner directory 装配。 | OwnerDirectoryServer 不依赖 SnServer listener 启动即可监听 owner 端口；任一 OwnerMember 可主动连接或接受连接，并可在同一 peer connection 上双向发送 owner directory 命令。 | 不把 owner peer transport 固化成固定客户端/服务器角色；不改变普通客户端 SN 协议。 |

### Quote: design.md sn_owner_directory_peer_transport
> | sn_owner_directory_peer_transport | P-SN-DIST-OWNER-PEER-TRANSPORT-1 | Owner peer transport and command node using `NetManager + TtpNode + DefaultCmdNodeService`; no `TtpClient`/`TtpServer` substitution | `Cargo.lock`, `p2p-frame/Cargo.toml`, `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/ttp/node.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | adds independent OwnerDirectoryServer listener/connect path and owner command dispatch | command service organization references existing SN command service pattern and `SnCmdService` |
