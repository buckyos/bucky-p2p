---
module: sn-miner
version: v0.1
status: approved
approved_by: user
approved_at: 2026-06-22T21:02:36+08:00
approved_content_sha256: 5c62d47ba0097b578627dc776964e86adb2d224e06bcc433c7d750d067a03644
---

# sn-miner 提案

## Background and Goal
- `sn-miner` 是负责 SN 行为的服务启动二进制。
- 目标是让启动流程、制品加载和默认运行时行为，和工作区其他模块一样受到严格的分阶段工作流约束。
- 新目标是让 `sn-miner` 通过配置文件选择启动为 Owner SN 或 Serving SN，且单个进程实例一次只能启动其中一种角色。
- `sn-miner` 只负责读取配置、校验角色互斥、装配运行时依赖并启动对应服务；owner membership、Serving SN online 心跳、PeerRoute publish、授权语义仍由 `p2p-frame` 的 SN directory/service 负责。

## Scope
### In scope
- CLI 和启动流程
- desc/sec 的加载或创建行为
- 运行时服务启动和配置默认值
- 配置文件加载、解析、默认值和错误报告
- 通过配置文件选择 `owner` 或 `serving` 角色
- Owner SN 启动时装配 OwnerDirectoryServer owner-peer listener 和 serving-facing listener
- Serving SN 启动时装配 owner membership、owner serving endpoints、Serving SN online 心跳配置、PeerRoute publish 配置和 SN service
- 单实例角色互斥校验：同一 `sn-miner` 进程不得同时启动 Owner SN 和 Serving SN

### Out of scope
- 拥有 CYFS 适配层逻辑
- 替换核心 SN 协议语义
- 在 `sn-miner` 内实现 owner election、Serving SN online 状态机、PeerRoute 分区算法或跨 SN relay 语义
- 在 `sn-miner` 内提供 owner 与 serving 同进程混合运行模式
- 用 CLI flag 取代配置文件作为长期部署接口

### Boundary with neighboring modules
- `cyfs-p2p` 提供底层栈和服务创建路径。
- `sn-miner` 只负责启动行为和操作默认值。
- `p2p-frame/src/sn/directory/**` 拥有 Owner SN control plane、owner peer transport、serving-facing directory command 和 route store。
- `p2p-frame/src/sn/service/**` 拥有 Serving SN peer-facing report/query/call、online reporter、route publish policy 和 inter-SN detail/relay。
- `sn-miner` 只能消费上述能力进行装配，不得重定义协议或绕过角色边界。

## Assumptions and Ambiguities
- Assumptions:
  - 生产部署更需要可审计配置文件，而不是依赖多组临时 CLI flag。
  - Owner SN 与 Serving SN 是不同运行角色；同一进程同时启动两者会掩盖真实跨进程 transport、端口和授权问题。
  - 角色互斥不禁止同一机器运行两个不同 `sn-miner` 进程，只禁止一个实例内混合启动。
  - `desc/sec` 仍由 `sn-miner` 负责加载或按既有默认行为创建。
- Open ambiguities:
  - 配置文件格式、默认路径、CLI override 优先级、错误提示格式和 schema 字段需要 design 固化。
  - Owner SN 与 Serving SN 的端口字段命名、endpoint 格式、必填/选填关系需要 design 固化。
  - Serving SN online 心跳周期、PeerRoute publish 周期和 owner membership 读取方式应由 design 与 `p2p-frame/sn-distributed-directory` 对齐。
  - 真实进程测试由 `sn-miner` 自身 testplan 承担，还是由 `p2p-frame/sn-distributed-directory` 的 DV/integration harness 调度多个 `sn-miner` 进程，需要 design/testing 决定。

## Constraints
- Allowed libraries/components:
  - 复用现有 `cyfs-p2p` / `p2p-frame` SN service、OwnerDirectoryServer、PnServer 和 identity 装配接口。
  - 配置解析可使用仓库既有依赖或 Rust 标准生态中已存在的配置解析依赖；具体由 design 决定。
- Disallowed approaches:
  - 不在 `sn-miner` 内实现 SN directory 算法或授权策略。
  - 不提供 owner+serving 单进程混合启动。
  - 不通过全局 registry 或同进程 shortcut 连接 OwnerDirectoryServer。
- System constraints:
  - 启动默认值必须保持明确。
  - 生成制品和配置默认值属于高风险操作面。
  - 影响运行时的改动需要 DV 证据。
  - 缺少配置文件时不得隐式启动 owner+serving 混合角色。
  - 配置同时声明 owner 和 serving 角色时必须启动失败，并给出明确错误。
  - Owner SN 配置必须包含 owner peer/control endpoint 与 serving-facing endpoint；两类 endpoint 不得混用。
  - Serving SN 配置必须包含 owner membership 和 owner serving-facing endpoint 信息。
  - Serving SN online 心跳与 PeerRoute publish 是两个独立配置面；即使实现复用内部 client，也不得把每个 `ReportSn` 等价为 online renew 或 route publish。
  - 配置解析错误、desc/sec 错误、端口冲突和远端 owner endpoint 缺失都必须成为可测试失败路径。

## Requirement Challenge
| question | evaluation | risk_or_tradeoff | decision |
|----------|------------|------------------|----------|
| Should one `sn-miner` instance support starting owner and serving roles together? | 不应支持。混合启动会绕过真实进程间 transport、端口隔离和授权边界，也会让测试误以为跨角色通信已验证。 | 部署时需要两个进程或两个配置文件，但能暴露真实运行问题。 | enforce exclusive `owner` or `serving` role per instance |
| Should long-term deployment rely on CLI flags or config file? | 应以配置文件为主。owner membership、双 listener endpoint、online heartbeat 和 route publish 参数较多，CLI 适合 bootstrap/debug，不适合可审计部署。 | 需要设计配置 schema 和兼容迁移，但更符合运行时可审计要求。 | add config-file driven startup |
| Should `sn-miner` own SN directory algorithms? | 不应拥有。算法和协议边界属于 `p2p-frame`，`sn-miner` 只做装配。 | 配置错误需要向底层传播清晰错误，但避免协议分叉。 | assembly only |
| Should Serving SN online heartbeat and PeerRoute publish share one config knob? | 不应。两者语义独立：online 表示 serving 实例可用性，route publish 表示用户归属路由刷新。 | 配置项更多，但能避免 ownerSN 负载与 peer report 高频耦合。 | separate config and startup wiring |
| Is real process testing required for this change? | 需要。单元测试不能证明配置、端口、进程启动、owner/serving transport 和 shutdown 行为。 | 需要新增 DV/integration harness，测试成本更高。 | require real-process tests |

## Large Module Submodule Decision
| Submodule | New or Existing | Responsibility | Proposal Packet | Reason |
|-----------|-----------------|----------------|-----------------|--------|
| sn-miner-configured-roles | existing module scope | 配置驱动的 Owner SN / Serving SN 单角色启动、配置校验、真实进程验证 | `docs/versions/v0.1/modules/sn-miner/proposal.md` | `sn-miner` 当前模块范围很小，只有启动二进制和配置资源，不需要拆 direct submodule。 |

## Trigger Matrix
| trigger_category | applies | evidence | required_checks | deferred_checks_and_reason |
|------------------|---------|----------|-----------------|----------------------------|
| contract/protocol | yes | 配置会选择 owner/serving 角色并装配 SN directory/service，但不新增客户端可见协议。 | design must map config fields to existing p2p-frame interfaces and preserve peer-facing SN compatibility. | owner: design/testing; risk: config accidentally changes runtime protocol boundary. |
| data/schema | yes | 新增配置文件 schema、角色枚举、endpoint/membership/heartbeat/publish 配置字段。 | design must define schema, defaults, validation, and migration behavior; tests must cover invalid config. | owner: design/testing; risk: deployment drift. |
| security/privacy/permission | yes | 配置决定 owner membership 和 serving 到 owner 的访问路径。 | design must ensure sn-miner does not bypass p2p-frame validators or synthesize authorization. | owner: design/testing; risk: unauthorized owner route writes. |
| runtime/integration | yes | 启动 owner or serving 进程、监听端口、连接 owner endpoint、online heartbeat、route publish。 | real-process DV/integration must start separate owner and serving processes and verify role exclusivity. | owner: testing; risk: unit-only coverage misses port/transport failures. |
| build/dependency/config/deployment | yes | 新增配置文件默认路径、CLI config flag、package config behavior。 | cargo check/test, config fixture tests, package cfg review. | owner: design/testing; risk: deployment command drift. |
| ui/datamodel/workflow | no | 无 UI 或外部产品工作流。 | not-applicable | owner: none; risk: none. |
| harness/process | yes | `sn-miner` 当前 proposal/design/testing 仍为 draft，不能实现。 | doc-structure-check, schema-check after approval, admission-check before implementation. | owner: proposal/design; risk: bypassing unapproved module packet. |

## High-Level Outcomes
- 服务启动改动变得可审计
- 默认制品和启动行为不再静默漂移
- 操作员可以通过配置文件明确选择 Owner SN 或 Serving SN。
- 一个 `sn-miner` 进程实例不会同时启动 owner 与 serving 两种角色。
- Owner SN 双 listener 与 Serving SN 连接 owner serving-facing endpoint 的行为有真实进程验证证据。

## Proposal Items
| proposal_id | change_id | Outcome | Scope Boundary | Success Evidence | Explicit Non-Goal |
|-------------|-----------|---------|----------------|------------------|-------------------|
| P-1 | sn_miner_startup_audit | 服务启动改动可审计 | `sn-miner-rust/src/main.rs` 启动入口与必要启动资源 | proposal/design/testing/acceptance 证据链齐全 | 不拥有 CYFS 适配层逻辑 |
| P-2 | sn_miner_artifact_defaults | 默认制品和启动行为不再静默漂移 | `sn-miner-rust/src/main.rs`、`sn-miner-rust/config/**` | testing 与 DV 证据覆盖启动和制品行为 | 不替换核心 SN 协议语义 |
| P-3 | sn_miner_config_file_roles | 配置文件声明 `owner` 或 `serving` 角色，并驱动对应启动路径 | `sn-miner-rust/src/main.rs`、配置 schema/fixture | config unit 覆盖 schema/default/invalid cases；DV 覆盖配置启动 | 不以 CLI flags 作为长期部署接口；不混合启动两种角色 |
| P-4 | sn_miner_owner_role_startup | Owner 角色启动 OwnerDirectoryServer owner-peer listener 和 serving-facing listener | `sn-miner-rust/src/main.rs`、配置资源、必要测试夹具 | 真实进程测试验证 owner 双 listener 可启动、端口冲突失败、进程可关闭 | 不启动 peer-facing SnService；不启动 PnServer |
| P-5 | sn_miner_serving_role_startup | Serving 角色启动 SnService/PnServer 并通过配置连接 OwnerDirectoryServer serving-facing endpoint | `sn-miner-rust/src/main.rs`、配置资源、必要测试夹具 | 真实进程测试验证 serving 进程通过 owner endpoint 完成 online/route/query/call 前置链路 | 不启动 OwnerDirectoryServer；不使用全局 registry fallback |
| P-6 | sn_miner_role_exclusive_validation | 同一配置或进程实例不能同时声明/启动 owner 与 serving | 配置解析和启动前校验 | invalid config test 和 real process negative test 均失败在启动前 | 不提供 owner+serving mixed mode |

## Success Criteria
- Concrete user-visible or system-visible result:
  - 操作员可以通过配置文件启动 Owner SN 或 Serving SN。
  - 配置同时声明 owner 和 serving 时，进程启动失败且错误清晰。
  - Owner SN 与 Serving SN 必须作为独立进程组合验证。
- Required evidence:
  - design 直接映射所有 `change_id`，定义配置 schema、角色互斥、启动流程和错误处理。
  - post-implementation testing 覆盖配置解析、invalid config、owner 进程、serving 进程和真实进程成功/失败路径。
  - implementation admission 必须绑定已批准 proposal/design，不能使用 chat-only 需求。
- Explicit non-goals:
  - 不实现 SN directory 核心语义。
  - 不提供单进程 owner+serving 混合运行。
  - 不依赖全局 registry fallback。

## Risks
- 意外的启动行为变化
- 生成的 desc/sec 制品带来意外结果
- 因缺少运行时证据而被掩盖的操作性回归
- 配置 schema 设计不清会造成部署不可迁移。
- 单进程互斥会要求现有本地一体化调试方式迁移为两个进程。
- 真实进程测试需要稳定端口分配、进程生命周期管理和日志/超时收敛。

## Downstream Follow-Up
| follow_up_id | Owning Stage | Reason | Triggering Proposal Item | Blocking |
|--------------|--------------|--------|--------------------------|----------|
| FU-SN-MINER-CONFIG-DESIGN | design | 需要定义配置 schema、默认路径、CLI `--config`、字段校验和角色互斥逻辑。 | P-3 / P-6 | yes |
| FU-SN-MINER-OWNER-SERVING-DESIGN | design | 需要定义 Owner 与 Serving 启动装配、endpoint/membership 字段、错误处理和 shutdown 边界。 | P-4 / P-5 | yes |
| FU-SN-MINER-REAL-PROCESS-TESTING | testing | 需要设计真实进程 DV/integration，覆盖 owner/serving 两进程、负例和端口冲突。 | P-3 / P-4 / P-5 / P-6 | yes |
| FU-SN-MINER-IMPLEMENTATION | implementation | 需要在 proposal/design/testing approved 后实现配置解析、角色启动和真实进程测试支撑。 | all proposal items | yes |

## Proposal Guardrails
- Proposal-stage tasks modify only `proposal.md` unless the user explicitly requests a multi-stage update.
- If this proposal change requires design, implementation, testing, or acceptance updates, record the needed follow-up instead of editing downstream artifacts by default.
- If this is a large module with many independent submodules, classify whether the requested feature is a new direct submodule before design or testing starts.
- Put the split submodule's proposal and design files in a submodule directory under this module packet; optional post-implementation testing artifacts may live there when generated. Do not use `design/<submodule>/` or `testing/<submodule>/` for independent submodule docs.
- Keep human-authored proposal docs under 1000 lines where practical; if a doc would exceed 1000 lines, split it and update the relevant document index.
- If the request has multiple plausible meanings, record the ambiguity instead of silently choosing one.
- Proposal approval should not depend on chat-only context; task-critical assumptions belong in this file.
- Keep implementation strategy out of proposal except where a constraint is part of the requirement.
- Every implementation-ready requirement must have a stable `change_id`.
- A broad module-level statement is not enough for implementation admission; the relevant `change_id` must name the concrete behavior, contract, or implementation unit being admitted.

## Approval Record
- approver: user
- approval_date: 2026-06-22T21:02:36+08:00
- user_statement: "批准 sn-miner proposal 和 p2p-frame/sn-distributed-directory proposal，并启动 auto-pipeline 自动处理后续 design、implementation、testing、acceptance。"
