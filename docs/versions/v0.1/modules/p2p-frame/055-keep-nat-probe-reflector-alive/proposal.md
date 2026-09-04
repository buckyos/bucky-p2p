---
task_manifest: task.yaml
status: approved
---

# 保持 NAT Probe Reflector 存活 Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 该缺陷改变生产 SN 后台 UDP reflector 的错误恢复与生命周期语义。一次临时发送错误当前会永久移除一个已发布的 probe 端口，影响后续 NAT 分类，属于实质性的 runtime/lifecycle 可用性边界。
- Proposal and tier confirmation: 用户于 2026-09-04 回复 `确认，自动完成`，确认 proposal、high-risk tier，并授权从 design 起自动完成后续阶段。

## Background and Goal
`NatProbeReflector::run` 已经把单个请求的解码、签名限流和签名失败视为请求级结果，但 `send_to` 的任意错误会通过 `?` 结束整个循环。外层 SN 服务任务只记录 reflector 停止，不重启任务或重新绑定端口。因此一次临时 `ENOBUFS`、路由或接口发送错误即可让该端口持续失效，直到 SN 服务整体重启。

目标是把 UDP 响应发送失败限定在当前数据报：记录错误、丢弃当前响应并继续接收后续请求，使一次临时错误不会永久终止 reflector。

## Scope

### In scope
- 在 `NatProbeReflector::run` 中显式处理 `send_to` 错误，记录包含目标地址和错误内容的诊断日志，然后继续循环。
- 增加可控的回归测试：第一次响应发送失败后，reflector 任务仍存活并能成功响应后续合法请求。
- 保持已有固定包长、签名、限流、请求校验和 socket 所有权语义。

### Out of scope
- 不为 reflector 增加服务级自动重启、socket 重新绑定或新的 supervisor。
- 不改变 `recv_from` 错误的终止语义；持续接收失败若无退避地继续可能产生忙循环。
- 不改变 PNAT wire format、身份签名、端口发布、NAT 分类或客户端探测策略。
- 不声称本机注入测试覆盖真实公网 `ENOBUFS`、接口撤销或路由故障。

### Boundary with neighboring modules
请求级发送恢复由 `p2p-frame/src/sn/nat_probe.rs` 拥有。`SnServer::start_nat_probe_reflectors` 仍只负责启动、持有和停止 reflector 任务，不增加重启策略；测试放在现有 NAT probe 独立 unit test 文件中。

## Requirement Review
“单次 UDP 发送错误不要永久终止 reflector”是合理的可用性要求。最小且更稳妥的处理是把发送失败视为该请求的失败，而不是在外层重启整个 reflector：UDP 本身不承诺交付，重建 socket 还会引入端口重绑失败、任务重复和状态监督复杂度。接收失败仍向外返回，避免永久 socket 故障造成无退避的错误自旋。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-keep-nat-probe-reflector-alive | 单次 NAT probe UDP 响应发送失败只丢弃当前响应，reflector 继续服务后续请求。 | 仅改变 `NatProbeReflector` 的发送错误分支；接收错误、SN 外层任务和协议语义保持不变。 | 持续发送故障期间会继续处理请求并产生诊断日志，但不会因一次临时错误永久丢失端口。 | 红绿回归测试可控注入首个发送错误，证明 `run` 未结束且第二个合法请求收到可验证的签名响应；目标 x509 unit 测试通过。 | 不实现 supervisor、socket 重绑、无限重试当前响应或协议变更。 |

## Success Criteria
- Concrete user-visible or system-visible result: 一次 UDP `send_to` 错误后，对应 NAT probe 端口仍能处理并响应下一次合法请求。
- Required evidence: 与 `CHG-keep-nat-probe-reflector-alive` 绑定的红绿回归测试；定向 x509 测试通过；复核发送失败不会结束 `run`，且接收错误语义未被放宽。
- Explicit non-goals: 不改变协议、签名/限流、服务启动停止、接收错误处理或公网部署配置。

## Risks
- 错误处理必须位于实际 `send_to` 调用点，不能用只测试辅助函数的断言替代循环存活证据。
- 测试注入必须仅用于确定性触发一次发送失败，不得改变非测试构建的 socket 行为。
- 持续发送失败可能产生重复警告；本任务优先保留可观测性且不主动重试同一数据报，避免额外流量和忙循环。

## Approval Record
- approver: user
- approval_date: 2026-09-04
- user_statement: `确认，自动完成`
