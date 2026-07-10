# Implementation Admission: SN Miner Role Readiness Observability

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/sn-miner/proposal.md` | pass | Approved proposal read; owner startup requires two real listeners and process shutdown evidence, while serving startup requires a real serving process path. |
| design_read | `docs/versions/v0.1/modules/sn-miner/design.md` | pass | Approved design read; owner/serving startup flows require listener/start failures to be observable and implementation phase 6 permits deterministic test hooks. |
| change_scope_matches_request | `sn_miner_owner_role_startup`, `sn_miner_serving_role_startup` | pass | A structured marker emitted only after each role's required start calls is the minimal production observability needed for the approved real-process readiness evidence. |
| active_module_resolved | `module: sn-miner`, `version: v0.1` | pass | `sn-miner-rust/src/main.rs` owns role assembly and stdout startup observability; process probes remain testing-stage code. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on the approved sn-miner packet, not the chat-only instruction. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/sn-miner/proposal.md | 17227d3aa5146f0f16ecfdfd213ebf5207af4d2ce21a16723f6e571852f3557b |
| docs/versions/v0.1/modules/sn-miner/design.md | 384817722f91d3ddc1a89f644778228e01fda78a01e76622e6b2e80bb9917e81 |

## Coverage Quotes

### Quote: proposal.md sn_miner_owner_role_startup
> | P-4 | sn_miner_owner_role_startup | Owner 角色启动 OwnerDirectoryServer owner-peer listener 和 serving-facing listener | `sn-miner-rust/src/main.rs`、配置资源、必要测试夹具 | 真实进程测试验证 owner 双 listener 可启动、端口冲突失败、进程可关闭 | 不启动 peer-facing SnService；不启动 PnServer |

### Quote: design.md sn_miner_owner_role_startup
> | sn_miner_owner_role_startup | P-4 | owner branch starts OwnerDirectoryServer owner peer/control and serving-facing listeners only | `sn-miner-rust/src/**`, `sn-miner-rust/config/**` | owner process no longer starts SnService/PnServer | real process bind failure required |

### Quote: proposal.md sn_miner_serving_role_startup
> | P-5 | sn_miner_serving_role_startup | Serving 角色启动 SnService/PnServer 并通过配置连接 OwnerDirectoryServer serving-facing endpoint | `sn-miner-rust/src/main.rs`、配置资源、必要测试夹具 | 真实进程测试验证 serving 进程通过 owner endpoint 完成 online/route/query/call 前置链路 | 不启动 OwnerDirectoryServer；不使用全局 registry fallback |

### Quote: design.md sn_miner_serving_role_startup
> | sn_miner_serving_role_startup | P-5 | serving branch starts SnService/PnServer with owner membership/endpoints, online heartbeat config, route publish config | `sn-miner-rust/src/**`, `sn-miner-rust/config/**` | serving process no longer starts OwnerDirectoryServer | relies on p2p-frame SN config APIs |
