---
task_manifest: task.yaml
status: approved
---

# 单次 SN 探测：rendezvous 失败后不再走 legacy 二次 SN 请求 Proposal

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- rationale: 改动集中在 p2p-frame 单模块的 `TunnelManager` 失败路径编排：移除 rendezvous 失败后的第二次 SN 请求、给失败分类、加一次 open 的总 deadline。这与 039（owner-guarded incoming subscribers，internal concurrency/lifecycle 修复归 standard）同型：内部并发/生命周期语义修复、无 wire/公开 API/数据/依赖/发布/安全面影响，targeted verification 可用；不满足 trivial（涉及并发/生命周期/runtime 集成与时限），也无 high-risk 的具体触发证据。若用户希望使用 high-risk 暂存生命周期，可在确认时替换。
- triggered_boundaries: 若测试/验证阶段发现需要修改 `SnTunnelRendezvousResp` 失败载体（携带原因码）才能满足确定性跳过的验收，属于需求范围变更，需回到 proposal 复核；本提案默认不改 wire。
- confirmation_statement: 用户于 2026-09-02 回复“确认”，接受三个推荐默认（Failed 按确定性直进 proxy、歧义保留 action-only 重试一次、总 deadline 一并实现）与 standard tier。确认后已更新 workflow_tier/task.yaml 的 baseline_manifest 并置 status: approved，按 standard 流程执行。

Risk profile: not-created (replace with ./risk-profile.yaml only after high-risk confirmation)

## Background and Goal

当前 `open_tunnel_from_id` 在入口通过 `DefaultDeviceFinder::get_peer_info`（即 `sn_service.query_with_context`，`p2p-frame/src/tunnel/device_finder.rs`）做一次 SN query 定位对端，得到 endpoints、NAT profiles 与 serving SN id；`open_tunnel_from_lookup` 把这份 `PeerLookupInfo` 同时喂给 rendezvous 与 legacy 两个流程，这是设计上唯一的 SN 探测点。

问题在于：`open_nat_aware_tunnel`（`p2p-frame/src/tunnel/tunnel_manager.rs`）在 `open_rendezvous_tunnel` 失败后无条件进入 `open_nat_aware_tunnel_legacy`，后者仍通过 `call_via_sn` 又向 SN 发起一次请求（与本地 action 并行，默认 `sn_call_timeout` 10s）。叠加 SN 侧 `process_rendezvous_request` 的 10s deliver 上限、客户端 `call_timeout` 与本地 action 的 `conn_timeout`，最坏链路可达 rendezvous（~10s）→ legacy SN call（~10s）→ 本地 action（~5s）→ proxy，量级二十几秒——同一 open 对 SN 做了不止一次请求。

目标：一次 open 的 SN 交互收敛为入口的这一次 query，rendezvous 与 legacy 都只消费 query 结果；rendezvous 失败后的 legacy 回退不再发出 `call_via_sn` 或任何新 SN 请求；失败路径的等待时间被确定性分类和总 deadline 约束。

## Scope

### In scope

- SN 交互收敛为单点：入口 `DefaultDeviceFinder::get_peer_info`（`sn_service.query_with_context`）是唯一 query；rendezvous 与 legacy 回退都消费该 `PeerLookupInfo` 派生的 endpoints/NAT profiles/SN id/context。
- `open_nat_aware_tunnel` 的 rendezvous 失败分支不再调用 `call_via_sn` / 不再进入带 SN call 的 legacy 回退。
- 对失败原因做确定性分类：
  - 确定性失败（目标未 arm / 不会再 arm）：SN 未激活、本地无 rendezvous endpoints、owner 冲突（`Conflict`/`AlreadyExists`）、`NotSupport`、本地请求/预测校验或编码失败（`RawCodecError`/`Expired`/`ErrorState` 等）、SN 返回的失败响应（`Failed`，含 SN 侧 TargetNotFound/begin 冲突/缓存冲突等不可区分原因）以及响应成功后的本地 action 失败/超时；直接进入 proxy，不再运行本地 action 重试。
  - 歧义失败（目标可能已被 arm）：传输 IO 错误、命令超时（`IoError`）、响应校验异常（`Unmatch`/`InvalidData`）；只重试本地 caller action（复用本次 rendezvous 的 tunnel_id 与 incoming waiter），重试期间不向 SN 发任何请求，失败后进入 proxy。
- 一次 `open_nat_aware_tunnel` 的总 deadline：覆盖 rendezvous + 歧义重试 + proxy，预算由 SN `call_timeout` 与 `conn_timeout` 派生（默认约 `sn_call_timeout + 2 * conn_timeout`，即 20s），各阶段共享剩余预算；本地 action 不再在确定性失败后空等到 `conn_timeout`。
- 回归测试证明：单次 SN 探测；确定性失败路径跳过本地 action 直接 proxy；歧义路径仅一次本地 action 后 proxy；总 deadline 生效。

### Out of scope

- 不修改 `SnTunnelRendezvousResp` 等 wire 结构，不为失败响应追加原因码；SN 侧 TargetNotFound 等与 timeout 在客户端仍统一为 `Failed`，按本提案的确定性分类处理。
- 不修改 SN 侧 `process_rendezvous_request` 的 10s deliver 上限、rendezvous 缓存/领导/owner 语义。
- 不改无 rendezvous plan 分支（`open_nat_aware_tunnel_legacy` 的保留入口）本身的 call+action 并行语义；该分支不在 NAT-aware 双探针链路内，其取消语义如需一并修复另立任务。
- 不改 `open_direct_path`/punch/proxy 的核心建立语义，不改公开 API 与配置项（deadline 由内部派生，不新增配置字段）。
- 不整理或归属工作区中其他既有未提交改动（harness refresh、030/031 等）。

### Boundary with neighboring modules

- `p2p-frame/src/sn/client/sn_service.rs`：rendezvous/SN call 命令通道语义不变；如实现需要，仅允许新增只读 `call_timeout()` 之类的内部访问或等价注入，不改 `call_via_sn` 行为。
- `p2p-frame/src/sn/service/**`：交付、缓存、冲突与 10s 上限全部保持不变。
- `p2p-frame/src/networks/**` 与 `p2p-frame/src/pn/**`：tunnel 匹配、punch、proxy 行为不变；本任务只改变 `TunnelManager` 对失败路径的编排。

## Requirement Review

需求合理：双份 SN 探测既增加最坏延迟，也在确定性失败上浪费一个并列的 call 和本地等待；去掉第二份探测符合"目标是否 arm"的事实——SN 已给出终结性失败响应时，重发请求不会改变安排结果。total deadline 将"不确定到底多久"变成可分阶段、可测试的有界行为。

选择的权衡：
- SN 响应为 `Failed` 时按确定性失败处理（直接 proxy）。这同时覆盖了 wire 上不可区分的 TargetNotFound/conflict 等"不 arm"情形，代价是 SN 侧 10s 送达超时（目标可能已被 arm）不再获得本地 action 重试；proxy 仍能恢复连接，且总延迟更短。若后续需要区分"SN 已拒绝"与"SN 超时"两类失败，需要扩展失败响应载体，属于后续独立任务。
- 歧义失败仅重试本地 action（不重发 SN 请求），这正是"目标可能已 arm"时能继续受益的并行侧；复用 rendezvous tunnel_id/waiter 使重试有机会匹配对端已布置的反向/matched 连接。
- deadline 预算派生自现有 `sn_call_timeout` 与 `conn_timeout`，不新增配置，避免公开契约变化；实现保证 rendezvous 阶段至少获得完整 `sn_call_timeout`。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-SNPROBE-1 | single_sn_probe_no_legacy_reprobe | SN 交互收敛为入口单次 query（`DefaultDeviceFinder::get_peer_info` / `sn_service.query_with_context`）；rendezvous 与 legacy 回退都只消费该 `PeerLookupInfo`（endpoints、SN id、NAT profiles、context）；rendezvous 失败后的 legacy 回退不再发起 `call_via_sn` 或任何新 SN 请求 | 不新增/移动 query 点；query 缓存 TTL、cert cache 与 `open_known_tunnel`/proxy 兜底语义不变；只改 `open_nat_aware_tunnel` 失败分支编排 | 放弃双通道冗余，换取单点 query + 单次安排请求与确定的最坏延迟 | 回归用例断言一次 open 恰好一次 SN query、rendezvous 失败后无第二个 SN 请求，且最终仍回到 proxy/成功 | 不改 query wire、不改 query 结果缓存格式，不把两个流程各自的 action 语义合并 |
| P-SNPROBE-2 | rendezvous_failure_classification | 确定性失败直接进入 proxy（不跑本地 action 重试）；歧义失败仅重试一次本地 caller action（复用 query 得到的 endpoints/profiles、rendezvous tunnel_id/waiter）后进 proxy | 失败码集合固定并注明；本地 action 共享总 deadline 剩余预算 | Failed 视为终结性响应，换取 TargetNotFound/conflict 等场景不再空等 | 分类单测 + 路径级用例证明确定性路径无 punch/等待、歧义路径恰好一次 action 且无新的 SN 请求 | 不改变 punch/proxy 或 wire 编码；不区分 SN 拒收与 SN 超时的具体原因 |
| P-SNPROBE-3 | open_attempt_total_deadline | 一次 open 有总 deadline，覆盖 query→rendezvous→歧义重试→proxy 的探测与建立链路，预算由 `sn_call_timeout` 与 `conn_timeout` 派生（默认约 `sn_call_timeout + 2 * conn_timeout`），各阶段共享剩余预算 | 只作用于本次 open 的 NAT-aware 建立链路，不新增配置项；现有 conn_timeout/sn_call_timeout 语义保持 | 硬上限可能提前于某阶段的单独超时，但保证 query 与 rendezvous 各自至少拿到完整 `sn_call_timeout` | deadline 上限测试：SN query/rendezvous 无响应 + action 挂起 + proxy 慢时总时长仍被限定 | 不加新的可配置字段，不改 SN 侧 10s 上限 |
| P-SNPROBE-4 | single_probe_regression_tests | 新增/调整测试证明：一次 open 恰好一次 SN query（入口），rendezvous 失败后无第二个 SN 请求；确定性失败路径跳过 action 直上 proxy；歧义路径 action-only 重试一次后 proxy；总 deadline 生效；既有 rendezvous/legacy/proxy 用例保持通过 | 仅修改 p2p-frame 测试与最小辅助；使用既有 mock/真实 socket 家族 | 以命令计数/日志断言请求次数，避免只靠持续时间推断 | 点名的 task 测试与 unified runner 证据（`UV_CACHE_DIR=.harness/uv-cache uv run --active python ./harness/scripts/test-run.py p2p-frame/042-single-sn-probe-no-legacy-reprobe all`） | 不新增合成计数器测试冒充生产行为，不引入固定 sleep 依赖 |

## Success Criteria

- Concrete system-visible result: 一次 open 在入口只做一次 SN query（`get_peer_info`），rendezvous 与 legacy 均消费同一 `PeerLookupInfo`；rendezvous 失败后不再出现第二次 SN 请求；确定性失败直接进 proxy；歧义失败仅本地 action 重试一次（复用 query 得到的 endpoints/profiles 与 rendezvous tunnel_id）；整个 SN 探测与建立链路被总 deadline 约束。
- Required evidence: 实现后的调用链 diff 与测试证据：单探针断言、确定性/歧义两条路径断言、总 deadline 上限断言，以及点名既有测试集保持通过；写明循环回环（loopback）测试不等于公网 NAT/跨 SN 证据。
- Explicit non-goals: 不改 wire failure 载体、不新增配置项、不扩展 SN 侧超时/缓存、不改变 legacy 保留分支并行语义、不把本任务成果误写为已部署多 SN/公网证据。

## Risks

- `Failed` 作为确定性失败会让"SN 已送达但响应超时"场景跳过本地 action 重试；接受代价（proxy 仍可恢复、总延迟更低），并作为显式 tradeoff 记录。若用户希望保留该场景，可把 `Failed` 改为歧义分类（仍不改 wire），需在确认时说明。
- action-only 重试复用 rendezvous tunnel_id/waiter 时，若 owner/waiter 清理与新 registration 顺序不正确，可能挂起或误匹配；design/测试必须覆盖 stale completion、owner token 与 incoming waiter 复用（沿用 026 owner-token 语义）。
- 总 deadline 派生预算若与运行时配置比例不一致，可能在慢速但正常的路径上提前取消；实现保证 rendezvous 完整预算并让共享 deadline 只收紧而不放宽阶段超时。
- 工作区存在大量既有未提交修改；lower-tier/high-risk baseline 与 stage-scope 证据将绑定本任务 manifest，避免把无关改动归入 042。

## Approval Record

- approver:
- approval_date:
- user_statement: ""
