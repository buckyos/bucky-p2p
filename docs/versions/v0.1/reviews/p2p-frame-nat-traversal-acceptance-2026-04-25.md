# 模块验收报告

## 对象与范围
- 模块：p2p-frame
- 版本：v0.1
- 评审日期：2026-04-25
- 范围内：单 SN NAT 打洞优化；direct/reverse 短延迟竞速；SN call 新鲜反连候选；proxy 短窗口脱代理；endpoint 评分按协议隔离；proxy 复用场景下可恢复 accept 错误处理。
- 范围外：多 SN fanout、SN 选择策略、STUN/TURN、协议线格式扩展、跨模块重构。

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/design/tunnel-nat-traversal.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `p2p-frame/src/tunnel/tunnel_manager.rs`
- `p2p-frame/src/stream/stream_manager.rs`
- `p2p-frame/src/datagram/datagram_manager.rs`
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
| 无 | - | - | unit、x509、integration、DV 窗口验证均未发现阻塞问题 | - | 否 |

## 一致性摘要
- Proposal 与 design：一致。实现限制在单 SN NAT 打洞优化，不包含多 SN fanout。
- Design 与模块边界文档：一致。改动保持在 p2p-frame tunnel、stream/datagram accept loop 与测试文档范围内。
- Design 与 implementation：一致。QUIC/UDP NAT 候选使用短 reverse 延迟；SN call 携带本地 listener、SN observed WAN 与 mapped 候选；proxy 脱代理使用 15s/30s/60s/120s 短窗口后回到封顶退避；endpoint score 按协议隔离。
- Testing 文档与 testplan：一致。测试计划覆盖短延迟竞速、反连候选、proxy 短窗口、endpoint 评分隔离和 proxy 复用回归。
- Testplan 与实际执行：一致。`p2p-frame` unit、x509 feature、workspace integration 与 DV 窗口验证均已执行。
- 验收标准可追溯性：满足 proposal 中的单 SN、无多 SN fanout、proxy 不作为升级成功、失败退避有上限等标准。

## 结论
- 通过或失败：通过
- 原因：实现与批准文档一致，新增策略和回归修复均有测试或 DV 证据支撑；未发现阻塞项。

## 退回路由
- Proposal 任务：无需退回
- Design 任务：无需退回
- Testing 任务：无需退回
- Implementation 任务：无需退回

## 残余风险
- 真实 NAT 成功率仍依赖现场 NAT 类型、端口保持行为和 SN 观察端点新鲜度；当前 DV 是本地运行时场景，不等价于公网矩阵。
- proxy 脱代理短窗口从 5 分钟提前到 15 秒起步，会增加短时 direct/reverse 探测；现有退避和并发保护可控，但生产环境仍应观察 SN call 与拨号日志量。
