---
task_manifest: task.yaml
status: approved
---

# P2P Frame Proposal

Risk profile: not-created (replace with ./risk-profile.yaml only after high-risk confirmation)

## Workflow Tier Judgment
- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 修复集中在 p2p-frame 测试面和一个 `#[cfg(test)]` 只读观察边界，但涉及 QUIC 取消生命周期与 TTP/PN 跨模块 readiness 同步，具有异步时序和跨模块测试行为影响；不能按单模块 trivial 处理。
- Proposal and tier confirmation: approved by user confirmation on 2026-09-02; user selected automatic completion

## Background and Goal

宽套件运行出现三个失败：

1. `udp_punch_quic_nat_connect_worker_task_is_aborted_with_owner_future` 在 owner future 被取消后，20 次调度让步内仍未观察到 worker task 资源释放。
2. `tcp_reverse_data_first_claim_pn_proxy_stream_uses_real_reverse_tcp_target` 等待 PN 缓存 B control tunnel 时 5 秒超时。
3. `server_cache_readiness_observer_preserves_connecting_tunnel` 断言 `Connecting` tunnel 进入 `Connected` 后仍可被测试观察器找到，当前实现失败。

当前源码证据显示：TTP 的测试 readiness 观察复用了 production `get_existing_tunnel`，而 production lookup 会清理所有非 `Connected` 缓存项。TCP 接受侧初始 `PassiveReady` 映射为 `Connecting`，因此观察器可能在正常状态提升前把 tunnel 移除，这直接解释 PN 和 TTP 失败。QUIC 失败需要进一步确定是 owner-drop 的取消保障缺陷，还是测试用让步循环对 worker abort 的资源释放观察不足。

## Scope

### In scope

- 让 `has_cached_tunnel_for_test` 变成只读观察：目标匹配且 tunnel 当前 production 可用时返回 true；`Connecting` 项返回 false 但不得被删除。
- 保留 PN 测试的一次 proxy request 约束，并确保其 readiness 前置观察不会破坏真实 reverse TCP fallback。
- 根据复现结果修复 QUIC connect owner 取消时 worker task 必然被 abort 的行为或其测试同步；若缺陷仅是测试观察不充分，则使用事件/有界等待替代固定让步计数。
- 重新运行三个点名测试以及 PN 并发压力用例。

### Out of scope

- 不改变 TTP production cache lookup、retain/prune、错误传播或创建语义。
- 不改变 TCP `PassiveReady -> Connected` 状态提升、PN relay、reverse TCP first-claim 或 wire protocol。
- 不延长 PN readiness deadline、不添加固定 sleep、proxy request/target-open 重试。
- 不改变 QUIC connect 的调度、超时、punch 或 NAT prediction 生产契约。
- 不处理无 `x509` 特性下既有 `tests/unit` 编译错误，也不整理工作区其他未提交改动。

### Boundary with neighboring modules

- `p2p-frame/src/ttp/**` 只允许在 `#[cfg(test)]` 提供非破坏性 readiness 观察；`TtpClient` 与 production multi-cache 行为不变。
- `p2p-frame/src/pn/service/pn_server/tests/**` 只调整测试前置同步和验证。
- `p2p-frame/src/networks/quic/**` 只覆盖 owner future drop 到 worker abort 的生命周期保障。
- `cyfs-p2p-test` 不作为实现或证据。

## Requirement Review

修复请求合理。TTP 失败不能用增大等待时间解决：观察器一旦在 `Connecting` 窗口触发 production lookup，缓存项已被删除，等待只会重复失败。最小修复是把测试观察与 production lookup 的清理副作用分离，同时继续使用 `is_tunnel_available` 和 `match_target` 判定 readiness，避免复制状态定义造成漂移。QUIC 断言要求的不是额外 sleep，而是 owner future 取消与 worker abort 之间可稳定观察到的前置关系；若生产 wrapper 已正确 abort，则测试必须等待同一事件，而不是依赖固定次数让步。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-036-QUIC | CHG-036-quic-owner-abort | owner future 被 abort/drop 后，其 worker-runtime task 必须被 abort，并且测试能在有界时间内观察到该任务资源释放 | 修复限于 QUIC connect owner wrapper 或其测试同步；不改变连接结果、调度策略或 runtime 配置 | 有界等待换取消保障的可观察性，不引入业务等待 | 点名 QUIC lifecycle 测试重复通过；不依赖固定 sleep | 不要求真实 QUIC 连接或公共 NAT 行为 |
| P-036-TTP | CHG-036-ttp-readiness | TTP readiness 测试观察不得修改缓存；PN 测试只有在匹配且 production 可用的 B tunnel 建立后才发送一次 proxy request | 仅修改 `#[cfg(test)]` 观察面和受影响测试；production cache/TCP/PN 状态机不变 | 增加窄只读测试入口，换取测试 setup 与真实 tunnel 状态提升的 happens-before | TTP 回归和 PN 精确/并发压力用例通过 | 不保证 production 在 `Connecting` tunnel 上成功打开业务流 |

## Success Criteria

- Concrete system-visible result: 三个用户点名失败不再出现；PN readiness 观察不再删除正常 `Connecting` control tunnel；QUIC owner drop 后 worker task 释放可稳定验证。
- Required evidence: 保留当前失败分析；三个点名测试重复通过；点名 PN 并发压力用例通过；standard task 完成变更记录和独立缺陷发现报告。
- Explicit non-goals: 不声明真实多机、公共 NAT、部署或全工作区验证。

## Risks

- 测试 readiness 观察若自行复制 availability/target-match 判定，可能与 production lookup 漂移；必须复用同一模块内现有判定。
- 只读观察不能修复 production tunnel 真正关闭或被清空的问题；若移除观察副作用后仍失败，应回到提案扩大范围。
- QUIC abort 修复若直接改变连接 wrapper 的结果语义，会扩大公开行为；必须仅在 owner 取消/资源释放边界处理。
