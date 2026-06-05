# 模块验收报告

## 对象与范围
- 模块：`p2p-frame`
- 版本：`v0.1`
- 评审日期：`2026-05-13`
- 范围内：`reverse_timeout_close_late_tunnel`
- 范围外：direct/proxy/普通 incoming publish 规则、SN 协议、公共 `Tunnel` / `TunnelNetwork` trait

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/design/tunnel-publish-lifecycle.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-publish-lifecycle.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `p2p-frame/src/tunnel/tunnel_manager.rs`
- 测试结果：
  - `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`
  - `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id reverse_timeout_close_late_tunnel`
  - `python3 ./harness/scripts/test-run.py p2p-frame unit`
  - `python3 ./harness/scripts/test-run.py p2p-frame integration`
  - `git diff --check`

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| none | none | acceptance | 审查 proposal/design/testing/testplan 与实现 diff | 未发现阻塞问题 | no |

## 一致性摘要
- Proposal 与 design：一致。proposal 要求 incoming reverse 无同 `(remote_id, tunnel_id)` waiter 时关闭，design 映射为 waiter 命中判定和 incoming close 分支。
- Design 与模块边界文档：一致。长期模块文档已同步 `src/tunnel/**` 的 reverse timeout close 边界。
- Design 与 implementation：一致。实现对 incoming reverse 先 `take_reverse_waiter(remote_id, tunnel_id)`；命中 waiter 时只 notify、不 register/publish，无 waiter 时 close；waiter 接收方随后 register/publish。
- Testing 文档与 testplan：一致。unit 覆盖无 waiter close、不 publish、不进入 `get_tunnel()`，覆盖命中 waiter 时 incoming 不注册，并覆盖 waiter 接收方 register/publish；integration 覆盖 workspace 兼容。
- Testplan 与实际执行：一致。unit 和 integration 入口均通过。
- 验收标准可追溯性：通过 `P-REV-TIMEOUT-1`、`reverse_timeout_close_late_tunnel`、`V-REV-TIMEOUT-UNIT` 和 `V-REV-TIMEOUT-INTEGRATION` 闭环。

## 结论
- 通过或失败：通过
- 原因：已满足 proposal 定义的 incoming reverse 无 waiter close、不发布、不注册，以及命中 waiter 只交付、接收方注册发布的语义；必需验证通过。

## 退回路由
- Proposal 任务：无
- Design 任务：无
- Testing 任务：无
- Implementation 任务：无

## 残余风险
- 当前实现不维护 reverse 过期表；是否接收 reverse incoming 仅由 pending waiter 决定。
- 若未来要覆盖“incoming 已匹配 waiter 但 reverse future 随后在接收方 register 前被取消”的 tunnel 关闭清理，需要单独补充 proposal/design/testing。
