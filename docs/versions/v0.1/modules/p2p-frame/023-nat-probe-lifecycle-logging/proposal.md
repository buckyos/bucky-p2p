---
module: p2p-frame
task_name: 023-nat-probe-lifecycle-logging
submodule: 023-nat-probe-lifecycle-logging
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# NAT 探测关键过程日志 Proposal

## Background and Goal

已完成的 sibling task `022-on-demand-nat-probing` 将 NAT 探测调度迁移到 SN，并实现 QUIC eligibility、事件/需求触发、两小时周期、generation 关联、失败退避和容量限制。但当前日志主要集中在客户端探测 I/O 失败、非法 directive 和 result report 失败；SN 端对调度触发原因、权威 tunnel 变化、directive 签发、结果接收、超时和 profile 发布状态缺少可检索的生命周期日志。

本任务作为 `022-on-demand-nat-probing` 的 sibling observability 补充，在不改变调度、wire、NAT 分类或连接策略的前提下，为 SN scheduler/service 与 SN client 的关键过程增加一致、可控且安全的日志，使运维人员能够用 peer/request/generation 关联一次探测从触发到完成或失败的全过程。

## Scope

### In scope

- SN 端记录低频、状态变化驱动的 NAT probe 生命周期事件：
  - probe endpoint 配置首次生效、generation 变化或配置无效；
  - 权威 QUIC registration 建立、外网 observation 变化、capability 丢失、权威 tunnel 消失或 peer 状态清理；
  - online、external-address、config、demand、periodic 等 trigger 成为待处理状态；
  - directive 实际签发，包含明确的 trigger reason；
  - 有真实待处理工作但因 capability、in-flight、failure backoff、global capacity 或 endpoint 配置而暂未签发；
  - result 被接受并发布 profile、返回 `Unknown`、因 correlation/freshness/identity/version 不匹配被拒绝，或 directive 超时；
  - profile 因 authority/config/address/timeout/result 等具体原因失效。
- 客户端记录一次 directive 的关键执行过程：收到并通过验证、开始 UDP probe、成功得到 observation、probe I/O 失败或得到 `Unknown`、结果 report 成功发送或发送失败。
- 客户端拒绝 directive 时记录稳定的 reason code，区分非 QUIC、SN/peer identity 不匹配、registration/request 过期或重放、deadline 过期、endpoint 数量/协议/IP/端口非法等类别；不得只输出统一的 `invalid`。
- 所有生命周期日志使用统一、可检索的 `nat_probe_*` event 名称，并按适用场景携带：`sn_id`、`peer_id`、`tunnel_id`、transport、registration generation、probe-config generation、request id、trigger/reason、deadline 或 elapsed time、最终 observation。字段不存在时不伪造占位值。
- 日志级别遵循以下边界：
  - `info`：低频且成功改变生命周期的事件，例如 authority 建立/失效、directive 签发、客户端 probe 开始/完成、服务端结果接受和 profile 更新；
  - `warn`：需要运维关注的可操作故障，例如 probe I/O 失败、directive 超时、无效服务端 probe 配置或 result report 发送失败；
  - `debug`：预期兼容/竞态拒绝、退避/容量抑制、demand 排队及详细 endpoint 诊断。
- 日志只能在状态首次变化或一次 request 的明确边界输出；稳定 report、未到期周期检查、250ms maintenance tick 和重复的相同 suppressed 状态不得形成 `info`/`warn` 日志风暴。
- 敏感信息边界：不得记录 identity certificate/secret、完整 ReportSn 或 directive/result 原始字节、probe token、UDP packet body、密钥、认证材料或任意用户业务 payload。外网 endpoint 与 probe endpoint 只允许在 `debug` 级别用于诊断；`info`/`warn` 使用变化标记、数量或 reason，不输出完整 endpoint 列表。
- 保持仓库既有 `log` facade，用 `log::{debug,info,warn}!` 风格实现；不新增日志依赖、日志初始化器、文件 sink 或配置键。

### Out of scope

- 修改 task 022 的触发条件、两小时周期、退避、capacity、generation、QUIC-only eligibility、profile validity 或 client online ordering。
- 修改 ReportSn/ReportSnResp、NatProbeDirective/NatProbeResult、UDP probe packet 等任何 wire 格式或公开 API。
- 新增 metrics、distributed tracing、OpenTelemetry、审计数据库、日志持久化/上传、日志轮转或 SN 管理接口。
- 把每次 report/query/call、每次 scheduler 判断或每次 maintenance tick 都记录为 `info`/`warn`。
- 记录证书、密钥、token、原始包、完整 endpoint 列表或用户业务内容。
- 修改已验收的 022 proposal、pipeline、testplan、artifact 或 acceptance report。

### Boundary with neighboring modules

- `p2p-frame/src/sn/service/nat_probe_scheduler.rs` 拥有调度状态变化与拒绝/抑制原因，后续 design 应使日志原因来自状态机实际分支，而不是 service 层根据结果反推。
- `p2p-frame/src/sn/service/service.rs` 拥有 authenticated peer/tunnel 上下文、profile cache 更新、server lifecycle 和对 scheduler 事件的最终日志关联。
- `p2p-frame/src/sn/client/sn_service.rs` 拥有 directive 本地验证、UDP probe 执行和 result report 过程日志。
- `p2p-frame/src/sn/nat_probe.rs` 保持 UDP packet 与 socket 语义；只有在已有错误无法提供足够诊断时才允许补充不含 token/payload 的局部 debug/warn 日志。
- `sn-miner-rust` 继续负责既有日志初始化与输出位置，不新增配置或行为。

## Requirement Review

- 该要求合理。NAT 探测现在是跨 SN scheduler、service、client 和 UDP runtime 的异步过程，仅靠最终 profile 或零散错误无法区分“未触发、被抑制、指令丢失、客户端失败、结果被拒绝或权威状态已失效”。
- 最有效的实现不是在每个函数入口打印日志，而是给状态机的真实 transition/rejection/suppression 赋予稳定 reason，并在责任边界输出一次。这样日志与行为同源，也能避免 service 层猜测触发原因。
- `info` 应只覆盖一次 request 或 authority 的低频生命周期边界；预期的 mixed-version、重放和容量/退避抑制放在 `debug`，否则恶意或异常 peer 可放大生产日志。
- NAT 诊断有时需要外网 IP/端口，但这些数据具有隐私和拓扑敏感性。因此 exact endpoint 仅允许在显式启用的 `debug` 级别出现，默认 `info`/`warn` 不输出 endpoint 列表；证书、token、原始 packet 和 secret 在所有级别禁止。
- 日志是 observability，不得成为调度或正确性的输入；日志 sink 不可用、级别关闭或格式化失败不能改变 directive、profile、online 或 fallback 行为。
- 当前任务不承诺跨版本永久稳定的机器解析 schema，但本任务定义的 `nat_probe_*` event 名和 reason code 应在本版本内稳定、可由测试断言，避免自由文本难以检索。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-NPLL-1 | sn_nat_probe_server_lifecycle_logging | SN 为配置、authority、trigger、directive、suppression、result、timeout 与 profile invalidation 输出状态变化驱动的 `nat_probe_*` 生命周期日志，并使用真实 trigger/reason | reason 由 scheduler transition 产生，service 只补 authenticated peer/tunnel/cache 上下文；稳定 report/tick 不输出高等级日志 | 增加少量低频日志与 reason 状态，换取端到端可诊断性 | unit 捕获并断言 online/address/config/demand/periodic、capability/in-flight/backoff/capacity、accept/reject/Unknown/timeout/authority-loss 的 event、level、关联字段与零重复 | 不改变调度、周期、容量、profile 或 wire 行为 |
| P-NPLL-2 | sn_nat_probe_client_lifecycle_logging | client 为 directive 验证、UDP probe start/completion/failure 与 result report 输出关联日志，拒绝时给出具体 reason code | 仅当前 SN client probe 路径；日志失败不能影响 active SN 或 result flow | 为验证分支引入可观测 reason，换取现场可定位“未执行”的原因 | unit/DV 捕获有效 QUIC、TCP 拒绝、identity/generation/request/deadline/endpoint 拒绝、真实 UDP success/Unknown 和 report failure 的 event、level、request 关联 | 不新增 client timer、重试器、metrics 或 tracing |
| P-NPLL-3 | sn_nat_probe_log_safety_and_noise_control | 所有新日志遵守级别、敏感信息和状态变化降噪边界；默认 info/warn 不含完整 endpoint，任何级别不含 cert/secret/token/raw packet/payload | 沿用 `log` facade，不改变全局 logger 或 sn-miner 配置 | debug 关闭时 endpoint 细节不可见，换取默认生产日志的隐私和容量安全 | 日志捕获/静态负向检查证明禁用字段不出现；稳定 report、未到期检查、maintenance tick 和重复 suppression 不产生 info/warn；日志关闭时行为测试不变 | 不建立永久结构化日志 schema、日志 sink、采集系统或轮转策略 |

## Success Criteria

- 运维人员可通过 `sn_id`/`peer_id`、registration/config generation 与 request id，把一次探测从 SN trigger/directive、client start/completion/report 关联到 SN accept/Unknown/reject/timeout/profile update。
- 每次实际 directive 都有且只有一个 `nat_probe_directive_issued` info 事件，并记录 `trigger=online|external_address|config|demand|periodic` 中的真实原因。
- 每次 client 实际执行都记录 start 和 terminal success/Unknown/failure；每次服务端匹配结果记录 terminal accepted/Unknown，超时记录 warn；旧、重放、晚到或错误关联结果只在 debug 记录具体拒绝原因。
- TCP-only、旧 capability、退避、in-flight、容量饱和或无有效 endpoint 时不执行 probe；存在真实 pending work 时可在 debug 看见具体 suppression reason，但稳定 report 与 maintenance tick 不产生重复 info/warn。
- 外网地址变化、probe 配置变化、authority 消失和 profile invalidation 均有一次带具体 reason 的生命周期日志，且日志顺序与实际状态更新一致。
- 默认 info/warn 日志不包含完整 observed/probe endpoint 列表；所有级别都不包含 certificate、secret、probe token、raw directive/result/UDP packet 或用户 payload。
- 关闭 debug 或禁用全部日志不会改变 022 的调度、online、profile、fallback、timeout、backoff 和 capacity 行为。
- Required evidence: unit 日志捕获覆盖 server/client 正常、边界、拒绝、失败、生命周期与降噪分支；DV 覆盖一次真实 QUIC directive→UDP probe→result 的关联日志和 TCP 零执行；静态负向扫描检查禁止的 raw/cert/token/payload 日志；现有 022 task-scoped 回归继续通过以证明行为未变。
- Explicit non-goals: 不修改协议或公开 API，不新增 metrics/tracing/sink/config，不记录敏感 payload，不把日志作为调度输入。

## Risks

- 如果日志原因由 service 层从最终结果反推，可能与 scheduler 的真实分支不一致；设计必须让 transition/rejection/suppression reason 与状态更新同源。
- 全局 `log` facade 的测试捕获存在并行测试相互干扰风险；testing 需要串行或可隔离的捕获方式，不能依赖人工查看 stdout 作为唯一证据。
- 对每次 report 或 250ms maintenance tick 输出日志会造成放大；必须以状态变化、request terminal event 或受控 debug suppression 为边界，并覆盖重复输入零高等级日志。
- 外网 IP、端口与 P2pId 可暴露网络拓扑和身份关联。默认级别必须最小化 endpoint 数据，禁止 certificate/token/raw packet，并在 acceptance 做敏感字段审计。
- rejection/suppression reason 若被设计成 public/wire 类型会不必要地扩大兼容面；它应保持 crate-private observability 数据，除非后续 proposal 明确改变契约。
- 自由文本日志容易漂移并难以测试；本版本应固定 event 名和 reason code，同时不承诺跨版本永久 schema。

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `nat_probe_*` event 名和 reason code 构成本版本的内部运维检索契约，但 `sn/protocol/sn.rs` wire 与公开 Rust API 明确不变 | design 列出 event/field/level 表；testing 做正向事件与负向缺失字段断言 | proposal 已定义事件族、关联字段与兼容边界 | owner: design/testing; reason: 尚未进入下游阶段；acceptance impact: 缺少稳定映射或日志捕获则阻断 | 自由文本字段仍可能被无意改名 |
| data/schema | no | 不修改持久化状态、cache schema、编码数据或迁移；日志不作为持久化业务数据 | 审计 Scope Paths 不含数据/codec 变更 | proposal 明确排除 wire/persistence | owner: none; reason: not applicable; acceptance impact: none | 外部 sink 的保留策略不在本仓库控制范围 |
| security/privacy/permission | yes | 新日志可能接触 `peer_id`、observed endpoint、directive/result 与 probe runtime；`security.md` 要求敏感日志审计 | design 定义允许/禁止字段；testing 做 cert/secret/token/raw/payload 负向检查与默认级别 endpoint 检查 | proposal 已定义所有级别禁止项和 debug-only endpoint 边界 | owner: design/testing; reason: 尚未进入下游阶段；acceptance impact: 任一敏感输出阻断 | debug endpoint 仍需运维按现有 logger 权限管理 |
| runtime/integration | yes | `sn/service/nat_probe_scheduler.rs`、`service.rs`、`client/sn_service.rs` 是异步调度与 UDP 执行路径，observability 属于 runtime trigger | design 保持状态同源和非阻塞；unit/DV 覆盖成功、失败、timeout、suppression、关闭日志行为与真实链路 | proposal 已限定日志不能改变行为或增加循环工作 | owner: implementation/testing; reason: 尚未进入下游阶段；acceptance impact: 行为回归或缺少失败流证据阻断 | 日志 sink 自身故障由既有 `log` facade 处理 |
| build/dependency/config/deployment | no | 沿用已有 `log` facade，不修改 Cargo、feature、sn-miner 日志初始化、配置或部署 | consumer compile 并审计依赖/配置 diff 为空 | proposal 明确禁止新依赖和配置键 | owner: testing; reason: implementation 后确认；acceptance impact: 若出现 build/config 变化则退回 proposal/design | 不同部署的实际 log level 仍由既有配置决定 |
| ui/datamodel/workflow | no | 不涉及 UI、显示数据模型或交互流程 | 审计 Scope Paths 不含 UI | proposal 明确为 SN runtime observability | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | 不修改 Harness 规则、脚本、模板、schema、CI 或 runner；只使用现有任务流程 | 运行现有 doc/scope/testing/acceptance checkers | proposal 未引入 process 变化 | owner: none; reason: not applicable; acceptance impact: 现有 checker 失败仍阻断 | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
