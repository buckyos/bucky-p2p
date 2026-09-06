---
task_manifest: task.yaml
status: approved
---

# Tunnel 建立时 NAT 穿透逻辑审查

Risk profile: not-created

## Workflow Tier Judgment
- Proposed tier: trivial
- Final tier: trivial
- Tier rationale / triggered boundaries: 本任务只读审查当前工作区代码并运行必要的已有定向测试，交付缺陷结论；不改变协议、生产行为或测试代码。检查对象涉及网络、并发、超时和身份授权，但审查本身不引入这些行为风险。
- Proposal and tier confirmation: 用户于 2026-09-06 回复“确认”，批准所展示提案及 trivial tier。

## Background and Goal
用户要求检查 tunnel 建立时的 NAT 穿透逻辑是否正确。以当前工作区（包含已有未提交修改）为准，追踪真实调用链，主动寻找导致穿透失败、状态竞态、错误回退或端点授权失效的反例。

## Scope
### In scope
- TunnelManager 建链入口、NAT profile 可用性及策略分支。
- NAT 类型组合对应的主动连接、反向连接、打洞与等待动作。
- SN rendezvous 请求、通知、响应及同 SN/跨 SN 关联和端点授权。
- QUIC socket 复用、预测候选、有效期、绑定代际及打洞时序。
- 入站 waiter、并发 owner、取消和超时清理；rendezvous 到 legacy SnCall 到 PN 的回退。
- 查看已有测试覆盖，必要时运行已有定向测试，区分静态证据、运行证据与未覆盖场景。
### Out of scope
- 不修复生产代码，不新增或调整测试，不提交 Git commit。
- 不部署公网环境或将本地测试作为真实公网 NAT 穿透成功的证明。
### Boundary with neighboring modules
主归属 p2p-frame；可只读检查调用方及直接依赖以核实运行语义，保留既有工作区修改。

## Requirement Review
请求合理。先按当前实现判断正确性，不能仅依赖历史审查或旧任务结论。缺陷需要具体触发条件、执行分支、影响和代码位置；无法证明的风险单独列为证据缺口。发现问题后交付审查结果，修复另行授权。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-review-nat-traversal | 审查 NAT 穿透完整建链逻辑并报告可验证缺陷 | 当前工作区及直接依赖 | 优先调用链与反例，验证按需定向 | 严重性、触发条件、影响、源码位置、测试及证据边界 | 不实施修复 |

## Success Criteria
- Concrete user-visible or system-visible result: 中文审查结论，以严重性排序的问题为先，说明正确路径与剩余不确定性。
- Required evidence: 每项缺陷提供当前源码位置和因果链；记录实际执行的定向测试结果或未运行原因；写任务内 completion-report.md 并完成 trivial 对应检查。
- Explicit non-goals: 不承诺所有真实 NAT 拓扑均可穿透，不修改当前实现。

## Risks
- 工作区已有未提交的 NAT 探测及测试修改，必须审查当前实际内容。
- 回环测试无法覆盖公网映射、过滤与跨 SN 部署差异。
- 静态发现的可疑点可能受上游校验或下游生命周期约束消除，报告前需追踪完整链路。
