---
task_manifest: task.yaml
status: approved
approved_by: user
approved_at: 2026-09-02
approved_content_sha256: 0a65b29b3e38e86fe657cdfb94754b6043ac167f4119b96aa3dd0d07b4fa139f
---

# 真实执行 p2p strategy matrix（替换 033/034 假矩阵证据）

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment

- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 本任务需要新增一个仅测试构建启用的 production 地址资格 seam（endpoint/rendezvous/punch 判定），并建立真实 socket 的受控 NAT 映射 fixture，贯通生产 `NatConnectPlan`、SN rendezvous wire、目标 action 与代表数据面。它涉及生产源文件、构建特性、真实 socket 集成、并发/定时和受控映射，均属于 033/034 已确认的高风险边界；当前证据不支持 trivial 或 standard 降级。
- Proposal and tier confirmation: confirmed by user statement `确认，自动完成`; auto-pipeline launched from design.

## Background and Goal

P1 审查确认当前矩阵测试没有执行生产策略分支：

- `p2p-frame/tests/real_p2p_tunnel_flow/strategy_matrix.rs:13-47` 的六行证据只是硬编码字符串；测试仅校验字段非空，并且显式断言 `actual_selected`、`request_sent`、`action_armed` 等全部为 `not-observable`（`:162-169`）。
- SymmetricLike 证据使用两个不同源 socket（`:142-151`），不能代表同一 NAT socket 对不同目标发生映射变化。
- 唯一真实连接用例 `callee_public_real_socket_flow_completes_payload`（`:172-199`）只断言 `Direct | Reverse`，没有断言 `WaitIncoming`、rendezvous 请求或目标动作。
- 当前 fixture 没有启用 SN NAT probe endpoints，profile 在真实查询中保持缺失/Unknown（`fallback.rs:159-175`）；生产 `open_tunnel_from_lookup` 因此在 profile 缺失分支直接走 `open_known_tunnel`（`tunnel_manager.rs:704-710`），`select_connect_plan`、prediction 与 rendezvous 分支根本没有运行。
- 因此即使生产 `NatConnectPlan`、prediction 或 rendezvous 全部损坏，当前矩阵测试仍会通过，不能被用作 033/034 的六组合真实覆盖证据。

目标：让六个矩阵条件从生产 `StreamManager::connect_from_id` 入口真实到达生产 `NatConnectPlan` → SN rendezvous 请求 wire → 目标 `on_sn_rendezvous` action，并在每个条件上记录并断言真实可观察的 `actual_selected`、`request_sent`、`action_armed`（以及可达条件的 `connected`/`payload-complete`）；SymmetricLike 证据改为同一逻辑内部 source socket 对不同目标的真实映射变化。

## Scope

### In scope

- 在 p2p-frame 增加一个仅测试构建启用的地址资格 seam（Cargo feature），使 `ServerReflexive` + IPv4 loopback/private 地址在以下生产判定中与其他路径一致：`TunnelManager::nat_candidates`/`rendezvous_base_endpoints`/`udp_punch_enabled_for_candidate`、`sn/protocol/sn.rs::validate_rendezvous_endpoints`、QUIC listener 的 `udp_punch_enabled_for_endpoint` 与 prediction 校验。默认构建（未启用 feature）行为必须与现状逐字节一致。
- 建立受控 NAT 映射 fixture（全部真实 UDP socket）：每个测试节点只有一个逻辑内部 source socket；SN 侧使用真实 NAT probe reflector，对同一 socket 访问不同目标端口时按条件回显稳定或变化的映射端口；SymmetricLike 映射使用确定的最小线性增量并提供有效 prediction hint；预测/映射端口由真实 relay socket 转发到节点真实 QUIC socket，保证数据面为真实网络包路径。
- 通过真实生产链路产生 profile：SN `set_nat_probe_endpoints` → 客户端真实 Report/probe/directive/result → SN peer manager 聚合 → `query_with_context` 返回 `local_net_profile` 与 `remote_net_profile`；测试断言之，不直接注入 `NatTraversalContext` 或 `NatProfile`。
- 重写 `strategy_matrix.rs` 为六个真实用例：从 `connect_from_id`/stream 入口发起，断言每行实际执行生产请求 operation（`WaitIncoming`、`ReverseConnectOnly`、`PunchOnly`、`PunchAndReverseConnect`）与 prediction 语义；捕获 initiator 的 request-sent 事件和目标 action 事件；公开/直接可达行必须完成真实双向唯一 payload 并记录 connection info。
- 在 integration test 二进制内安装全局 `log` sink，按 `remote_id` correlation 捕获 `event=sn_rendezvous_requesting`、`event=sn_rendezvous_target_finished` 等分支事件作为结构化证据；事件只作分支证据，不替代 payload。
- 保持 fallback 与 collision/cross-SN 测试现有职责；矩阵证据与本任务 testplan/runner 注册一致。

### Out of scope

- 不声明或模拟公网、真实路由器/运营商 NAT、跨主机或生产 SN/PN 网络；本任务仍是进程内真实 loopback socket 的受控 NAT 映射证据。
- 不改变 wire/protocol 契约、生产默认行为、默认构建产物、release/打包或非 p2p-frame 模块。
- 不要求六个 NAT 行全部完成 peer tunnel；对于 loopback 语义下必须借助 prediction 但暂不能绑定/送达的行，允许以真实 `request-sent` + `action-armed` + 有界否定/fallback 结果收口，但每行至少真实执行生产计划与 rendezvous/action，并断言实际操作与结果边界。
- 不修改 `cyfs-p2p-test/**`，不引用其 binary/日志/场景作为证据。
- 不删除或改写 033/034 已批准 packet；本 sibling task 只取代其矩阵部分被 P1 否定的证据要求，033 的原始公网/真实 NAT punch 目标及其 blocking 审计保持记录。

### Boundary with neighboring modules

- production seam 仅限 `p2p-frame/src/endpoint.rs`（或等价 predicate）及消费该 predicate 的 `tunnel_manager.rs`、`sn/protocol/sn.rs`、`networks/quic/**`；无 feature 时编译结果与行为不变。
- fixture 与矩阵用例只落在 `p2p-frame/tests/real_p2p_tunnel_flow/**`；SN、TunnelManager、QUIC/TCP 全部使用生产入口，不替换 listener、不预注册 tunnel、不调用私有 handler。
- 测试反射器/relay 只模拟地址映射与转发，不复制 TunnelManager、SN 或 QUIC 决策。

## Requirement Review

该 P1 结论成立：当前测试把“硬编码期望”当成覆盖，且 profile 缺失时生产路径直接进入 legacy，六行无一真正执行生产矩阵。033 audit（`.harness/pipelines/v0.1/p2p-frame/033-real-p2p-tunnel-socket-tests/implementation-audit.md`）已明确列出选项 1：允许窄范围 test-only production seam 使 loopback ServerReflexive 地址进入真实 rendezvous/punch 路径；本任务选该项并补充一个保持“同一逻辑 source socket + 每目标独立映射”语义的真实 socket NAT mapping fixture，避免 033 的不可执行结论再次阻断。

材料取舍：为换取矩阵真实可达，必须引入 feature-gated 判定 seam 与 relay-based NAT simulation；代价是 production 源码出现测试面（默认关闭）和更高实现复杂度。作为平衡，方向选择为：

- 所有 profile 经真实 probe/Report/Query 链生成，避免直接注入 plan。
- 全部 socket 为 OS 真实 socket，relay 只做映射转发。
- 分支证据来自生产结构化事件 + connection info + payload；不把日志当作连接成功证据。
- 六行中仅要求明确 loopback 可达的代表行完成 payload，其余行以真实到达的 action 边界和有界结果收口。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-040-1 | CHG-040-nat-matrix-test-seam | feature-gated loopback/private ServerReflexive 资格 seam，覆盖 rendezvous 候选、SN wire 校验、QUIC punch/prediction 判定 | 仅 `p2p-frame` 测试 feature 生效；默认构建判定字符串与行为不变；不改变 wire 格式 | 生产源码增加测试 seam，换取生产矩阵真实可达 | 默认 feature 关闭时同一 predicate 输出与现状一致（编译期/单元断言）；feature 打开时六行均越过 pre-request 校验到达 request-sent | 不引入运行时 env 开关，不影响 release 产物 |
| P-040-2 | CHG-040-nat-matrix-fixture | 受控 NAT reflector + relay fixture：每个节点单一逻辑内部 socket，SN probe 经真实 reflector 观察到 Stable/Changed 映射与有效 prediction hint，映射端口由真实 relay 转发 | fixture 全部在 `p2p-frame/tests/real_p2p_tunnel_flow/**`，使用动态端口/绝对 deadline/RAII；profile 必须经真实 probe/Report/Query 链 | relay 增加数据面真实性与端口占用；返回确定性映射与 prediction | `query_with_context` 返回预期 observation/endpoint/hint；SymmetricLike 证明同一 socket 多目标映射变化；双向唯一 payload 可重复 | 不直接注入 profile/context，不用 mock socket 或固定 sleep |
| P-040-3 | CHG-040-nat-matrix-execution | 六行真实执行生产 `NatConnectPlan` → `open_rendezvous_tunnel` → SN wire → 目标 action，并断言 operation/prediction/request-sent/action-armed 及可达行 connected/payload | 以 QUIC 为主；TCP 保留代表性公开/反连闭环；从 `connect_from_id` 生产入口发起，不替换 listener | 完整矩阵比 034 flow-only 边界更强且更耗时 | 每行捕获并断言 `event=sn_rendezvous_requesting`（含 operation/predict）与目标 action/完成事件及实际 operation；可达行完成 payload；矩阵串行可重复运行 | 不把 not-observable 当作正式六行输出；不以日志代替 payload；不宣称公网 NAT |

## Success Criteria

- Concrete system-visible result: 六个矩阵条件的真实用例从生产入口执行生产策略与 rendezvous 分支；SymmetricLike 证据使用同一逻辑 source socket 的逐目标映射变化；每个条件输出真实 `actual_selected`/`request_sent`/`action_armed`（可达行为 `connected`/`payload-complete`）。
- Required evidence: `UV_CACHE_DIR=.harness/uv-cache uv run --active python ./harness/scripts/test-run.py p2p-frame/040-execute-real-p2p-strategy-matrix all` 通过且可重复；矩阵用例至少一次串行完整运行与一次并发/重跑验证；每行 artifact 包含 profile、expected operation、actual selected、request-sent event、action-armed event、bounded result 和（如适用）payload 证据；default 构建 seam 关闭证据；testing coverage/scope、lifecycle 与 acceptance 检查通过。
- Explicit non-goals: 不声明公网/多机/真实路由器 NAT 或生产部署验证；不要求六行全部 peer tunnel；不修改生产协议或默认行为；不修改 `cyfs-p2p-test`；不把 033/034 已批准 packet 改写为已完成。

## Risks

- feature seam 泄漏到默认构建：需要 default-build 断言与 feature-gated cfg，并在本任务测试/验收阶段明确核对。
- relay 端口分配与 QUIC socket 绑定竞争：矩阵串行运行、动态端口 + 有限重试 + RAII guard，禁止固定端口/固定 sleep。
- 全局 log sink 跨用例污染：按 `remote_id`/correlation 过滤，矩阵用例单测或独立 correlation 上下文；事件只作分支证据。
- 真实 punch 在 loopback relay 上仍可能受生产 QUIC 收发时序影响：允许有界否定/fallback 结果，但必须在请求/action 已真实发生之后记录，不能把 pre-request 拒绝冒充 action-armed。
- 该任务同样可能存在真实环境和验收无法覆盖的组合缺口；若设计阶段发现需要超出本边界的行为，返回 proposal 修订并重新确认。
