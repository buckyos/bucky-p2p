# pn_server 设计补充

本补充文档聚焦于直接 `pn` 子模块内 relay 侧 `pn_server` 的设计。它来源于 [p2p-frame/docs/pn_design.md](/mnt/f/work/p2p/p2p-frame/docs/pn_design.md)，并与当前 [p2p-frame/src/pn/service/pn_server.rs](/mnt/f/work/p2p/p2p-frame/src/pn/service/pn_server.rs) 中的 relay 实现保持对齐。

## 范围

### 范围内
- relay 侧 `PROXY_SERVICE` 流接收与生命周期
- `ProxyOpenReq` 的首帧解析
- 打开目标侧之前的 relay 准入钩子
- 目标流获取、响应转发和双向桥接
- relay 侧超时与错误到结果码的映射
- 多 PN server 下的新建逻辑 `PnTunnel` assigned target admission
- relay session registry 对既有逻辑 tunnel 后续双向 channel 的识别
- 成功握手后的 relay 双向 bridge 流量统计
- 成功握手后的 relay 双向 bridge 用户级限速
- `pn_server` 对 `sfo-io` 统计/限速实现的装配边界

### 范围外
- 已有 `TtpServer` tunnel 之外的 relay 发现与维护
- client 侧 `PnClient`、`PnListener` 和 `PnTunnel` 的责任归属
- 替换 `p2p-frame/docs/` 下现有 PN 协议说明
- 在 `p2p-frame` 内重写 `sfo-io` 的统计器或限速器实现
- 为 `pn` 以外的 transport/tunnel 路径同时引入统一限速策略
- 在 relay 上终止、代理或恢复 proxy TLS 会话
- 在 `p2p-frame` 内维护 `peer_id -> PN` 全局目录、自动选择 PN server、自动切换错误 PN、PN server 之间目录同步或跨 PN 二跳业务 bridge
- 删除旧单 PN / allow-all PN server 默认构造路径

## 运行时角色

| 组件 | 职责 | 入口点 | 输出 |
|------|------|--------|------|
| `PnServer` | 负责 listener 注册、accept-loop 生命周期、任务监管、默认 allow-all validator 装配和显式 assigned target admission 装配 | `new`、`new_with_connection_validator`、`new_with_options` 或等价显式策略构造器、`start`、`stop` | `PROXY_SERVICE` 上活跃的 relay listener |
| `PnService` | 负责单连接级请求处理 | `handle_proxy_connection`、`handle_proxy_open_req` | 已转发的 open 请求、已转发的 open 响应、字节 bridge |
| `PnRelayAdmission` | 在打开目标侧之前判断一个规范化 relay 请求是否允许新建逻辑 tunnel 或复用既有 relay session | `validate_new_tunnel`、`validate_existing_session` | accept 或 reject |
| `PnAssignedTargetPolicy` | 判断某个 target peer 是否被分配到本 PN server | `is_assigned_target` | true 或 false |
| `PnRelaySessionRegistry` | 记录已通过 assigned target admission 建立的逻辑 tunnel endpoint pair，允许同一 tunnel 后续双向 channel | `register_session`、`contains_session`、`remove_session` | session 命中或未命中 |
| `PnTargetStreamFactory` | 打开下游 `S -> B` 流，避免测试与具体 `TtpServer` 耦合 | `open_target_stream` | 目标侧 `(read, write)` 流对 |
| `PnTrafficAccountant` | 把 relay bridge 中成功转发的上下行字节分别归属到 source 与 target 两个用户视图 | bridge 装配点 | 与 `sfo-io` 对齐的双边统计更新 |
| `PnRateLimiter` | 在 relay bridge 数据路径上对已认证用户施加收发背压 | bridge 装配点 | 被整形后的读写节奏 |

## 构造与默认值

| 构造器 / helper | 新契约 |
|-----------------|--------|
| `PnServer::new(ttp_server)` | 必须安装显式 allow-all `PnConnectionValidator`；保持未配置部署和现有调用点兼容 |
| `PnServer::new_with_connection_validator(ttp_server, validator)` | 显式注入 relay admission 策略；多 PN assigned target 部署通过该路径或等价 options 路径启用限制 |
| `PnServer::new_with_options(ttp_server, options)` 或等价 options 构造器 | 可包含显式 assigned target admission、统计、限速和 target stream factory 等依赖；未显式提供 admission 时必须等价于默认 allow-all，而不是隐式拒绝 |
| `PnServerOptions::assigned_target_policy` 或等价字段 | 可选但多 PN 部署必须显式提供；用于判断本 PN server 可作为新建 logical `PnTunnel` target 的用户集合 |
| `PnServerOptions::traffic_policy` 或等价字段 | 可选；未提供时不启用限速，统计可使用 no-op 或默认 sink |
| `allow_all_pn_connection_validator()` | 默认 validator helper；必须允许所有规范化 relay 请求通过，用于兼容构造和显式测试 |

多 PN server 新需求要求区分“默认兼容路径”和“显式 assigned target 路径”：

- `PnServer::new(...)` 必须保持所有连接默认允许，不因缺少 assigned target policy 而启动失败。
- 需要多 PN target 归属约束的部署方必须在构造 `PnServer` 时显式提供 assigned target policy 或等价 validator；该显式路径负责拒绝错误 PN。
- 若部署方需要真实用户级限速，必须通过显式构造路径注入 `sfo-io` 适配层，而不是在 `PnService` 内硬编码全局单例。
- `cyfs-p2p-test`、`sn-miner-rust` 和其他下游 `PnServer::new(...)` 调用点是兼容边界；不得为了多 PN assigned target admission 强制迁移这些调用点。
- proxy tunnel 是否在 `stream` 上启用 TLS 不是 `PnServer` 构造参数，也不通过 open 请求显式声明；端点侧 `PnClient` / `PnTunnel` 通过 tunnel 外约定决定是否在 open 成功后进入 TLS。

## 新增校验逻辑

- relay 不再直接信任源端报文里的 `req.from`；它总是先用已接收流元数据中的已认证远端 peer id 做规范化覆盖。
- 规范化后的请求会被收敛成 `PnRelayAdmissionContext { from, to, tunnel_id, kind, purpose }` 或等价结构，并在打开目标侧流之前交给 `PnRelayAdmission`。
- `PnRelayAdmission` 先用 `PnRelaySessionRegistry` 判断 `(tunnel_id, from, to)` 是否属于已建立 logical tunnel 的 endpoint pair。命中既有 session 时，后续 stream/datagram channel 可继续打开，包括与初始方向相反的 channel，不重新按 `req.to` 执行 assigned target 拒绝。
- 未命中既有 session 时，该请求被视为新建 logical tunnel，必须通过 `PnAssignedTargetPolicy::is_assigned_target(req.to)`；只有 `req.to` 属于本 PN server assigned target 时才允许继续打开目标侧 stream。
- assigned target 拒绝、策略不可用或校验过程返回 `PermissionDenied` 时，relay 对源端暴露的握手结果统一是 `TunnelCommandResult::InvalidParam` 或等价参数/权限错误。
- 这层校验是 relay 的前置 admission gate，不替代目标端收到 `ProxyOpenReq` 之后的最终业务接受判定。
- 当新建 logical tunnel 成功收到目标侧 `ProxyOpenResp::Success` 后，relay 将 `(tunnel_id, source, target)` 注册到 `PnRelaySessionRegistry`；bridge/control channel 关闭或 tunnel 终止时移除该 session。

## 用户归属与统计口径

### 用户身份归属

- relay 侧 source 统计与限速主体使用已规范化后的 `req.from`，即底层连接元数据中的已认证远端 peer id。
- relay 侧 target 统计主体使用 relay 成功打开的目标用户 `req.to`；它可以与 source 统计共享同一 bridge 生命周期，但必须是独立可查询的用户视图。
- `tunnel_id`、`kind` 和 `purpose` 作为统计标签或限速决策的上下文，但不替代主体身份。
- 这样做要求 source 统计主体与 `PnConnectionValidator` 看到的身份完全一致，避免把源端原始报文中未被 relay 信任的 `from` 用作记账键；同时 target 统计主体必须锚定到 relay 已成功打开的 target，而不是任何业务载荷中的未认证字段。
- 本轮只把统计主体扩展为 source/target 两个独立用户视图；限速主体仍保持 source 单边模型。若未来需要 target 限速或 `(from, to)` 组合配额，应通过新的 proposal 扩展。

### 统计口径

- 统计对象仅限成功握手之后的透明 bridge payload，不包含 `ProxyOpenReq`、`ProxyOpenResp` 控制帧。
- source 视图中的上行字节口径是 relay 从 source 读出并成功写入 target 的数据字节数；source 视图中的下行字节口径是 relay 从 target 读出并成功写回 source 的数据字节数。
- target 视图中的上行/下行字节口径与 target 自身观察方向一致：当 relay 成功把 source 数据写入 target 时，target 视图累计其入向字节；当 relay 成功把 target 数据写回 source 时，target 视图累计其出向字节。
- 仅已被底层 writer 确认成功写出的字节可计入统计；停留在用户态缓冲或因错误回滚的字节不得提前入账。
- 若 bridge 在任一方向异常退出，source 与 target 两个统计视图都保持“已成功转发多少记多少”的单调累加语义，不因连接提前关闭而回滚。
- 当请求选择 TLS-over-proxy 时，relay 的统计口径默认切换为“成功转发的 TLS record 字节数”；除非后续 proposal 另行扩展，relay 不尝试恢复或估算明文字节数。

## `sfo-io` 接入边界

- `sfo-io` 只进入 relay 成功握手后的字节数据路径，不参与握手前控制帧解析、目标流打开和结果码映射。
- `p2p-frame` 不实现新的令牌桶、统计器或周期聚合器；它只负责把 `ProxyStream` 包装为 `sfo-io` 可消费的 I/O 端点，或把 `copy_bidirectional` 替换为等价的、基于 `sfo-io` 的双向 pump。
- 由于当前 `pn_server` 直接调用 `tokio::io::copy_bidirectional`，实现阶段允许把这一步替换为“先装配 `sfo-io` 统计/限速，再执行等价 bridge”的结构，只要保持现有握手成功后的透明转发契约不变。
- 若 `sfo-io` 提供的是 `AsyncRead`/`AsyncWrite` wrapper，优先通过 wrapper 装配；若 `sfo-io` 只能通过显式 pump API 计量/限速，则由 `pn/service` 层实现最小适配，而不是把 `sfo-io` 逻辑散落到调用点。
- source 与 target 的双边统计视图必须尽量复用同一套 `sfo-io` tracker / wrapper 体系，通过装配和映射区分归属；不得为了 target 可查询视图再复制一套平行统计实现。
- 当请求选择 TLS-over-proxy 时，`sfo-io` 看到的仍是 relay bridge 上的 TLS 握手字节和后续密文 I/O；统计和限速都基于该密文路径执行，不要求 `sfo-io` 理解端点侧的 TLS 语义。

## 配置与装配模型

- 统计和限速装配点位于 `PnServer` / `PnService` 内部，且只对 `pn/service` 暴露最小内部接口。
- 配置输入应由部署方在构造 `PnServer` 时提供，避免在 bridge 热路径中查找全局配置。
- 对于未显式开启限速的部署，bridge 路径应保持当前吞吐行为，除统计开销外不引入新的等待。
- 对于显式开启限速的部署，限速应按“每个已认证 `from` 用户”生效；同一用户同时发起多条 PN bridge 时，共享同一用户速率预算。
- target 侧统计视图必须有可查询入口，但该入口只暴露统计结果，不引入新的 target 侧限速配置面。
- 如果 `sfo-io` 需要额外 runtime 组件或后台清理器，应由 `PnServer` 外围装配并作为依赖注入，避免 `pn/service` 自行创建隐式后台任务。
- 对于显式开启 TLS-over-proxy 的端点，relay 不新增单独的部署配置；它只继续桥接 open 成功后的 TLS 握手字节和后续密文。

## 控制流

1. `PnServer.start()` 注册 `TtpServer.listen_stream(PROXY_SERVICE)` listener，并启动 accept loop。
2. 每条被接收的 relay 流都会交给 `PnService.handle_proxy_connection`。
3. `handle_proxy_connection` 读取第一帧控制头，目前只处理 `ProxyOpenReq`。
4. `handle_proxy_open_req` 会把 `req.from` 重写为来自已接收流元数据中的已认证远端 peer id，而不是信任请求体中的原始值。
5. 规范化后的请求会通过 `PnRelayAdmission` 校验，使用的上下文是 `PnRelayAdmissionContext { from, to, tunnel_id, kind, purpose }`。
6. 如果命中既有 relay session，relay 允许该 channel 复用既有 logical tunnel；如果未命中 session，relay 必须先确认 `req.to` 是本 PN server 的 assigned target。
7. 如果 admission 通过，relay 会在 `PN_OPEN_TIMEOUT` 限制下，通过 `PnTargetStreamFactory` 打开目标侧流。
8. relay 将 `ProxyOpenReq` 原样转发到目标流，等待 `ProxyOpenResp`，并检查返回的 `tunnel_id` 是否与请求一致。
9. relay 再把得到的 `ProxyOpenResp` 写回源端。
10. 只有在 `TunnelCommandResult::Success` 时，relay 才会根据规范化后的 `req.from` 和已确认打开的 `req.to` 创建双边统计上下文，并只为 source 侧创建限速上下文；如果这是新建 logical tunnel，还要注册 relay session。
11. relay 将源端与目标端 stream 装配进 `sfo-io` 的统计/限速路径，再启动双向 bridge；若端点在 open 成功后显式进入 TLS-over-proxy，bridge 上流经 relay 的将是 TLS 握手流量和后续密文，而不是业务明文。
12. 任何失败都会在 bridge 启动前终止，两端只完成 open-result 交换。
13. bridge 或 control channel 退出时，relay 只结束当前连接并清理对应 session，不在退出路径中补发额外协议命令。

## 协议契约

| 项目 | 当前 relay 契约 |
|------|-----------------|
| 控制服务 | 以 `TunnelPurpose` 表达的 `PROXY_SERVICE` |
| 首个命令 | `ProxyOpenReq` |
| 结果命令 | `ProxyOpenResp` |
| 身份来源 | relay 使用已接收流的已认证 peer id 作为 `from` |
| target admission | 新建 logical tunnel 必须满足 `req.to` 属于本 PN server assigned target；既有 session 后续双向 channel 只校验 session 命中 |
| 安全元数据 | relay 不携带额外 TLS 模式字段；TLS 仅在 open 成功后的字节流上叠加 |
| bridge 启动条件 | `ProxyOpenResp.result == TunnelCommandResult::Success` |
| 成功后的数据路径 | 透明字节转发；明文模式下 relay 看到业务 payload，启用 TLS-over-proxy 后 relay 只转发 TLS 握手流量和密文，但都会在 bridge 路径上执行 `sfo-io` 统计与 source 单边限速，并同时沉淀 source/target 两个统计视图 |

旧版 PN 说明仍然使用 `vport` 描述服务选择。当前 relay 实现则把下游服务选择放在 `ProxyOpenReq.purpose: TunnelPurpose` 中。这是一个显式、与实现对齐的差异，在参考 PN 说明完成统一前，必须持续留在证据链中。

## 依赖与边界

- `PnServer` 依赖 `TtpServer` 完成 listener 注册，并通过既有 `TtpConnector::open_stream(...)` target 接口打开目标流。
- `PnService` 依赖网络命令辅助逻辑来解析和发送 PN 控制帧。
- `PnService` 在默认构造下依赖 allow-all validator；在多 PN 显式策略构造下依赖 assigned target policy 判断新建 logical tunnel target 归属；该策略由库使用者提供，不由 `p2p-frame` 查询目录或同步 PN server 状态。
- `PnService` 持有 relay session registry，用于区分新建 logical tunnel 和既有 tunnel 后续 channel；registry 是本 relay 进程内状态，不是跨 PN 共享目录。
- `PnService` 依赖 `sfo-io` 适配层完成 source/target 双边统计装配，以及 source 单边限速装配。
- relay 可以在打开目标侧之前拒绝请求，但在目标侧接收到请求之后，它不负责做最终业务接受判定。
- bridge 成功启动后，relay 只承担传输职责，不再检查应用 payload，也不会在初始 `kind` 之外区分 `Stream` 与 `Datagram` 语义。
- `cyfs-p2p-test` 与 `sn-miner-rust` 现有 `PnServer::new(...)` 调用点属于兼容性边界；实现阶段不得要求它们为了默认 allow-all 行为迁移为显式 options/policy 构造。
- 即使请求启用了 TLS-over-proxy，relay 也不能成为 TLS 终止点；端点侧 TLS 上下文只存在于 `pn/client` 和相应被动侧。

## 错误与超时模型

| 条件 | relay 行为 | 返回的 open 结果 |
|------|------------|------------------|
| 默认构造缺少 assigned target policy | 使用显式 allow-all validator；继续进入 relay open 路径 | not-applicable: compatible default |
| 显式多 PN 策略构造缺少 required policy | 构造或启动失败，或按 options 校验返回配置错误；不进入受策略保护的 relay open 路径 | 配置错误 |
| 非既有 session 且 target 不属于本 PN assigned target | 在打开目标前停止；不会尝试打开目标流 | `InvalidParam` |
| 既有 session 未命中且请求不能作为新建 logical tunnel 通过 admission | 在打开目标前停止；不会尝试打开目标流 | `InvalidParam` |
| 打开目标流超时 | 在转发前停止 | `Timeout` |
| 打开目标流返回 `NotFound` 或被中断 | 在转发前停止 | `Interrupted` |
| admission 返回 `Reject` 或产生 `PermissionDenied` | 在打开目标前停止；源端只看到参数无效 | `InvalidParam` |
| 目标侧响应中的 `tunnel_id` 不匹配 | 视为协议失败 | `ProtocolError` |
| 目标侧响应解码为非成功结果 | 原样转发结果，不启动 bridge | 转发后的原始结果 |
| 成功后 bridge I/O 失败 | bridge 任务退出，不重试 | 连接关闭 |
| `sfo-io` 统计上报失败但 bridge 仍可继续 | 记录日志；不改写已建立的握手结果 | 连接保持 |
| `sfo-io` 限速上下文创建失败 | 在 bridge 启动前停止；按内部错误处理 | `InternalError` |

`result_from_error` 是 relay 侧将内部 `P2pErrorCode` 映射为 `TunnelCommandResult` 的规范函数。当前设计明确把 assigned target 拒绝、`InvalidParam`、`PermissionDenied` 和 `Reject` 折叠成同一个对外可见的 `InvalidParam` 握手结果。未来若调整 PN 错误语义，必须同步更新该映射和对应测试。

## 实现布局

| 路径 | 职责 | 备注 |
|------|------|------|
| [p2p-frame/src/pn/service/pn_server.rs](/mnt/f/work/p2p/p2p-frame/src/pn/service/pn_server.rs) | relay listener、校验钩子、目标打开、响应转发、字节 bridge 以及 `sfo-io` 装配点 | 本补充文档对应的实现事实来源 |
| [p2p-frame/src/pn/protocol.rs](/mnt/f/work/p2p/p2p-frame/src/pn/protocol.rs) | `ProxyOpenReq`、`ProxyOpenResp`、`PnChannelKind`、`PROXY_SERVICE` | relay 消费的协议字段 |
| [p2p-frame/docs/pn_design.md](/mnt/f/work/p2p/p2p-frame/docs/pn_design.md) | 更宽范围的 PN 协议说明 | 上游参考输入，不属于数据包自有的 relay 补充文档 |
| `cyfs-p2p-test/src/main.rs` | 现有 `PnServer::new(...)` 调用点 | backward-compatible：继续使用默认 allow-all validator |
| `sn-miner-rust/src/main.rs` | 现有 `PnServer::new(...)` 调用点 | backward-compatible：继续使用默认 allow-all validator |

## 风险与回滚

- relay 当前只接受 `ProxyOpenReq` 作为第一条控制帧；畸形或意外帧会被直接丢弃，且没有替代恢复路径。
- 5 秒 open 超时同时约束目标打开和目标响应等待，因此过载 relay 可能会让健康 peer 也失败。
- 参考 PN 说明与实现对 `vport` 和 `purpose` 的使用尚未完全对齐；acceptance 必须把这一差异视为可审计的设计偏差。
- 当前工作区尚未显式引入 `sfo-io`；具体依赖版本和适配方式若与当前 runtime 特征不兼容，需要退回 design。
- 直接替换 `copy_bidirectional` 可能改变 bridge 关闭顺序；实现必须用测试锁住既有成功/失败语义。
- 默认 allow-all PN server 构造和显式 assigned target 构造若混淆，会让错误 PN 被默认路径误放行，或让未配置部署被误拒绝；实现必须把 `PnServer::new(...)` 的兼容语义和显式策略路径的多 PN 准入语义分开。
- relay session registry 若清理不完整，可能允许旧 logical tunnel 的后续 channel 在 target assignment 已变化后继续通过；实现必须在 control/bridge 关闭、超时和错误路径清理 session。
- 如果错误地以源报文 `from` 而不是规范化后的认证身份做 source 记账键，或错误地把 target 视图挂到未经确认的身份上，会直接破坏双边统计隔离。
- 如果实现把 target 统计查询与 target 限速绑定在一起，会静默扩大 proposal 范围并改变现有配额模型。
- 若两端对 TLS 模式的 tunnel 外约定不一致，错误会延后到 TLS 建立阶段才暴露；relay 无法替端点提前发现这类错配。
- 回滚应优先撤销 `pn_server.rs` 中的 relay 实现改动，同时保留本补充文档作为下一轮迭代的设计基线。
