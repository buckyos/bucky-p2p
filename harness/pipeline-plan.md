# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-05-13` 确认 reverse incoming 只接受命中本地 reverse waiter 的 tunnel；无 waiter 时直接关闭，并要求按自动 pipeline 规则处理
- 当前 `change_id`：`reverse_timeout_close_late_tunnel`

## 验收基线
- 最终验收以下列文档为准：
  - `docs/versions/v0.1/modules/p2p-frame/proposal.md`

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|------|--------|--------|------|----------|
| P-REV-TIMEOUT-1 | planning | 为 reverse incoming 无 waiter 关闭创建阶段图、依赖、输出和退回路由 | `p2p-frame` / `tunnel` | root | 用户确认需求并启动自动流水线 | `harness/pipeline-plan.md` | 本计划覆盖 proposal、design、testing、implementation、acceptance 任务 |
| PR-REV-TIMEOUT-1 | proposal | 把“reverse incoming 无 waiter 直接关闭”补充为已批准需求边界 | `proposal.md` | root | 用户确认语义变更 | proposal 制品 | proposal approved 且覆盖 `reverse_timeout_close_late_tunnel` |
| D-REV-TIMEOUT-1 | design | 把已批准 proposal 转成 reverse incoming waiter 命中判定和关闭路径设计 | `design.md`、`design/tunnel-publish-lifecycle.md`、必要长期边界文档 | root | PR-REV-TIMEOUT-1 approved | design 制品 | design approved 且覆盖 `reverse_timeout_close_late_tunnel` |
| T-REV-TIMEOUT-1 | testing | 把设计映射为 unit 与 integration 验证面 | `testing.md`、`testing/tunnel-publish-lifecycle.md`、`testplan.yaml` | root | D-REV-TIMEOUT-1 approved | testing 制品 | testing approved，且验证入口覆盖 reverse incoming 无 waiter close |
| I-REV-TIMEOUT-1 | implementation | 在已批准 proposal/design/testing 边界内实现关闭无 waiter reverse tunnel 和测试 | `p2p-frame/src/tunnel/tunnel_manager.rs`、必要测试 | root | proposal/design/testing 均 approved，implementation admission 通过 | code + tests | 实现完成并提供 testing 要求的验证证据 |
| A-REV-TIMEOUT-1 | acceptance | 审计 proposal、design、testing、implementation 和验证证据是否一致 | `p2p-frame` reverse timeout 交付 | root | implementation 证据已就绪 | acceptance report | acceptance 通过或明确退回责任阶段 |

## 子模块任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|--------|--------|--------|------|----------|
| D-REV-TIMEOUT-1.1 | design | 定义 reverse incoming 必须命中同 `(remote_id, tunnel_id)` waiter 的边界 | `tunnel` | D-REV-TIMEOUT-1 | proposal approved | `design.md`、`design/tunnel-publish-lifecycle.md` | 无 waiter 时关闭，不 register、不 publish |
| D-REV-TIMEOUT-1.2 | design | 定义命中 waiter 的 reverse incoming 仍延后 publish 的路径 | `tunnel` | D-REV-TIMEOUT-1 | proposal approved | `design.md`、`design/tunnel-publish-lifecycle.md` | waiter 收到 tunnel 后由 reverse open 完成路径 publish |
| T-REV-TIMEOUT-1.1 | testing | 补充 reverse incoming 无 waiter close 的 unit 验证 | `tunnel` unit | T-REV-TIMEOUT-1 | D-REV-TIMEOUT-1 approved | `testing.md`、`testing/tunnel-publish-lifecycle.md`、`testplan.yaml` | 已声明关闭、未注册、未发布和命中 waiter 正常交付的断言 |
| I-REV-TIMEOUT-1.1 | implementation | 简化 incoming reverse 分支为 take waiter 或 close | `p2p-frame/src/tunnel/tunnel_manager.rs` | I-REV-TIMEOUT-1 | T-REV-TIMEOUT-1 approved | code + unit tests | 无 waiter reverse 被关闭且不会进入候选表 |

## 退回规则
- 如果 proposal 无法支撑“reverse incoming 无 waiter close”：
  - 退回 proposal 澄清任务
- 如果 waiter 命中、关闭路径或 publish 时机不明确：
  - 退回 design 任务
- 如果缺少关闭、不发布、不注册或命中 waiter 正常交付的验证覆盖：
  - 退回 testing 任务
- 如果 implementation admission 未通过，或实现需要改变 `Tunnel` / `TunnelNetwork` trait、SN 协议或非 reverse incoming 语义：
  - 退回对应前置阶段
- 如果验收发现证据链不一致：
  - 按问题归属退回 proposal、design、testing 或 implementation

## 退出条件
- [x] 所有阻塞问题已关闭
- [x] proposal、design、testing 均为 approved
- [x] reverse incoming 无同 `(remote_id, tunnel_id)` waiter 时会被关闭
- [x] 无 waiter reverse tunnel 不进入 `TunnelManager` 候选表，不 publish，不唤醒 tunnel 订阅者
- [x] 命中 waiter 的 reverse tunnel 仍按延后 publish 规则交付
- [x] 必需 unit/integration 证据存在
- [x] 已基于 `proposal.md` 通过最终验收
