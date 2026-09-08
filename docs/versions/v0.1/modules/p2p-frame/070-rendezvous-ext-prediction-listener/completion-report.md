# Completion Report: 070-rendezvous-ext-prediction-listener

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/070-rendezvous-ext-prediction-listener.md

## Delivery Summary

- Outcome: `TunnelManager` 的 rendezvous 预测不再固定选择 QUIC 网络；`predict_owned_rendezvous_endpoints` 以及 `open_rendezvous_tunnel`/`on_sn_rendezvous` 中的预测校验都按探测快照/prediction endpoints 的协议从 `NetManager` 选择对应 `UdpTunnelNetwork`。Ext(1) 探测快照会委托到 Ext(1) 网络执行 predict/validate，QUIC 路径行为不变；缺少对应协议网络时 fail-closed 返回 `NotFound`，不带 UDP 能力时返回 `NotSupport`。
- Handoff: 生产改动只有 `p2p-frame/src/tunnel/tunnel_manager.rs` 的网络选择；`p2p-frame/src/sn/client/sn_service.rs` 仅新增 `#[cfg(test)]` 的 active-snapshot 注入 helper；wire、candidate/punch 协议域未改。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|--------|--------|
| CHG-rendezvous-ext-prediction-listener | `TunnelManager` 预测与校验按探测快照/预测端点协议选择对应 UDP 网络 | proposal.md P-001 | `udp_prediction_network` 按端点协议选网络；三处 `get_network(Protocol::Quic)` 已替换；新增 Ext(1) 委托与缺失网络 fail-closed 测试 | 定向 2 项、全量 lib 505 项通过 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | `predict_owned_rendezvous_endpoints` 的协议推导、预测后立即校验、另两处 validation 的网络选择 | 构造 Ext(1) 探测快照并注册仅 Ext(1) UDP 网络，确认 predict/validate 均委托 Ext 网络而不是 QUIC | QUIC 旧路径改为由第一个端点协议驱动；带两个 Ext 端点的快照会选 Ext(1) 网络，注册缺失时返回 NotFound | pass |
| boundaries-and-failure-paths | 空 prediction endpoints、协议未注册、网络未实现 `UdpTunnelNetwork` 三种分支 | 空 endpoints 先返回 `NotFound`；只注册不匹配协议的其它网络时 `get_network(protocol)` 拒绝；网络存在但不具备 UDP 能力时 `NotSupport` | fail-closed 行为覆盖；不会再落入 QUIC 网络为非 QUIC 目标抛 InvalidParam | pass |
| regression-and-side-effects | QUIC 既有预测/validation 测试、NatPlan/rendezvous 测试、sn_service 既有 Ext UDP 测试 | 重跑全部 lib `--test-threads=1` 505 项 | 505/505 通过，无与本改动相关的回归；`git diff --check` 无空白错误 | pass |

## Verification

- Targeted check:
  - `cargo test -p p2p-frame --features x509 --lib rendezvous_ext_prediction -- --nocapture`：2 项通过，含 `rendezvous_ext_prediction_uses_snapshot_protocol_network` 与 `rendezvous_ext_prediction_fails_closed_when_snapshot_protocol_network_missing`
  - `cargo test -p p2p-frame --features x509 --lib -- --test-threads=1`：505 项通过
- Result: pass
- Exception reason: n/a

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 070-F-001 | none | 定向 2 项与全量 505 项通过；失败分支断言明确 | 未发现新增缺陷 | no |
| 070-F-002 | info | wire 校验仍是 `Quic/Tcp` 且 punch 要求 QUIC | 方案 A 明确保留：若 Ext 网络预测返回不被现有 rendezvous wire 接受的协议端点，Wire/candidate/punch 支持需后续任务处理 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: `TunnelManager` 已按探测快照协议选择对应 UDP 网络，Ext UDP 预测不再把非 QUIC 目标丢给 QUIC listener；QUIC 回归与缺失网络 fail-closed 均有定向证据，全量 lib 通过。
