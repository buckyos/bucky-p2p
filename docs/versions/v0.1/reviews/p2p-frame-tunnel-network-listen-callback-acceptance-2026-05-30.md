# 模块验收报告

## 对象与范围
- 模块：`p2p-frame`
- 版本：`v0.1`
- 评审日期：`2026-05-30`
- 范围内：`tunnel_network_listen_callback`，以及与 `networks_sfo_reuseport_tcp_listener`、`networks_sfo_reuseport_quic_listener_socket` 共享的 listener 注册和入站 tunnel 交付路径
- 范围外：TCP/QUIC tunnel 线协议、TLS 身份校验、QUIC NAT punch policy、heartbeat 语义和 tunnel publish 规则的额外变更

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testing/sfo-reuseport-listeners.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `p2p-frame/src/networks/network.rs`
- `p2p-frame/src/networks/net_manager.rs`
- `p2p-frame/src/networks/tcp/network.rs`
- `p2p-frame/src/networks/quic/network.rs`
- `p2p-frame/src/pn/client/pn_client.rs`
- `p2p-frame/src/stack.rs`
- 测试结果：`cargo check -p p2p-frame`、`python3 ./harness/scripts/test-run.py p2p-frame unit`、`python3 ./harness/scripts/test-run.py p2p-frame integration`

## 评审顺序
1. 审查已批准 proposal 的新增 `P-TUNNEL-NETWORK-CALLBACK-1` 基线。
2. 审查 design 和长期模块边界是否同步描述 `TunnelNetwork::listen(...)` 回调化。
3. 审查 testing/testplan 是否为 `tunnel_network_listen_callback` 映射 unit 和 integration 证据。
4. 审查实现是否移除公共 `TunnelNetwork::listeners()` 并把入站 tunnel 交付改为回调。
5. 审查实际验证结果。

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| 无 | - | - | - | 未发现阻塞问题 | 否 |

## 一致性摘要
- Proposal 与 design：一致。proposal 要求 `listen(...)` 传入异步回调、返回 `P2pResult<()>`、移除 `listeners()`；design 明确 `IncomingTunnelCallback`、`NetManager` 回调分发和 `listener_infos()` 保留。
- Design 与模块边界文档：一致。`docs/modules/p2p-frame.md` 已同步说明 TCP/QUIC 入站 tunnel 通过 `TunnelNetwork::listen(...)` 回调交付。
- Design 与 implementation：一致。`TunnelNetwork` trait 已新增 `IncomingTunnelCallback`，`listen(...)` 返回 `P2pResult<()>`，公共 trait 不再包含 `listeners()`；`NetManager` 注册回调并复用 validator、订阅发布和 reject close；stack proxy listener 改用 `listener_infos()` 判断幂等监听。
- Testing 文档与 testplan：一致。`testing.md`、`testing/sfo-reuseport-listeners.md` 和 `testplan.yaml` 均映射 `tunnel_network_listen_callback` 的 unit/integration 覆盖。
- Testplan 与实际执行：一致。unit 和 integration 入口均已运行通过。
- 验收标准可追溯性：通过 `tunnel_network_listen_callback` 直接回溯到 `P-TUNNEL-NETWORK-CALLBACK-1`。

## 结论
- 通过或失败：通过
- 原因：文档、实现、测试设计和实际验证结果均覆盖并满足 `TunnelNetwork` listener 回调化要求；未发现 proposal/design/code/test 之间的阻塞性不一致。

## 退回路由
- Proposal 任务：无
- Design 任务：无
- Testing 任务：无
- Implementation 任务：无

## 残余风险
- QUIC/TCP concrete network 的部分历史 unit 仍通过 test-only listener accessor 观察内部 listener；公共 trait 已移除 `listeners()`，但这些测试保留了内部观测点用于既有 QUIC 行为断言。
- `TunnelNetwork::listen(...)` 回调为异步 boxed future；后续若有高频入站 tunnel 压力测试，应继续观察回调执行背压和任务生命周期。
