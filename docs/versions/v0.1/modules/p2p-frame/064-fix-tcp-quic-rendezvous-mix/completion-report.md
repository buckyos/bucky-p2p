# Completion Report: 064-fix-tcp-quic-rendezvous-mix

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/064-fix-tcp-quic-rendezvous-mix.md

## Delivery Summary

- Outcome: 修复非打洞 rendezvous 候选在构造侧混入 TCP+QUIC、而 `sn.rs` 校验要求整组同协议，导致请求在发送前被 `InvalidParam` 本地拒绝并静默退回 legacy 的问题。`rendezvous_base_endpoints` 现构造单一传输数组：优先 QUIC，无合格 QUIC 候选时回退 TCP；打洞分支保持仅 QUIC。`sn.rs` 与 wire/协议不变。
- Handoff: 交付改动仅 `p2p-frame/src/tunnel/tunnel_manager.rs` 与 `p2p-frame/tests/unit/tunnel/rendezvous_endpoint_policy_tests.rs`；既有混协议断言已更新，新增纯 TCP 回退与混合输入锚定 TCP 两个回归用例。变更记录标记 complete。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| homogeneous_rendezvous_nonpunch_transport | 非打洞 rendezvous 候选为单一传输协议（优先 QUIC、无则回退 TCP），与 `sn.rs` 校验契约一致；打洞分支不变 | proposal.md P-064-HOMO Scope、Success Criteria | `rendezvous_base_endpoints` 单传输锚定实现；`reverse_connect_falls_back_to_single_tcp_transport_when_no_quic_candidate` / `reverse_connect_mixed_transport_with_no_quic_eligible_anchors_to_tcp` 回归 | 交付缺陷：无；范围外未改 `sn.rs`/`nat_candidates` | pass |

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 064-F-NONE | P3 | 独立反证轮次覆盖全部三类的证据均无失败 | 未发现阻塞性生产缺陷；`rendezvous_base_endpoints` 唯一生产构造点、QUIC 回退、空数组不变式均核验通过 | no |

## Independent Defect Discovery

本节为定向测试之后执行的独立反证轮次；未把"全部 492 项 lib 测试通过"作为消除各反例的依据。

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | `rendezvous_base_endpoints`、`nat_candidates`、`new_rendezvous_request` 及其唯一生产调用点 | 从候选构造追到请求发送前的 `validate`：确认唯一本地候选来源已收敛为单协议；打洞分支断言仍严格 QUIC | 未发现仍向请求喂混合协议的生产路径 | pass |
| boundaries-and-failure-paths | `requires_endpoints`、锚 QUIC 无候选时回退、`MAX_NAT_PLAN_CANDIDATES`、端口/区域过滤、peer 入站 notify 校验 | 混合输入无 QUIC 合格候选时锚定 TCP 是否仍保留全部合格 TCP；空数组不变式是否被破坏；入站 notify 是否绕过单协议约束 | TCP 回退保留全部合格 TCP 且至少一个合格 endpoint 时结果非空；入站 notify 走未改的 `sn.rs` 校验属既有 wire 契约，非本任务范围 | pass |
| regression-and-side-effects | 既有 `reverse_connect_request_candidates_accept_public_wan_and_mapped` 断言、punch 分支、`nat_type_aware/tunnel_manager_tests.rs` 的 nat_candidates | 更新后既有断言是否仍表达正确单协议语义；punch 结果是否被锚定逻辑改变；完整 lib 套件是否回归 | 既有 reverse 断言已收敛为单一 QUIC；punch 断言不变；492/492 通过证明无侧效应 | pass |

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib reverse_connect`
- Result: pass

（新实现/回归依赖 x509 才能编译 crate 内 `sn::tests`；`cargo test -p p2p-frame --features x509 --lib` 全套 492 passed / 0 failed，68.78 秒。）

- Exception reason: 不新增公网/跨进程端到端场景；候选到请求校验的组合以新增纯 TCP 与混合输入锚定用例 + 全量 lib 套件覆盖。既有变更已被批准范围约束，未部署真实双公网 NAT。

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 交付范围与批准提案一致；独立缺陷发现三轮全部 pass 且无阻塞发现；定向测试 red->green（原混协议断言改为单协议语义、新增 2 个回归均通过）；完整 lib 单测 492/492 无回归。变更记录已标记 complete。