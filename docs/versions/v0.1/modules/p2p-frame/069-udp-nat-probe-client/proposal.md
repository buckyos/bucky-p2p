---
task_manifest: task.yaml
status: approved
---

# 非 QUIC UDP authority 的客户端探测执行 Proposal

Risk profile: not-created

## Workflow Tier Judgment
- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 修复集中在 p2p-frame 客户端 `SNClientService` 的 directive 接受、本地探测目标构造与 UDP 网络选择，涉及 UDP 族协议扩展并影响运行时探测连路，但不改 `ReportSn`/`ReportSnResp`/`NatProbeDirective` wire 字段、证书签名、持久化或发布/部署；按默认规则归为 standard。
- Proposal and tier confirmation: 用户于 2026-09-08 确认按 standard tier 执行。

## Background and Goal
服务端 `NatProbeScheduler` 已将 authority 资格放宽为 UDP 族（`Endpoint::is_udp()`），因此 `Protocol::Ext(_)` 自定义 UDP 隧道可以建立 authority 并收到 directive。但客户端 `SNClientService` 仍把接受与执行锁死在 QUIC：

- `validate_nat_probe_target` 要求 `active_protocol == Protocol::Quic` 且 `sn_endpoint.protocol() == Protocol::Quic`，导致自定义 UDP 协议上的 directive 被 `TransportNotQuic` 拒绝，`execute_probe_directive` 返回 `None`。
- `build_nat_probe_endpoints` 始终把探测目标硬编码为 `Protocol::Quic`，本地回退/ActiveSN 快照无法按真实 active 协议构造目标。
- `probe_endpoints` / `probe_local` 固定取 `self.net_manager.get_network(Protocol::Quic)`，即使目标端点已是 `Protocol::Ext(_)`，也不会走对应 UDP 网络执行 `probe_nat_profile`。

目标：把客户端能力补齐到与用户确认的服务端语义一致的“UDP 协议都支持”，即任意非 TCP UDP 协议（含 `Protocol::Ext(_)`）都可以接受 directive、构造探测目标并调用该协议注册的 `UdpTunnelNetwork` 执行探测。

## Scope
### In scope
- `validate_nat_probe_target`：资格从「仅 QUIC」改为「非 TCP 的 UDP 族协议」，并要求 `sn_endpoint.protocol() == active_protocol`；其余 IP 与地址校验保持。
- `build_nat_probe_endpoints`：探测目标端点协议改为 `active_protocol`，不再硬编码 `Protocol::Quic`。
- `probe_endpoints` / `probe_local`：通过端点协议从 `NetManager::get_network` 选择网络，并继续要求 `as_udp_tunnel_network()` 存在；不再固定取 QUIC 网络。
- 保持 `ReportSn`/`ReportSnResp`/`NatProbeDirective` wire 与证书签名不变，服务端调度器 authority 语义不变。
- 新增/调整客户端测试：Ext(1) UDP active 协议可接受 directive 并重建 Ext 探测端点；使用伪造 Ext UDP 网络验证 directive 执行和本地回退确实调用该协议网络。

### Out of scope
- 不修改服务端 authority/调度器判断（已完成于 068）。
- 不新增真实 socket 的自定义 UDP 端到端用例；单元层使用伪造 `TunnelNetwork + UdpTunnelNetwork` 证明客户端调用链。
- 不改变 NAT 画像新鲜度/`observed_at`、report 周期、本地探测周期或 owner/conn 语义。
- 不把 `Protocol::Ext(_)` 从 UDP 族语义改回独立协议族；本项目 `Endpoint::is_udp()` 以「非 TCP」定义 UDP 族。

### Boundary with neighboring modules
客户端 `ActiveSN.nat_probe_endpoints` 将按真实 active 协议保存（QUIC 任务仍是 QUIC；Ext 任务变为 Ext），`probe_local_if_due`、初始 online 本地回退和 `execute_probe_directive` 共用同一网络选择逻辑。服务端只下发端口向量，不感知客户端如何选网络。

## Requirement Review
需求成立。服务端已按 UDP 族接受 authority 并下发 directive，客户端对非 QUIC UDP directive 的直接拒绝造成服务端可以下发、客户端不执行的不一致；本地回退也构造不出正确目标。把资格、目标构造和网络选择统一为 UDP 族即可贯通端到端路径，且与用户明确要求“udp协议都支持”一致。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-udp-nat-probe-client-execution | 客户端对任意非 TCP UDP 协议接受 NAT probe directive、构造同协议探测目标，并用该协议注册的 UDP 网络执行探测 | 只影响 `SNClientService` 校验/构造/网络选择及对应单元测试 | 把 Ext(_) 视为 UDP 族；自定义 UDP 网络必须实现 `UdpTunnelNetwork` 才能执行 | 单元测试证明 Ext(1) directive 可接受、Ext 目标被交给 Ext 网络探测、本地回退同协议执行 | 不改 wire；不新增真实 socket 自定义 UDP 集成用例 |

## Success Criteria
- 系统可见结果：客户端通过 `Protocol::Ext(_)` UDP 隧道收到 directive 时，`validate_probe_directive` 不再因 transport 拒绝，`execute_probe_directive` 会调用该协议注册的 `UdpTunnelNetwork::probe_nat_profile`。
- 系统可见结果：`build_nat_probe_endpoints` 生成的探测目标协议与 active 协议一致；QUIC 仍生成 QUIC 目标，Ext(1) 生成 Ext(1) 目标。
- 所需证据：
  - `cargo test -p p2p-frame --features x509 nat_probe_directive --lib` 通过（含新增 Ext directive/执行/本地回退测试）
  - `cargo test -p p2p-frame --features x509 --lib` 通过
  - 如涉及日志断言，`cargo test -p p2p-frame --test nat_probe_logging_contract` 通过
- 非目标：不声明公网/多主机/自定义 UDP socket 集成环境验证完成；本任务只补客户端能力与本地单测。

## Risks
遵守项目 `Endpoint::is_udp()` 语义，把 `Protocol::Tcp` 之外的所有协议视为 UDP 族；若未来某个 `Protocol::Ext(_)` 实际代表非 UDP 传输，它的探测资格将错误放行。当前用户确认该语义即为期望边界。另外，若某 Ext 协议已注册为 `TunnelNetwork` 但没有实现 `UdpTunnelNetwork`，`probe_endpoints`/`probe_local` 仍会按 `NotSupport` 失败并回退 `unknown`，这与 QUIC 行为一致。
