# 模块验收报告

## 对象与范围
- 模块：`p2p-frame`
- 版本：`v0.1`
- 评审日期：2026-05-13
- 范围内：`server_reflexive_quic_nat_keepalive`，包括 UDP punch candidate policy 收窄为 `EndpointArea::ServerReflexive` QUIC endpoint、QUIC heartbeat timeout 调整为 30 秒、相关 proposal/design/testing/testplan/pipeline 文档与 unit/integration 证据。
- 范围外：既有 untracked 文档/规则/历史验收报告、PN control channel 既有变更、endpoint area 旧交付、真实 NAT 成功率 DV。

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/design/tunnel-nat-traversal.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `harness/pipeline-plan.md`
- `p2p-frame/src/tunnel/tunnel_manager.rs`
- `p2p-frame/src/networks/quic/listener.rs`
- `p2p-frame/src/networks/quic/tunnel.rs`
- 测试结果：`schema-check` passed；`admission-check --change-id server_reflexive_quic_nat_keepalive` passed；`cargo test -p p2p-frame` passed；`python3 ./harness/scripts/test-run.py p2p-frame integration` passed。

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| 无 | none | acceptance | 完整 diff 与测试结果 | 未发现阻塞问题。 | no |

## 一致性摘要
- Proposal 与 design：一致。proposal 要求 punch 只面向 `ServerReflexive` QUIC candidate，且 heartbeat interval 不变、timeout 为 30 秒；design 已直接映射到 `tunnel_manager.rs`、`networks/quic/tunnel.rs` 和必要 listener 测试。
- Design 与模块边界文档：一致。长期模块文档已同步 QUIC UDP punch 只由 `ServerReflexive` candidate 触发，并记录 heartbeat timeout 为 30 秒。
- Design 与 implementation：一致。`TunnelManager::udp_punch_enabled_for_candidate(...)` 和 QUIC listener 内部二次策略都要求 `EndpointArea::ServerReflexive`、QUIC、非 LAN IPv4、非 0 端口；`COMMAND_HEARTBEAT_INTERVAL` 保持 5 秒，`COMMAND_HEARTBEAT_TIMEOUT` 调整为 30 秒。
- Testing 文档与 testplan：一致。testing 和 testplan 均包含 `server_reflexive_quic_nat_keepalive` 的 unit、disabled DV gap 和 integration 映射。
- Testplan 与实际执行：一致。unit 入口 `cargo test -p p2p-frame` 通过，integration 入口 `python3 ./harness/scripts/test-run.py p2p-frame integration` 通过。
- 验收标准可追溯性：满足。新增 unit 覆盖 `ServerReflexive` only punch policy、反例集合和 heartbeat interval/timeout 常量。

## 结论
- 通过或失败：通过。
- 原因：proposal、design、testing、testplan、implementation 和验证证据一致；未发现需要退回的 proposal/design/testing/implementation 问题。

## 退回路由
- Proposal 任务：无。
- Design 任务：无。
- Testing 任务：无。
- Implementation 任务：无。

## 残余风险
- unit/integration 验证的是策略与兼容性，不证明任意真实 NAT 环境下的穿透成功率。
- 工作树中存在多项本轮未触碰的 untracked 文件；本报告未对其内容作通过结论。
