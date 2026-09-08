---
task_manifest: task.yaml
status: approved
---

# 地址变更后不再立即清除/恢复服务端 NAT 画像 Proposal

Risk profile: not-created

## Workflow Tier Judgment
- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 行为修复集中在 p2p-frame 服务端 `NatProbeScheduler` 的地址变更路径，影响 NAT 画像发布时序（runtime/concurrency 边界），但不改 SN wire、证书签名、持久化或协议；按默认规则归为 standard，不进入 high-risk。
- Proposal and tier confirmation: 用户于 2026-09-07 确认按 standard tier 执行。

## Background and Goal
当前 `handle_report_sn`（`p2p-frame/src/sn/service/service.rs` 1626 附近）先调用 `observe_capable_report`，再把同一条 `ReportSn.net_profile` 无条件合并进去。客户端外网地址变化而本地画像尚未过期时，`observe_capable_report` 先清空旧画像并下发 directive，随后 `observe_reported_profile` 又接受同一 report 携带的旧画像，覆盖失效通知，导致服务端刚下发的失效被同一请求立即恢复，画像在地址变化后仍然沿用旧值。

目标：服务端观察到外网地址变化时不再主动清空/发布失效，而是保留当前画像，等待客户端后续上报更新（周期性本地探测或 directive 结果）。这对应“服务端的旧画像不需要清空，只需等客户端上报更新就好了”的确认方向。

## Scope
### In scope
- `NatProbeScheduler::observe_capable_report`：外网地址变化（had_registration && needs_registration）时保留现有 profile，不再把 `profile_update` 置为 `Some(None)`；仍推进 registration generation 并保持 external_address 的 pending trigger，使后续 capable report 继续发出 directive。
- `NatProbeScheduler::observe_control`：地址变化时同样保留现有 profile，不再置失效 transition；保持现有“不立即在此下发 directive，依赖后续 report/observe_capable_report 下发”的行为。
- 同步调整 `nat_probe_scheduler_tests.rs` 中地址变化语义断言：地址变化后旧画像保持可查询，directive 仍在下一条 capable report 发出。
- 若托管流程要求，补充 `docs/changes/067-preserve-server-nat-profile-on-address-change.md` 与任务内 completion report。

### Out of scope
- 不修改 `ReportSn`/`ReportSnResp`/`NatProbeDirective` 等 wire 字段或证书签名。
- 不改变端口配置变更（`set_ports`）的清除语义；该项已由 066 明确为 non-goal。
- 不改变 authority 消失/断开/超时、TCP 不兼容等其它失效路径。
- 不改变客户端本地探测/周期上报逻辑。

### Boundary with neighboring modules
服务端处理边界集中在 `p2p-frame/src/sn/service/nat_probe_scheduler.rs` 与 `handle_report_sn` 的合并点；客户端无需改动。下游 query/detail 继续从 scheduler/peer_mgr 读取 profile，行为差异只体现在地址变更窗口内是否保留旧画像。

## Requirement Review
需求合理。外网地址变化本身只说明客户端可能获得了新的外网地址；在没有新探测结果前，客户端上报携带的旧画像仍是当前可用的测量值。此时服务端先清空、又立即被同一 report 恢复既产生无意义的失效/回填振荡，也不符合“客户端主导探测、服务端只在数据变化时触发”的语义。保留旧画像并等待客户端在 directive/周期探测后上报新值，是最小且符合用户预期的修复方向。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-preserve-nat-profile-on-address-change | `observe_capable_report` 与 `observe_control` 在外网地址变化时不清除/不发布 profile 失效，保留当前画像；directive 仍在 capable report 上通过 pending_trigger 发出。 | 只影响 `p2p-frame/src/sn/service/nat_probe_scheduler.rs` 与 `p2p-frame/tests/unit/sn_tests/service/service/nat_probe_scheduler_tests.rs`；service.rs 的合并逻辑保持现状。 | 地址变更后旧画像可能保留到客户端新值上报（上界约 2h，directive 路径约 30s 窗口），换取不再出现“清空后立即被同一 report 恢复”的振荡。 | 调度器单测证明：地址变化后 `profile_update` 不再是 `Some(None)`，`current_profile` 仍返回旧画像；下一条 capable report 仍发出 external_address directive；收到新 directive 结果后更新为新画像。 | 不修改 wire/协议；不改变端口配置变更清理语义。 |

## Success Criteria
- 系统可见结果：客户端地址变化时，服务端不再先清空后恢复旧画像，而是在新值上报前继续保持旧画像可查询。
- 系统可见结果：地址变化仍通过 capable report 触发 external_address directive，客户端仍能借指令加速重测。
- 所需证据：
  - `cargo test -p p2p-frame --features x509 nat_probe_scheduler --lib` 通过（含新增/调整的地址变化保留画像测试）
  - `cargo test -p p2p-frame --features x509 --lib` 全量通过（或至少 SN service 相关模块）
  - `cargo test -p p2p-frame --test nat_probe_logging_contract` 通过（如日志断言受影响）
- 非目标：不声明公网/多主机/部署环境验证完成；本任务只验证服务端画像更新语义与现有本地套件。

## Risks
保留旧画像意味着地址变化到新值上报之间，query/detail 可能继续返回基于旧地址的 NAT 画像；这是本任务确认的取舍，客户端周期或 directive 完成时间（2h / 指令窗口约 30s）是上界。若后续发现旧画像在特定网络切换场景下显著有害，可再独立评估是否需要快速失效并同步处理同一 report 的 `net_profile`。
