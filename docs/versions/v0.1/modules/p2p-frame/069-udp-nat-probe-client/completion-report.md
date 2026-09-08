# Completion Report: 069-udp-nat-probe-client

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/069-udp-nat-probe-client.md

## Delivery Summary

- Outcome: `SNClientService` 已按“非 TCP UDP 协议都支持”接受 NAT probe directive：`Protocol::Ext(_)` active 协议不再被 transport 拒绝，探测目标按 active 协议重建，`probe_endpoints`/`probe_local` 按端点协议选择注册的 UDP 网络执行探测。本地伪造 Ext UDP 网络测试证明 directive 与本地回退都会调用对应协议的 `probe_nat_profile`。
- Handoff: 改动集中在 `p2p-frame/src/sn/client/sn_service.rs` 的校验/目标构造/网络选择，以及 `p2p-frame/tests/unit/sn_tests/client/nat_probe_directive_tests.rs` 的客户端执行与本地回退测试；wire、证书、服务端 authority 语义不变。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|--------|--------|
| CHG-udp-nat-probe-client-execution | 客户端对任意非 TCP UDP 协议接受 NAT probe directive、构造同协议探测目标，并用该协议注册的 UDP 网络执行探测 | proposal.md P-001 | `validate_nat_probe_target`/`validate_probe_directive` 只拒绝 TCP；`build_nat_probe_endpoints` 使用 `active_protocol`；`probe_endpoints`/`probe_local` 按端点协议选网络；新增 Ext(1) directive/执行/本地回退测试 | 定向 9 项、全量 lib 503 项、日志契约 3 项通过 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | `validate_nat_probe_target` 与 `validate_probe_directive` 的 TCP/UDP 判定、`build_nat_probe_endpoints` 协议字段、`probe_endpoints`/`probe_local` 的 `NetManager::get_network` 选择 | 反向核对 TCP active、`sn_endpoint.protocol()` 不匹配 active 协议、Ext 网络未实现 `UdpTunnelNetwork`、端点列表为空时是否仍能安全拒绝或回退 | TCP 仍被 `transport_not_udp` 拒绝；端点协议不匹配仍被 `active_endpoint_protocol` 拒绝；只有实现 `UdpTunnelNetwork` 的网络能执行探测；空端点默认回退 QUIC 的行为与旧路径一致 | pass |
| boundaries-and-failure-paths | ActiveSN `protocol`/`sn_endpoint` 对 Ext(1) 的匹配、directive `expires_at`/版本/重放校验顺序、QUIC 既有路径 | 构造 Ext(1) directive 与注册的 Ext 网络，确认执行调用 Ext 网络而非 QUIC；同时保持 QUIC 原测试全绿 | QUIC 行为不变；Ext directive 在版本/身份/到期/重放校验通过后执行；`expires_at` 过期仍会正确拒绝，测试用未来到期时间覆盖正例 | pass |
| regression-and-side-effects | 全量 lib 503 项、`nat_probe_logging_contract` 3 项、既有 `nat_probe_directive` 测试 | 检查并发全量 lib 下是否暴露与本次改动相关的失败 | 全量复跑 503 项通过；首轮唯一失败是与本次改动无关的 TCP listener `AddrInUse` 并行冲突，单独重跑该用例通过；日志契约通过 | pass |

## Verification

- Targeted check:
  - `cargo test -p p2p-frame --features x509 nat_probe_directive --lib`：9 项通过，含新增 Ext directive 接受/目标重建/客户端执行/本地回退测试
  - `cargo test -p p2p-frame --features x509 --lib`：503 项通过
  - `cargo test -p p2p-frame --features x509 --test nat_probe_logging_contract`：3 项通过
- Result: pass
- Exception reason: n/a

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 069-F-001 | none | 定向/全量/日志测试全绿 | 未发现新增缺陷 | no |
| 069-F-002 | none | 全量首轮出现的 `tcp_only_registration_never_receives_or_executes_probe` TCP `AddrInUse` 并行冲突 | 单独重跑该用例通过，属于既有并行端口分配抖动，与本任务改动无因果链 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 客户端已按 UDP 族（含 `Protocol::Ext(_)`）贯通 directive 接受、目标构造与网络选择，定向/全量/日志证据通过；TCP 仍 fail-closed，服务端 authority 语义未改。
