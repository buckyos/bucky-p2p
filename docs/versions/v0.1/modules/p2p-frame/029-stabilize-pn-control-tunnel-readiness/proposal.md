---
module: p2p-frame
task_name: 029-stabilize-pn-control-tunnel-readiness
submodule: 029-stabilize-pn-control-tunnel-readiness
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# PN control tunnel readiness observer 修复 Proposal

## Background and Goal

已验收任务 `028-fix-regression-test-authority-readiness` 为 PN reverse TCP 组合测试增加了 cache-ready 前置观察，但当前精确用例仍可能在 5 秒后失败：

```text
PN should cache B control tunnel before the proxy request: Elapsed(())
```

源码时序表明，这不是等待上限不足。PN 侧接受的 TCP control tunnel 初始处于 `PassiveReady`，对外映射为 `TunnelState::Connecting`；收到主动端首个控制帧后才提升为 `Connected`。`TtpServer` 可以在该提升前完成 attach 并把 tunnel 放入缓存，而 task 028 的 `has_cached_tunnel_for_test` 随后调用生产 lookup。该 lookup 会清理所有尚非 `Connected` 的缓存项，因此观察器可能在 tunnel 正常过渡期间把它永久移除，后续等待只能超时。

本任务修正这个测试观察器，使它在判断“尚未 ready”时不修改缓存；测试仍只在匹配 tunnel 已进入 `Connected` 后发送唯一一次 proxy request。

## Scope

### In scope

- 把 task 028 增加的 TTP cache-ready 测试观察器改为非破坏性查询。
- 观察结果只有在目标 identity/endpoint 匹配且缓存 tunnel 当前可用于 production lookup 时才为 true。
- tunnel 处于正常 `Connecting` 过渡时返回 false，但不得删除该缓存项；其变为 `Connected` 后同一观察器必须可返回 true。
- 增加能够确定性暴露“观察导致 Connecting tunnel 被清理”问题的回归覆盖，并重新执行 PN 精确用例及并发压力用例。

### Out of scope

- 不延长 5 秒 setup deadline，不增加固定 sleep、ProxyOpenReq 重试或 target-open 重试。
- 不改变 production TTP cache 的 lookup、清理、选择或错误传播语义。
- 不改变 TCP `PassiveReady -> Connected` 提升、reverse data first-claim、wire protocol 或 PN relay 行为。
- 不修改已批准的 task 028 packet，也不整理工作区中的其他既有改动。

### Boundary with neighboring modules

- `p2p-frame/src/networks/tcp/tunnel.rs` 继续拥有被动 TCP tunnel 的连接状态转换；本任务只把该既有转换作为测试前置条件观察。
- `p2p-frame/src/ttp/client.rs` 的 production cache availability、pruning 和 target-match 语义保持不变。
- `p2p-frame/src/ttp/server.rs` 仅允许在 `#[cfg(test)]` 范围内提供不改变缓存的 readiness 观察。
- `p2p-frame/src/pn/service/**` 仍通过真实 `TtpServer` 发起一次 target open，并保留真实错误与双向 bridge 行为。

## Requirement Review

用户要求修复该超时是合理的。直接增加等待时间不能修复问题，因为当前观察器一旦在 `Connecting` 窗口调用 production lookup，目标项已经被删除；等待更久只会延迟同一个失败。改变 production cache 以保留 `Connecting` 项会扩大 task 028 明确排除的生产语义，也不是修复测试观察器自身副作用所必需。

选择最小修正：测试观察器在锁内检查匹配项及当前 availability，但不执行 retain/prune。这样它不会创建状态、推进连接或打开 stream；只等待真实 TCP 控制面把已缓存项提升为 production lookup 可选的 `Connected`。如果后续证据显示没有观察器参与时 production lookup 也会丢失正常 tunnel，则应创建独立生产缺陷任务，不能在本任务中静默扩大范围。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-PN-READY-1 | pn_cache_readiness_observer_non_destructive | cache-ready 测试观察不得清理仍在正常连接过渡中的匹配 tunnel，并且只能在该 tunnel 满足当前 production availability 时报告 ready | 仅修改 `#[cfg(test)]` 观察与测试覆盖；production lookup/cache/PN/TCP 行为不变 | 测试观察逻辑必须复用当前 target-match 与 availability 定义，但不能复用带清理副作用的 lookup 操作 | 确定性回归证明 Connecting 观察返回 false 且缓存项仍可在 Connected 后被找到；点名 PN 精确用例和并发压力用例通过 | 不保证或改变真实请求在目标 tunnel 尚未 Connected 时的生产等待语义 |

## Success Criteria

- Concrete system-visible result: 点名 PN 测试不再因 readiness 观察器删除正常过渡中的 B control tunnel 而超时。
- Required evidence: 保留本次用户提供的失败证据；新增确定性 red/green 回归；通过点名精确用例、现有 12-case 并发压力用例以及后续 task-scoped unified runner。
- Explicit non-goals: 不修改 production cache 策略、TCP/PN 协议、请求/错误语义或 deadline。

## Risks

- 若观察器复制而不是复用 production 的 target-match/availability 定义，测试可能与真实 lookup 漂移；后续 design 必须绑定到现有 helper 或提供同 owner 的窄测试专用只读 helper。
- 非破坏性观察只能证明测试 setup 已完成，不能证明 production 应等待 `Connecting` tunnel；本任务不得把二者混为一谈。
- 当前工作区包含其他未提交修改；后续阶段必须使用任务专属 manifest/baseline，避免覆盖或归属无关改动。

## Approval Record

- approver:
- approval_date:
- user_statement: ""
