---
task_manifest: task.yaml
status: approved
---

# 由客户端主导 NAT 探测周期 Proposal

Risk profile: not-created

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 改动集中在 `p2p-frame` 客户端启动/周期探测路径和服务端调度器语义，不修改 SN wire 格式、证书签名或持久化结构，也不需要跨仓库部署协调；运行时行为影响可通过定向单元测试和现有 lib 套件证明，按默认规则归为 standard，不进入 high-risk。
- Proposal and tier confirmation: 用户确认以 standard tier 执行；端口配置变更导致的强制重测暂不在本任务范围，保持 non-goal，后续如需由新任务处理。

## Background and Goal

当前客户端是否探测、何时探测依赖服务端下发的 `NatProbeDirective`：客户端启动时靠 065 的 `first_report_pending` 强制发一条 report，期望服务端因此下发 directive；服务端另有自己的 2 小时周期和 `demand` 主动下发逻辑。这在客户端重启但服务端仍保留旧 authority 时会产生运行时盲区：启动 report 落在同一条 authority 上、没有 pending trigger，服务端返回 `nat_probe_directive=false`，客户端就既不探测也不重试。

目标把探测职责按用户确认的边界拆分：

- 客户端规则：重启后立即探测一次，之后每 2 小时探测一次。
- 服务端规则：只在观察到客户端外网地址变化（含首次建立 authority / 地址变更）时下发探测指令；不做服务端周期定时，也不响应客户端普通上报下发指令。

因此客户端的启动/2 小时探测不应依赖服务端新 instruction，而应使用已从 `ReportSnResp` 拿到的 `nat_probe_ports` 和 `peer_info`（signer）本地构造探测目标直接执行。

## Scope

### In scope

- 在 `SnService` 中为每个 ActiveSN 维护客户端自己的探测计划：启动/上线后立即执行一次，完成后下一次探测时间为 `now + 2h`。
- 客户端新增“本地探测”路径：使用 `ActiveSN.nat_probe_endpoints` 与 `nat_probe_signer` 直接调用现有 `probe_nat_profile`，不要求服务端先下发 directive；成功后更新本地 `net_profile`。
- 周期本地探测在 600 秒 report 之前按 `next_probe_at` 独立触发，并用 `ReportSn.net_profile` 随本次上报携带最新画像；`next_probe_at` 在探测完成后按完成时间 + 2h 排期。
- 指令探测和本地探测完成后的画像立即写入 `ActiveSN.net_profile` 并推进 `next_probe_at`，与结果上报是否成功解耦；上报失败后本地画像保留，下一次 report 继续携带。
- 服务端消费 `ReportSn.net_profile`：新鲜且不旧于当前 scheduler profile 时更新 `NatProbeScheduler` profile，使 query/detail 能返回客户端周期画像；陈旧/Unknown 画像不覆盖现有 profile。
- 保留客户端对服务端主动指令的处理：当服务端因外网地址变化下发 directive 时，仍按现有流程执行并回传结果。
- 服务端 `NatProbeScheduler` 移除周期定时与纯 demand 主动下发；仅在新 authority 建立或观察到外网地址变化时下发 directive。
- 增加定向测试覆盖“无 directive 时客户端本地启动/周期探测仍执行”和“服务端不再因周期/demand 主动下发”。

### Out of scope

- 不修改 `ReportSn`/`ReportSnResp`/`NatProbeDirective` 等 wire 字段或证书签名方式。
- 不改变 600 秒 SN 上报/保活刷新机制，该机制只维护 online 状态，不代表探测周期。
- 不改变 `wait_online()` 返回语义。
- 不引入新的 wire 字段或证书签名方式；`ReportSn.net_profile` 沿用在客户端已存在但此前未填充/未消费的扩展字段，本次改为填充与服务端消费。
- 不处理端口配置变更时强制重探测的服务端行为，除非用户后续提出。

### Boundary with neighboring modules

客户端行为收口在 `p2p-frame/src/sn/client/sn_service.rs`；服务端调度收口在 `p2p-frame/src/sn/service/nat_probe_scheduler.rs`。`vpn-client`/`vpn-server` 等下游无需修改，升级后获得新的客户端探测节奏和服务端地址变化驱动语义。

## Requirement Review

需求合理。将探测节奏交给客户端、服务端只在自身观察条件满足时下发指令，能消除“客户端重启但服务端保留旧 authority 导致不探测”的盲区，也避免服务端周期/demand 与客户端计划耦合。客户端周期画像通过既有 `ReportSn.net_profile` 扩展随普通 report 携带，服务端只接收新鲜且不旧于当前值的画像；这等价于把“本地已有测量的汇报”作为周期性保持逻辑的一部分。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-client-owned-nat-probe-schedule | ActiveSN 增加 `next_probe_at`；启动/上线即测，完成后 `next_probe_at = now + 2h`；探测使用本地 `nat_probe_endpoints` + `nat_probe_signer`，不依赖服务端 directive。 | 只影响 `p2p-frame/src/sn/client/sn_service.rs` 与相关客户端测试；服务端指令仍按现有流程处理。 | 客户端多一次本地探测路径，服务端不再因客户端普通上报下发指令；探测结果只更新本地 profile。 | 定向测试证明无 directive 时客户端启动/周期仍执行本地探测，完成后 2 小时再测。 | 不改变 wire；不改变 600 秒上报保活。 |
| P-002 | CHG-server-address-change-only-directive | `NatProbeScheduler` 不再由周期或纯 demand 主动下发；仅在新 authority 建立或观察到外网地址变化时下发 directive。 | 只影响 `p2p-frame/src/sn/service/nat_probe_scheduler.rs` 与调度器测试；地址变化检测沿用现 `remote_endpoint.addr()` 比较。 | 服务端不再兜底定期/查询触发探测，但满足用户“服务器只根据外网地址变化下发”的规则。 | 调度器测试证明周期/demand 不再下发，地址变化仍 trigger=online/external 下发。 | 不改变 profile TTL 语义；不做服务端新端口配置强制重测。 |

## Success Criteria

- 系统可见结果：客户端重启后即使服务端返回 `nat_probe_directive=false`，客户端仍会立即用本地快照完成一次 NAT 探测；之后每 2 小时再测一次。
- 系统可见结果：服务端日志不再出现周期或纯 demand 触发的 `nat_probe_directive_issued`；只有新 authority/外网地址变化触发。
- 所需证据：新增客户端定向测试、调度器定向测试，以及 `cargo test -p p2p-frame --features x509` 相关模块与 lib 套件通过。
- 非目标：不声明本地探测即代表公网/部署环境验证完成；服务端仍以地址变化 directive 的结果为权威验证，但可用客户端周期画像维持两端 profile 一致性。

## Risks

主要风险是客户端周期画像直接进入服务端 profile，可能被陈旧数据覆盖更新结果；实现通过 `observed_at` 新旧比较和 Unknown/过期过滤避免。另需防止本地周期探测与服务端 directive 探测并发重复；实现按 `next_probe_at` 单飞推进，同一 report 周期只触发一次本地探测。
