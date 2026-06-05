# p2p-frame NetManager Async Incoming Callback 验收报告

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| F-001 | Low | governance | `stage-scope-check.py --stage proposal/design/testing` 均报告当前 worktree 中已有大量跨阶段修改与未跟踪文件 | 当前工作区在本轮开始前已有 unrelated dirty state，scope checker 无法只隔离本轮自动流水线阶段；审计未发现这些已有文件导致 `net_manager_async_incoming_tunnel_callback` 的文档、实现或测试证据不一致。 | 否 |

## 对象与范围
- 模块：`p2p-frame`
- 版本：`v0.1`
- change_id：`net_manager_async_incoming_tunnel_callback`
- 评审日期：`2026-05-30`
- 范围内：`NetManager` 的 `IncomingTunnelCallback` 返回 future；incoming publish 在 validator accept 后 await callback；await 前释放订阅表锁；迁移 `TunnelManager`、`TtpServer` 和测试 helper。
- 范围外：`TunnelListenerCallback`、listener `accept_tunnel(...)` trait、incoming validator trait、TCP/QUIC 线协议、TLS 身份校验、订阅 key、无订阅者 drop 行为、`TunnelManager` register/publish 生命周期。

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/design/pn-proxy-encryption.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `p2p-frame/src/networks/net_manager.rs`
- `p2p-frame/src/tunnel/tunnel_manager.rs`
- `p2p-frame/src/ttp/server.rs`
- `harness/pipeline-plan.md`

## 生成验收规则与预期结果
- Proposal 必须直接包含 `P-NET-CB-1` / `net_manager_async_incoming_tunnel_callback`。
- Design 必须映射同一 `change_id`，并明确 boxed future、`Send + 'static`、publish await、锁释放和调用点迁移。
- Testing 必须把该 change_id 映射到 unit 与 integration 入口。
- Implementation 必须让 `IncomingTunnelCallback` 返回 `BoxFuture<'static, ()>`，并在 `NetManager` 释放 `subscriptions` 锁后 await。
- Existing subscriber behavior must remain compatible: `TunnelManager` can perform async registration in the callback future; `TtpServer` may keep its spawn-based decoupling with a ready future.
- Tests must prove callback future await and lock-release behavior.

## 覆盖审计
- Proposal 覆盖：`proposal.md` 新增目标、范围内、范围外、约束、风险、验收锚点和 `P-NET-CB-1` proposal item。
- Design 覆盖：`design.md` 新增公共接口摘要、运行约束、直接映射和 `Directly Mapped Change Items`；`pn-proxy-encryption.md` 消除了旧的泛化表述冲突。
- Testing 覆盖：`testing.md` 和 `testplan.yaml` 增加 `V-NET-CB-UNIT`、`V-NET-CB-INTEGRATION` 和统一入口映射。
- Implementation 覆盖：`net_manager.rs` type alias 返回 `BoxFuture<'static, ()>`；`publish_tunnel(...)` async await callback；callback lookup 与 lock 释放在 await 前完成；listener dispatch 完成后 re-arm；`TunnelManager` callback 返回 boxed async future；`TtpServer` 返回 ready future 并保留内部 spawn。
- Test coverage：`dispatch_tunnel_waits_for_async_callback_completion` 覆盖 callback future 未完成时 dispatch 不完成；`dispatch_tunnel_releases_subscription_lock_before_awaiting_callback` 覆盖 await 前未持有订阅表锁。

## 一致性摘要
- Proposal 与 design：一致。Design 没有扩大 proposal 范围，明确不改变 listener accept、validator、wire protocol 或 register/publish 生命周期。
- Design 与 implementation：一致。实现使用 `BoxFuture<'static, ()>`，await callback，释放锁后 await，并保留 TTP server 解耦策略。
- Testing 文档与 testplan：一致。两者都包含 `net_manager_async_incoming_tunnel_callback` 的 unit/integration 映射。
- Testplan 与实际执行：一致。unit、integration、all-all 入口均通过统一 harness 执行。
- 逻辑审计：未发现死锁、订阅锁跨 await、无订阅者路径误触发 callback、validator reject/error 触发 callback 或下游 trait 漂移问题。

## 命令证据
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`：passed
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id net_manager_async_incoming_tunnel_callback`：passed
- `cargo check -p p2p-frame`：passed
- `python3 ./harness/scripts/test-run.py p2p-frame unit`：passed，`102 passed; 0 failed`
- `python3 ./harness/scripts/test-run.py p2p-frame integration`：passed，workspace tests/doc-tests passed
- `python3 ./harness/scripts/test-run.py all all`：passed
- `./test-run.sh`：passed after rerun with filesystem escalation for uv cache access; first sandboxed attempt failed creating `/root/.cache/uv` temp file on read-only filesystem.
- `git diff --check`：passed
- `stage-scope-check.py --stage proposal/design/testing`：failed due pre-existing cross-stage dirty worktree entries; see F-001.

## 结论
- 通过或失败：通过
- 原因：已批准 proposal/design/testing 直接覆盖 `net_manager_async_incoming_tunnel_callback`；实现满足异步 callback、await 和锁释放约束；unit/integration/all-all 证据通过；未发现行为或文档一致性阻塞问题。

## 退回路由
- Proposal 任务：无需退回
- Design 任务：无需退回
- Testing 任务：无需退回
- Implementation 任务：无需退回

## 残余风险
- 当前 worktree 存在大量本轮外的已修改和未跟踪文件，会持续影响 stage-scope 机械检查的可解释性。
- `IncomingTunnelCallback` future 长时间不返回会延迟当前 listener re-arm；这是 proposal/design 已记录的时序风险，当前实现按“await subscriber future 完成”要求执行。
