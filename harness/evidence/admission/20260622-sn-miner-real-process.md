# Implementation Admission: SN Miner Configured Roles And Real Process SN Validation

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/sn-miner/proposal.md`; `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | pass | Approved proposals read; they define config-driven owner/serving roles, no registry fallback, independent online/route loops, and real process evidence. |
| design_read | `docs/versions/v0.1/modules/sn-miner/design.md`; `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | pass | Approved designs read; they map all admitted change_ids to concrete Scope Paths. |
| change_scope_matches_request | user request and approved proposal/design | pass | The task matches the approved request to implement sn-miner config roles, role exclusivity, p2p no-fallback semantics, independent Serving SN online/route loops, and real process tests. |
| active_module_resolved | `module: sn-miner`; `module: p2p-frame`, `submodule: sn-distributed-directory` | pass | Cross-module work is bound to the sn-miner packet and the p2p-frame direct submodule packet. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Implementation admission uses approved docs, not chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/sn-miner/proposal.md | 17227d3aa5146f0f16ecfdfd213ebf5207af4d2ce21a16723f6e571852f3557b |
| docs/versions/v0.1/modules/sn-miner/design.md | 384817722f91d3ddc1a89f644778228e01fda78a01e76622e6b2e80bb9917e81 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | 81c581a51230151d554da820006fdd08f1710a49eeb17990364bda59dc515d11 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | a793c946805488aa0bc4401a4485c886b48614d13c0b7712c70359550773ab83 |

## Coverage Quotes
### Quote: proposal.md sn_miner_startup_audit
> | P-1 | sn_miner_startup_audit | 服务启动改动可审计 | `sn-miner-rust/src/main.rs` 启动入口与必要启动资源 | proposal/design/testing/acceptance 证据链齐全 | 不拥有 CYFS 适配层逻辑 |

### Quote: design.md sn_miner_startup_audit
> | sn_miner_startup_audit | P-1 | split config load, identity load, and role startup into auditable paths | `sn-miner-rust/src/**`, `sn-miner-rust/Cargo.toml` | internal startup structure only | keeps assembly boundary |

### Quote: proposal.md sn_miner_artifact_defaults
> | P-2 | sn_miner_artifact_defaults | 默认制品和启动行为不再静默漂移 | `sn-miner-rust/src/main.rs`、`sn-miner-rust/config/**` | testing 与 DV 证据覆盖启动和制品行为 | 不替换核心 SN 协议语义 |

### Quote: design.md sn_miner_artifact_defaults
> | sn_miner_artifact_defaults | P-2 | preserve desc/sec defaults while making role defaults explicit | `sn-miner-rust/src/**`, `sn-miner-rust/config/**` | config/default behavior visible to operators | default path must be tested |

### Quote: proposal.md sn_miner_config_file_roles
> | P-3 | sn_miner_config_file_roles | 配置文件声明 `owner` 或 `serving` 角色，并驱动对应启动路径 | `sn-miner-rust/src/main.rs`、配置 schema/fixture | config unit 覆盖 schema/default/invalid cases；DV 覆盖配置启动 | 不以 CLI flags 作为长期部署接口；不混合启动两种角色 |

### Quote: design.md sn_miner_config_file_roles
> | sn_miner_config_file_roles | P-3 | `--config` loads schema with top-level role and role-specific sections | `sn-miner-rust/src/**`, `sn-miner-rust/Cargo.toml`, `sn-miner-rust/config/**` | new operator config contract | parser dependency allowed if needed |

### Quote: proposal.md sn_miner_owner_role_startup
> | P-4 | sn_miner_owner_role_startup | Owner 角色启动 OwnerDirectoryServer owner-peer listener 和 serving-facing listener | `sn-miner-rust/src/main.rs`、配置资源、必要测试夹具 | 真实进程测试验证 owner 双 listener 可启动、端口冲突失败、进程可关闭 | 不启动 peer-facing SnService；不启动 PnServer |

### Quote: design.md sn_miner_owner_role_startup
> | sn_miner_owner_role_startup | P-4 | owner branch starts OwnerDirectoryServer owner peer/control and serving-facing listeners only | `sn-miner-rust/src/**`, `sn-miner-rust/config/**` | owner process no longer starts SnService/PnServer | real process bind failure required |

### Quote: proposal.md sn_miner_serving_role_startup
> | P-5 | sn_miner_serving_role_startup | Serving 角色启动 SnService/PnServer 并通过配置连接 OwnerDirectoryServer serving-facing endpoint | `sn-miner-rust/src/main.rs`、配置资源、必要测试夹具 | 真实进程测试验证 serving 进程通过 owner endpoint 完成 online/route/query/call 前置链路 | 不启动 OwnerDirectoryServer；不使用全局 registry fallback |

### Quote: design.md sn_miner_serving_role_startup
> | sn_miner_serving_role_startup | P-5 | serving branch starts SnService/PnServer with owner membership/endpoints, online heartbeat config, route publish config | `sn-miner-rust/src/**`, `sn-miner-rust/config/**` | serving process no longer starts OwnerDirectoryServer | relies on p2p-frame SN config APIs |

### Quote: proposal.md sn_miner_role_exclusive_validation
> | P-6 | sn_miner_role_exclusive_validation | 同一配置或进程实例不能同时声明/启动 owner 与 serving | 配置解析和启动前校验 | invalid config test 和 real process negative test 均失败在启动前 | 不提供 owner+serving mixed mode |

### Quote: design.md sn_miner_role_exclusive_validation
> | sn_miner_role_exclusive_validation | P-6 | validation rejects missing role, mixed owner/serving sections, or role/section mismatch before startup | `sn-miner-rust/src/**` | startup safety contract | invalid config exits non-zero |

### Quote: proposal.md sn_directory_no_registry_fallback
> | P-SN-DIST-NO-FALLBACK-1 | sn_directory_no_registry_fallback | 删除 Owner SN / Serving SN 目录通信的全局 registry fallback；生产、测试和兼容路径都必须使用真实 transport 或显式 fake transport seam。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、`p2p-frame/src/sn/service/**`、必要测试支撑。 | 搜索和测试证据证明 Serving SN directory publish/query、inter-SN detail/relay、owner control command 不依赖 global registry fallback；unit fake 必须显式注入。 | 不禁止单元测试使用 fake transport trait；禁止隐式 singleton registry fallback。 |

### Quote: design.md sn_directory_no_registry_fallback
> | sn_directory_no_registry_fallback | P-SN-DIST-NO-FALLBACK-1 | production/test/compat directory communication uses real transport or explicit fake seam; global registry fallback and same-process owner lookup are removed | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/service/**`, `p2p-frame/src/sn/mod.rs` | missing configured transport is an error instead of silent fallback | fake transport must be constructor-injected |

### Quote: proposal.md sn_serving_online_route_independent_loops
> | P-SN-DIST-ONLINE-ROUTE-LOOPS-1 | sn_serving_online_route_independent_loops | Serving SN online heartbeat/renew/offline 与 PeerRoute publish/query 是独立 runtime loop、独立配置和独立失败处理。 | `p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/directory/**`、`sn-miner-rust/**` 装配。 | 测试证明 `ReportSn` 只更新本地 detail；online heartbeat 不写 PeerRoute；route publish 不刷新 Serving SN online；两者失败可独立观测。 | 不要求两者使用不同网络连接；只要求语义和触发独立。 |

### Quote: design.md sn_serving_online_route_independent_loops
> | sn_serving_online_route_independent_loops | P-SN-DIST-ONLINE-ROUTE-LOOPS-1 | Serving SN online heartbeat loop and PeerRoute publish loop have separate config, trigger, lifecycle, retry/error handling and observations | `p2p-frame/src/sn/service/**`, `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | Serving SN availability and peer route freshness remain semantically separate | shared client allowed; shared trigger/failure state not allowed |

### Quote: proposal.md sn_real_process_owner_serving_dv
> | P-SN-DIST-REAL-PROCESS-1 | sn_real_process_owner_serving_dv | 新增真实进程 DV/integration，启动独立 Owner SN 与 Serving SN 进程验证配置、listener、connector、query/call 和失败路径。 | `p2p-frame` DV/integration harness、`sn-miner-rust/**` 配置/启动、必要测试夹具。 | 测试证据包含进程启动、端口分配、owner 双 listener、serving 连接 owner serving-facing endpoint、online heartbeat、route publish/query、跨 serving query/call、shutdown 和至少一个 invalid config/port conflict 失败路径。 | 不把 unit 5x5 command matrix 当作真实进程证据。 |

### Quote: design.md sn_real_process_owner_serving_dv
> | sn_real_process_owner_serving_dv | P-SN-DIST-REAL-PROCESS-1 | process-level seams expose owner listener config, serving endpoint config, shutdown, invalid config and port conflict behavior | `p2p-frame/src/sn/**`, `sn-miner-rust/**` | testing owns process harness; implementation exposes stable startup/config behavior | no unit-only substitute for final evidence |
