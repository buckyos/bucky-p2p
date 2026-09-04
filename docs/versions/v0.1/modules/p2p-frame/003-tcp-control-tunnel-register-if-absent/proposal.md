---
module: p2p-frame
task_name: 003-tcp-control-tunnel-register-if-absent
submodule: 003-tcp-control-tunnel-register-if-absent
version: v0.1
status: approved
approved_by: user
approved_at: 2026-07-13T01:12:31+08:00
approved_content_sha256: 1cd4fabc12a6edc4b44d53106a4ace40ba924c98fd3c65462db185eea72b4120
---

# TCP Control Tunnel Post-Accept Registry Commit Proposal

## Background and Goal

`TcpTunnelRegistry` 当前在接收入站 TCP control connection 时，使用 `(local_id, remote_id, tunnel_id, candidate_id)` 作为 key 无条件写入新 tunnel。若同 key 的旧 tunnel 已经存活并被 `TunnelManager` 接受，新 control tunnel 会先覆盖 registry，随后才可能被 `TunnelManager` 判定为重复并关闭。此时 registry 中留下的是已关闭的新 tunnel；旧 tunnel 的后续 data connection 查找该 key 时会得到 `TunnelNotFound`，破坏仍然有效的原 tunnel。

目标是把 registry 更新推迟到上层完成 validator 与 `TunnelManager` 接受之后：上层明确接受新 tunnel 时，registry 才把同 key 映射提交为该 tunnel；上层拒绝、关闭或处理失败时，registry 保持原映射不变。这样 registry winner 与上层实际接受的 winner 由同一次接受结果决定。

## Scope

### In scope

- TCP listener 创建入站 control tunnel 后，先异步交给既有上层 validator、subscriber 与 `TunnelManager` 处理，不再预先写入 registry。
- 上层处理路径必须向 TCP listener 返回与 subscriber 存活状态相互独立的明确接受结果；只有“该 tunnel 已被接受并成为有效 candidate”才能触发 registry commit。
- 接受成功后的 commit 在 registry 互斥区内把同 key 映射替换为被接受的准确 `TcpTunnel`；拒绝、duplicate、validator error、无 subscriber、subscriber 已关闭或其他上层失败均不得替换/移除旧映射。
- 接受等待期间，同 key 的 data connection lookup 继续使用提交前的旧映射；若没有旧映射，继续返回既有 `TunnelNotFound`，不把尚未接受的新 tunnel 暴露为可路由对象。
- 保持 duplicate 新 tunnel 被 `TunnelManager` 关闭后，原有已接受 tunnel 的 data connection lookup 可达。
- 后续 design 明确内部回执类型、callback/dispatch 传播、reverse tunnel 处理、commit 时序与锁边界；post-implementation testing 覆盖接受、拒绝、duplicate 及随后 data connection 的回归场景。

### Out of scope

- 改变 TCP control/data connection wire frame、TLS 身份校验、tunnel key 字段或 data connection hello 格式。
- 改变 `TunnelManager` 的重复 candidate 判定、candidate winner 规则、publish 语义或关闭策略。
- 由 registry 自行判断 winner、执行 register-if-absent，或依据连接新旧程度抢占上层未接受的 candidate。
- 重构 TCP tunnel registry 为新的并发容器，或改变其定期 stale cleanup 周期。
- 改变 QUIC、PN 等其他 tunnel listener 的 registry 或 publish 行为；若共享 callback 契约必须调整，design 需限制兼容影响并保持其他协议行为不变。
- 在本 proposal 阶段修改生产代码、测试代码、design 或 testing artifact。

### Boundary with neighboring modules

- `p2p-frame/src/networks/tcp/listener.rs` 继续拥有 TCP tunnel registry、control/data connection 分流及 registry lookup，并在收到上层接受回执后执行 commit。
- `p2p-frame/src/networks/net_manager.rs` 负责把 validator/subscriber 的真实处理结果传播回 listener；subscriber 存活与 tunnel 接受是两个不同结果，不得继续用无条件成功或单一布尔值混淆。
- `p2p-frame/src/tunnel/tunnel_manager.rs` 继续决定新 tunnel 是否为重复 candidate；registry 不提前发布 tunnel，也不替代上层 candidate 管理。
- `TcpTunnel` 生命周期、公共 `Tunnel` trait、TCP wire 与 TLS 身份边界保持不变。

## Requirement Review

- 请求合理且属于数据路由正确性修复。registry 在上层接受之前无条件替换，会让两个注册层的时序产生悬空映射，必须消除。
- 按用户选择采用“上层接受成功后再替换”，不再采用 register-if-absent。上层已经拥有 validator、duplicate candidate 判定、reverse waiter 与 publish 决策，因此由它产生 winner 回执可避免 registry 重复实现另一套 winner/stale 规则。
- 当前 `IncomingTunnelCallback` 返回 `()`，`NetManager` 的 subscriber `bool` 只表达订阅是否继续存活，而 `TunnelManager` duplicate 失败会被记录后仍返回 subscriber 存活。后续 design 必须新增或收紧内部回执，使 listener 能区分“上层已接受该 tunnel”“已拒绝/处理失败”和“subscriber 生命周期状态”。
- registry commit 只能发生在上层接受 future 完成之后，且必须写入回执所对应的同一个 tunnel；不得先写后回滚，因为并发 data connection 可在回滚前观察到错误映射。
- 上层等待可能延长新 tunnel 可接收 data connection 的时间窗口。选择在接受前不暴露新 tunnel，优先保证 registry 不路由到未接受/即将关闭的对象；具体 ready/dispatch 时序由 design 审计，不能改变 wire handshake。
- 本修复不改变 duplicate tunnel 最终由谁关闭；它使 registry 只提交上层确认的 winner。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-TCTPAC-1 | tcp_control_tunnel_commit_after_accept | 入站 TCP control tunnel 只有在 validator/subscriber/`TunnelManager` 上层处理明确接受成功后，才原子提交为同 key registry 映射；所有拒绝、duplicate、关闭或失败结果保持旧映射不变 | 调整 TCP listener 到 NetManager/TunnelManager 的内部接受回执与 commit 时序；不改 TCP wire/TLS、key、公共 `Tunnel` trait 或上层 winner/publish 规则 | 接受结果必须跨异步 callback 返回，增加内部接口与时序复杂度；换取 registry 只指向上层真实接受的 candidate，并消除预注册可见窗口 | unit/contract 证据覆盖接受后才 commit、等待期间不暴露新 tunnel、validator/subscriber/duplicate/error 不替换；TCP 回归证明 duplicate 新 tunnel 关闭后旧 tunnel 的同 key data connection 仍关联成功且不返回错误覆盖导致的 `TunnelNotFound` | 不实现 register-if-absent，不由 registry 选择 winner，不改变其他协议行为、TCP wire、TLS 或 `TunnelManager` duplicate/publish 语义 |

## Success Criteria

- Concrete user-visible or system-visible result: 新 TCP control tunnel 在上层接受前不改变 registry；上层接受成功后 registry 指向该 accepted tunnel；同 key duplicate 被拒绝并关闭后 registry 仍指向原有 accepted tunnel，后续 data connection 不返回由错误覆盖导致的 `TunnelNotFound`。
- Required evidence: 后续 approved design 定义接受回执契约、validator/subscriber/`TunnelManager` 分支、reverse 语义、commit 原子性、失败状态、具体 Scope Paths 与日志语义；post-implementation testing 覆盖 accepted commit、pending lookup、validator reject、无/关闭 subscriber、duplicate rejection、upper error 和 control-then-data 回归，并通过 `p2p-frame` 规范 unit、DV/integration 中适用的 TCP 验证入口。
- Explicit non-goals: 不实现 register-if-absent，不改变 TCP wire、TLS、tunnel key、公共 `Tunnel` trait、上层 duplicate winner、publish/close 语义或 registry cleanup 周期。

## Risks

- 若把 subscriber “继续存活”的 `bool` 当作 tunnel 接受结果，duplicate 仍可能被错误 commit；回执必须表达 tunnel 级处理结果。
- 若 callback 仍是 fire-and-forget 或在返回前吞掉 `TunnelManager` 错误，listener 无法安全 commit；错误传播与日志责任必须在 design 中明确。
- 若上层接受完成与 tunnel close 并发，registry 可能刚提交即指向关闭对象；design 必须定义 commit 前后的关闭检查、cleanup 责任和可接受竞态结果。
- registry 锁内不得等待上层 callback、网络 IO 或异步操作；只在接受完成后以短临界区执行精确替换。
- reverse tunnel 无 waiter 时当前上层会关闭并返回正常处理，不能被误分类为 accepted registry candidate。
- 共享 `IncomingTunnelCallback` 的 QUIC 等路径可能受内部回执签名影响；design 与测试必须证明其 publish 行为未被改变。

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/src/networks/network.rs` 的 `IncomingTunnelCallback` 当前输出 `()`，`p2p-frame/src/networks/net_manager.rs` 的 subscriber 输出 `bool`；接受回执会收紧内部 callback/dispatch 契约，但 TCP wire 与公共 `Tunnel` trait 保持不变 | design 列出 callback、validator、subscriber、`TunnelManager` 的正负结果映射与其他 listener 兼容影响；testing 覆盖 accepted/rejected/error contract | proposal 阶段已检查当前接口与调用链 | owner: design/testing；reason: 接口形态尚待 design；acceptance impact: 未证明调用方兼容前不得验收 | 回执语义不清会把 subscriber liveness 误当作 tunnel acceptance |
| data/schema | no | `p2p-frame/src/networks/tcp/listener.rs` 的内存 `HashMap` 不持久化，且不修改 codec、key 字段、文件格式或迁移数据 | source diff review 确认无持久化/schema 变更 | proposal 阶段已确认范围 | owner: none；reason: not applicable；acceptance impact: none | none |
| security/privacy/permission | yes | `p2p-frame/src/networks/net_manager.rs` 的 `incoming_tunnel_validator` 位于 commit 之前，错误回执若绕过 validator 会把未授权 tunnel 暴露给 data lookup | design 保证 validator reject/error 均为 not accepted；testing 至少覆盖 validator reject 不 commit 的负向路径并检查日志不泄露敏感信息 | proposal 阶段已确认 validator 调用位置 | owner: design/testing；reason: 负向实现证据尚未生成；acceptance impact: reject 可写 registry 时阻塞验收 | fail-open 回执可能绕过 incoming tunnel trust boundary |
| runtime/integration | yes | `p2p-frame/src/networks/tcp/listener.rs` 在 control/data 并发路径中访问 registry，`p2p-frame/src/tunnel/tunnel_manager.rs` 异步执行 duplicate/reverse/publish 决策 | design 描述 pending/accept/reject/close 状态、并发与失败行为；unit 加 DV/integration 覆盖 duplicate control 后 data lookup、pending lookup 和 close race | proposal 阶段已检查现有预注册时序 | owner: design/testing；reason: 实现后生成并发证据；acceptance impact: 缺少回归证据不得验收 | 异步等待和 close 竞态可能造成短暂 NotFound 或 stale 映射 |
| build/dependency/config/deployment | no | 请求不修改 Cargo、feature、依赖、配置、部署或生成资源 | source diff review 确认无相关路径 | proposal 阶段已确认范围 | owner: none；reason: not applicable；acceptance impact: none | none |
| ui/datamodel/workflow | no | `p2p-frame` TCP tunnel 内部路径无 UI、展示模型或用户交互工作流 | 确认无 UI surface | proposal 阶段已确认范围 | owner: none；reason: not applicable；acceptance impact: none | none |
| harness/process | no | 本任务只使用既有 proposal/design/admission/testing/acceptance 规则，不修改 `harness/**`、schema、CI 或统一测试入口 | 运行既有 doc-structure、stage-scope、后续 admission/testing/acceptance checks | proposal 阶段运行 doc-structure 与 stage-scope | owner: downstream stages；reason: 后续检查须由对应阶段运行；acceptance impact: 缺少既有门禁证据不得验收 | none |

## Approval Record

- approver: user
- approval_date: 2026-07-13
- user_statement: "批准，自动处理后续步骤"
