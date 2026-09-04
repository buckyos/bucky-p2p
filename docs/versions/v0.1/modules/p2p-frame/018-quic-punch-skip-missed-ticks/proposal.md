---
module: p2p-frame
task_name: 018-quic-punch-skip-missed-ticks
submodule: 018-quic-punch-skip-missed-ticks
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# QUIC UDP Punch 跳过错过 Tick Proposal

## Workflow Tier Judgment

- Proposed tier: `high-risk`
- Final tier: `pending`
- Rationale: 修复范围局限于 `p2p-frame` QUIC punch 调度，但它直接改变异步运行时暂停/延迟后的公网 UDP 发送节奏。错误实现可能继续产生突发流量、意外降低 active/reverse 覆盖，或破坏 connect deadline 与 listener close 边界，因此触发 runtime/integration 与 capacity 风险，需要完整 design、implementation、post-implementation testing 和 acceptance 证据链。
- Confirmation statement: 待用户确认本 proposal 及 `high-risk` tier；确认前不修改生产代码或测试。

## Background and Goal

`017-quic-nat-traversal-improvement` 把 punch 生命周期延长到 QUIC connect deadline，并保留 active `250ms`、reverse `0ms` 的首包偏移与 `50ms` cadence。当前循环以 `next_offset.saturating_sub(elapsed)` 计算等待时间，发送后却只把 `next_offset` 增加一个 `50ms` interval。

如果 runtime、宿主线程或进程被暂停数秒，恢复后大量历史 offset 都会得到零等待，循环会连续补发已经错过的 punch tick。例如 reverse punch 暂停 `5s` 后可能集中补发约 `101` 个包，形成瞬时 UDP burst，并随 candidate 数量放大。

本任务要求调度恢复后跳过已经错过的 tick，不重放历史发送次数。一次已在等待中的到期调度最多产生一个发送，随后直接越过所有过期或不足以满足 `50ms` 最小发送间隔的 tick，等待下一个合格 tick；不得通过紧密循环补偿暂停期间的包数。

## Scope

### In scope

- 修正 `run_udp_punch_burst` 的 tick 推进语义：调度延迟后直接跳过错过的 tick，而不是按 `50ms` 逐项追赶历史 offset。
- 保持原有 active 首包偏移 `250ms`、reverse 首包偏移 `0ms` 和 `50ms` cadence 基线；暂停恢复时一次到期调度最多发送一个 punch，后续发送必须重新等待至少一个 `50ms` interval。
- 保持 tick 与原 connect-attempt 起点、active/reverse 起始偏移和 connect deadline 的绑定；跳过 tick 不得延长 punch deadline，也不得补偿性增加总包数。
- 当计算出的下一合格 tick 已超过 connect deadline 时立即结束 punch，不进行 deadline 后的补发。
- 增加确定性回归覆盖，证明数秒调度延迟不会形成连续补发，并覆盖轻微延迟、跨多个 interval、deadline 附近以及 active/reverse 起始偏移。

### Out of scope

- 修改 `50ms` interval、active/reverse 首包偏移、connect timeout 默认值、outer early-error retry 或 Quinn PTO/loss recovery。
- 改成从每次实际发送时刻完全重置一条新的、脱离原 connect-attempt 起点的时间线。
- 修改 punch payload、candidate policy、SN/QUIC/TLS wire protocol、listener 同源端口或 best-effort send-error 语义。
- 修改 punch owner cancellation、listener close、connect success/error/timeout 的既有终止边界。
- 修改多 candidate 选择、竞速、tunnel publish、PN fallback 或上层 API。
- 在 proposal 阶段修改生产代码、测试代码、design、testing 或运行时资源。

### Boundary with neighboring modules

- `p2p-frame/src/networks/quic/listener.rs` 继续拥有 punch tick 调度、listener close 观察与 best-effort UDP 发送；本任务只修正其错过 tick 的推进语义。
- `p2p-frame/src/networks/quic/network.rs` 继续拥有 connect attempt 与 punch future 的共同生命周期；本任务不改变 owner composition、retry 或 connect result。
- `p2p-frame/src/tunnel/**`、`src/sn/**`、`src/pn/**` 及相邻 crates 不新增或修改契约。

## Requirement Review

- 用户选择“跳过已经错过的 tick”是合理且更符合固定 cadence 的修复方向。它保留以 connect-attempt 起点为基准的 active/reverse 调度，同时避免 runtime 恢复后追赶历史包数。
- 仅把 `saturating_sub` 换成普通减法不充分：过期 offset 仍需安全地直接推进到下一合格 tick，并处理 duration 溢出与 deadline 边界。
- 单纯在每次发送后继续执行 `next_offset += 50ms` 也不充分，因为暂停期间积累的每个 offset 仍会依次产生零等待。
- 本 proposal 将 cadence 解释为发送频率上限，而不是必须补齐的包数配额：调度暂停会减少实际发送总数；恢复时允许当前到期等待最多发送一个包，但不得随后立即补发历史 tick。
- 选择保留原始 attempt 时间基线并跳 tick，而不是从实际发送时间建立新的漂移时间线。具体无溢出的 tick 计算与可测试边界由 design 定义。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-QPSM-1 | quic_punch_skip_missed_ticks | QUIC UDP punch 在调度延迟后跳过所有待补偿的历史 tick；一次到期等待最多发送一个包，后续发送重新等待至少一个 `50ms` interval，且下一 tick 超过 connect deadline 时直接结束 | 限于既有 `run_udp_punch_burst` schedule；保持 active/reverse 首包偏移、attempt 起点、deadline、payload、candidate policy、source socket 和 owner cancellation 不变 | runtime 暂停期间的 punch 包会被永久丢弃，实际总包数少于理想无延迟调度；这是避免恢复 burst 的有意取舍 | 受控时钟或纯调度函数回归证明暂停 `5s` 后不会连续补发约 `101` 个包、任意两个恢复后发送之间至少重新经过一个 interval，并证明 deadline 后零发送以及 active/reverse 边界不回归 | 不改变 interval/timeout，不重建 Quinn connection，不修改 wire、candidate、owner 或 retry 语义 |

## Success Criteria

- runtime 暂停或调度延迟跨过任意多个 `50ms` interval 后，恢复路径不会为每个历史 offset 连续发送 UDP punch。
- 一次已到期的等待最多触发一个发送；完成该发送后，下次发送前至少重新经过一个 `50ms` interval，错过的包数不补偿。
- active 首包仍以 `250ms` 为基线，reverse 仍可立即开始；无显著调度延迟时保持现有 punch 行为和 connect deadline 覆盖。
- 下一合格 tick 超过 connect deadline 时 punch 正常结束，deadline 后没有补发。
- connect success、final error、timeout、future cancellation、listener close、send failure best-effort、同源 listener socket 和非 punch candidate 行为保持不变。
- post-implementation testing 至少覆盖无延迟基线、轻微迟到、跨多个 interval、`5s` 级暂停、deadline 边界、duration 溢出保护以及 active/reverse 两类 intent。

## Risks

- 如果只在发送前推进 offset，发送 future 本身的长耗时仍可能留下临近 tick；design 必须明确以哪一个发送完成/调度观察点保证下一发送重新等待完整 interval。
- 如果为避免迟到而无条件跳过当前等待，正常 timer 的微小唤醒抖动可能导致持续漏发；实现必须区分“一次当前到期发送”与“随后待追赶的历史 tick”。
- 错误的向上取整或 `Duration` 算术可能越过 deadline、溢出或破坏 active `250ms` 首包；需要边界测试。
- 多 candidate 仍可各自并发 punch，但本任务应使每个 candidate 的发送频率受限，且不会因共同 runtime 恢复而各自产生历史补发倍增。

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | no | 不修改 API、punch payload、SN/QUIC/TLS wire 或 mixed-version 语义 | 最终 diff 与 consumer review 确认无 contract 变化 | proposal 已限定为内部调度修复 | owner: design/acceptance; reason: 待实现后核对 | none |
| data/schema | no | 无持久化数据、schema 或迁移 | source review | proposal 已排除持久状态 | owner: none; reason: not applicable | none |
| security/privacy/permission | yes | 修复公网 UDP 瞬时 burst 与多 candidate 流量放大 | 证明无历史补发、deadline 后零发送、payload/policy 不变 | proposal 已定义容量边界 | owner: testing/acceptance; reason: 需可运行回归证据 | 多 candidate 可在同一 tick 各发一个包，但不再各自追赶历史 tick |
| runtime/integration | yes | runtime 暂停、timer 唤醒、UDP send future 与 deadline 共同影响调度 | design 定义 tick 算法和状态边界；testing 覆盖迟到、暂停、deadline、close/cancel 与溢出 | proposal 已确认当前 `saturating_sub` + 单 interval 推进缺陷 | owner: design/testing; reason: 具体实现尚未交付 | timer 抖动处理错误可能造成漏发或短间隔发送 |
| build/dependency/config/deployment | no | 不新增依赖、feature、配置或构建资源 | final diff/build review | proposal 明确不改配置与依赖 | owner: implementation/acceptance; reason: 待最终 diff | none |
| ui/datamodel/workflow | no | 无 UI、数据模型或用户工作流变化 | scope review | proposal 限于 Rust QUIC runtime | owner: none; reason: not applicable | none |
| harness/process | no | 仅消费既有任务阶段和检查 | 各阶段运行任务级门禁 | proposal 使用 sibling packet | owner: later stages; reason: 后续证据尚未生成 | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
