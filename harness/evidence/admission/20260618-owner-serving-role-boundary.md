# Task 20260618-owner-serving-role-boundary Admission Evidence

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | pass | Read approved proposal boundary for independent Owner SN and Serving SN runtime roles. |
| design_read | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | pass | Read approved design requiring OwnerDirectoryServer transport to stay outside SnServer serving listener logic. |
| change_scope_matches_request | proposal P-SN-DIST-ROLE-1 / design sn_owner_serving_role_boundary | pass | The implementation removes owner peer transport responsibility from SnServer and keeps serving peer-facing listener logic separate. |
| active_module_resolved | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory | pass | The touched code belongs to the p2p-frame SN distributed directory direct submodule. |
| no_chat_only_evidence | versioned docs only | pass | Implementation admission uses approved proposal/design content and not chat-only assumptions. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | 94d93618471b4f857384efd32d5276fc2096e5fbb9c21ceab189914b3ef00d12 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | e471e5128a12225fe790348e8900a05d328b4620fb0f0c73d63102fb37570d5e |

## Coverage Quotes

### Quote: proposal.md sn_owner_serving_role_boundary
> | P-SN-DIST-ROLE-1 | sn_owner_serving_role_boundary | Owner SN 与 Serving SN 作为独立运行角色和业务边界建模，生产逻辑不假设两者同进程或共享 peer-facing handler。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、`p2p-frame/src/sn/service/**` 中角色接口与装配边界。 | Owner 目录能力可独立于 serving detail/call 逻辑验证；Serving SN 通过接口发布/query lease 和拉取/relay detail，不直接访问 owner 内部 store。 | 不为了单 SN 兼容把 owner/serving 逻辑重新混成同一个业务路径。 |

### Quote: design.md sn_owner_serving_role_boundary
> | sn_owner_serving_role_boundary | P-SN-DIST-ROLE-1 | Owner role vs Serving role runtime boundary, Owner lease publish/query flow, role-owned Data and State | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | separates owner lease authority from serving peer-facing detail/call authority | local single-process fallback must use the same role interface |
