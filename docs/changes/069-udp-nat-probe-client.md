# 非 QUIC UDP authority 的客户端探测执行

- Status: complete
- Owner module: p2p-frame
- Task manifest: `docs/versions/v0.1/modules/p2p-frame/069-udp-nat-probe-client/task.yaml`
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/069-udp-nat-probe-client/proposal.md`
- Affected paths: `p2p-frame/src/sn/client/sn_service.rs`, `p2p-frame/tests/unit/sn_tests/client/nat_probe_directive_tests.rs`
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

服务端在 068 已允许任意 UDP 族协议（含 `Protocol::Ext(_)`）建立 authority 并下发 directive，客户端仍把接受与执行锁在 QUIC。本任务把客户端打通为与用户确认一致的“UDP 协议都支持”：

- `validate_nat_probe_target` / `validate_probe_directive`：active 协议为 TCP 时才拒绝（`transport_not_udp`），其它 UDP 族协议（含 `Protocol::Ext(_)`）放行；`sn_endpoint.protocol() == active_protocol` 仍必须成立。
- `build_nat_probe_endpoints`：探测目标协议改为 `active_protocol`，QUIC 仍生成 QUIC 目标，Ext(1) 生成 Ext(1) 目标。
- `probe_endpoints` / `probe_local`：按端点协议从 `NetManager::get_network` 选择网络并继续要求 `as_udp_tunnel_network()`；不再固定取 QUIC 网络。

测试新增三块：Ext(1) directive 正例与目标重建；伪造 Ext UDP `TunnelNetwork + UdpTunnelNetwork` 验证 directive 执行路径确实调用该协议网络；本地回退路径同样通过 Ext 网络执行探测。

## Verification

- Targeted check:
  - `cargo test -p p2p-frame --features x509 nat_probe_directive --lib`：9 项通过，含 `nat_probe_directive_gate_requires_udp_identity_deadline_and_new_request`、`nat_probe_directive_reconstructs_wan_targets_for_any_udp_protocol`、`nat_probe_directive_ext_udp_executes_on_registered_network`、`nat_probe_directive_ext_udp_local_fallback_uses_registered_network`
  - `cargo test -p p2p-frame --features x509 --lib`：503 项通过（复跑通过；首轮在并行运行时出现一次与本次改动无关的 TCP 端口占用冲突，单独重跑该用例通过）
  - `cargo test -p p2p-frame --features x509 --test nat_probe_logging_contract`：3 项通过
- Result: pass
- Residual risk or follow-up: 遵守项目 `Endpoint::is_udp()` 语义，所有非 TCP 协议视为 UDP 族；若未来某个 `Protocol::Ext(_)` 实际代表非 UDP 传输，需单独引入协议族判定。真实公网/自定义 UDP socket 集成不在本任务验证范围。
