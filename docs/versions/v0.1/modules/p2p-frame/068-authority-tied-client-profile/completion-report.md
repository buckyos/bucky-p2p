# Completion Report: 068-authority-tied-client-profile

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/068-authority-tied-client-profile.md

## Delivery Summary

- Outcome: `observe_reported_profile` 现在必须同时满足当前 authority 隧道匹配与 UDP 族协议才接受客户端普通画像。非 authority UDP 或 TCP 上报即使在 `observe_capable_report` 之后走 profile 路径，也不能覆盖当前 authority 画像；QUIC 与其它 UDP 自定义协议隧道（如 `Protocol::Ext(1)`）可正常担任 authority 并接收画像。`handle_report_sn` 已把 `tunnel_id`/`observed_tunnel` 传入画像接收路径。
- Handoff: 生产改动集中在 `nat_probe_scheduler.rs` 的 authority 资格与画像接收校验，以及 `service.rs` 的 `handle_report_sn` 合并点。调度器新增两把层级测试覆盖非 authority 反例与 UDP 族 authority 正例。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-authority-tied-client-profile | `observe_reported_profile` 只在当前 authority UDP 隧道上报时接受画像；authority 资格统一为 UDP 族 | proposal.md P-001 | `observe_reported_profile` 增加 `tunnel_id`/`remote_endpoint` 并校验 `authority_tunnel_id` 与 `is_udp()`；`observe_capable_report`/`observe_control` 使用 `is_udp()` | 新增两个测试证明非 authority UDP/TCP 被忽略、Ext(1) UDP authority 可正常接收 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | `observe_reported_profile` 的新增校验顺序、`handle_report_sn` 的字段传入、peer_mgr publish 合并 | 反向核对：非 authority QUIC、TCP、无 observed_tunnel 时是否仍可能写入；authority UDP 上报是否仍被接受 | 无 observed_tunnel 时 profile 不进入 scheduler；非 authority 与 TCP 均在写 profile 前返回；fresh/old 排序规则未变 | pass |
| boundaries-and-failure-paths | authority 建立（QUIC vs Ext UDP）、TCP ineligible、authority 隧道变更、profile 新鲜度、`observe_ineligible_report` 失效语义 | 检查 UDP 族放宽是否误放行 TCP，或意外改变既有 QUIC 上 authority/report 行为 | `is_udp()` 等价排除 TCP，既有 QUIC 测试全部通过；`observe_ineligible_report` 的 authority 移除语义未改 | pass |
| regression-and-side-effects | scheduler 16 项、全量 lib 500 项、`nat_probe_logging_contract` 3 项 | 检查日志事件名与隐私/计数约束是否受 `transport=udp` 文本影响 | 日志契约只要求事件名和禁止字段，不依赖 `transport=quic`；三项测试通过 | pass |

## Verification

- Targeted check:
  - `cargo test -p p2p-frame --features x509 nat_probe_scheduler --lib`：16 项通过，含新增 `nat_probe_scheduler_client_profile_requires_udp_authority_tunnel` 与 `nat_probe_scheduler_accepts_any_udp_protocol_as_authority_client_profile`
  - `cargo test -p p2p-frame --features x509 --lib`：500 项通过
  - `cargo test -p p2p-frame --test nat_probe_logging_contract`：3 项通过
- Result: pass
- Exception reason: n/a

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 068-F-001 | none | 定向/全量/日志测试全绿 | 未发现新增缺陷 | no |
| 068-F-002 | none | address-change 等既有保留画像路径与新 authority 校验叠加 | 保留画像语义仍按 067 决策生效，非 authority 上报只能读取/确认，不能覆盖 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 已把普通画像接收绑定到当前 UDP 族 authority 隧道，并增加反例与 UDP 族 authority 正例测试；调度器、全量 lib 与日志契约全部通过。
