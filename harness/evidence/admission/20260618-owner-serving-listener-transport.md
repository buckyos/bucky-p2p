# Task 20260618-owner-serving-listener-transport Admission Evidence

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | pass | Read approved proposal boundary for OwnerDirectoryServer dual listeners and serving-facing directory entrypoint. |
| design_read | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | pass | Read approved design requiring owner peer `NetManager + TtpNode + DefaultCmdNodeService` and serving-facing `NetManager + TtpServer + DefaultCmdServerService`. |
| change_scope_matches_request | proposal P-SN-DIST-OWNER-SERVING-LISTENER-1 / design sn_owner_serving_listener_transport | pass | The request asks OwnerDirectoryServer to create a separate listener for Serving SN communication. |
| active_module_resolved | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory | pass | The touched code belongs to the p2p-frame SN distributed directory direct submodule. |
| no_chat_only_evidence | versioned docs only | pass | Implementation admission uses approved proposal/design content and not chat-only assumptions. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | 44b62063d7c6b01fd8bc7857a580717f37598174004b1e1a6514aae9916ae496 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | 355e4d1dbbf59006338215dd5d070a18afd668be8a3cd37fccf6479dad67f874 |

## Coverage Quotes

### Quote: proposal.md sn_owner_serving_listener_transport
> | P-SN-DIST-OWNER-SERVING-LISTENER-1 | sn_owner_serving_listener_transport | OwnerDirectoryServer 提供独立 serving-facing listener，供 Serving SN 发布/query directory lease；该入口与 owner peer listener 端口、命令空间和 handler 集合隔离。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 serving directory command 接入、`sn-miner-rust` owner directory 装配。 | OwnerDirectoryServer 启动后同时拥有 owner peer listener 和 serving-facing listener；Serving SN 能通过 serving-facing listener 访问 owner directory；owner peer 命令和 serving-facing 命令互不复用。 | 不通过 SnServer listener、owner peer listener 或同进程 shortcut 处理 Serving SN 到 owner directory 的 publish/query。 |

### Quote: design.md sn_owner_serving_listener_transport
> | sn_owner_serving_listener_transport | P-SN-DIST-OWNER-SERVING-LISTENER-1 | Serving-facing OwnerDirectoryServer listener and command server using `NetManager + TtpServer + DefaultCmdServerService`; isolated from owner peer `NetManager + TtpNode + DefaultCmdNodeService` | `Cargo.lock`, `p2p-frame/Cargo.toml`, `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/protocol/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | adds independent Serving SN -> OwnerDirectoryServer publish/query entrypoint | does not reuse SnServer listener or owner peer listener |
