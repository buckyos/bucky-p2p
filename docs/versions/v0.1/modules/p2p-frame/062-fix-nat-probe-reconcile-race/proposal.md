---
task_manifest: task.yaml
status: approved
---

# 修复旧 NAT 注册清理误删重连后新注册的竞态

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 单模块（p2p-frame SN service）并发缺陷修复：改动集中在 `reconcile_nat_probe_authority` 及其调用的 scheduler/peer_mgr 收口，不改变公开协议、wire format、持久化数据或部署兼容性；竞态可用确定性交错测试受控复现。并发敏感但影响有界，故建议 standard；若用户希望完整 design/testing/acceptance 分阶段证据，可选择 high-risk。
- Proposal and tier confirmation: 用户于 2026-09-05 在确认请求（无未决问题）中选择“确认 standard（推荐）”。

## Background and Goal

`SnService::reconcile_nat_probe_authority()`（p2p-frame/src/sn/service/service.rs:1456）清理已消失的 NAT probe authority 注册时存在跨 await 的竞态：

1. 先在 scheduler 锁内快照读取旧 authority tunnel（`T_old`）。
2. 随后 `get_peer_tunnels(...).await` —— 在该 await 期间，另一任务可能已因断线执行 `on_peer_disconnected` → `remove_peer(PeerDisconnected)` 删除 `T_old` 注册，且对端在新连接上通过 `observe_control`/`observe_capable_report` 建立了新注册（`T_new`，新的 `registration_generation`），甚至完成探测并通过 `apply_nat_probe_transition` 发布了新画像。
3. 旧清理任务恢复后仅凭"隧道列表中无 `T_old`"即调用 `remove_peer(peer_id, TunnelMissing)` —— 该删除只按 `peer_id` 匹配，实际删除的是新注册；随后 `invalidate_net_profile(peer_id)` 无条件失效画像，抹掉新注册刚发布的画像。后果：新注册状态丢失（in-flight、pending trigger、画像记账），后续探测结果因 `MissingAuthority` 被拒，新探测结果失去关联。

## Scope

### In scope

- 将清理改为条件化删除：快照读取 `(authority_tunnel_id, registration_generation)`；在 scheduler 锁内重新比对当前注册仍与快照一致（tunnel 与 generation 均匹配）才执行 `remove_peer`，否则本次 reconcile 视为过期放弃，不做任何删除与画像失效。
- 保护画像失效操作：将 profile 应用（`apply_nat_probe_transition` 的 set/invalidate、maintain 的 expire 失效、`set_ports` 的失效）与 reconcile 的失效收口到 scheduler 锁作用域内执行，使"删除判定 + peer_mgr 画像失效"对并发重注册原子；锁序固定 scheduler → peer_mgr（peer_mgr 不反向获取 scheduler 锁，无死锁风险）。
- 被删除注册无画像时不做无意义的画像覆盖，保持与现有 `remove_peer` 日志语义一致。
- 新增确定性交错测试：以分步 API 复现"快照 → 断线重注册并发布新画像 → 旧快照清理"的交错，断言旧清理被拒绝、新注册与新画像幸存；补充 maintain 路径回归。

### Out of scope

- 不改变 NAT probe 协议、wire format、`NatProbeDirective`/`NatProbeResult` 字段或客户端行为。
- 不重构 `NatProbeScheduler` 数据模型或 `PeerManager` 的画像存储结构（不引入画像 generation token）。
- 不处理 `on_peer_disconnected`（service.rs:1731）自身删除路径的并发语义（其触发与删除之间无 await，属另一路径，如发现需另行立项）。
- 不做多线程真实竞争的压力复现；以确定性交错单测作为受控复现证据。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-nat-probe-reconcile-race | 旧 NAT 注册清理删除时重新比对 authority 与注册 generation，仅当快照仍匹配才删除，并保护对应画像的失效操作；重注册发生后旧清理放弃。 | 判定与失效属 p2p-frame `sn/service` 内部；快照重比对在 scheduler 锁内，与重注册的 profile 写共享同一串行点。 | 扩大 scheduler 标准互斥锁持锁范围覆盖同步的 peer_mgr 调用（无 await）；锁序固定 scheduler → peer_mgr。 | 确定性交错测试证明重注册后旧清理不删新注册/不失效新画像，且 genuine 清理仍生效；nat-probe 与完整 lib 套件通过。 | 不改变协议/wire/公开契约；不加画像 generation token；不处理 `on_peer_disconnected` 自身路径。 |

## Success Criteria

- 确定性交错测试证明：持有旧快照的清理任务在重注册发生后不会删除新注册、不会失效新画像，且函数正确放弃。
- 旧行为回归保持：authority 真正消失（无重注册）时清理与画像失效仍发生；现有 nat probe scheduler/config/service 测试全部通过。
- `cargo build -p p2p-frame` 及相关测试通过。

## Material Assumptions / Tradeoffs

- 通过扩大 scheduler `std::sync::Mutex` 的持锁范围覆盖同步的 peer_mgr 调用（无 await），不引入新锁，死锁面经锁序审查可控。
- reconcile 在 await 后仅依据快照一致性判定删除，不重新查询隧道列表；隧道 conn_id 为每连接唯一，重连新隧道必然携带新 conn_id，该假设由现有 `CmdTunnelId` 语义支撑。
