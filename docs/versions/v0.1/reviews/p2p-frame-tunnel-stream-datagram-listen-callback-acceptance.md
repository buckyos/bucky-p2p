# 模块验收报告

## 对象与范围
- 模块：p2p-frame
- 版本：v0.1
- 评审日期：2026-06-02
- 范围内：`tunnel_stream_datagram_listen_callback`
- 范围外：TCP/QUIC/PN 线协议、TLS 身份校验、PN proxy channel 协议、业务 payload 格式、vport/purpose 编解码、tunnel publish 规则

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/design/tunnel-channel-listen-callback.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/versions/v0.1/modules/p2p-frame/acceptance.md`
- `docs/modules/p2p-frame.md`
- 实现代码与测试代码
- 测试结果：`rustup run stable cargo test -p p2p-frame --no-run`；`python3 ./harness/scripts/test-run.py p2p-frame unit`

## 评审顺序
1. 审查已批准 proposal 的基线
2. 审查 design 与边界同步
3. 审查 testing 策略与机器可读测试计划
4. 审查实现与测试代码
5. 审查实际结果
6. 记录一致性与路由结论

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| A-TUNNEL-CHANNEL-001 | non-blocking | Testing | `testplan.yaml` integration step | 本轮未执行完整 workspace integration；直接行为已由 p2p-frame unit 和 test target 编译覆盖。 | no |

## 一致性摘要
- Proposal 与 design：一致。design 明确定义公共 `Tunnel` trait 移除 `accept_stream()` / `accept_datagram()`，由 `listen_stream(...)` / `listen_datagram(...)` callback 接收入口 channel。
- Design 与模块边界文档：一致。长期模块文档已记录 tunnel 入站 channel 回调模型。
- Design 与 implementation：一致。TCP/QUIC/PN tunnel 内部 dispatch loop 通过 callback 投递 stream/datagram；TTP、stream manager 和 datagram manager 调用点已迁移。
- Testing 文档与 testplan：一致。`tunnel_stream_datagram_listen_callback` 已映射到 unit 编译/行为证据，并保留 integration 兼容性入口。
- Testplan 与实际执行：unit 入口已执行通过；test target 编译已执行通过；workspace integration 未执行，记录为非阻塞残余。
- 验收标准可追溯性：满足 proposal/design/testing 的直接要求，未发现线协议、TLS、payload、vport/purpose 或 publish 规则变更证据。

## 结论
- 通过或失败：通过，带非阻塞残余。
- 原因：实现满足已批准 proposal/design 的公共 trait 和入站回调要求，`python3 ./harness/scripts/test-run.py p2p-frame unit` 通过 105 个单元测试，`rustup run stable cargo test -p p2p-frame --no-run` 通过 test target 编译。

## 退回路由
- Proposal 任务：无需退回
- Design 任务：无需退回
- Testing 任务：无需退回；后续可补跑 workspace integration 作为下游兼容性证据
- Implementation 任务：无需退回

## 残余风险
- 完整 workspace integration 未在本轮执行，下游 crate 若仍直接依赖公共 `Tunnel::accept_*` 会在后续 integration 中暴露。
- callback 由调用方提供，长时间阻塞的 callback 仍可能占用对应 dispatch task；当前 design 要求调用方不得阻塞 worker/control loop。
