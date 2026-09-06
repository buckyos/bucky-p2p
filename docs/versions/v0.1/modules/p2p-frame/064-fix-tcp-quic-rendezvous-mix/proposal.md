---
task_manifest: task.yaml
status: approved
---

# Repair TCP/QUIC 混合 rendezvous 候选导致请求被本地拒绝

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 改动集中在 p2p-frame 内部 `TunnelManager::rendezvous_base_endpoints` 的候选构造与实际 wire 请求构造，不改协议、不改 wire 编码、不改公开接口签名、不涉及并发/生命周期/runtime 集成、依赖、部署、安全面；但会改变生产行为（ReverseConnectOnly 的候选数组从"可混协议"变为"单一协议"）并需同步改一个既有单元测试的断言，不满足 trivial 的"影响局部且无行为变更/无需测试改动"，故按 bounded single-project bugfix 归 standard。无任何 high-risk 触发。

## Background and Goal

`ReverseConnectOnly` / `WaitIncoming` 等不打洞的 rendezvous 操作，其候选由 `TunnelManager::rendezvous_base_endpoints`（`p2p-frame/src/tunnel/tunnel_manager.rs:1132`）构造。其中非 `punches()` 分支的传输协议筛选为 `matches!(endpoint.protocol(), Protocol::Quic | Protocol::Tcp)`，允许同一数组同时含 TCP 与 QUIC；`reverse_endpoints_for_sn` 又把 TCP+QUIC 的 listener 结果与 WAN endpoint 全量混排。

但请求发送前的协议校验 `validate_rendezvous_endpoints`（`p2p-frame/src/sn/protocol/sn.rs:134`）要求 `Some(endpoint.protocol()) == transport`——即整个数组必须同协议。构造方"允许混协议"与消费方"要求同协议"的契约不一致，导致公网双 listener 等配置下请求尚未发出即在本地被 `InvalidParam` 拒绝，随后静默退回 legacy。legacy 可能经 proxy 补偿成功，故故障常被掩盖，不表现为稳定连接失败。

目标：让非打洞的 rendezvous 候选数组满足单一传输协议不变式，与校验方契约对齐。采用构造侧约束（轻、不动 wire/协议语义），不改 `sn.rs` 消费方。

## Scope

### In scope

- `p2p-frame/src/tunnel/tunnel_manager.rs` `rendezvous_base_endpoints`：
  - 非 `punches()` 分支改为构造**单一传输协议**数组：优先 QUIC，无任何合格 QUIC 候选时回退 TCP；整组只保留该锚协议（同构数组）。
  - `punches()` 分支保持现状（仅 QUIC）。
  - 保留区域/端口过滤、去重、`MAX_NAT_PLAN_CANDIDATES` 截断与"至少一个合格 endpoint 才非空"的语义。

- 测试：
  - 更新 `p2p-frame/tests/unit/tunnel/rendezvous_endpoint_policy_tests.rs` 中 `ReverseConnectOnly` 现有断言（当前断言含混协议 TCP endpoint）。
  - 新增回归：TCP 在前的合格 endpoint 产生纯 TCP 数组；QUIC 在前的产生纯 QUIC 数组；混合输入下数组协议一致，且 `validate` 可通过。

### Out of scope

- 不修改 `p2p-frame/src/sn/protocol/sn.rs` 及任何 wire/协议编码语义。
- 不改变 `rendezvous_reverse_connect_eligible_area` / `rendezvous_eligible_area` 等区域资格判定。
- 不修改 `nat_candidates`（Predicted 模式）、非本数组的其它构造路径、`cyfs-p2p-test/**` 或其它 crate。
- 不改公开接口签名、不加兼容层、不改部署/依赖/安全面。

### Boundary with neighboring modules

- `sn.rs` 的求值不变式（整组同协议）作为本任务的**契约输入**保持不变；仅构造侧收敛到该契约。
- `nat_connect_plan.rs` 决定 `request_candidates = Some(connector_candidates)` 且 operation 为 `ReverseConnectOnly` 的路径是触发点，本任务不改该文件。

## Requirement Review

合理。备选方向：
- 构造侧同构（本提案）：轻量、只改构造、不动 wire，与现有校验契约天然一致。
- 消费侧放宽为按协议分组合法：改动 `sn.rs` 校验语义并牵连远端消费，超出本轮 bug 修复范围、风险更大。

用户已明确"修复"，未指定方向；默认选风险最小的构造侧同构。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-064-HOMO | homogeneous_rendezvous_nonpunch_transport | 非打洞 rendezvous 候选数组为单一传输协议（优先 QUIC、无则回退 TCP），与 `sn.rs` 校验契约一致；打洞分支不变 | `tunnel_manager.rs` `rendezvous_base_endpoints` 与 `rendezvous_endpoint_policy_tests.rs` | 不再同时公告 TCP 与 QUIC 两个传输；以 QUIC 优先（无 QUIC 合格候选时回退 TCP）作为整组锚协议 | `reverse_connect_request_candidates_*` 回归用例含纯 TCP 数组验证；`cargo test -p p2p-frame` 定向组通过 | 不改 wire/协议、不改 `sn.rs` 消费方 |

## Success Criteria

- Concrete system-visible result: 公网双 listener 下 `ReverseConnectOnly` 构造的候选中不再出现同一数组混协议，请求可正常发出，不再因 `InvalidParam` 在发送前被本地拒绝而静默退回 legacy。
- Required evidence: 更新既有测试 + 新增同构（纯 TCP / 纯 QUIC）回归用例 red->green；`cargo test -p p2p-frame` 定向测试组通过；`docs/changes/064-*.md` + `completion-report.md` 记录独立缺陷发现。
- Explicit non-goals: 不宣称跨进程/多节点/公共 NAT/部署环境证据；不改协议与公开 wire 契约；不保证本任务外的其它候选/校验问题。

## Risks

- 行为变化：ReverseConnectOnly 反转连接通告不再同时携带两个传输的 endpoint。单个协议仍由反向直连可拨通，且与校验契约一致；如某场景依赖双传输候选同时公告，需另开任务。本任务按最小修复处理。
- 锚协议固定为 QUIC（无合格 QUIC 候选时回退 TCP），不随 listener 枚举顺序漂移；QUIC 不可达但 TCP 可达时走 TCP 回退。
- 测试断言变更：既有 `reverse_connect_request_candidates_accept_public_wan_and_mapped` 断言混协议结果，将按新语义更新，属预期。

## Approval Record

- approver: user
- approval_date: 2026-09-06
- user_statement: 确认按 standard 执行；非打洞候选的单一传输协议选择"优先 QUIC、无 QUIC 合格候选则回退 TCP"。