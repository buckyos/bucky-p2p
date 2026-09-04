---
task_manifest: task.yaml
status: approved
---

# 对齐 SN Loopback 测试与端点安全策略 Proposal

Risk profile: not-created

## Workflow Tier Judgment

- Proposed tier: trivial
- Final tier: trivial
- Tier rationale / triggered boundaries: 四个失败均来自既有真实 socket 测试仍假设默认构建会发布 loopback observation；任务 053 已明确生产默认过滤该地址，仅 `test-real-socket-matrix` feature 允许测试拓扑使用 loopback。修复只调整现有测试的 feature 预期/启用条件，不改变生产逻辑、公开协议、安全边界、依赖或运行时生命周期。
- Proposal and tier confirmation: 用户于 2026-09-04 回复“确认”，批准所展示的 proposal 与 `trivial` tier；无未决问题。

## Background and Goal

用户报告四个 SN 测试失败：两个 query 测试期待非空 QUIC endpoint，两个 rendezvous response-owner 测试期待 serving-SN observation。当前 `SnService::sanitize_reported_endpoints` 会丢弃 loopback 上报地址，`SnService::get_peer_observed_ep` 在默认构建中也会过滤 loopback，仅在显式启用 `test-real-socket-matrix` 时保留。这是任务 053 的生产安全边界，而这些使用 `127.0.0.1` 的旧测试没有同步该边界。

目标是在不放宽生产端点净化和 ownership 校验的前提下，让默认 `x509` SN 测试与 opt-in loopback matrix 测试各自断言正确行为。

## Scope

### In scope

- 默认 `x509` 构建下，query 测试继续验证已注册 peer identity，但接受 loopback endpoint 被策略过滤为空。
- `test-real-socket-matrix` 构建下，保留 query 返回 QUIC loopback endpoint 的正向断言。
- 仅在 `test-real-socket-matrix` 构建中运行依赖 serving-SN loopback observation 的两个 rendezvous response-owner 测试，保留其 predicted-port 接受与混合未拥有 prediction 拒绝语义。
- 定向运行用户报告的默认-feature 回归，并运行 opt-in feature 下的对应测试。

### Out of scope

- 不修改 `sanitize_reported_endpoints`、`get_peer_observed_ep` 或 rendezvous ownership 的生产策略。
- 不允许默认生产构建把 loopback 当作可发布公网端点或 serving-SN ownership 证据。
- 不修改 wire format、NAT probing、SN report/query/call 运行时流程，也不清理当前脏工作区中的其他任务改动。

### Boundary with neighboring modules

变更限定在 `p2p-frame` 的现有 SN 测试与同一测试模块 include 的 rendezvous 测试文件。生产 `sn::service`、endpoint policy 和 tunnel 逻辑只作为只读验证对象。

## Requirement Review

直接让生产代码重新接受 loopback 会回退任务 053 的安全修复，不适合作为测试修复。更合理的方向是显式区分两种编译边界：默认构建验证 loopback 被过滤，opt-in matrix feature 验证真实本机 socket 拓扑下的 endpoint/observation 与 response-owner 行为。这样既恢复默认测试集，也不丢失 feature-gated 正向覆盖。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-057-LOOPBACK | CHG-align-sn-loopback-tests | 让四个旧测试遵守默认过滤 loopback、opt-in feature 允许 loopback 的当前策略，同时保留 identity、query/call 和 rendezvous response-owner 覆盖。 | 仅修改两个现有测试文件的条件断言或 feature gate。 | 默认构建不执行依赖 loopback observation 的 response-owner 场景，但同一场景继续由显式 matrix feature 执行。 | 用户报告的四个默认 `x509` 失败不再出现；opt-in feature 下对应 endpoint/observation 断言通过；定向复核确认生产代码无改动。 | 不改变生产 endpoint admission、ownership 认证或协议行为。 |

## Success Criteria

- Concrete user-visible or system-visible result: `--features x509` 下用户报告的四个失败消失；`--features x509,test-real-socket-matrix` 下 loopback endpoint 与 serving-SN observation 场景仍实际执行并通过。
- Required evidence: 用户提供的四个 panic 作为 red evidence；默认 x509 精确/模块测试与 opt-in feature 精确测试作为 green evidence；完成前对变更、边界和回归副作用做独立比例复核。
- Explicit non-goals: 不宣称 loopback 测试证明公网 NAT、跨主机或部署环境行为；不修改任何生产安全或协议边界。

## Risks

主要风险是通过跳过断言掩盖生产回归。为避免这一点，默认路径要反向断言 endpoint 被过滤；只有本质上无法在默认策略下取得 serving-SN loopback observation 的两个场景才使用 feature gate，并要求在 opt-in feature 下实际运行。当前无未决需求问题。
