---
module: p2p-frame
task_name: 017-quic-nat-traversal-improvement
submodule: 017-quic-nat-traversal-improvement
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# QUIC NAT Punch 生命周期扩展 Proposal

## Workflow Tier Judgment

- Proposed tier: `high-risk`
- Final tier: `pending`
- Rationale: 改动虽然集中在 `p2p-frame` QUIC 建连路径，但会改变 punch 后台任务、Quinn `Connecting`、连接超时、future 取消和 listener close 之间的异步生命周期关系。若取消边界不正确，可能在连接已经成功、失败或关闭后继续发送 UDP，因此需要 design、implementation、post-implementation testing 和 acceptance 的完整证据链。
- Confirmation statement: 待用户确认本 proposal 及 `high-risk` tier；确认前不修改生产代码或测试。

## Background and Goal

当前 `QuicTunnel` 已通过 SN 协调主叫与被叫双方，对 IPv4 `ServerReflexive` QUIC candidate 从 listener 同源端口发送 UDP punch，并并发执行 QUIC handshake。active punch 延迟 `250ms` 起发，reverse punch 立即起发，两者每 `50ms` 发送一次，但都被固定的 `1s` deadline 截止。

主叫方的 reverse SN call 还会延迟 `300ms` 启动。SN 往返、被叫调度或系统负载稍高时，双方 punch 窗口可能只有很短重叠，甚至完全错开；此时 Quinn `Connecting` 仍在按 PTO 维护同一次 QUIC handshake，但 punch 已经提前停止。

本任务只把 punch-enabled QUIC candidate 的 punch 生命周期延长至该 candidate 的 QUIC connect deadline。punch 必须与现有单个 Quinn `Connecting` 并行，并在连接成功、失败、超时、调用 future 被取消或 listener 关闭时立即停止，不得脱离连接 owner 继续运行。

## Scope

### In scope

- 删除 punch 调度中的固定 `1s` 截止语义；punch 的最晚截止时间改为对应 candidate 已计算出的 QUIC `connect_timeout`。
- 保持现有时序基线：active 首包偏移 `250ms`、reverse 首包偏移 `0ms`、punch cadence `50ms`。本任务不引入新的自适应退避算法。
- punch 发送任务必须由对应 Quinn `Connecting` 所在的 QUIC connect attempt 拥有。attempt 成功、失败、超时、future drop/cancel 或 listener close 时，后续 punch 必须停止。
- 保持 Quinn 单个 `Connecting` 内部的 PTO/loss recovery 行为；不得把 `50ms` punch cadence 实现成周期性重建 Quinn connection。
- 保持现有 outer early-error retry 规则和 `UDP_PUNCH_CONNECT_RETRY_INTERVAL` 语义不变；本任务不延长或重新分类该错误重试窗口。
- punch 继续使用当前 QUIC listener 的同源本地端口，只对现有策略允许的 IPv4 `ServerReflexive` QUIC candidate 启用。
- 保持 punch 为 best-effort：单个 UDP punch 发送失败不得独立终止 QUIC connect；最终结果仍由经过 TLS 身份验证的 QUIC handshake 决定。
- 增加足够的生命周期诊断，使日志能够区分 punch 因 success、final error、timeout、cancel 或 listener close 停止；不得记录业务载荷、密钥或其他秘密。
- post-implementation testing 覆盖调度边界、pending `Connecting` 期间持续到 deadline、成功/错误提前停止、future cancellation、listener close 和非 punch candidate 兼容行为。

### Out of scope

- 修改 SN `call` / `called` wire protocol、增加 connectivity-check token、为 punch 包增加 echo/ack，或改变 TLS 身份验证。
- 修改 SN endpoint 的观察、缓存、新鲜度、candidate 分类、排序或 `300ms` reverse hedge delay。
- 增加多 SN、多 reflector、NAT 类型推断、端口预测、端口扫描、UPnP/NAT-PMP、额外 UDP 端口或 IPv6 punch。
- 修改 `50ms` cadence、active/reverse 首包偏移、QUIC connect timeout 的配置值或默认值。
- 修改 Quinn PTO/loss recovery、周期性新建 `Connecting`，或延长/重新分类现有 outer early-error retry。
- 让 punch 超过 connect deadline、在 tunnel 建立后作为 keepalive 继续运行，或把 punch 到达视为 tunnel 建立成功。
- 改变多 candidate 竞速、tunnel register/publish、reverse waiter、PN proxy fallback、stream/datagram 或上层消费者契约。
- 承诺穿透 symmetric NAT、UDP 阻断网络或任何需要本任务范围外能力的网络。
- 在 proposal 阶段修改生产代码、测试、design、testing 或运行时资源。

### Boundary with neighboring modules

- `p2p-frame/src/networks/quic/**` 负责 punch 与单个 Quinn `Connecting` 所在 connect attempt 的共同 deadline、发送节奏和取消/关闭收敛；Quinn PTO 与现有 outer early-error retry 保持不变。
- `p2p-frame/src/tunnel/**` 继续决定 candidate、direct/reverse 竞速和最终 tunnel 发布；本任务不改变其现有选择语义。
- `p2p-frame/src/sn/**` 继续提供当前 endpoint 观察和 `SnCall` / `SnCalled` 协调，不修改协议或缓存语义。
- `p2p-frame/src/pn/**` 继续提供既有 proxy fallback，不参与 punch 生命周期。
- `cyfs-p2p`、`cyfs-p2p-test` 和 `sn-miner-rust` 不新增生产契约；相邻模块只需要验证现有行为兼容。

## Requirement Review

- 把 punch 窗口延长到 connect timeout 是合理的最小改进。当前默认 QUIC connect timeout 为 `10s`，而固定 `1s` punch 窗口会在连接仍被允许等待时过早放弃 NAT filter 打开动作。
- 不能只把 `UDP_PUNCH_DEADLINE` 改成更大的固定值。当前 punch 使用 detached task；若缺少 attempt-owned cancellation，连接提前成功或调用 future 被取消后仍可能继续发送。
- punch 延长期间必须维持现有单个 Quinn `Connecting`，由 Quinn 自己按 PTO 重传 Initial/Handshake。`50ms` 只属于 punch cadence，不得据此周期性创建新的 Connection ID 或握手状态。
- 如果 Quinn `Connecting` 自身提前返回成功或错误，punch 必须随该 attempt 立即停止；本任务不为了延长 punch 而吞掉终止错误或扩大 outer early-error retry。
- 保持 `50ms` cadence 到默认 `10s` deadline 时，单 candidate 最多约发送 `200` 个长度 `5..=30` 字节的 punch 包，另有 QUIC handshake 流量。该成本是本轮明确接受的权衡，但发送必须以 connect deadline 和 owner cancellation 为硬边界，不能后台常驻。
- 本轮不同时修改 endpoint freshness、SN 双向介绍、端口预测或多端口策略。延长窗口后若用户环境仍失败，应依据日志另行确认是否属于候选过期、socket 映射不一致或 symmetric NAT，而不是在本任务中继续扩大范围。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-QNPL-1 | quic_nat_punch_connect_lifetime | punch-enabled QUIC candidate 从现有 active/reverse 首包偏移开始，按 `50ms` cadence 持续 punch，与现有单个 Quinn `Connecting` 并行，直至该 candidate 成功、失败或其 connect deadline 到达 | 限于现有 IPv4 `ServerReflexive` listener-based QUIC candidate；直接复用已经计算的 connect timeout，不改变 timeout 配置、Quinn PTO 或 outer early-error retry | 默认 `10s` 下每 candidate 最多约 `200` 个短 punch 包，换取完整覆盖 SN 延迟和连接等待窗口 | 受控时钟测试证明超过原 `1s` 后仍发送 punch、deadline 后不再发送，且一个 pending connect attempt 只对应一个 Quinn `Connecting` | 不修改 candidate 来源、SN wire、punch cadence、首包偏移、timeout 默认值、Quinn 重传或非 punch connect 行为 |
| P-QNPL-2 | quic_nat_punch_owner_cancellation | punch task 与对应 QUIC connect attempt 共享生命周期；success、final error、timeout、future drop/cancel 和 listener close 都必须终止后续发送，且 punch send error 保持 best-effort | 不改变多 candidate 竞速和 tunnel publish 所有权；不要求为 punch 增加网络应答 | 需要显式取消/所有权结构和并发边界测试，但避免连接结束后的无主 UDP 流量 | 确定性测试证明每个终止路径均停止发送，无 detached task 泄漏、late send 或重复 tunnel publish | 不把 punch 改成 keepalive，不在 connect attempt 结束后保留 NAT 状态 |

## Success Criteria

- punch-enabled active/reverse QUIC candidate 不再受固定 `1s` deadline 限制；在连接尚未完成且 connect deadline 未到时，超过 `1s` 仍会按现有 cadence 发送 punch。
- punch 延长期间继续使用同一个 Quinn `Connecting`，由 Quinn 按现有 PTO/loss recovery 发送 Initial/Handshake probe；不得按 `50ms` cadence 周期性创建新连接。
- 现有 outer early-error retry 的触发条件、截止窗口和错误返回保持不变。
- connect success、final error、deadline、future cancellation 和 listener close 后均没有该 attempt 的后续 punch；取消和关闭不得遗留 detached task。
- punch 仍从当前 listener 同源本地端口发出，收到的私有 punch payload 仍不会进入 Quinn 或业务层；最终连接成功只由 QUIC/TLS handshake 确认。
- 非 `ServerReflexive`、非 IPv4、非 QUIC、零端口、LAN、无 SN 或无 listener 路径保持当前行为，不因本任务获得 punch 或额外 retry。
- post-implementation testing 通过任务级 unit、DV/integration 入口，至少证明一个人工延迟超过 `1s` 但小于 connect timeout 的双端场景能够继续尝试并完成连接，同时覆盖所有停止条件。

## Risks

- 默认 `10s`、`50ms` cadence 会明显增加最坏情况下的 punch 包数；若多个 candidate 并发失败，流量按 candidate 数量增长。connect deadline、现有 candidate 集合和及时取消是本任务的硬上限。
- detached task 若未与 connect future 正确绑定，会在 success/cancel/close 后继续发包；design 必须明确 owner、取消信号、drop 行为和 listener close 顺序。
- punch 生命周期若错误地驱动新的 `Connecting`，会产生多个 Connection ID 和独立握手状态。design 必须把 punch ticker 与单个现有 connect future 并行，而不是用 ticker 触发重连。
- 连接 timeout 可以由调用方配置得很长。本任务遵循该显式配置，不另设 `1s` 隐藏上限；但不得允许零 deadline、duration 溢出或任务跨 listener 生命周期存活。
- 本任务只增加双方窗口重叠概率，不能修复过期/错误公网 candidate、SN 观察 socket 与 listener socket 映射不一致、symmetric NAT 或 UDP 阻断。

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | no | 不修改 SN、punch payload、QUIC/TLS 或上层 wire contract | design/source review 确认无 wire diff；integration 保持现有节点行为 | proposal 已追踪现有内部 punch 和 QUIC retry 路径 | owner: design/testing; reason: implementation 尚未交付 | 内部时序变化可能暴露既有竞态，但不会形成 mixed-version wire 不兼容 |
| data/schema | no | 无持久化数据、schema 或迁移 | source review 确认仅有短期 attempt 状态 | proposal 排除持久状态 | owner: design; reason: attempt owner 形态待设计 | 错误 owner 可能造成短期任务泄漏 |
| security/privacy/permission | yes | 延长公网 UDP punch 会增加最坏包数和潜在资源消耗 | 保持固定小载荷、既有 candidate policy、connect deadline 和取消硬边界；测试超时/取消/close | proposal 已量化默认包数并禁止 deadline 外发送 | owner: design/testing; reason: 精确停止机制待设计与验证 | 超长用户配置会放大包数，但仍受显式 connect timeout 限制 |
| runtime/integration | yes | punch task、单个 Quinn `Connecting`、future drop、listener close 跨异步生命周期协作 | design 明确 owner/state/cancel；testing 覆盖全部终止路径、延迟场景、单连接不变量和无 late send | proposal 已确认当前 punch 为 detached task且固定 `1s` | owner: design/testing; reason: 并发实现与确定性证据属于下游阶段 | 竞态可能导致 late send、提前停止或意外创建额外连接 |
| build/dependency/config/deployment | no | 不新增依赖、配置或默认值 | diff/build review | proposal 明确复用现有 connect timeout | owner: implementation/acceptance; reason: 待最终 diff 确认 | none |
| ui/datamodel/workflow | no | 无 UI、公开数据模型或用户工作流变化 | scope review | proposal 限于 Rust QUIC runtime | owner: none; reason: not applicable | none |
| harness/process | no | 仅消费现有 Harness 阶段与检查 | 各阶段运行任务级门禁 | proposal 使用现有 sibling packet | owner: later stages; reason: 普通任务证据由各阶段负责 | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
