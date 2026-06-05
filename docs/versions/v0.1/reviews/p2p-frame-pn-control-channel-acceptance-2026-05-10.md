# 模块验收报告

## 对象与范围
- 模块：p2p-frame
- 版本：v0.1
- 评审日期：2026-05-10
- 范围内：PnTunnel 控制通道建链、控制通道 ready gate、PN relay 控制流转发、远端 Close/EOF/控制通道错误/心跳超时触发本端关闭、业务 stream/datagram 与已有 PN proxy 场景兼容。
- 范围外：修改 TCP/QUIC tunnel 控制通道协议、改变 SN/NAT 策略、改变上层 stream/datagram 公开接口、生产环境公网 NAT 成功率验证。

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/design/pn-tunnel-control-channel.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-control-channel.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `p2p-frame/src/pn/protocol.rs`
- `p2p-frame/src/pn/client/pn_client.rs`
- `p2p-frame/src/pn/client/pn_listener.rs`
- `p2p-frame/src/pn/client/pn_tunnel.rs`
- `p2p-frame/src/pn/service/pn_server.rs`
- `p2p-frame/src/stack.rs`
- 实际验证结果

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
| 无 | - | - | unit、integration、模块包结构检查、准入检查与 DV 窗口验证均未发现阻塞问题 | - | 否 |

## 一致性摘要
- Proposal 与 design：一致。已批准 proposal 要求 PnTunnel 在打开时建立控制通道，并参考 TCP/QUIC tunnel 的控制通道语义；design 将其拆分为 open handshake、relay 转发、远端关闭检测与心跳检测。
- Design 与模块边界文档：一致。长期模块文档已声明 PnTunnel 具备独立控制通道，业务 stream/datagram 仍沿用既有 PN proxy 通道。
- Design 与 implementation：一致。实现新增 PN control open/ping/pong/close 命令；active tunnel 在注册前完成 control ready gate；passive listener 先接受 control open 再登记 tunnel；relay 对 control stream 执行对端转发；PnTunnel 在 close、EOF、decode/error 与 heartbeat timeout 时收敛到本端关闭。
- Testing 文档与 testplan：一致。测试计划覆盖协议编解码、主动/被动控制通道安装、远端关闭、EOF、心跳超时、手动关闭发送 control close，以及 all-in-one 运行时兼容性。
- Testplan 与实际执行：基本一致。unit、integration、结构检查与准入检查完整通过；DV 入口为无限循环运行器，已在多轮成功和 PN control open/bridge 日志出现后手动终止，终止码 143 记录为人工停止而非用例失败。
- 验收标准可追溯性：满足。核心标准“远端关闭时本端能够通过控制通道感知并关闭”由 unit 精确覆盖；默认 PN proxy 建链路径通过 integration 与 DV 窗口验证保持兼容。

## 结论
- 通过或失败：通过
- 原因：实现与批准文档一致，新增控制通道语义有针对性单元测试，workspace integration 通过；DV 在无限循环入口中持续通过多轮并实际触发新增 PN control open/relay 路径，未观察到回归失败。

## 退回路由
- Proposal 任务：无需退回
- Design 任务：无需退回
- Testing 任务：无需退回
- Implementation 任务：无需退回

## 残余风险
- DV 入口当前不会自然退出，只能作为窗口验证证据；若需要可审计的自动 DV 结果，后续应为 `cyfs-p2p-test all-in-one` 增加轮次或时长上限参数。
- 心跳默认超时依赖控制流正常调度；极端 runtime stall 或 relay 长时间背压仍可能延迟本端发现远端失活。
