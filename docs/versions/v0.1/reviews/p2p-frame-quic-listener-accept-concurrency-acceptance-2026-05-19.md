# 模块验收报告

## 对象与范围
- 模块：`p2p-frame`
- 版本：`v0.1`
- 评审日期：`2026-05-19`
- 范围内：`quic_listener_accept_concurrency`，包括 QUIC listener incoming 接收并发化、有界 incoming 队列、可配置并发上限、关闭后返回防护、文档映射和验证证据
- 范围外：QUIC tunnel 命令协议、`TunnelListener` / `TunnelNetwork` trait、UDP punch 策略、heartbeat 策略、reverse tunnel publish 规则

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `harness/pipeline-plan.md`
- `p2p-frame/src/networks/quic/listener.rs`
- `p2p-frame/src/networks/quic/network.rs`
- `p2p-frame/src/stack.rs`
- 实际验证结果：
  - `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`：通过
  - `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id quic_listener_accept_concurrency`：通过
  - `cargo check -p p2p-frame`：通过
  - `python3 ./harness/scripts/test-run.py p2p-frame unit`：通过，101 tests passed
  - `python3 ./harness/scripts/test-run.py p2p-frame integration`：通过，执行 `cargo test --workspace`

## 评审顺序
1. 审查已批准 proposal 的基线。
2. 审查 design 与边界同步。
3. 审查 testing 策略与机器可读测试计划。
4. 审查实现与测试代码。
5. 审查实际结果。
6. 记录一致性与路由结论。

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| 无 | none | acceptance | 文档映射、实现 diff、验证命令均已审查 | 未发现阻塞问题 | no |

## 一致性摘要
- Proposal 与 design：一致。proposal 要求 QUIC listener 不因单个慢 incoming 阻塞后续 incoming，要求 incoming queue 默认 1024、并发上限默认 256、两者可外部配置，并要求关闭后不返回新 tunnel；design 将其映射到 `listener.rs` 的有界 incoming 队列、`accept_tunnel()` 直接处理路径、`network.rs` / `stack.rs` 配置入口和关闭防护。
- Design 与模块边界文档：一致。本次只改 `networks/quic` 内部调度和专有配置入口，不改变公开 `TunnelListener` / `TunnelNetwork` trait、QUIC tunnel 命令协议或 NAT punch/heartbeat 语义。
- Design 与 implementation：一致。实现新增 `QuicListenerConfig`，默认 `accept_concurrency_limit = 256`、`incoming_queue_len = 1024`；主 accept loop 不再执行 `accept_connection(conn).await`，而是把 `Incoming` 写入有界队列；`accept_tunnel()` 获取 permit 后消费 incoming，并直接完成握手、证书处理、cert cache 写入和 `QuicTunnel::accept(...)`，不再为每个 incoming 新建处理任务。
- Testing 文档与 testplan：一致。`testing.md` 的 `V-QUIC-ACCEPT-*` 映射已接入 `testplan.yaml` 的 unit、dv disabled 和 integration 层级。
- Testplan 与实际执行：一致。unit 入口和 integration 入口均已执行并通过；DV 明确 disabled 且有原因。
- 验收标准可追溯性：可追溯到 `P-QUIC-ACCEPT-1` / `quic_listener_accept_concurrency`。

## 结论
- 通过或失败：通过。
- 原因：proposal、design、testing、testplan 和 implementation 对 `quic_listener_accept_concurrency` 保持一致；准入检查通过；代码实现满足有界 incoming queue、可配置默认值、`accept_tunnel()` 直接处理、受限并发和关闭后返回防护；unit 与 integration 验证通过。

## 退回路由
- Proposal 任务：无需退回。
- Design 任务：无需退回。
- Testing 任务：无需退回。
- Implementation 任务：无需退回。

## 残余风险
- 当前自动测试主要验证现有多连接 QUIC 行为和工作区兼容性；没有专门构造一个长期挂起的 QUIC/TLS slow handshake 来测量 head-of-line blocking 消除效果。
- 当前自动测试覆盖配置入口和现有多连接回归，但没有专门压测 incoming queue 满载时的延迟曲线；该容量策略仍需在真实负载中观察。
