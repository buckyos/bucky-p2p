---
task_manifest: task.yaml
status: draft
---

# 启动/首次 online 时立即执行 NAT 探测 Proposal

Risk profile: not-created

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: pending
- Tier rationale / triggered boundaries: 这是 `p2p-frame` 单模块的生产行为修复，不改变 SN 协议 wire format、证书签名、调度器数据模型或部署兼容性；但涉及客户端启动/首次上线生命周期，会改变 NAT 画像拿到的时间和首测覆盖，属于运行时行为变更。影响有界、可用定向测试证明，按默认规则归为 standard，不进入 high-risk。
- Proposal and tier confirmation: 原确认后的实现经用户复核发现空启动首报未取得 directive 的路径仍会等待 600 秒；本修订新增有界重试范围，需重新确认。

## Background and Goal

SN 侧的设计意图是客户端第一次携带 QUIC 控制能力上报时就建立 authority 并下 `trigger=online` 的探测 directive，客户端首轮 report 后也立即执行。但客户端 `ping_proc`（p2p-frame/src/sn/client/sn_service.rs:1029）对已有 active SN 只做 600 秒门控刷新：当启动路径没有走“空 active 列表 → 新建 protocol → 立即 report”，而是复用已有 active SN，或首轮 report 没能带回 directive 时，下一次拿到探测指令要等 600 秒刷新点，导致外部观察为“启动后约 10 分钟才开始 NAT 探测”。

目标是把“客户端启动/首次 online 必测一次”从偶发行为变成保证行为：客户端在 `SnService::start()` 后立即对当前 active SN 发一次 report，并执行返回的探测 directive。对空 active 启动路径，第一次 report 若因服务端全局容量等原因没有返回 directive，客户端应把首测状态绑定到该新注册的探测进度，并按有界重试在正常周期前再次 report，不再被动等待下一个 600 秒刷新点。

## Scope

### In scope

- 在 `SnService::start()` 或首次进入 online 的路径增加一次强制首测：已有 active SN 存在时跳过 `latest_time > 600s` 门控并立即 report；没有 active SN 时沿用现有立即收包的路径，不引入额外等待。
- 首测完成后将 active SN 的 `latest_time` 更新到当前时间并恢复正常周期刷新，让后续仍按 600 秒门控刷新。
- 将首测状态绑定到新注册的 `ActiveSN`：首次注册后若尚未完成一次可接受的探测（首报没有 directive 或 directive 被客户端拒绝），保持 pending，并在既有 active 刷新节奏上做有界重试（建议最多 6 次、约 60 秒），而不是直接等 600 秒。
- 若首测 report 返回探测 directive，保持现有 `execute_probe_directive` 与结果回传逻辑执行顺序；接受并执行 directive 后清除该 active SN 的首测 pending，恢复正常周期。
- 增加定向回归覆盖，至少证明“已有 active SN 的 `start()` 会立即 report/执行 directive，而不是等到 600 秒刷新”。

### Out of scope

- 不修改 SN 协议、`NatProbeDirective`/`ReportSnResp` wire field 或服务端 authority 触发逻辑。
- 不改变 600 秒正常刷新门控、2 小时 profile TTL、失败 backoff 或探测结果回传机制。
- 不改变 `wait_online()` 的返回条件，也不为本任务引入“首测完成才 online”的新就绪语义。
- 不改动 `sn/service` 调度器、`sn/protocol`、`tunnel` 或服务端部署配置。

### Boundary with neighboring modules

行为收口在 `p2p-frame/src/sn/client/sn_service.rs` 的客户端启动/首次 report 时序。服务端 `nat_probe_scheduler.rs:281` 的 authority 建立与 directive 下发作为只读验证对象；`vpn-server` 等下游只在升级后获得更早的首测行为，不需要生产代码改动。

## Requirement Review

需求合理。当前设计的“首测立即执行”与客户端 600 秒静态刷新门不一致，属于启动路径的时序缺陷。修复应放在客户端启动/首测路径而非放宽 600 秒门控，否则会改变正常刷新语义且周期性地触发无意义 report。首测对已有 active SN 立即执行是安全、可定向验证的最小修复；但仅靠 `start()` 全局标记不足以覆盖空 active 启动后首报未取得 directive 的路径，因此需要把首测 pending 绑定到新注册的 active SN，并做有界重试。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-startup-first-nat-probe | `SnService::start()` 后对当前 active SN 立即 report 并执行返回的 NAT probe directive，不受 600 秒 `latest_time` 门控影响；完成后恢复正常刷新节奏。 | 客户端启动/首次 online 时序归 `p2p-frame/sn/client`；服务端 authority/directive 与客户端 report/execute 的协议内涵不变。 | 启动阶段对已有 active SN 会多一次立即 report，首测完成后的正常周期从该时间点起算；不改变后续周期和失败 backoff。 | 定向测试证明已有 active SN 时 `start()` 后会立即 report 并执行 directive；相关 p2p-frame 测试与完整 lib 套件通过。 | 不改变协议/wire/服务端行为；不改变 `wait_online()` 就绪语义；不引入“首测成功才 online”的新契约。 |
| P-002 | CHG-first-probe-deferred-directive-retry | 空 active 启动完成首次注册后，若未取得/未接受首个探测 directive，客户端保持首测 pending 并按有界重试再次 report（建议最多约 60 秒），接受并执行首个 directive 后退出首测重试。 | 重试状态与清理归 `p2p-frame/sn/client` 的 active SN 生命周期；服务端 `global_capacity` 仍返回空 directive 并保留 pending trigger。 | 空启动 + 无 directive 时会有最多约 60 秒的短周期额外 report；超过边界后回到 600 秒周期或既有失败 backoff。 | 定向测试证明首报无 directive 时会在正常 600 秒前再次 report，且接受 directive 后停止首测重试、恢复周期；原有流程测试无回归。 | 不改变服务端容量/调度；不把无 directive 视为错误；不改 `wait_online()`/上线语义。 |

## Success Criteria

- 系统可见结果：客户端启动/首次 online 时会主动立即进行一次 NAT probe；若服务端下发了 authority directive，首测在秒级完成，而不是等 600 秒刷新点。
- 回归后系统可见结果：首测后的正常刷新仍按现有 600 秒门控与失败 backoff 节奏执行，不在每个周期重复首测；有界重试只在首次注册未取得 directive 时短暂启用。
- 所需证据：定向新增测试覆盖已有 active SN 的 `start()` 立即 report/执行；用户描述的“启动后等约 10 分钟”路径测试无超时；`cargo test -p p2p-frame` 相关模块与 lib 套件通过。
- 非目标：不在本任务声明 NAT 探测通过即代表公网可达或部署环境验证完成；不对 `wait_online()`/上游 online 语义做扩大声明。

## Risks

主要风险是首测与已有周期刷新并发时可能出现重复 report，以及无 directive 时重试风暴。实现应把首测 pending/`latest_time` 更新放进同一状态更新临界区，重试次数固定上限；接受首个 directive 后立即清除 pending。该风险可在定向测试中通过可观察的 report 次数/时序断言受控验证。
