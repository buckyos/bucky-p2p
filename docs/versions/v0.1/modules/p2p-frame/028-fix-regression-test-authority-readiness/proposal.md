---
module: p2p-frame
task_name: 028-fix-regression-test-authority-readiness
submodule: 028-fix-regression-test-authority-readiness
version: v0.1
status: approved
approved_by: user
approved_at: 2026-09-01
approved_content_sha256: dd02c28610209749c302422fe6154599c5a8c920f08e7f92ced1d6004fbd8b78
---

# 回归测试 authority 与 readiness 修复 Proposal

## Background and Goal

当前 `p2p-frame` 测试套件存在两个失败：

- `cold_distributed_query_returns_remote_profile_in_final_sn_query_response` 仍把 `PeerManager` 中直接注入的 `NatProfile` 当作可发布 profile，但 task `022-on-demand-nat-probing` 已将发布资格迁移到绑定当前 QUIC registration generation 的 `NatProbeScheduler`。该旧夹具因此稳定得到 `None`。
- `tcp_reverse_data_first_claim_pn_proxy_stream_uses_real_reverse_tcp_target` 在独立串行运行时通过，但宽套件并行时可能在 PN/TTP 尚未完成 incoming tunnel attach/cache 前发起 proxy open，并在有限次数的探测全部失败后报错。控制隧道已建立不等于目标隧道已进入 TTP 可用缓存。

本任务修正这两个测试的前置条件与同步方式，使测试继续验证已经批准的 NAT profile authority 和 reverse TCP first-claim 合约，而不改变生产行为来迁就旧夹具或调度时序。

## Scope

### In scope

- 让 cold distributed SN query 测试通过符合 task 022 权威状态约束的远端 detail fixture 提供 profile，并继续证明查询 SN 不建立远端 profile cache、最终 `SnQueryResp` 保留 profile。
- 为 PN reverse TCP 组合测试提供显式、可等待且有界的 incoming-tunnel cache-ready 测试信号；只在该信号成立后发起 proxy open。
- 如需从 `TtpServer` 观察 cache-ready，只增加 `#[cfg(test)]` 测试 seam，并保持 `mod.rs` facade 与生产接口不变。
- 通过现有 unified runner 注册并执行两个回归用例，保留 red/green 结果。

### Out of scope

- 不给 `SnService::local_peer_detail` 或 query/call 路径增加 `PeerManager` profile fallback。
- 不改变 `NatProbeScheduler` 的 generation、失效、QUIC eligibility、两小时周期或 profile 发布语义。
- 不修改 TCP reverse data first-claim 状态机、wire protocol、PN relay 生产逻辑或 TTP 生产缓存策略。
- 不以增加固定 sleep、放大 timeout 或增加盲目 retry 次数掩盖 readiness 竞态。
- 不修复或整理工作区中与这两个测试无关的既有改动。

### Boundary with neighboring modules

- `p2p-frame/src/sn/service/**` 的 scheduler-owned profile publication 合约保持不变；测试只构造与该合约一致的 inter-SN detail 输入。
- `p2p-frame/src/ttp/**` 仍拥有 accepted incoming tunnel 的 attach/cache；测试 seam 只观察现有状态，不创建新的生产状态或等待路径。
- `p2p-frame/src/pn/**` 继续通过 `TtpServer::open_stream` 选择目标 tunnel；测试只把请求时刻绑定到明确的 cache-ready 事实。

## Requirement Review

修复请求合理，但两处失败不能用同一种“放宽生产逻辑”处理。SN 失败暴露的是旧测试夹具与后续 authority 设计冲突；若生产查询回退到 `CachedPeerInfo.net_profile`，会允许已经失去权威 QUIC registration 的旧 profile 再次发布，违反 task 022。PN 失败则具有并行调度敏感性，独立测试通过，说明应补齐测试可观察的 readiness happens-before，而不是修改 reverse first-claim 状态机或延长探测窗口。

选择最小测试修正：SN 用例在 inter-SN 边界注入明确的 remote detail，同时继续断言 querying SN cold cache；PN 用例等待 TTP 已缓存目标 tunnel 的显式条件。若实现阶段发现必须改变生产时序或公开接口，本任务必须退回 design，不能在 testing 中静默扩大范围。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-RTR-1 | distributed_nat_profile_authority_fixture | 修正 cold distributed query 夹具，使远端 profile 通过当前 inter-SN detail 值语义进入最终响应，同时 querying SN 保持无本地 peer/profile cache | 不恢复 `PeerManager` 对 profile 发布资格的所有权，不绕过最终 `handle_query_sn` 聚合 | fixture 不再重复验证 serving SN 的 scheduler 生命周期；该生命周期继续由 task 022 专用测试负责 | 该精确用例先复现 `None`，修正后断言 profile、endpoint、peer info 与 cold-cache 不变量全部通过 | 不改 query wire、scheduler 或 peer cache 生产逻辑 |
| P-RTR-2 | pn_reverse_tcp_cache_ready_synchronization | PN reverse TCP 组合测试必须在 accepted B tunnel 已完成 TTP attach/cache 后才发起 proxy open，并继续验证真实 reverse TCP data fallback 与双向业务字节 | readiness 只能来自现有 cache 状态的测试观察，不用 sleep、timeout 放大或盲重试替代 | 增加一个窄的 `#[cfg(test)]` 状态观察 seam，换取并行套件中的确定性 happens-before | 精确用例与 task runner 在并行压力下通过；仍关闭 B 的直接 data listener，并验证 PN 到 B 的 reverse fallback 和双向字节 | 不改 TCP first-claim、PN service 或 TTP 生产行为 |

## Success Criteria

- Concrete system-visible result: 两个点名测试在当前工作区中稳定通过，且不再依赖旧 profile authority 或异步 cache 建立的偶然时序。
- Required evidence: 保留当前 red 复现；新增/修正断言覆盖 scheduler-authority 边界、querying SN cold cache、TTP cache-ready happens-before、真实 reverse TCP fallback 与双向数据；通过 `p2p-frame/028-fix-regression-test-authority-readiness all` unified runner 入口。
- Explicit non-goals: 不修改生产协议、NAT 调度语义、reverse first-claim 状态机、PN relay 行为或任何无关测试。

## Risks

- 若 SN fixture 直接返回固定 detail 而不保留 owner lease 与最终 query 聚合路径，可能把用例弱化成字段复制测试；design/testing 必须保留 cold distributed lookup 的 lease/remote-detail/final-response 链路及 cold-cache 断言。
- 若 readiness seam 暴露为非测试 API，会扩大 crate interface；必须限制为 `#[cfg(test)]` 且只读。
- 若 cache-ready 后仍出现 reverse TCP 失败，说明存在第二个状态机缺陷；届时应记录新证据并退回 design/implementation，而不是继续增加等待与重试。
- 当前工作区已有大量未提交改动；阶段 manifest 与基线必须只归属本任务新增的测试修正，不能把其他任务路径计入本任务完成证据。

## Approval Record

- approver: user
- approval_date: 2026-09-01
- user_statement: "确认，travial任务完成"
