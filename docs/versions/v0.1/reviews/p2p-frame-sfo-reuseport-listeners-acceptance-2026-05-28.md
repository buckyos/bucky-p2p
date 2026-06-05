# 模块验收报告

## 对象与范围
- 模块：p2p-frame
- 版本：v0.1
- 评审日期：2026-05-28
- 范围内：`networks_sfo_reuseport_tcp_listener`、`networks_sfo_reuseport_quic_listener_socket`
- 范围外：新增协议语义、`TunnelNetwork` trait 改造、独立 `NetworkServerRuntime` 抽象

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testing/sfo-reuseport-listeners.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `p2p-frame/src/stack.rs`
- `p2p-frame/src/networks/tcp/listener.rs`
- `p2p-frame/src/networks/tcp/network.rs`
- `p2p-frame/src/networks/quic/listener.rs`
- `p2p-frame/src/networks/quic/network.rs`
- 实际验证结果

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| SFO-ACC-001 | note | acceptance | `stage-scope-check.py` | 当前 worktree 在本任务前已有跨阶段和无关改动，阶段范围检查不能作为本次变更的干净证据。已用 schema、admission、module packet、unit 和 integration 结果补充验收证据。 | no |

## 一致性摘要
- Proposal 与 design：一致。两个 change_id 均映射到 `sfo-reuseport` listener 重构，且明确不引入 `NetworkServerRuntime`。
- Design 与模块边界文档：一致。长期模块文档已声明 TCP 使用 `TcpServer`、QUIC 使用 `QuicServer` 和 Quinn `AsyncUdpSocket` 适配。
- Design 与 implementation：一致。`P2pConfig` 暴露 `sfo_reuseport::ServerRuntime` 注入；stack 默认创建并共享 runtime；TCP listener 使用 `TcpServer::serve(...)`；QUIC listener 使用 `QuicServer::serve(...)`、`listener_socket()` 和自定义 `AsyncUdpSocket`。
- Testing 文档与 testplan：一致。新增测试补充文档和 testplan change_id 覆盖已同步。
- Testplan 与实际执行：一致。unit 和 integration 入口均已执行通过。
- 验收标准可追溯性：通过 proposal、design、testing、testplan、实现代码和命令结果可追溯。

## 实际结果
- `cargo check -p p2p-frame`：通过。
- `python3 ./harness/scripts/test-run.py p2p-frame unit`：通过，100 个 p2p-frame 测试通过。
- `python3 ./harness/scripts/test-run.py p2p-frame integration`：通过，workspace 测试通过。
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`：通过。
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id networks_sfo_reuseport_tcp_listener`：通过。
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id networks_sfo_reuseport_quic_listener_socket`：通过。
- `python3 ./harness/scripts/verify-module-packet.py v0.1 p2p-frame`：通过。

## 结论
- 通过。
- 原因：已批准 proposal/design、测试制品、实现和实际验证结果一致；未发现阻塞项。

## 退回路由
- Proposal 任务：无。
- Design 任务：无。
- Testing 任务：无。
- Implementation 任务：无。

## 残余风险
- TCP `TcpServer` 当前不暴露 listener socket；端口 `0` 由实现先分配具体端口再交给 `TcpServer`，存在极小端口竞争窗口。
- QUIC `AsyncUdpSocket` 当前只实现单包 receive queue 和基础 send 分段；如后续启用高级 UDP offload，需要扩展 adapter。
