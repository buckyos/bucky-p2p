---
task_manifest: task.yaml
status: approved
---

# 对齐 SN 真实网络 Query 测试与 Loopback 策略 Proposal

Risk profile: not-created

## Workflow Tier Judgment

- Proposed tier: trivial
- Final tier: trivial
- Tier rationale / triggered boundaries: 失败来自既有 integration test 未同步任务 053 已确立、任务 057 已在相邻测试采用的 feature 边界。拟议修复只调整一个现有测试函数的条件断言，不修改生产逻辑、公开协议、安全边界、依赖、运行时生命周期或发布行为。
- Proposal and tier confirmation: 用户于 2026-09-04 回复“确定”，批准所展示的 proposal 与 `trivial` tier；无未决问题。
- Post-confirmation evidence refinement: 默认完整矩阵证明 TCP 监听使用 unspecified 地址并发布由本机非 loopback 地址展开的有效 TCP endpoint，只有绑定 loopback 的 QUIC observation 在默认策略下为空；因此在同一已批准范围内保留 TCP 原正向断言，并只对 QUIC 使用 feature 正反分支。

## Background and Goal

用户报告 `sn_protocol_real_network_quic_matrix` 在 `exercise_query` 中因已注册 target 的 `end_point_array` 为空而失败。该拓扑使用 `127.0.0.1`；当前 `SnService::sanitize_reported_endpoints` 在默认构建中会过滤 loopback，只在显式启用 `test-real-socket-matrix` 时允许测试拓扑保留 loopback observation。任务 057 已让 crate 内同类 query 测试区分这两个 feature 边界，但漏掉了独立的 `sn_protocol_real_network.rs`。

目标是在不放宽生产端点净化策略的前提下，使真实网络 TCP/QUIC integration matrix 对默认构建和 opt-in loopback matrix 分别断言正确结果。

## Scope

### In scope

- 在默认 `x509` 构建下继续验证 query 返回正确 target identity 和协议版本；QUIC 断言 loopback observation 被过滤为空，TCP 保留有效非零端口 endpoint 的原正向断言。
- 在 `x509,test-real-socket-matrix` 构建下保留已注册 target 返回有效 QUIC loopback endpoint 的正向断言，并继续验证 TCP endpoint。
- 定向运行用户报告的 QUIC matrix，并覆盖默认与 opt-in feature 两侧；必要时同时运行 TCP matrix，确认共享 `exercise_query` 没有传输特定回归。

### Out of scope

- 不修改 `SnService::sanitize_reported_endpoints`、`get_peer_observed_ep`、Report/Query wire format 或任何生产行为。
- 不修改其他历史任务文件或清理当前脏工作区。
- 不把 loopback 真实 socket 测试表述为公网 NAT、跨主机或部署环境证据。

### Boundary with neighboring modules

变更限定在 `p2p-frame/tests/sn_protocol_real_network.rs` 的 query 结果断言；生产 `p2p-frame/src/sn/**` 只作为只读契约依据。

## Requirement Review

直接恢复生产代码对 loopback 的接受会回退任务 053 的安全边界。删除非空断言也会丢失 opt-in matrix 的正向覆盖。合理修复是复用任务 057 已验证的双 feature 模式：默认路径明确断言过滤结果，opt-in 路径继续证明真实本机 QUIC socket 注册端点可被 query 返回。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-058-QUERY | CHG-align-sn-real-network-query-loopback | 让真实网络 query matrix 按当前 transport/feature 策略验证已注册 peer：默认 QUIC endpoint 为空，opt-in QUIC 返回有效 loopback endpoint；TCP 在两侧均保留有效非零端口 endpoint，同时保留 identity、协议版本和 unknown-peer 断言。 | 只修改 `exercise_query` 的条件断言。 | 默认构建不再要求发布 QUIC loopback endpoint，但 opt-in 构建仍保留正向证据；TCP 原覆盖不弱化。 | 用户提供及本任务复现的 panic 作为 red evidence；默认 `x509` QUIC/TCP matrix 与 opt-in feature QUIC/TCP matrix 通过；scoped diff/check 无生产改动。 | 不放宽 endpoint 安全策略，不改变协议或真实网络拓扑。 |

## Success Criteria

- Concrete user-visible or system-visible result: 用户报告的默认 `x509` QUIC matrix 不再在第 323 行失败；默认与 opt-in QUIC 分别验证过滤/允许 loopback，TCP 在两侧均保留原 endpoint 正向覆盖并通过。
- Required evidence: 用户提供的现有失败；修复后四个定向 feature/transport 组合，或等价且不弱于该覆盖的命令；任务范围 diff 检查；完成前独立比例缺陷复核。
- Explicit non-goals: 不修改生产逻辑，不扩展测试到公网 NAT/跨主机，不处理当前工作区中的其他改动。

## Risks

主要风险是让测试变绿却丢失 endpoint 发布的正向覆盖，或误把 QUIC 的 loopback 规则套用到由 unspecified listener 展开本机地址的 TCP 路径。修复必须让默认 QUIC 反向断言为空，让 opt-in QUIC 对协议、loopback 地址和非零端口做正向断言，并让 TCP 在两个 feature 边界都继续验证协议与非零端口；当前无未决需求问题。
