---
module: sn-miner
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-06-22T21:11:15+08:00
approved_content_sha256: 9bfa9a442900e478c87e461c6ed64a2c7e8ffb84a9380be472b86feff9840f4f
---

# sn-miner 设计

## Design Scope
### Goals
- 用配置文件作为长期部署入口，显式选择 `owner` 或 `serving` 角色。
- 保证一个 `sn-miner` 进程实例一次只启动一种角色。
- Owner 角色只装配 OwnerDirectoryServer owner peer/control listener 与 serving-facing listener。
- Serving 角色只装配 peer-facing SnService/PnServer、owner membership、owner serving endpoint、Serving SN online heartbeat 配置和 PeerRoute publish 配置。
- 保留 desc/sec 加载或创建行为，并把配置错误、制品错误、端口错误和远端 owner endpoint 缺失作为启动前或启动期失败。

### Non-goals
- 不在 `sn-miner` 内实现 owner election、online state machine、PeerRoute 分区算法、detail merge 或 relay call 语义。
- 不提供 owner+serving 单进程混合模式。
- 不用 CLI flag 替代配置文件作为长期部署接口。
- 不通过全局 registry 或同进程 shortcut 连接 Owner SN 与 Serving SN。

## Overall Approach
`sn-miner` 保持 assembly binary 职责。入口先解析 `--config` 和可选 legacy `--desc`，再加载配置文件并做 schema/role validation。配置模型只有一个顶层 `role`，取值为 `owner` 或 `serving`；角色专属字段只能出现在对应 section。配置同时声明 owner 和 serving section、缺少 role、role 与 section 不匹配、缺少必填 endpoint 或 membership 时，进程在启动服务前失败。

Owner 角色构建本地身份、owner membership、owner peer/control endpoints 和 serving-facing endpoints，然后调用 `OwnerDirectoryServer::new_with_default_runtime(...).start()`。Owner 角色不得创建 `SnService` 或 `PnServer`。

Serving 角色构建本地身份、`SnServiceConfig`、owner membership、owner serving-facing endpoints、online heartbeat 配置和 route publish 配置，然后创建 `SnService` 和 `PnServer`。Serving 角色不得创建 `OwnerDirectoryServer`。Serving SN online heartbeat 与 PeerRoute publish 是两个配置面和两个 runtime task；`sn-miner` 只把配置传给 `p2p-frame`，不把每个 `ReportSn` 等价为 online renew 或 route publish。

CLI 兼容边界：`--config <path>` 是新主入口；未传 `--config` 时只允许走现有单 SN/debug 默认路径，且不得隐式启动 owner+serving 混合模式。legacy owner endpoint flags 在 implementation 中应被配置文件取代或保留为 explicit debug path，但不能绕过 role validation。

## Simplicity Check
- Smallest sufficient approach: 一个配置结构、一个 role enum、两个 startup 分支和共享 desc/sec loader。
- Existing components reused: `OwnerDirectoryServer`、`OwnerMembership`、`SnServiceConfig`、`create_sn_service`、`PnServer`、现有 desc/sec loader。
- New abstractions introduced: `SnMinerConfig`、`SnMinerRole`、`OwnerRoleConfig`、`ServingRoleConfig`、`OnlineHeartbeatConfig`、`RoutePublishConfig`、`run_owner_role`、`run_serving_role`。
- Why necessary: 配置结构隔离 schema validation；role enum 强制互斥；role config 避免把 owner/serving 必填字段混在一起；两个 run 函数让 admission/test 能分别验证启动边界。

## Current Structure
- `sn-miner-rust/src/main.rs` 当前同时解析 desc、owner-members、owner-peer-endpoints、owner-serving-endpoints，并在 owner endpoints 配齐时启动 OwnerDirectoryServer，随后仍继续启动 SnService/PnServer。
- 该结构会让单个进程同时具备 Owner SN 与 Serving SN 行为，不满足批准 proposal 的角色互斥要求。
- `sn-miner-rust/config/package.cfg` 仅是 package/config 表面，没有部署级 SN role schema。
- `harness/scripts/test-run.py` 对 `sn-miner` 的 DV 只运行 `cargo run -p sn-miner -- --help`，缺少真实 owner/serving 进程证据。

## Invariants to Preserve
- `--desc` 或配置中的 desc path 仍指向不带扩展名的 desc/sec 文件基路径。
- 缺失 desc/sec 时的既有创建行为除非显式配置禁止，否则保持兼容。
- 未配置 owner directory 的单 SN/debug 路径不得隐式启动 owner role。
- 客户端可见 SN 协议不由 `sn-miner` 改变。
- `sn-miner` 不拥有 `p2p-frame` SN directory/service 的协议状态。
- 配置解析失败、角色冲突、端口冲突和必填 owner endpoint 缺失必须以非零退出码暴露。

## Submodules
| Submodule | Type | Responsibility | Depends On | Exported Interface | Notes |
|-----------|------|----------------|------------|--------------------|-------|
| `config_loader` | technical | 读取配置文件、解析 schema、应用默认值、输出 validated config | filesystem, config parser crate or existing dependency | `SnMinerConfig::load(path)` | 只处理部署配置，不启动服务 |
| `artifact_loading` | technical | 加载或创建 desc/sec，返回 identity input | filesystem, bucky objects/crypto | existing `load_device_info` behavior | shared by both roles |
| `role_startup` | assembly | 根据 validated role 调用 owner 或 serving startup | `config_loader`, `artifact_loading`, `cyfs-p2p`, `p2p-frame` reexports | `run_owner_role`, `run_serving_role` | assembly only; no module depends on it |
| `process_test_harness` | assembly | 为 DV/integration 提供配置 fixture、端口分配、进程启动/停止约定 | test runner, OS process APIs | test-only process launcher | testing stage owns implementation details |

## Large Module Submodule Decision
| Submodule | Source Proposal | Decision | Design Packet | Reason |
|-----------|-----------------|----------|---------------|--------|
| `sn-miner-configured-roles` | P-3 / P-4 / P-5 / P-6 | keep in module packet | `docs/versions/v0.1/modules/sn-miner/design.md` | `sn-miner` 只有一个小型启动 binary 和配置资源，不需要 direct submodule packet。 |

## Boundary Decision Matrix
| boundary | classification | business_responsibility | shared_logic_or_technical_area | decision |
|----------|----------------|-------------------------|--------------------------------|----------|
| config parsing vs service startup | technical | 配置决定部署角色，启动执行副作用 | `config_loader` vs `role_startup` | split so invalid config fails before listeners start |
| owner role vs serving role | business | Owner SN owns directory/control listeners; Serving SN owns peer-facing service | role enum and role-specific config | split into mutually exclusive startup branches |
| desc/sec loading vs role config | technical | identity artifacts are shared; role behavior is deployment-specific | `artifact_loading` reused by both roles | keep loader shared, but role validation separate |
| online heartbeat vs route publish | business | Serving availability and peer route publication have different semantics | separate config fields and runtime handles | split; no shared trigger knob |
| real process tests vs unit config tests | technical | config validation is local; listener/transport/shutdown needs OS process evidence | unit parser seam vs process harness | both required at different test levels |

## Boundary Rationale
| Boundary | Classification | Why Separate | Shared Logic / Technical Area | Notes |
|----------|----------------|--------------|-------------------------------|-------|
| Owner SN startup vs Serving SN startup | business | 同进程混合会隐藏真实 transport 和端口问题 | shared identity loader only | role enum is the enforcement point |
| Config file vs CLI flags | technical | 配置文件可审计且能表达 membership、endpoint、heartbeat、publish 参数 | CLI only locates config/debug desc | CLI override cannot bypass schema |
| sn-miner assembly vs p2p-frame SN logic | business | SN directory/service owns protocol state | reexported constructors/config structs | sn-miner does not duplicate algorithms |

## Dependency Graph
| Source | Depends On | Reason | Cycle Check |
|--------|------------|--------|-------------|
| `config_loader` | filesystem, parser dependency | load deployment config | acyclic |
| `artifact_loading` | bucky objects/crypto | load identity artifacts | acyclic |
| `role_startup` | `config_loader`, `artifact_loading`, `cyfs-p2p`, `p2p-frame` SN APIs | assemble selected role | acyclic; assembly has no consumers |
| `process_test_harness` | built binary, config fixtures | validate process lifecycle | acyclic; testing-only |

## Key Call Flows
| Flow | Caller | Callee / Submodule Path | Purpose | Failure Handling | Notes |
|------|--------|--------------------------|---------|------------------|-------|
| Config load | `main` | `config_loader` | read and validate config file | unreadable file, parse error, missing role, mixed role sections, missing required fields return clear startup error and exit non-zero | happens before service startup |
| Identity load | `main` | `artifact_loading` | load/create desc/sec | file decode/create errors exit non-zero before listeners start | shared by both roles |
| Owner role startup | `main` | `role_startup::run_owner_role` -> `OwnerDirectoryServer` | start owner peer/control and serving-facing listeners | invalid membership or endpoint parse fails before start; listener bind failure exits non-zero; no SnService/PnServer is created | owner-only process |
| Serving role startup | `main` | `role_startup::run_serving_role` -> `create_sn_service` / `PnServer` | start peer-facing serving SN service | missing owner membership/endpoints or service start failure exits non-zero; no OwnerDirectoryServer is created | serving-only process |
| Online heartbeat config | `run_serving_role` | `SnServiceConfig` / p2p-frame online reporter | configure Serving SN availability loop | heartbeat failure is observable by p2p-frame loop and does not publish PeerRoute | independent loop |
| Route publish config | `run_serving_role` | `SnServiceConfig` / p2p-frame route publisher | configure PeerRoute publish loop | publish failure is observable by route publisher and does not renew Serving SN online | independent loop |
| Real process validation | test runner | built `sn-miner` binary with config fixtures | prove owner/serving process behavior | timeout kills child and records logs; invalid config/port conflict must fail with non-zero exit | testing stage owns fixtures and runner wiring |

## Trigger Matrix
| trigger_category | applies | evidence | design_coverage | required_checks | deferred_checks_and_reason |
|------------------|---------|----------|-----------------|-----------------|----------------------------|
| contract/protocol | yes | role config selects existing owner/serving startup interfaces | Key Call Flows, Interfaces | unit config tests and p2p-frame compatibility checks | owner: testing; risk: wrong role starts wrong protocol surface |
| data/schema | yes | new config schema and defaults | Data and State, Interfaces | config parser unit tests, invalid config tests | owner: testing; risk: deployment drift |
| security/privacy/permission | yes | membership and owner endpoints feed SN validators | Interfaces, Key Call Flows | tests ensure sn-miner does not bypass p2p-frame validators | owner: testing; risk: unauthorized directory writes |
| runtime/integration | yes | owner/serving listeners and independent processes | Key Call Flows, Testability | real process DV/integration | owner: testing; risk: unit tests miss bind/shutdown/transport failures |
| build/dependency/config/deployment | yes | parser dependency, config fixture, package config | Implementation Order, Scope Paths | cargo check/test, config fixture review | owner: implementation/testing |
| ui/datamodel/workflow | no | no UI | not-applicable | not-applicable | owner: none |
| harness/process | yes | approved proposal and auto-pipeline plan | Directly Mapped Change Items | doc-structure, schema, admission, stage-scope | owner: all stages |

## Directly Mapped Change Items
| change_id | proposal_id | Design Coverage | Scope Paths | Interface / Boundary Impact | Notes |
|-----------|-------------|-----------------|-------------|-----------------------------|-------|
| sn_miner_startup_audit | P-1 | split config load, identity load, and role startup into auditable paths | `sn-miner-rust/src/**`, `sn-miner-rust/Cargo.toml` | internal startup structure only | keeps assembly boundary |
| sn_miner_artifact_defaults | P-2 | preserve desc/sec defaults while making role defaults explicit | `sn-miner-rust/src/**`, `sn-miner-rust/config/**` | config/default behavior visible to operators | default path must be tested |
| sn_miner_config_file_roles | P-3 | `--config` loads schema with top-level role and role-specific sections | `sn-miner-rust/src/**`, `sn-miner-rust/Cargo.toml`, `sn-miner-rust/config/**` | new operator config contract | parser dependency allowed if needed |
| sn_miner_owner_role_startup | P-4 | owner branch starts OwnerDirectoryServer owner peer/control and serving-facing listeners only | `sn-miner-rust/src/**`, `sn-miner-rust/config/**` | owner process no longer starts SnService/PnServer | real process bind failure required |
| sn_miner_serving_role_startup | P-5 | serving branch starts SnService/PnServer with owner membership/endpoints, online heartbeat config, route publish config | `sn-miner-rust/src/**`, `sn-miner-rust/config/**` | serving process no longer starts OwnerDirectoryServer | relies on p2p-frame SN config APIs |
| sn_miner_role_exclusive_validation | P-6 | validation rejects missing role, mixed owner/serving sections, or role/section mismatch before startup | `sn-miner-rust/src/**` | startup safety contract | invalid config exits non-zero |

## Implementation Order
| Phase | Goal | Preconditions | Outputs | Depends On | Parallel |
|-------|------|---------------|---------|------------|----------|
| 1 | Introduce config model and parser | approved design/admission | `SnMinerConfig` and validation errors | none | no |
| 2 | Extract shared identity loader and startup helpers | Phase 1 | reusable owner/serving inputs | Phase 1 | no |
| 3 | Implement owner-only startup branch | Phase 2 | OwnerDirectoryServer-only process path | Phase 2 | no |
| 4 | Implement serving-only startup branch | Phase 2 | SnService/PnServer-only process path with owner config | Phase 2 | no |
| 5 | Replace or constrain legacy CLI flags | Phase 3-4 | `--config` primary path and no mixed-mode debug bypass | Phase 4 | no |
| 6 | Add test hooks needed by post-implementation testing | Phase 1-5 | deterministic exit/error/log behavior for process tests | Phase 5 | yes |

## Key Decisions
| Decision | Chosen | Alternatives Considered | Rejection Reason |
|----------|--------|-------------------------|------------------|
| Deployment interface | config file plus `--config` locator | many CLI flags for every field | too hard to audit and cannot express nested owner/serving config cleanly |
| Role model | single required `role` with role-specific section | infer role from present fields | inference makes mixed config ambiguous and risks starting wrong services |
| Owner/serving process model | one role per process | start both roles when all fields are present | hides real transport and violates approved proposal |
| Loop configuration | independent online heartbeat and route publish config | one shared interval or trigger | conflates serving availability with peer route publication |
| Real process testing | post-implementation DV/integration process launcher | unit-only startup tests | unit tests cannot prove port bind, process shutdown, or transport behavior |

## Data and State
| Data or State | Owner Submodule | Access For Others | State Transitions |
|---------------|-----------------|-------------------|-------------------|
| `SnMinerConfig` | `config_loader` | read-only by `role_startup` | absent -> loaded -> validated -> rejected/accepted |
| role selection | `config_loader` | `main` dispatches exactly one branch | missing -> rejected; owner -> owner branch; serving -> serving branch; mixed -> rejected |
| desc/sec artifacts | `artifact_loading` | identity input passed to selected branch | missing -> created; present -> decoded; invalid -> rejected |
| owner process runtime | `role_startup` | OS process lifecycle only | not started -> listeners started -> shutdown/error |
| serving process runtime | `role_startup` | OS process lifecycle only | not started -> peer service started -> heartbeat/publish loops active -> shutdown/error |
| online heartbeat config | `config_loader` | passed to p2p-frame service config | defaulted/explicit -> validated -> runtime loop input |
| route publish config | `config_loader` | passed to p2p-frame service config | defaulted/explicit -> validated -> runtime loop input |

## Testability
- `config_loader` can be unit-tested with valid owner config, valid serving config, missing role, mixed role sections, invalid endpoint, missing membership, and invalid intervals.
- `run_owner_role` should expose a seam that lets tests start on ephemeral ports and verify no SnService/PnServer branch was constructed.
- `run_serving_role` should expose a seam that lets tests inject owner endpoint config and verify no OwnerDirectoryServer branch was constructed.
- Process tests use generated temp configs and OS-assigned or reserved ports, collect stdout/stderr/logs, enforce timeouts, and always kill child processes on failure.
- Failure tests must cover invalid config and at least one port/listener conflict or missing endpoint path.

## Interfaces and Dependencies
### Exported interfaces
| Interface | Consumer | Compatibility | Notes |
|-----------|----------|---------------|-------|
| `--config <path>` CLI | operators, `sn_miner_config_file_roles` | new | primary deployment entry |
| `SnMinerConfig` schema | `sn_miner_config_file_roles`, process tests | new | owned inside sn-miner unless implementation needs a module split |
| owner role config section | `sn_miner_owner_role_startup` | new | includes desc path, membership, owner peer/control endpoints, serving-facing endpoints |
| serving role config section | `sn_miner_serving_role_startup` | new | includes desc path, membership, owner serving endpoints, heartbeat config, route publish config |
| legacy `--desc` path | existing debug usage | backward-compatible | may remain for no-config local/default mode, but cannot start mixed owner+serving |

### External module dependencies
- `cyfs-p2p` / `p2p-frame` provide OwnerDirectoryServer, SnServiceConfig, SnService, PnServer, owner membership and directory/service behavior.
- `p2p-frame/sn-distributed-directory` owns no-registry transport and independent online/route loop semantics.

### External service or protocol constraints
- OwnerDirectoryServer serving-facing endpoint is not interchangeable with owner peer/control endpoint.
- Serving SN online heartbeat and PeerRoute publish may share a network client, but their triggers, config and failure state remain separate.

## Document Index
| Document | Topic | Scope |
|----------|-------|-------|
| `proposal.md` | sn-miner role/config requirements | module proposal |
| `design.md` | config-driven owner/serving startup design | module design |

## Risks and Rollback
- Risk: existing local debugging expected mixed owner+serving behavior. Mitigation: require two configs/processes; keep no-config single SN/debug path if compatible.
- Risk: parser dependency changes build surface. Mitigation: prefer existing dependencies if available; otherwise keep new dependency scoped to `sn-miner-rust/Cargo.toml`.
- Risk: process tests are flaky around ports and shutdown. Mitigation: stable port allocation, log capture, timeout cleanup, and lower-level unit tests for parser branches.
- Rollback: disable `--config` role startup and keep legacy single SN path, but do not reintroduce owner+serving mixed mode without returning to proposal.

## Design Guardrails
- Do not rewrite approved proposal intent in design.
- Keep implementation code and testing artifacts out of design-stage work.
- Every implementation-ready design item must carry a proposal `change_id`.

## Approval Record
- approver:
- approval_date:
- user_statement: ""
