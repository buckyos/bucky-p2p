---
task_manifest: task.yaml
status: approved
---

# 使用单一 TTP runtime 分流 SN 客户端与 inter-SN 通信

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment

- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 本修复改变 `p2p-frame` 的 TTP tunnel 接收所有权、主动连接和析构清理关系，并直接影响 membership-enabled `SnServer`、客户端 Report/online 以及跨 SN Query/Rendezvous 控制链。它属于并发/生命周期/runtime integration 和核心模块边界变更，需使用 high-risk 分阶段设计、实现、真实 socket 测试和独立验收。
- Proposal and tier confirmation: confirmed by user statement `确认，自动完成`

## Background and Goal

任务 034 的真实双 serving-SN 测试发现：`SnServer` 先用同一 `local_identity + NetManager` 创建 `TtpServer`，随后在启用 owner membership 时又创建 `TtpNode`。`NetManager` 对同一 identity 只允许一个 incoming tunnel subscriber，第二次注册返回 `AlreadyExists`；失败构造的 `TtpNode` 随后无条件注销该 identity，误删 `TtpServer` 已安装的 subscriber。结果是普通客户端到 serving SN 的 tunnel 在入站发布时因无 subscriber 被关闭，`ReportSn` 无法成功，`wait_online` 超时；inter-SN client 本身也未能创建。

本任务按用户指定方向修复：一个 SN identity 只由一个可共享的 TTP runtime 接收 incoming tunnel，然后在该 runtime 内按 `TunnelPurpose` 将 `sn_service` 和 `sn_inter_service` control stream 分发给各自处理器。`TtpServer` 和 `TtpNode` 都必须支持从外部传入同一个 runtime handle 创建，也必须能够向外返回该 handle，使外部可以继续创建共享该 runtime 的其他 `TtpServer` 或 `TtpNode` facade。该 runtime 还需提供 inter-SN 所需的主动 tunnel/control-stream 能力，但不得通过创建第二个 incoming subscriber 实现。

## Scope

### In scope

- `SnServer` 对同一 identity/`NetManager` 只安装一个 incoming tunnel subscriber，并由其唯一 `TtpRuntime` 同时注册和分发 `sn_service`、`sn_inter_service` purpose。
- 将 `TtpRuntime` 提升为公开但不透明、可克隆/传递的共享所有权 handle；它绑定 `local_identity`、`NetManager`、唯一 incoming subscriber、incoming validator、purpose registries、attached tunnel 集合和统一 tunnel cache，但不向调用方公开这些字段或 operational methods。
- `TtpServer` 与 `TtpNode` 都支持以外部提供的共享 runtime 创建，并提供只暴露受控 handle 的 runtime getter；从任一 facade 取得的 runtime 均可用于创建其他共享 facade。
- attach、主动 connect、listen/unlisten、cache、subscriber register/unregister 和 validator 等 runtime 操作只能由 `TtpServer`/`TtpNode` facade 在封装模块内部调用；其他仓库代码和 crate 外消费者只能持有、克隆或把 handle 传回这两个 facade 的构造入口。
- 保留 `TtpServer::new`、`TtpNode::new` 等现有 convenience constructor，由它们内部创建独占 runtime；新增共享构造入口采用 additive/backward-compatible API，不要求现有调用方迁移。
- 共享 runtime 同时提供被动接收和主动连接能力；主动建立的 tunnel 也附着到同一 runtime/cache，使入站 stream/control stream 继续按 purpose 分发。
- `TtpInterSnClient` 支持复用该共享接收者，而不是强制创建拥有第二个 subscriber 的 `TtpNode`。
- 保持现有 `TtpNode`、`TtpServer` 和 `TtpInterSnClient` 对仓库其他消费者的兼容；若需要新增构造入口，应采用 additive/backward-compatible 方式，不改变现有 wire 编码和 purpose 值。
- 修正 incoming subscriber 的注册/析构所有权：构造或注册失败的对象不得注销其他对象已安装的 subscriber；清理只能移除自身成功拥有的注册。
- 以真实 socket 回归 membership-enabled 单 SN Report/online，以及双 serving-SN inter-SN Query/Rendezvous 控制流程；复用任务 034 已生成的专用测试面时，保持其真实 socket、绝对 deadline 和双向 payload/目标 action 证据边界。

### Out of scope

- 不把 `NetManager` 改造成同一 identity 可注册多个并列 subscriber；分流责任留在唯一 TTP runtime 的 purpose registry。
- 不改变 SN、inter-SN、TTP 或 tunnel wire format，不修改 `sn_service` / `sn_inter_service` purpose 常量。
- 不改变客户端 tunnel 建立策略、NAT profile、真实 punch、PN fallback 或 endpoint classification。
- 不以新增测试 hook、mock SN、私有 handler 直调或替换生产 listener 证明修复。
- 不移除公开 `TtpNode`，不把本修复扩展成整个 TTP API 重构。
- 不向 facade 暴露可绕过 ownership、validator 或 purpose registry 不变量的裸可变内部状态；外部只获得受控、可克隆的 runtime handle。
- 不把 runtime operational methods 标记为 `pub` 或宽泛的 `pub(crate)`；不允许其他模块借由 getter 绕过 `TtpServer`/`TtpNode` 直接操作 tunnel/runtime 生命周期。

### Boundary with neighboring modules

- 生产改动限于 `p2p-frame` 的 `networks`、`ttp` 和 `sn` 内部边界；`cyfs-p2p`、`cyfs-p2p-test`、`sn-miner-rust` 不改变协议或运行时行为。
- `NetManager` 继续拥有 identity 到顶层 incoming subscriber 的唯一映射；共享 TTP runtime 是该注册的唯一所有者，并拥有 purpose 到 stream/control-stream/datagram handler 的多路分发。
- `SnService` 继续分别拥有客户端 SN command 处理和 inter-SN command 语义，本任务只修复承载它们的 TTP runtime/connector 组装与生命周期。

## Requirement Review

用户选择的单 runtime 分流与现有架构一致：`TtpRuntime` 已经按 `TunnelPurpose` 维护独立 registry，因此无需在 `NetManager` 顶层引入多 subscriber 竞争。相比允许同一 identity 注册多个 subscriber，单接收者能维持明确的 tunnel/cache 所有者，并避免同一 incoming tunnel 被重复接受或由多个 runtime 竞争 attach。

当前 `TtpRuntime` 只拥有 purpose registries 和 attached tunnel 弱引用；subscriber 注册、identity/`NetManager` 和 tunnel cache 分散在 `TtpServer`/`TtpNode`。若只增加 `new_with_runtime(Arc<TtpRuntime>)` 和 getter，两个 facade 仍会分别注册 subscriber、分别缓存 tunnel，不能解决冲突。因此选定方向是把 runtime 提升为共享 ownership core，再由 server/node facade 使用它；共享构造入口不得重复注册或覆盖 validator。

Rust 没有只对两个任意 sibling 类型开放方法的 friend 可见性；把方法设为 `pub(crate)` 不能满足“其他地方不能调用”。Design 必须通过模块嵌套、私有 core、sealed capability 或等价的编译期封装实现：公开的 runtime handle 不提供 operational methods，只有 `TtpServer`/`TtpNode` 的实现能访问私有 core。现有 `TtpClient` 如需复用底层 dispatch 逻辑，应依赖更低层私有组件或自己的内部 wrapper，不能获得共享 handle 的公共操作权限。

实现上也不能直接改变 `TtpServer` 当前 `TtpConnector` 的既有语义，因为它目前只在已存在的 incoming tunnel 上开流，而 inter-SN 需要在没有现有 tunnel 时主动建立连接。共享 runtime 必须提供显式的 node-capable 主动连接入口，同时保留 `TtpServer` 现有消费者的兼容行为，避免 PN 等调用方从“仅复用已接收 tunnel”静默变成“主动外连”。incoming validator 归 runtime 所有并在 runtime 创建时固定；后续 facade 只能复用，不能静默替换同一 subscriber 的验证策略。

仅移除第二个 `TtpNode` 仍不足以闭合根因：当前失败构造对象的 `Drop` 可以删除不属于自己的注册。因此本任务同时要求注册清理绑定到实际所有权，作为共享 runtime 之外的必要生命周期防线。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-035-1 | shared_ttp_runtime_sn_purpose_demux | `TtpServer`、`TtpNode` 可由同一个外部 opaque runtime handle 创建并返回该 handle；runtime 唯一拥有 subscriber、validator、cache 和 attach lifecycle，按 `sn_service`、`sn_inter_service` 分发且能主动建立 inter-SN control tunnel | runtime operational methods 仅能由 server/node facade 在封装模块内调用；保留现有 purpose、wire 和 convenience constructor；共享 facade 不得重复注册 subscriber 或替换 runtime validator | 需要以私有 core/模块封装替代简单公开 `Arc<TtpRuntime>`，API 只增加受控 handle 传递能力 | 双向构造回归覆盖 server→runtime→node、node→runtime→server 共享；compile-fail 外部消费者证明 handle 无 operational API；只产生一个 subscriber/cache；单 SN 真实 socket `ReportSn/wait_online` 成功；双 SN真实 TTP control stream 完成远端 detail Query 和 Rendezvous target delivery/action acknowledgement | 不要求 NAT punch、跨公网 peer data tunnel或所有 facade 共用同一 TCP connection |
| P-035-2 | ttp_incoming_subscription_lifecycle | incoming subscriber 的注销必须与成功注册的所有者绑定，失败注册或失败构造不得删除 incumbent subscriber | 保持每 identity 单 subscriber，不引入多订阅广播 | 增加少量注册状态或 ownership token/guard 生命周期管理 | red-green 回归复现 duplicate registration 后 incumbent 仍可接收真实或受控 tunnel dispatch；共享 runtime teardown 后注册被准确移除且可重新注册 | 不重构整个 `NetManager` subscription API，不允许 stale owner 删除新 owner |

## Success Criteria

- Concrete system-visible result: 启用 owner membership 不再破坏 serving SN 的普通客户端在线流程；`TtpServer`/`TtpNode` 可显式共享并向外传递同一 runtime；该 runtime 同时承载客户端 SN command 与 inter-SN command，并能主动联系其他 serving SN。
- Required evidence: admission 覆盖两个 change_id；API/生命周期测试覆盖独占 constructor、外部 runtime 注入、两种 facade 创建顺序、runtime getter、唯一 subscriber/cache、purpose registry、主动 connector、duplicate-registration cleanup 和 last-handle teardown；外部 compile-fail 证明 runtime handle 不能直接调用 operational methods 或访问 core；真实 socket 单 SN Report/online 回归；任务 034 的双 serving-SN production membership 测试可越过 source online，完成 inter-SN Query 和 Rendezvous target action 的有界闭环；任务级统一 runner、testing coverage、consumer closure、stage scope 和 acceptance 均通过。
- Explicit non-goals: 不声明真实 NAT 打洞、公网部署或全工作区质量门禁已验证；不改变 wire、purpose 常量、客户端策略或 PN 行为；不要求同一底层 TCP connection 承载所有角色，只要求唯一 incoming runtime/subscriber 按 purpose 分流。

## Risks

- `TtpServer` 当前 connector 只允许复用 incoming tunnel；若直接改成自动外连，可能改变 PN 和其他消费者失败语义。Design 必须使用显式的新能力或适配器，并记录消费者兼容性。
- 把 `TtpRuntime` 提升为公开共享 core 会新增 API/build surface；Design 必须给出具体类型/构造/getter 签名、现有消费者、兼容决策及 crate 导出闭包，不能暴露可破坏注册所有权的内部可变 registry。
- Rust 可见性若只依赖 `pub(crate)` 会扩大权限到整个 crate，违反本任务边界；Design 和 compile-fail 证据必须证明公开 handle 不具有可直接调用的 runtime 操作面。
- runtime 创建时必须固定 identity、`NetManager` 和 incoming validator；共享 facade 的构造顺序不得影响验证策略，试图用不匹配上下文创建 facade 必须在类型或构造边界被阻止。
- 同一 runtime 的 tunnel cache 会同时包含客户端和 serving-SN peer；cache key、endpoint 匹配及 teardown 必须维持 identity/endpoint 隔离，不能把不同远端 tunnel 错复用。
- Drop 与注册之间存在失败窗口；只用 identity 删除而没有 owner 匹配会再次产生 stale cleanup。实现和测试必须覆盖失败注册、正常 drop、重新注册三个生命周期状态。
- 双 serving-SN 测试还依赖 owner membership/lease 路由；本修复只闭合 TTP 接收与连接阻塞，若随后暴露独立的 lease 发布缺陷，必须作为新 finding 路由，不能把未到达 Query/Rendezvous 误报为本修复成功。

## Approval Record

- approver: user
- approval_date: 2026-09-02
- user_statement: "确认，自动完成"
