# rendezvous 预测按探测快照协议选择 listener

- Status: complete
- Owner module: p2p-frame
- Task manifest: `docs/versions/v0.1/modules/p2p-frame/070-rendezvous-ext-prediction-listener/task.yaml`
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/070-rendezvous-ext-prediction-listener/proposal.md`
- Affected paths: `p2p-frame/src/tunnel/tunnel_manager.rs`, `p2p-frame/tests/nat_type_aware/tunnel_manager_tests.rs`, `p2p-frame/src/sn/client/sn_service.rs`
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

任务 069 已让 `ActiveSN.nat_probe_endpoints` 保存 active 协议（含 `Protocol::Ext(_)`）的目标，但 `TunnelManager::predict_owned_rendezvous_endpoints` 仍固定执行 `get_network(Protocol::Quic)`，导致 Ext 探测快照被交给 QUIC listener，在 `probe_nat_profile`/`predict_traversal_endpoints` 的目标协议校验中返回 `InvalidParam`。

修复为按快照端点协议选择对应 `UdpTunnelNetwork`：
- `predict_owned_rendezvous_endpoints` 从 `SnNatProbeSnapshot.endpoints` 取协议，经 `NetManager::get_network` 选择网络并要求 `as_udp_tunnel_network()`，预测与立即校验都使用该网络。
- `open_rendezvous_tunnel` 与 `on_sn_rendezvous` 中已有的 `validate_traversal_prediction` 改为按 prediction endpoints 协议选择网络。
- QUIC 活动 SN 行为不变；未注册协议或未实现 `UdpTunnelNetwork` 时 fail-closed 返回 `NotFound`/`NotSupport`。

新增伪造 Ext UDP 网络的定向测试，证明快照协议 `Ext(1)` 时 predict/validate 委托到对应网络；另覆盖未注册协议 fail-closed。wire/candidate/punch 的 UDP 族扩展明确不在本任务范围。

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: yes，预测网络选择按探测快照协议变化，影响 Ext UDP 活动 SN 的 rendezvous 预测链路；QUIC 路径等价
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib rendezvous_ext_prediction_uses_snapshot_protocol_network`
- Result: pass
- Residual risk or follow-up: 仅修复预测/listener 选择；若 Ext 网络预测返回不被现有 rendezvous wire 校验接受的协议端点，需后续单独扩展 `validate_rendezvous_endpoints`/candidate/punch 的 UDP 族支持。
