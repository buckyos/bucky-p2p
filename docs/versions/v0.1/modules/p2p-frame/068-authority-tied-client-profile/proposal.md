---
task_manifest: task.yaml
status: approved
---

# 普通画像接收绑定 UDP authority 隧道 Proposal

Risk profile: not-created

## Workflow Tier Judgment
- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 修复集中在 p2p-frame 服务端 `NatProbeScheduler` 与 `handle_report_sn` 的 authority/画像发布边界，涉及并发连接下的授权一致性（runtime/concurrency 信任边界），但不改 SN wire、证书签名、持久化或协议；按默认规则归为 standard，不进入 high-risk。
- Proposal and tier confirmation: 用户于 2026-09-07 确认按 standard tier 执行。

## Background and Goal
`handle_report_sn`（`p2p-frame/src/sn/service/service.rs` 1628 附近）在调用 `observe_capable_report` 之后，把同一 `ReportSn.net_profile` 无条件交给 `NatProbeScheduler::observe_reported_profile`。该方法只接收 peer id，没有 tunnel id，也不判断传输协议。若同一认证身份同时存在多条连接，非 authority QUIC 或 TCP 上报即使已被 `observe_capable_report` 忽略，仍可经该路径覆盖当前 authority 的画像，进而被 `handle_query_sn` / `local_peer_detail` 作为目标 peer 的 NAT 分类对外服务。

目标：普通画像接收必须来自当前有效的 UDP 族 authority 隧道。同时把 authority 资格判断统一为 UDP 族（`Endpoint::is_udp()`），使 QUIC 与其它 UDP 自定义协议隧道都能作为 authority，而 TCP 一律排除。

## Scope
### In scope
- `NatProbeScheduler::observe_reported_profile`：增加 `tunnel_id` 与 `remote_endpoint` 参数，仅在 `state.authority_tunnel_id == tunnel_id` 且 `remote_endpoint.is_udp()` 时接受并发布画像；否则返回空 transition。
- `NatProbeScheduler::observe_capable_report` / `observe_control`：authority 资格从 `Protocol::Quic` 放宽/统一为 `is_udp()`，保证 UDP 族隧道能建立 authority，profile 接收与 authority 建立共用同一资格定义。
- `handle_report_sn`：把本次 `tunnel_id` 与 `observed_tunnel` 一并传入 `observe_reported_profile`；无隧道信息时不再接受画像。
- 增加反例测试：非 authority UDP 隧道或 TCP 隧道携带 fresh `net_profile` 不能覆盖 authority 画像；UDP 族 authority 隧道（含非 QUIC 自定义 UDP 协议）可正常上报画像。

### Out of scope
- 不修改 `ReportSn`/`ReportSnResp`/`NatProbeDirective` 等 wire 字段或证书签名。
- 不改变画像新鲜度/`observed_at` 排序策略。
- 不改变 authority 消失/断开/超时清理或 `observe_ineligible_report` 的既有失效语义。
- 不改变客户端本地探测/周期上报逻辑。

### Boundary with neighboring modules
服务端收口在 `p2p-frame/src/sn/service/nat_probe_scheduler.rs` 与 `service.rs` 的合并点；客户端无需改动。下游 query/detail 继续从 scheduler/peer_mgr 读取 profile，行为差异只体现在非 authority 上报不再能覆盖当前 authority 画像。

## Requirement Review
需求成立。`observe_capable_report` 已经把 authority 注册、probe result、directive 的接收绑定到当前 authority 隧道，但画像接收路径漏掉了同一绑定；这使同一身份可借非 authority 连接污染服务端官方画像。TCP 上报本就不具备 authority 资格；UDP 族自定义协议（如 PN 的 `Protocol::Ext(1)`）按项目 `Endpoint::is_udp()` 语义同样应被接受为 authority 隧道。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-authority-tied-client-profile | `observe_reported_profile` 只在当前 authority UDP 隧道上报时接受画像；authority 资格统一为 UDP 族 | 只影响 `nat_probe_scheduler.rs`、`service.rs` 的 profile 合并与调度器协议判定，以及 scheduler 测试 | 非 authority 连接上的普通上报不再能发布画像；UDP 自定义协议可成为 authority | 定向测试证明非 authority UDP/TCP 上报被忽略、UDP 族 authority 测绘可正常发布；调度器/服务层/lib/logging 套件通过 | 不改 wire/协议；不改新鲜度策略 |

## Success Criteria
- 系统可见结果：同身份非 authority UDP 或 TCP 连接携带 `net_profile` 上报时，scheduler 与 peer_mgr profile 不变。
- 系统可见结果：UDP 族 authority 隧道（QUIC 或非 QUIC UDP 协议）的普通画像上报仍可更新服务端画像。
- 所需证据：
  - `cargo test -p p2p-frame --features x509 nat_probe_scheduler --lib` 通过（含新增反例测试）
  - `cargo test -p p2p-frame --features x509 --lib` 全量通过
  - `cargo test -p p2p-frame --test nat_probe_logging_contract` 通过（如日志断言受影响）
- 非目标：不声明公网/多主机/部署环境验证完成；本任务只验证服务端 authority 绑定与现有本地套件。

## Risks
放宽 authority 资格到所有 UDP 族协议可能让此前只有 QUIC 的路径接受其它 UDP 自定义协议；这是用户确认的语义边界，交由 `is_udp()` 统一行使。非 authority UDP/TCP 上报携带的画像在本次修复后会被忽略，查询/穿越仍只使用 authority 隧道确认的画像。
