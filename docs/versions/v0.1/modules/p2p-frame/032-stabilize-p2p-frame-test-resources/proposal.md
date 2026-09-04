---
task_manifest: task.yaml
module: p2p-frame
task_name: 032-stabilize-p2p-frame-test-resources
submodule: 032-stabilize-p2p-frame-test-resources
version: v0.1
status: approved
approved_by: user
approved_at: 2026-09-01T16:54:15+08:00
approved_content_sha256: 7f58837faaa199ec8f541842cead8b60f80e5d2de437c05fead59f7d57ab681b
---

# 稳定 p2p-frame 网络测试资源与 readiness 同步

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 两个失败均位于 `p2p-frame` 专用测试面，预期只修正测试同步与本地监听资源冲突恢复，不改变公开协议、生产运行时、依赖、构建图或跨项目行为；改动跨 PN/TTP 与 SN 两组测试路径，故采用 bounded single-project bugfix 的 standard tier，而不是 trivial。
- Proposal and tier confirmation: confirmed by user statement `确认，按 standard 完成`

## Background and Goal

当前宽测试运行暴露两个独立的稳定性缺陷：

1. PN reverse TCP 真实路径测试在 `TtpServer::has_cached_tunnel_for_test` 的 5 秒轮询中超时。task 029 已消除旧观察器的 destructive lookup，但当前布尔观察仍把“已 attach/cache”“目标匹配”“tunnel 已进入 production 可用状态”合并成一个不可诊断条件；失败时无法判断真实生命周期停在哪个边界，也没有可靠的事件前置关系。
2. `sn_report_late_response_does_not_complete_next_tunnel_report` 从进程内 `AtomicU16` 顺序分配固定 localhost 端口。该计数器不向 OS 预留端口，也不避开其他测试进程；端口 42050 被占用时，TCP listener 将 bind 失败映射为 `AlreadyExists`，测试在业务断言前退出。

目标是让两项测试只依赖真实生命周期和可恢复的本地资源分配：PN 用例在唯一 proxy request 前得到可诊断的 attach/readiness 前置关系；SN 用例遇到本地 bind 冲突时重建整套隔离拓扑，而不是把某个固定端口视为可用。

## Scope

### In scope

- 将 PN reverse TCP 测试的 cache/connection readiness 观察拆成可诊断状态，并使用真实 attach/cache 与 tunnel 状态变化建立前置关系。
- 保持 PN 用例只发送一次 `ProxyOpenReq`、关闭 B 的直接 data listener，并继续验证真实 reverse TCP fallback 的双向字节。
- 为 `sn_report_late_response_does_not_complete_next_tunnel_report` 使用 OS 协调的动态端口选择和/或仅针对 bind 冲突的 bounded whole-topology retry。
- 增加或调整专用回归覆盖，证明端口冲突不会被当作 SN QA correlation 失败，且非 bind 错误不会被重试掩盖。
- 通过 task-scoped 命令重复运行两个点名失败用例，并保留并发/串行覆盖。

### Out of scope

- 不延长 PN 5 秒 deadline，不增加固定 sleep、proxy request 重试或 target-open 重试。
- 不改变 production TTP lookup/cache、TCP state promotion、PN relay、SN QA correlation 或 TCP listener 错误映射语义。
- 不把 `AlreadyExists` 无条件视为可忽略错误；恢复逻辑必须限定在测试拓扑建立时可证明的 bind conflict。
- 不修改已完成的 task 029、旧 `sn-client-qa-correlation-fix` packet，或整理工作区中的其他既有改动。

### Boundary with neighboring modules

- `p2p-frame/src/ttp/**` 与 `p2p-frame/src/pn/**` 的生产行为保持不变；允许增加或修正 `#[cfg(test)]` 只读/通知观察面。
- `p2p-frame/src/sn/tests.rs` 只负责测试拓扑和资源生命周期，不改变 `sn/client`、`sn/service` 或 `networks/tcp/listener.rs` 的生产契约。
- `cyfs-p2p-test` 不参与测试实现或正式证据。

## Requirement Review

用户提供的两个 panic 都发生在被测业务断言之前，修复测试基础条件是合理的。简单增加 timeout 不能区分 PN attach、cache、target match 与 Connected 状态，也不能修复已终止的 tunnel；顺序递增固定端口同样不能保证跨进程可用。选择最小方向：PN 暴露只读、事件可等待且状态可诊断的测试边界；SN 在测试层按完整拓扑重建处理 bind 冲突，并保持其他错误立即失败。若后续证据证明 PN 的 production tunnel 在没有测试观察时也无法进入可用状态，或 TCP listener 的错误分类必须成为公开修复，本任务应回到 proposal 扩大范围，而不能静默修改生产语义。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-032-1 | pn_reverse_tcp_readiness_synchronization | PN 真实 reverse TCP 测试必须在唯一 proxy request 前确定目标 tunnel 已 attach/cache 且进入 production 可用状态；超时必须报告最后观察到的具体生命周期状态 | 仅测试代码和 `#[cfg(test)]` 观察面；不推进状态、不打开 stream、不改变 production cache/lookup | 增加窄测试诊断状态以消除不可诊断布尔轮询，换取稳定的 happens-before 与失败定位 | 点名真实 PN 用例和既有并发压力用例重复通过；仍验证一次请求、关闭直接 listener 与双向字节 | 不通过延长 deadline、sleep 或请求重试掩盖问题 |
| P-032-2 | sn_test_bind_conflict_recovery | SN late-response 回归必须在端口被其他进程占用时重选资源并重建完整拓扑，且只重试可证明的 bind conflict | 仅 `sn/tests.rs` 测试资源分配/启动辅助；生产 listener 和 SN QA 行为不变 | bounded retry 会增加冲突环境中的 setup 次数，但避免固定端口把环境噪声误判为业务失败 | 预占候选端口的 red/green 回归或等价确定性注入通过；点名 late-response 用例重复通过；非 bind 错误仍立即返回 | 不吞掉任意 `AlreadyExists`，不修改 production error code |

## Success Criteria

- Concrete system-visible result: 两个用户点名用例在同一工作区的重复/并发运行中不再分别以 readiness `Elapsed(())` 或 localhost `AddrInUse` 退出。
- Required evidence: 两个缺陷各有 red/green 证据；PN 精确用例及并发压力用例通过；SN late-response 精确用例在端口冲突覆盖后通过；通过 standard task 的 targeted verification 与独立 completion review。
- Explicit non-goals: 不声明已执行全工作区测试、真实多机 PN/SN、公开 NAT 或部署环境验证；不修改生产协议和错误契约。

## Risks

- 测试观察面若主动推进 tunnel 状态会制造假成功；它必须只读或等待真实 owner 发出的变化。
- 将所有 `AlreadyExists` 都重试会隐藏重复注册等真实缺陷；重试条件必须绑定到 listener bind 的具体错误链或由测试确定性控制的冲突分支。
- 当前工作区包含大量既有未提交修改；后续 standard baseline 和 changed-path manifest 必须把本任务改动与既有内容分离。

## Approval Record

- approver: user
- approval_date: 2026-09-01T16:54:15+08:00
- user_statement: "确认，按 standard 完成"
