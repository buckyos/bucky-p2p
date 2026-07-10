# Implementation Admission Evidence: Owner Directory Server Service Split

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | pass | Read approved proposal covering `sn_directory_client_server_boundary`. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | pass | Read approved design direct mapping for `sn_directory_client_server_boundary`. |
| change_scope_matches_request | user request and approved packet | pass | User requested separating `OwnerDirectoryServer` communication and logic like `SnServer`/`SnService`; this remains inside `p2p-frame/src/sn/directory/**` and preserves directory server/client boundaries. |
| active_module_resolved | packet front matter and path | pass | Active version is `v0.1`, module is `p2p-frame`, direct submodule is `sn-distributed-directory`. |
| no_chat_only_evidence | proposal/design mapping tables | pass | Implementation admission relies on approved versioned proposal/design rows and the existing server/client boundary change_id. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | fceaffe056acabc64ba2c1a3faad3bf1044f0e4a5cc32b20539971021cce90d4 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | dc8d2a408cc3d41468d49860bacd66903d971a5db44274cb5194b4a97f3f41f9 |

## Coverage Quotes

### Quote: proposal.md sn_directory_client_server_boundary
> | P-SN-DIST-DIRECTORY-SERVER-1 | sn_directory_client_server_boundary | Owner SN 功能从 `SnService` 中拆出，作为 `sn/directory` 模块内部 server 侧能力承载；Serving SN 只通过 directory client 接口交互。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/mod.rs`、必要装配路径。 | `SnService` 不包含 owner membership/resolver/store/owner command handler；directory server 可独立验证 lease publish/query；Serving SN 只通过 directory client/inter-SN 接口交互。 | 不在 `SnService` 内保留 ownerSN 状态字段或 ownerSN command 分发；不再暴露独立 `sn::owner` 模块。 |

### Quote: design.md sn_directory_client_server_boundary
> | sn_directory_client_server_boundary | P-SN-DIST-DIRECTORY-SERVER-1 | `sn_directory` client/server split, serving-only `SnService`, OwnerDirectoryClient interface | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | moves owner membership/resolver/store/command handler out of `SnService` while keeping ownerSN in directory domain | `SnService` remains servingSN only; no `sn::owner` module |
