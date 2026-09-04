---
task_manifest: task.yaml
status: approved
---

# Wan/Mapped Rendezvous 反连支持 Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment

- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 改动会放宽默认生产构建的 rendezvous endpoint 接受域，直接影响 SN wire 请求/通知校验、NAT-aware tunnel 运行时分支与 direct/PN fallback 结果。虽然 endpoint 编码布局不变，但协议合法输入集合和生产连接行为发生变化，触发 contract/protocol 与 runtime/integration 边界，因此建议 high-risk。
- Proposal and tier confirmation: confirmed by user statement `确认，自动完成`; auto-pipeline launched from design.

## Background and Goal

当前 NAT-aware 计划在 caller 带静态公网 endpoint 时选择 Callee 执行 `ReverseConnectOnly`。但请求候选构造与 SN rendezvous 协议校验通过 `rendezvous_eligible_area` 只在默认生产构建中接受 `ServerReflexive`；`Wan`/`Mapped` 仅在 `test-real-socket-matrix` feature 下额外放行。SN 又会把与 peer 上报 endpoint 完全相同的公网观察地址标为 `Wan`，并可生成 `Mapped` endpoint。因此 caller-public 的合法公网 endpoint 会在生产路径被过滤为空或被协议拒绝，最终错误降级到 PN proxy；feature-gated real-socket matrix 没有暴露该差异。

目标：默认生产构建对非 LAN IPv4、非零端口的 `Wan` 与 `Mapped` endpoint 支持 rendezvous 反连，使 `ReverseConnectOnly`（以及包含反连动作的 rendezvous 操作）能把这两类 endpoint 传到执行方并通过协议校验；测试 feature 只用于放宽 loopback/private 地址，不再决定 `Wan`/`Mapped` area 是否合法。

## Scope

### In scope

- 调整 rendezvous endpoint area 资格：默认生产构建接受 `ServerReflexive`、`Wan`、`Mapped`，继续拒绝 `Lan`；所有 area 仍受非 LAN IPv4 与非零端口约束。
- 让 `TunnelManager` 的 base candidate/request endpoint 构造保留符合上述边界的 `Wan` 与 `Mapped`，使 caller-public 计划生成非空 `ReverseConnectOnly` 请求候选。
- 让 SN rendezvous request/notify 的协议校验接受符合上述边界的 `Wan` 与 `Mapped`，保持 endpoint 数量、同传输、QUIC punch、非零端口与去重约束。
- 复核共享 eligibility 在 UDP punch 路径的影响；若共享 helper 会使纯 punch 行为超出“支持反连”的需求，则拆分反连候选与 punch eligibility，保持 punch 原边界。最终设计必须显式记录该选择。
- 增加不启用 `test-real-socket-matrix` 时即可运行的 production-default 回归测试，分别覆盖 `Wan`、`Mapped` 的候选保留和协议接受，并保留 `Lan`、private/loopback、零端口、重复或不匹配 transport 的拒绝断言。
- 保留并运行 feature-gated real-socket caller-public matrix，证明真实 loopback socket 拓扑下仍走 `ReverseConnectOnly` 并建立 direct tunnel；明确该测试不等同于公网 NAT 或部署环境证据。

### Out of scope

- 不改变 endpoint wire 编码、`EndpointArea` 枚举值、SN command code、rendezvous message 字段或协议版本。
- 不放行 `Lan`、IPv6、unspecified/broadcast/multicast、私网/loopback生产地址或零端口。
- 不改变 NAT plan 的 caller/callee 选择、prediction 算法、SN 公网地址分类规则、legacy query 复用、deadline、owner/token 生命周期、PN 协议或 proxy fallback 本身。
- 不声称本地测试已经验证真实公网 NAT、跨主机、多 SN 部署或运营环境路由。
- 不整理、覆盖或归属工作区中既有的未提交修改。

### Boundary with neighboring modules

- `p2p-frame/src/endpoint.rs`：定义 rendezvous 地址与 area 的共享资格边界；feature 仅允许测试地址。
- `p2p-frame/src/tunnel/tunnel_manager.rs`：复用资格边界构造反连请求候选，不改变 NAT plan 或 fallback 顺序。
- `p2p-frame/src/sn/protocol/sn.rs`：校验合法 endpoint 域，不改变 wire shape。
- `p2p-frame/src/networks/quic/listener.rs`：仅在共享 helper 影响 punch 时更新或拆分谓词，并固定所选边界。
- integration tests：默认 feature 测试负责证明生产 predicate/协议行为，`test-real-socket-matrix` 只负责 loopback socket 端到端动作证据。

## Requirement Review

该要求合理，且应修复生产/测试语义不一致。`Wan` 表示 peer 明确上报且被 SN 公网观察匹配的静态公网 endpoint，`Mapped` 表示公网映射 endpoint；两者本来就是可直连目的地址。把它们从反连请求中过滤掉与 caller-public 计划相矛盾，也会把可 direct 的路径错误推向 PN。

建议不简单删除所有过滤：只把 area 合法集合从 `ServerReflexive` 扩展为 `ServerReflexive | Wan | Mapped`，继续复用严格地址判断与端口/transport/去重约束。由于当前 helper 同时被候选构造、协议校验和 UDP punch 使用，设计阶段需逐个消费者确认：反连必须放行 `Wan/Mapped`；纯 punch 是否保持只对 `ServerReflexive` 生效，以避免本任务顺带扩大无关的打洞策略。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-WMRC-1 | wan_mapped_reverse_connect_eligibility | 默认生产构建把非 LAN IPv4、非零端口的 `Wan` 与 `Mapped` 保留为 rendezvous 反连候选，caller-public 的 Callee `ReverseConnectOnly` 请求不再为空 | 仅扩展反连候选合法 area；`Lan` 和生产私网/loopback 等地址仍拒绝 | 共享 helper 若直接扩展可能同时影响 UDP punch，因此实现前按消费者拆分或证明共享语义正确 | default-feature 单元/集成断言 `Wan`、`Mapped` 被保留，caller-public request 为 `ReverseConnectOnly` 且 endpoint 非空；`Lan`/非法地址负例仍失败 | 不改变 plan 选择、地址分类、prediction、fallback 顺序 |
| P-WMRC-2 | wan_mapped_rendezvous_protocol_validation | SN rendezvous request/notify 校验接受反连所需的 `Wan` 与 `Mapped` endpoint，并保持现有数量、协议、地址、端口、去重约束 | wire 编码、字段、command/version 不变，只改变合法输入域 | 新旧节点对相同 wire payload 的接受结果可能不同，需记录滚动升级兼容与失败行为 | 默认构建的正负 codec/validate 测试分别覆盖 `Wan`、`Mapped`、`Lan`、private/loopback、零端口、重复和 transport 不匹配 | 不扩展 endpoint 编码或新增协议字段 |
| P-WMRC-3 | production_default_reverse_connect_regression_tests | 测试不得依赖 `test-real-socket-matrix` 才证明 `Wan/Mapped` area 合法；feature 只提供 loopback/private 地址缝，并保留真实 socket caller-public 连接证据 | 默认测试证明生产 predicate/协议；loopback matrix 证明本地真实 socket 动作，不冒充公网证据 | 无可控公网 NAT 环境时，公网端到端仍是明确残余验证缺口 | default-feature targeted tests 通过；feature-gated caller-public real-socket matrix 通过；相关 p2p-frame x509 lib/protocol 回归通过 | 不把 loopback、mock 或静态校验称为真实公网部署验证 |

## Success Criteria

- Concrete user-visible or system-visible result: caller 带合法公网 `Wan` 或 `Mapped` endpoint 时，NAT-aware caller-public 分支能够发送包含该 endpoint 的 rendezvous 请求，SN/目标端校验接受，Callee 可执行反连；不会仅因 area 类型而错误降级 PN。
- Required evidence: current-code consumer closure；默认 feature 下 `Wan`/`Mapped` 正例和非法 endpoint 负例；caller-public `ReverseConnectOnly` 请求 endpoint 非空；feature-gated real-socket caller-public direct tunnel；相关 x509/p2p-frame 回归。报告明确区分 action armed、direct tunnel connected 与公网部署未验证。
- Explicit non-goals: 不改 wire layout、NAT 策略矩阵、SN 分类、prediction、生命周期、fallback 协议，也不宣称本地证据覆盖真实公网 NAT。

## Risks

- 协议接受域变化会造成滚动升级期间行为不对称：新节点接受 `Wan/Mapped`，旧节点仍会拒绝并 fallback；不发生误解码，但升级顺序可能影响 direct 成功率。
- 共享 eligibility 当前也控制 UDP punch。若直接放宽而未按 operation/consumer 审核，可能无意改变纯 punch 策略；设计必须明确拆分或验证。
- `Wan` 来源于上报地址与 SN 观察地址相等，`Mapped` 来源于观察映射；仍需保持 authenticated peer endpoint ownership 与非 LAN 地址约束，不能把任意第三方地址变成反连目标。
- 本地 loopback feature 可以验证真实 socket 调用链但不能复现公网 NAT；最终仍保留部署环境验证缺口。

## Approval Record

- approver: user
- approval_date:
- user_statement: "确认，自动完成"
