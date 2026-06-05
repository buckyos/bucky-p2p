# 模块验收报告

## 对象与范围
- 模块：p2p-frame
- 版本：v0.1
- 评审日期：2026-05-29
- 范围内：sfo-reuseport QUIC listener 新 `serve_socket` 接口适配；每 worker socket 一个 Quinn Endpoint；worker id 写入 DCID；accept loop 进入 socket 回调；UDP punch 保留首个 worker socket。
- 范围外：TCP reuseport 行为、非 QUIC NAT 策略、PN/SN 业务逻辑。

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testing/sfo-reuseport-listeners.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `p2p-frame/src/networks/quic/listener.rs`
- `p2p-frame/Cargo.toml`
- `Cargo.lock`
- 实际执行结果：schema/admission、`cargo check -p p2p-frame`、`cargo test -p p2p-frame udp_punch_tests`、`python3 ./harness/scripts/test-run.py p2p-frame unit`

## 评审顺序
1. 审查已批准 proposal 的基线。
2. 审查 design 与长期模块边界同步。
3. 审查 testing 策略与机器可读测试计划。
4. 审查实现与测试代码。
5. 审查实际结果。
6. 记录一致性与路由结论。

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| 无 | - | - | 已执行检查全部通过 | 未发现阻塞问题 | 否 |

## 一致性摘要
- Proposal 与 design：一致，均要求改用 `QuicServer::serve_socket`，每个 worker socket 创建独立 Quinn Endpoint，并使用 worker shard CID 生成器保持连接路由稳定。
- Design 与模块边界文档：一致，`docs/modules/p2p-frame.md` 已同步 QUIC listener 采用 sfo-reuseport worker socket 与 worker-shard DCID。
- Design 与 implementation：一致，`QuicTunnelListener::bind` 通过 `serve_socket` 注册回调，回调内创建 endpoint 与 accept loop；`close` 关闭所有 endpoint 与 `QuicServer`；UDP punch 使用首个注册的 `SfoUdpSocket`。
- Testing 文档与 testplan：一致，测试文档和 `testplan.yaml` 覆盖新 socket 模型、CID route、close 和 UDP punch 首 socket 行为。
- Testplan 与实际执行：已执行 p2p-frame unit 入口并通过；额外执行目标测试和编译检查。
- 验收标准可追溯性：用户提出的四项约束均映射到 proposal/design/testing/implementation。

## 结论
- 通过或失败：通过。
- 原因：实现、文档和测试结果与已批准范围一致，未发现阻塞缺口。

## 退回路由
- Proposal 任务：无需退回。
- Design 任务：无需退回。
- Testing 任务：无需退回。
- Implementation 任务：无需退回。

## 残余风险
- 当前 active connect 在已注册 endpoint 中随机选择，但未承诺长期稳定的负载均衡策略。
- 本轮验证覆盖 unit 与编译检查，未执行跨进程/native eBPF reuseport 环境下的端到端 QUIC 路由压力测试。
