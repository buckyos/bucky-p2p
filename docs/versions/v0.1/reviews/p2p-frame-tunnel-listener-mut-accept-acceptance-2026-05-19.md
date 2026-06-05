# 模块验收报告

## 对象与范围
- 模块：p2p-frame
- 版本：v0.1
- 评审日期：2026-05-19
- 范围内：`tunnel_listener_mut_accept`
- 范围外：QUIC listener 接收并发化、NAT 打洞、PN control channel 等既有工作项

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `harness/pipeline-plan.md`
- 实现代码：`p2p-frame/src/networks/listener.rs`、TCP/QUIC/PN listener、NetManager/TunnelManager/TTP/SN/PN 测试调用点
- 测试结果：
  - `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`：passed
  - `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id tunnel_listener_mut_accept`：passed
  - `cargo check -p p2p-frame`：passed
  - `python3 ./harness/scripts/test-run.py p2p-frame unit`：passed，101 tests
  - `python3 ./harness/scripts/test-run.py p2p-frame integration`：passed，workspace tests
  - `cargo test -p p2p-frame --features x509 --no-run`：passed
  - `rg -n "TcpTunnelListenerInner|QuicTunnelListenerInner|Arc<TcpTunnelListener|Weak<TcpTunnelListener|Arc<QuicTunnelListener|Weak<QuicTunnelListener|Box<Arc|impl TunnelListener for Arc|Arc<PnListener>|listeners\(\)\[0\]|listeners\(\)\." p2p-frame/src`：no matches

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| none | none | acceptance | docs + implementation + tests | 未发现阻塞问题 | no |

## 一致性摘要
- Proposal 与 design：一致。proposal 要求 `accept_tunnel(&mut self)` 并移除 `TunnelListener` 的 `Sync` / `'static` supertrait；design 给出 trait、handle 和调用点迁移方案。
- Design 与 implementation：一致。实现将 `TunnelListener` 收窄为 `Send`，`accept_tunnel` 改为 `&mut self`，将 `TunnelListenerRef` 定义为 `Box<dyn TunnelListener>`，并移除 `TunnelNetwork::listeners()` accept handle 入口；调用方持有唯一可变 listener handle 执行 accept。TCP、QUIC、PN 和 fake test network 均不再通过 `impl TunnelListener for Arc<...>` 或 `Box<Arc<...>>` 适配 listener；TCP/QUIC listener 本体也不再包含 `Arc<...ListenerInner>`，由 `Box<dyn TunnelListener>` 唯一持有。
- Testing 文档与 testplan：一致。`tunnel_listener_mut_accept` 已映射到 unit 与 integration。
- Testplan 与实际执行：一致。unit 和 integration 入口均已执行并通过。
- 验收标准可追溯性：`P-TUNNEL-LISTENER-MUT-1` -> `tunnel_listener_mut_accept` -> `V-TUNNEL-LISTENER-MUT-UNIT` / `V-TUNNEL-LISTENER-MUT-INTEGRATION` -> 测试执行结果。

## 结论
- 通过或失败：通过
- 原因：文档链、实现和验证结果均覆盖 `TunnelListener` 可变 accept 接口、trait bound 收窄和唯一 listener handle；未发现阻塞不一致。

## 退回路由
- Proposal 任务：无
- Design 任务：无
- Testing 任务：无
- Implementation 任务：无

## 残余风险
- QUIC network 内部保留 `QuicListenerControl` 传输控制元数据用于主动拨号、close、listener info 和 UDP punch 辅助；该控制对象不是 listener，不能调用 `accept_tunnel` 或重新发布 accept handle。TCP network 仅保留 listener info 元数据，不再保留 TCP listener 或其 inner 的强/弱引用。
- 现有工作区已有其他未提交改动，本报告仅验收 `tunnel_listener_mut_accept`。
