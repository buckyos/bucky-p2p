---
task_manifest: task.yaml
status: approved
---

# rendezvous 预测按探测快照协议选择 listener Proposal

Risk profile: not-created

## Workflow Tier Judgment
- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 修复集中在 p2p-frame `TunnelManager` 的 rendezvous 预测网络选择，影响运行时 UDP 族协议（含 `Protocol::Ext(_)`）的预测调用链，但不改 wire 字段、证书签名、持久化或部署；按默认规则归为 standard。
- Proposal and tier confirmation: 用户于 2026-09-08 选择方案 A（只改 tunnel_manager 按 endpoint 对应 listener；wire/candidate/punch 扩展留作后续），确认按 standard tier 执行。

## Background and Goal
任务 069 已把 `ActiveSN.nat_probe_endpoints` 按 active 协议保存（QUIC 或 `Protocol::Ext(_)` 等 UDP 族协议），因此当活动 SN 使用 Ext UDP 时，快照端点是 Ext 协议。但 `TunnelManager::predict_owned_rendezvous_endpoints` 仍固定执行 `net_manager.get_network(Protocol::Quic)`，并把 Ext 探测目标原样传给 QUIC listener；`QuicTunnelListener::probe_nat_profile`/`predict_traversal_endpoints` 会以「target protocol must be QUIC」返回 `InvalidParam`，导致 rendezvous 预测步骤失败。

目标：`TunnelManager` 按探测快照/预测端点所属协议选择对应 listener（`NetManager::get_network(endpoint.protocol())` 并要求 `as_udp_tunnel_network()`），使 Ext UDP 活动 SN 的预测调用落到正确网络；QUIC 路径行为保持不变。

## Scope
### In scope
- `predict_owned_rendezvous_endpoints`：从 `SnNatProbeSnapshot.endpoints` 推导探测协议，按该协议从 `NetManager` 选择网络，并要求实现 `UdpTunnelNetwork`；不再固定取 `Protocol::Quic`。
- `open_rendezvous_tunnel` 与 `on_sn_rendezvous` 中已有的两个 `validate_traversal_prediction` 调用：按 prediction endpoints 对应协议选择网络进行校验，不再固定 QUIC。
- 保留空快照/未注册协议/未实现 `UdpTunnelNetwork` 时的明确失败行为。
- 新增单元/模块测试：用伪造 Ext UDP `TunnelNetwork + UdpTunnelNetwork` 验证预测与校验被委托到对应协议网络，QUIC 回归不变。

### Out of scope
- 不修改 `validate_rendezvous_endpoints`/`SnTunnelRendezvous*` wire 校验、`nat_candidates`/`rendezvous_base_endpoints` 的协议锚定、`udp_punch_enabled_for_candidate` 的 QUIC-only 问题或其它 rendezvous 地址域判断。
- 不新增真实 socket 的自定义 UDP 端到端测试。
- 不改变 NAT 画像、TTL、探测周期、owner/conn 语义。

### Boundary with neighboring modules
`sn_service.rs` 只负责构造与保存探测快照；本任务只改 `tunnel_manager.rs` 如何根据该快照选择预测网络。若某个 Ext 网络已注册但没有实现 `UdpTunnelNetwork`，预测应 fail-closed 为 `NotSupport`，与 `sn_service` 探测回退行为一致。

## Requirement Review
需求成立：`build_nat_probe_endpoints` 已按 active 协议生成目标，但预测侧仍写死 QUIC，是任务 069 打通的 UDP 族能力在 tunnel/rendezvous 层的遗漏。按端点协议选择 listener 是对同一个 UDP 族语义的对称补齐。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-rendezvous-ext-prediction-listener | `TunnelManager` 预测与校验按探测快照/预测端点协议选择对应 UDP 网络 | 只影响 `tunnel_manager.rs` 中预测网络选择及对应测试 | 自定义 UDP 网络必须实现 `UdpTunnelNetwork` 且与快照协议匹配；QUIC 不变 | 新测试证明 Ext(1) 快照会把 predict/validate 委托给 Ext(1) 网络；全量 lib 及 rendezvous 相关测试通过 | 不改 wire、candidate/punch 协议域、真实 socket 集成 |

## Success Criteria
- 系统可见结果：`TunnelManager::predict_owned_rendezvous_endpoints` 对 `Protocol::Ext(_)` 探测快照不再调用 QUIC listener，而是调用该协议注册的 `UdpTunnelNetwork::predict_traversal_endpoints` 与 `validate_traversal_prediction`。
- 系统可见结果：QUIC 活动 SN 的预测仍走 QUIC 网络并保持既有成功/失败语义。
- 所需证据：
  - 新增定向测试：Ext 探测快照的预测/校验委托到 Ext 网络；未注册协议或非 UDP 网络 fail-closed。
  - `cargo test -p p2p-frame --features x509 nat_type_aware_tests --lib`（或对应定向测试名）通过。
  - `cargo test -p p2p-frame --features x509 --lib` 通过。
- 非目标：不声明公网/多主机/自定义 UDP socket 集成已验证；若 Ext 网络预测返回的端点协议被现有 rendezvous wire 校验拒绝，该问题属于后续 wire/candidate 范围。

## Risks
一个明确的未决边界：本任务只修复「预测时选择哪个 listener」。如果 Ext UDP 网络的 `predict_traversal_endpoints` 返回 `Protocol::Ext(_)` 端点，而 `validate_rendezvous_endpoints`（`sn/protocol/sn.rs`）仍只允许 `Quic`/`Tcp` 且 punch 要求 QUIC，那么预测调用本身会成功，但该请求/响应在 wire 层仍可能被拒。需要用户确认：Ext 网络预测输出是否被约束为现有 wire 可接受协议，还是要把 UDP 族协议支持扩展到 rendezvous wire/candidate/punch 校验。
