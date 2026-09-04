---
module: p2p-frame
task_name: 025-sn-query-target-protocol-version
submodule: 025-sn-query-target-protocol-version
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# SnQuery 目标协议版本 Proposal

## Background and Goal

当前 `ReportSn`、`SnCall` 和 `SnQuery` 已包含 `protocol_version` 字段，但 SN client 发送时固定填写 `0`，SN service 解码后既不校验、不保存，也不把目标节点的版本返回给查询方。因此调用方通过 `SnQuery` 获得目标证书、endpoint 和 NAT profile 后，仍无法知道目标节点声明的 SN 协议基线版本。

本任务要求客户端用一个明确的当前 SN 协议版本替代散落的硬编码值；SN 将认证客户端在 `ReportSn` 中声明的版本绑定到该 peer 的当前内存注册；`SnQueryResp` 在本地和跨 SN 查询中返回目标节点的自报协议版本。旧节点、缺失注册、旧 SN 不支持转发或多 SN 结果冲突时必须表示为未知，不能把未知误判成版本 `0`。

## Scope

### In scope

- 定义单一权威的 SN 应用协议基线版本，并由客户端在现有 `ReportSn.protocol_version`、`SnCall.protocol_version` 和 `SnQuery.protocol_version` 字段中一致使用，消除当前硬编码 `0` 的漂移。
- `ReportSn.protocol_version` 是客户端对其当前 SN 协议基线的自报值。SN 必须使用 control tunnel 的认证 peer 身份绑定该值，消息中的 `from_peer_id` 不得覆盖认证身份。
- SN 在目标 peer 的现有内存注册信息中保存最新一次有效 `ReportSn` 声明的协议版本；重复 report 更新该值，peer 注册删除或过期时版本随同删除，不新增独立永久缓存。
- `SnQueryResp` 增加目标协议版本结果。结果语义必须区分：
  - 已知版本，包括合法的 legacy 版本 `0`；
  - 未知版本，包括目标不存在、没有可信 report、旧响应缺字段、旧 inter-SN 节点无法转发以及分布式结果无法安全合并。
- 本地 SN 命中目标的当前注册时，以该本地注册版本为权威结果；本地未命中而依赖多个 serving SN 时，只有所有参与返回的已知版本一致且没有缺失时才返回该版本，缺失或冲突一律返回未知。
- 跨 SN detail 查询必须携带相同的可选目标协议版本，使最终 `SnQueryResp` 的本地查询和分布式查询语义一致；该传递不建立跨 SN 持久化版本状态。
- wire 变化采用 additive、可版本化且可选的响应扩展：
  - 新客户端解码旧 `SnQueryResp` / `SnDetailResp` 时得到未知；
  - 旧客户端解码带版本扩展的新响应时仍能读取原有证书、endpoint 和 NAT profile；
  - decoder 对已知扩展、未知尾部和 remainder 的处理必须由 design 明确定义，不能依赖偶然忽略剩余字节。
- 协议版本是粗粒度兼容信息。NAT probe、tunnel rendezvous 等已有专项 capability/version 仍按各自合同判断，调用方不得仅凭目标协议版本推导未建立映射关系的功能支持。

### Out of scope

- 不把 `stack_version`、crate semver、Git commit、构建时间、操作系统或部署版本加入 `SnQueryResp`。
- 不改变 command frame header 的 `cmd_version`、package length、command id、TLS 身份或 control tunnel 建立语义。
- 不用 SN 对客户端声明的实现真实性背书；SN 只保证版本值来自与目标 peer 身份绑定的认证 report。
- 不新增磁盘、数据库、desc/sec、identity cert 或跨重启版本持久化。
- 不以目标协议版本替代 feature-specific capability negotiation，不在本任务中修改 NAT probe 或 rendezvous 的功能选择策略。
- 不在 proposal 阶段确定扩展 magic、最终 Rust 字段名、具体编码顺序或测试文件布局。
- 不在 proposal 阶段修改 design、生产代码、测试代码、构建资源或运行时配置。

### Boundary with neighboring modules

- `p2p-frame/src/sn/client/**` 负责使用同一权威协议版本构造现有 SN 请求，并消费 `SnQueryResp` 的目标版本结果。
- `p2p-frame/src/sn/service/**` 负责把认证 `ReportSn` 版本绑定到 peer 的现有内存注册生命周期，并按本地优先、冲突转未知的规则生成查询结果。
- `p2p-frame/src/sn/protocol/**` 负责 `SnQueryResp` / `SnDetailResp` 的 additive wire 扩展及新旧解码兼容，不定义上层 feature 策略。
- `p2p-frame/src/sn/inter_sn/**` 只转发 serving SN 已知的目标版本和未知状态，不复制或长期缓存该版本。
- `p2p-frame/src/tunnel/**`、`cyfs-p2p` 和 `sn-miner-rust` 不在本任务中依据该版本改变运行时策略；后续若要用版本选择行为，必须另行定义明确的版本到 capability 映射和任务范围。

## Requirement Review

- 需求合理：`SnQuery` 已是查询目标临时 peer 信息的规范路径，把目标协议版本放入同一响应比新增独立查询或客户端侧远端版本缓存更一致。
- 版本来源必须是目标自己的认证 `ReportSn`，不能从请求方的 `SnQuery.protocol_version`、命令帧版本、成功解码某个消息或 SN 软件版本反推。
- `0` 已经存在于当前 wire 字段中，应保留为合法 legacy 值；因此响应必须用显式可选语义区分“已知为 0”和“未知”。
- 分布式查询可能在滚动升级期间看到不同 serving SN 的旧快照。没有跨 SN 的统一时间戳/代际证明时，任意选择第一个版本会产生错误能力判断；冲突转未知是更安全的基线。
- 单一递增协议版本只能表达粗粒度基线，不能可靠表示所有正交能力。本任务返回版本但保留专项 capability，避免把查询能力扩大成未经定义的协商系统。
- 当前工作区已有大量未提交的 SN/NAT/rendezvous 改动；本 proposal 是独立 sibling packet，后续实现必须基于当时现代码重新核对冲突和消费者闭包，不得覆盖无关修改。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-SQPV-1 | sn_protocol_version_registration | 客户端以单一权威值填写现有 SN 消息的 `protocol_version`；SN 从认证 `ReportSn` 保存并随 peer 注册更新、过期和删除目标版本 | `sn/client`、`sn/service/peer_manager` 与 report 认证路径；仅内存、自报、peer-bound | 统一版本值增加升级纪律，但不能证明客户端真实实现或替代专项 capability | unit/DV 证明客户端字段一致、认证身份绑定、重复 report 更新、peer 删除/过期清理及版本 0 不被当作未知 | 不保存 stack/build/semver，不新增持久化或远端客户端缓存 |
| P-SQPV-2 | sn_query_target_protocol_version | `SnQueryResp` 返回可区分 known-0 与 unknown 的目标协议版本；`SnDetailResp` 和 inter-SN detail 闭包保持相同语义，本地注册优先，远端缺失/冲突 fail closed 为未知 | `sn/protocol`、`sn/service`、`sn/inter_sn` 和 query client；additive optional wire | 增加响应扩展与分布式合并逻辑，换取调用方能可靠读取已知目标版本且不误用陈旧冲突值 | 新旧双向 wire、目标不存在、local hit、remote-only、一致多 SN、缺失、冲突、旧 inter-SN 和 query consumer 测试全部通过 | 不改变现有证书/endpoint/NAT profile 结果，不据此自动启用 NAT probe/rendezvous |

## Success Criteria

- Concrete system-visible result: `SnQuery` 调用方能从响应中区分目标“已知协议版本 N（包括 N=0）”与“协议版本未知”。
- 当前客户端不再分别硬编码 `ReportSn`、`SnCall`、`SnQuery` 的协议版本；同一权威版本值覆盖这些现有字段。
- SN 只返回与目标认证 report 和当前 peer 注册生命周期绑定的版本；伪造 `from_peer_id` 不能污染其他 peer 的版本。
- 本地查询、跨 SN 查询、目标不存在、多 SN 一致、多 SN 缺失和多 SN 冲突均符合本 proposal 的确定性语义。
- 新旧客户端/SN 双向兼容：旧响应对新客户端表现为未知，带新扩展的响应不破坏旧客户端现有查询字段。
- Required evidence: design 必须列出 producer/cache/local-query/inter-SN/query-consumer 闭包、版本分配与升级规则、wire layout/remainder 策略和 mixed-version 矩阵；post-implementation testing 必须提供协议边界正负例、状态生命周期、跨 SN 合并与实际统一入口结果。
- Explicit non-goals: 不返回 stack/build/crate 版本，不持久化，不改变 command frame，不替代专项 capability，不在本任务中改变 tunnel 策略。

## Risks

- 若把未知编码为 `0`，调用方无法区分旧协议目标与旧 SN/缺失数据，可能错误启用或禁用行为。
- 若 report 身份绑定不严格，恶意客户端可以为其他 peer 注入虚假版本并影响查询方决策。
- 若 inter-SN 只修改最终 `SnQueryResp` 而遗漏 `SnDetailResp` / `ServingPeerDetail`，本地查询会通过但分布式查询静默丢失版本。
- 滚动升级期间不同 SN 可能保存不同版本；任意首值选择会形成顺序依赖，必须冲突转未知或由后续 design 提供更强的新鲜度证明。
- additive extension 若破坏现有 NAT profile 扩展顺序或错误消费 decoder remainder，会使旧新节点互通失败。
- 单一协议版本容易被误当成全部能力位图；没有明确版本到能力映射时，上层只能展示、记录或做保守判断。

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/src/sn/protocol/sn.rs` 的 `ReportSn.protocol_version`、`SnQueryResp`、`SnDetailResp` wire contract 和 `p2p-frame/src/sn/inter_sn/mod.rs` detail consumer closure 将变化 | design 固化版本语义、additive layout、remainder、producer/consumer 与 mixed-version 矩阵；testing 做新旧双向正负兼容和字段边界 | proposal 已定义 known-0/unknown、旧响应和旧客户端边界 | owner: design/testing; reason: 精确编码与兼容夹具属于下游；acceptance impact: 任一方向兼容或 consumer 闭包缺失阻断验收 | 扩展顺序或版本语义漂移可破坏互通 |
| data/schema | no | 版本仅进入 `p2p-frame/src/sn/service/peer_manager.rs` 的现有内存 peer 注册，不写数据库、文件、desc/sec 或 serialized durable state | design/acceptance 审计 Scope Paths 不含持久化表面 | proposal 明确随 peer 注册生命周期删除 | owner: acceptance; reason: 仅需边界审计；acceptance impact: 发现持久化改动退回 proposal | 多 SN 内存快照仍可能短时不一致，归 runtime 风险 |
| security/privacy/permission | yes | `p2p-frame/src/sn/service/service.rs::handle_report_sn` 从公网认证 control tunnel 接收自报版本；错误绑定可污染其他 peer 查询结果 | 保持连接认证；负例覆盖 `from_peer_id` 与认证 peer 不一致、未认证/缺证书 report；审计日志不泄漏额外身份材料 | proposal 限定版本只能绑定认证 sender 且 SN 不为真实性背书 | owner: design/testing; reason: trust-boundary 细节和 abuse case 属于下游；acceptance impact: 无身份绑定负例阻断验收 | 认证 peer 仍可虚报自己的版本，调用方必须视为自报值 |
| runtime/integration | yes | `handle_report_sn`、peer 注册更新/删除、`handle_query_sn`、`query_remote_details` 和 inter-SN detail 合并形成分布式运行时路径 | design 描述更新/过期/删除、local precedence、remote missing/conflict；unit、DV、integration 覆盖生命周期及同/跨 SN 查询 | proposal 已定义本地优先和远端 fail-closed 合并 | owner: design/testing; reason: 可运行场景待 implementation 后生成；acceptance impact: 生命周期或跨 SN 证据缺失阻断验收 | 滚动升级和陈旧 serving lease 可暂时返回 unknown |
| build/dependency/config/deployment | no | 需求不修改 `Cargo.toml`、依赖、feature、配置、构建、打包或部署入口 | 审计 implementation Scope Paths 和 lockfile 无变化 | proposal 不引入配置项或依赖 | owner: none; reason: not applicable; acceptance impact: none | 协调发布属于兼容运维注意事项，不要求构建面变化 |
| ui/datamodel/workflow | no | 仓库无本任务 UI 消费者，目标字段属于 Rust SN 查询结果 | 审计 Scope Paths 不含 UI | proposal 不改变用户界面或交互流程 | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | 本任务使用既有 Harness packet/checker，不修改 `harness/**`、模板、CI 或规则 | 运行正常 proposal/design/testing/acceptance checker | proposal 仅新增任务 packet 与索引 bookkeeping | owner: none; reason: not applicable; acceptance impact: checker 失败仍阻断阶段完成 | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
