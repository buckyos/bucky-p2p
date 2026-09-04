---
module: p2p-frame
task_name: 002-quic-listener-close-connect-race
submodule: 002-quic-listener-close-connect-race
version: v0.1
status: approved
approved_by: user
approved_at: 2026-07-12T18:33:51+08:00
approved_content_sha256: 437424750e96ea148ce8e5173cdbeb1b026a88c5d1a8f31e1c9338eb14f93403
---

# QUIC Listener Close/Connect Race Proposal

## Background and Goal

`QuicListener` 关闭时会清空 worker endpoints；与此同时，主动建链路径可能在持锁前后继续选择 endpoint。当前选择逻辑既包含对首个 endpoint 的直接 `unwrap()`，也包含以 `endpoints.len()` 为上界的随机索引。当关闭已经把集合清空时，这两条路径都可能 panic。

目标是让 listener 关闭与主动 QUIC 建链并发发生时以可观察的错误正常结束，而不是因空 endpoint 集合触发 panic，并保持 listener 正常运行期间既有 worker endpoint 选择行为。

## Scope

### In scope

- 主动建链读取 listener worker 状态时，在同一次加锁形成的原子快照内检查 listener 是否已关闭以及 endpoint 集合是否为空。
- 已关闭的 listener 拒绝新的主动建链，并返回 `Interrupted` 或项目中与关闭状态等价的错误。
- 未标记关闭但 endpoint 集合为空时返回 `ErrorState` 或项目中与非法内部状态等价的错误。
- 移除主动建链路径对非空 endpoint 集合的隐式假设，禁止空集合上的 `first().unwrap()` 和 `random_range(0..0)`。
- 保持关闭时 worker endpoint 清理、正常状态下 endpoint 选择策略以及 QUIC 建链接口的既有边界。
- 后续 design 和 post-implementation testing 必须覆盖关闭/建链竞态及空 endpoint 防御分支。

### Out of scope

- 改变 QUIC wire protocol、TLS/身份校验、CID 路由或 worker socket 分发模型。
- 改变 listener 关闭流程的资源回收语义，或让关闭后的 listener 自动重启 worker。
- 修改 TCP、PN、SN 或上层 tunnel candidate 选择策略。
- 在本 proposal 阶段修改生产代码、测试代码、design 或 testing artifact。

### Boundary with neighboring modules

- 修复边界位于 `p2p-frame/src/networks/quic/listener.rs` 的 listener worker 生命周期与主动 endpoint 选择逻辑。
- 调用方继续通过现有 QUIC connect/tunnel 接口观察错误；不新增公共 API，也不要求 `cyfs-p2p` 或其他下游适配错误恢复协议。
- 错误类型复用 `p2p-frame` 现有 `Interrupted` / `ErrorState` 语义，不引入新的跨 crate 错误契约。

## Requirement Review

- 请求合理且必要：关闭状态和 worker endpoint 集合属于同一生命周期状态，必须在同一锁保护下联合判断，否则分离检查仍可能留下检查后清空的竞态窗口。
- 对已关闭与异常空集合进行区分有助于保留诊断价值：正常关闭优先报告 `Interrupted`，未关闭却没有 endpoint 属于内部状态错误，报告 `ErrorState`。若现有错误构造 API 只能提供语义等价变体，design 必须明确映射。
- 正常运行时仍需保留现有选择意图：需要固定 endpoint 的分支选择首个 worker，需要负载分散的分支从非空集合随机选择；修复不应退化为始终固定到一个 worker。
- 单纯把 `unwrap()` 换成默认值或为随机上界钳制到 1 会掩盖无 endpoint 状态且可能制造越界/错误路由，因此不采用。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-QLCCR-1 | quic_listener_close_connect_race | 主动建链必须在同一 worker-state 锁内联合检查 closed 与 endpoints 非空后再选择 endpoint；closed 返回 Interrupted，未关闭但为空返回 ErrorState，任何关闭/建链交错均不得 panic | 仅修改 QUIC listener 内部生命周期读取和 endpoint 选择，不改公共接口、正常选择策略或关闭资源回收 | 锁内增加常量时间状态检查；错误路径会更早拒绝建链 | 后续测试可控地覆盖 closed+empty、open+empty、正常首项选择、正常随机选择以及关闭与并发 connect 压测/交错，证明返回预期错误且无 panic | 不重构 worker 管理模型，不改变 wire/TLS/CID 行为，不为关闭后的 listener 提供重启 |

## Success Criteria

- Concrete user-visible or system-visible result: QUIC listener 关闭与主动建链并发时不再因空 worker endpoint 集合 panic；建链调用以 `Interrupted`/关闭等价错误结束，异常的 open+empty 状态以 `ErrorState`/状态错误结束。
- Required evidence: 后续 design 明确锁保护状态、检查顺序、错误映射、真实 Scope Paths 与 endpoint 选择分支；post-implementation testing 覆盖确定性的空集合分支、正常非空选择和并发关闭回归，并通过 `p2p-frame` 规范测试入口。
- Explicit non-goals: 不改变公共 API、QUIC 协议、TLS/身份校验、worker 数量/创建策略、关闭资源回收或相邻模块行为。

## Risks

- 如果 closed 标志与 endpoints 不受同一把锁保护，仅把两次读取写在相邻代码中仍不能建立一致快照；design 必须核对实际字段所有权和锁边界。
- 错误优先级需要稳定：`closed && empty` 应报告关闭语义，而 `!closed && empty` 才报告内部状态错误，否则正常 shutdown 会被误诊为损坏状态。
- 并发测试若只依赖概率调度可能产生假通过；testing 阶段应优先设计可控状态或同步点，并用压力交错作为补充。
- endpoint guard、引用或 clone 的持有范围若扩大，可能延迟关闭资源回收；design 应选择最小且足以保证所选 endpoint 有效的锁/所有权范围。

## Approval Record

- approver: user
- approval_date: 2026-07-12
- user_statement: "批准，自动处理后续步骤"
