# pn_server 设计补充

本补充文档聚焦于直接 `pn` 子模块内 relay 侧 `pn_server` 的设计。它来源于 [p2p-frame/docs/pn_design.md](/mnt/f/work/p2p/p2p-frame/docs/pn_design.md)，并与当前 [p2p-frame/src/pn/service/pn_server.rs](/mnt/f/work/p2p/p2p-frame/src/pn/service/pn_server.rs) 中的 relay 实现保持对齐。

## 范围

### 范围内
- relay 侧 `PROXY_SERVICE` 流接收与生命周期
- `ProxyOpenReq` 的首帧解析
- 打开目标侧之前的 relay 准入钩子
- 目标流获取、响应转发和双向桥接
- relay 侧超时与错误到结果码的映射
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

## 运行时角色

| 组件 | 职责 | 入口点 | 输出 |
|------|------|--------|------|
| `PnServer` | 负责 listener 注册、accept-loop 生命周期和任务监管 | `new`、`start`、`stop` | `PROXY_SERVICE` 上活跃的 relay listener |
| `PnService` | 负责单连接级请求处理 | `handle_proxy_connection`、`handle_proxy_open_req` | 已转发的 open 请求、已转发的 open 响应、字节 bridge |
| `PnConnectionValidator` | 在打开目标侧之前允许或拒绝一个已规范化的 relay 请求 | `validate` | `ValidateResult::Accept` 或 `ValidateResult::Reject` |
| `PnTargetStreamFactory` | 打开下游 `S -> B` 流，避免测试与具体 `TtpServer` 耦合 | `open_target_stream` | 目标侧 `(read, write)` 流对 |
| `PnTrafficAccountant` | 把 relay bridge 中成功转发的上下行字节归属到一个已认证用户 | bridge 装配点 | 与 `sfo-io` 对齐的统计更新 |
| `PnRateLimiter` | 在 relay bridge 数据路径上对已认证用户施加收发背压 | bridge 装配点 | 被整形后的读写节奏 |

## 构造与默认值

| 构造器 / helper | 当前契约 |
|-----------------|----------|
| `PnServer::new(ttp_server)` | 便捷构造器；安装 `allow_all_pn_connection_validator()` 作为默认准入策略 |
| `PnServer::new_with_connection_validator(ttp_server, validator)` | 面向需要 relay 侧准入控制的部署场景的显式构造器 |
| `allow_all_pn_connection_validator()` | 默认 validator 实现；始终返回 `ValidateResult::Accept` |

因此，当前实现默认保持既有 relay 行为不变，同时允许在不改变 bridge 管线本身的情况下注入策略。

本轮设计要求 `PnServer` 继续保持默认构造路径可用，但把统计/限速依赖收敛为与 validator 同级的显式装配点：

- 若部署方未提供用户流量策略，`PnServer::new(...)` 应保持“不开限速但可接线统计默认实现或 no-op”的兼容路径。
- 若部署方需要真实用户级限速，必须通过显式构造路径注入 `sfo-io` 适配层，而不是在 `PnService` 内硬编码全局单例。
- `cyfs-p2p-test` 和 `sn-miner-rust` 当前只调用 `PnServer::new(...)`；因此新增能力必须允许这些调用点在不立即提供额外配置的情况下继续编译和启动。
- proxy tunnel 是否在 `stream` 上启用 TLS 不是 `PnServer` 构造参数，也不通过 open 请求显式声明；端点侧 `PnClient` / `PnTunnel` 通过 tunnel 外约定决定是否在 open 成功后进入 TLS。

## 新增校验逻辑

- relay 不再直接信任源端报文里的 `req.from`；它总是先用已接收流元数据中的已认证远端 peer id 做规范化覆盖。
- 规范化后的请求会被收敛成 `PnConnectionValidateContext { from, to, tunnel_id, kind, purpose }`，并在打开目标侧流之前交给 `PnConnectionValidator`。
- `PnServer::new(...)` 通过默认的 allow-all validator 保持兼容；只有 `PnServer::new_with_connection_validator(...)` 才会启用部署方自定义准入策略。
- validator 返回 `Reject`，或校验过程返回 `PermissionDenied` 时，relay 对源端暴露的握手结果统一是 `TunnelCommandResult::InvalidParam`。
- 这层校验是 relay 的前置 admission gate，不替代目标端收到 `ProxyOpenReq` 之后的最终业务接受判定。

## 用户归属与统计口径

### 用户身份归属

- relay 侧统计与限速主体使用已规范化后的 `req.from`，即底层连接元数据中的已认证远端 peer id。
- `req.to`、`tunnel_id`、`kind` 和 `purpose` 作为统计标签或限速决策的上下文，但不替代主体身份。
- 这样做要求统计主体与 `PnConnectionValidator` 看到的身份完全一致，避免把源端原始报文中未被 relay 信任的 `from` 用作记账键。
- 若未来需要双边配额或 `(from, to)` 组合配额，应通过新的 proposal 扩展；本轮不把统计主体扩展为多维配额模型。

### 统计口径

- 统计对象仅限成功握手之后的透明 bridge payload，不包含 `ProxyOpenReq`、`ProxyOpenResp` 控制帧。
- 上行字节口径是 relay 从源端读出并成功写入目标端的数据字节数。
- 下行字节口径是 relay 从目标端读出并成功写入源端的数据字节数。
- 仅已被底层 writer 确认成功写出的字节可计入统计；停留在用户态缓冲或因错误回滚的字节不得提前入账。
- 若 bridge 在任一方向异常退出，统计保持“已成功转发多少记多少”的单调累加语义，不因连接提前关闭而回滚。
- 当请求选择 TLS-over-proxy 时，relay 的统计口径默认切换为“成功转发的 TLS record 字节数”；除非后续 proposal 另行扩展，relay 不尝试恢复或估算明文字节数。

## `sfo-io` 接入边界

- `sfo-io` 只进入 relay 成功握手后的字节数据路径，不参与握手前控制帧解析、目标流打开和结果码映射。
- `p2p-frame` 不实现新的令牌桶、统计器或周期聚合器；它只负责把 `ProxyStream` 包装为 `sfo-io` 可消费的 I/O 端点，或把 `copy_bidirectional` 替换为等价的、基于 `sfo-io` 的双向 pump。
- 由于当前 `pn_server` 直接调用 `tokio::io::copy_bidirectional`，实现阶段允许把这一步替换为“先装配 `sfo-io` 统计/限速，再执行等价 bridge”的结构，只要保持现有握手成功后的透明转发契约不变。
- 若 `sfo-io` 提供的是 `AsyncRead`/`AsyncWrite` wrapper，优先通过 wrapper 装配；若 `sfo-io` 只能通过显式 pump API 计量/限速，则由 `pn/service` 层实现最小适配，而不是把 `sfo-io` 逻辑散落到调用点。
- 当请求选择 TLS-over-proxy 时，`sfo-io` 看到的仍是 relay bridge 上的 TLS 握手字节和后续密文 I/O；统计和限速都基于该密文路径执行，不要求 `sfo-io` 理解端点侧的 TLS 语义。

## 配置与装配模型

- 统计和限速装配点位于 `PnServer` / `PnService` 内部，且只对 `pn/service` 暴露最小内部接口。
- 配置输入应由部署方在构造 `PnServer` 时提供，避免在 bridge 热路径中查找全局配置。
- 对于未显式开启限速的部署，bridge 路径应保持当前吞吐行为，除统计开销外不引入新的等待。
- 对于显式开启限速的部署，限速应按“每个已认证 `from` 用户”生效；同一用户同时发起多条 PN bridge 时，共享同一用户速率预算。
- 如果 `sfo-io` 需要额外 runtime 组件或后台清理器，应由 `PnServer` 外围装配并作为依赖注入，避免 `pn/service` 自行创建隐式后台任务。
- 对于显式开启 TLS-over-proxy 的端点，relay 不新增单独的部署配置；它只继续桥接 open 成功后的 TLS 握手字节和后续密文。

## 控制流

1. `PnServer.start()` 注册 `TtpServer.listen_stream(PROXY_SERVICE)` listener，并启动 accept loop。
2. 每条被接收的 relay 流都会交给 `PnService.handle_proxy_connection`。
3. `handle_proxy_connection` 读取第一帧控制头，目前只处理 `ProxyOpenReq`。
4. `handle_proxy_open_req` 会把 `req.from` 重写为来自已接收流元数据中的已认证远端 peer id，而不是信任请求体中的原始值。
5. 规范化后的请求会通过 `PnConnectionValidator` 校验，使用的上下文是 `PnConnectionValidateContext { from, to, tunnel_id, kind, purpose }`。
6. 如果校验通过，relay 会在 `PN_OPEN_TIMEOUT` 限制下，通过 `PnTargetStreamFactory` 打开目标侧流。
7. relay 将 `ProxyOpenReq` 原样转发到目标流，等待 `ProxyOpenResp`，并检查返回的 `tunnel_id` 是否与请求一致。
8. relay 再把得到的 `ProxyOpenResp` 写回源端。
9. 只有在 `TunnelCommandResult::Success` 时，relay 才会根据规范化后的 `req.from` 创建用户级统计/限速上下文。
10. relay 将源端与目标端 stream 装配进 `sfo-io` 的统计/限速路径，再启动双向 bridge；若端点在 open 成功后显式进入 TLS-over-proxy，bridge 上流经 relay 的将是 TLS 握手流量和后续密文，而不是业务明文。
11. 任何失败都会在 bridge 启动前终止，两端只完成 open-result 交换。
12. bridge 退出时，relay 只结束当前连接，不在退出路径中补发额外协议命令。

## 协议契约

| 项目 | 当前 relay 契约 |
|------|-----------------|
| 控制服务 | 以 `TunnelPurpose` 表达的 `PROXY_SERVICE` |
| 首个命令 | `ProxyOpenReq` |
| 结果命令 | `ProxyOpenResp` |
| 身份来源 | relay 使用已接收流的已认证 peer id 作为 `from` |
| 安全元数据 | relay 不携带额外 TLS 模式字段；TLS 仅在 open 成功后的字节流上叠加 |
| bridge 启动条件 | `ProxyOpenResp.result == TunnelCommandResult::Success` |
| 成功后的数据路径 | 透明字节转发；明文模式下 relay 看到业务 payload，启用 TLS-over-proxy 后 relay 只转发 TLS 握手流量和密文，但都会在 bridge 路径上执行 `sfo-io` 统计与限速 |

旧版 PN 说明仍然使用 `vport` 描述服务选择。当前 relay 实现则把下游服务选择放在 `ProxyOpenReq.purpose: TunnelPurpose` 中。这是一个显式、与实现对齐的差异，在参考 PN 说明完成统一前，必须持续留在证据链中。

## 依赖与边界

- `PnServer` 依赖 `TtpServer` 完成 listener 注册和按 peer id 打开目标流。
- `PnService` 依赖网络命令辅助逻辑来解析和发送 PN 控制帧。
- `PnService` 依赖 `sfo-io` 适配层完成用户级统计与限速装配。
- relay 可以在打开目标侧之前拒绝请求，但在目标侧接收到请求之后，它不负责做最终业务接受判定。
- bridge 成功启动后，relay 只承担传输职责，不再检查应用 payload，也不会在初始 `kind` 之外区分 `Stream` 与 `Datagram` 语义。
- `cyfs-p2p-test` 与 `sn-miner-rust` 现有 `PnServer::new(...)` 调用点是兼容性边界；新增依赖不得要求这些调用点立刻理解 `sfo-io` 细节。
- 即使请求启用了 TLS-over-proxy，relay 也不能成为 TLS 终止点；端点侧 TLS 上下文只存在于 `pn/client` 和相应被动侧。

## 错误与超时模型

| 条件 | relay 行为 | 返回的 open 结果 |
|------|------------|------------------|
| validator 拒绝 | 在打开目标前停止；不会尝试打开目标流 | `InvalidParam` |
| 打开目标流超时 | 在转发前停止 | `Timeout` |
| 打开目标流返回 `NotFound` 或被中断 | 在转发前停止 | `Interrupted` |
| validator 返回 `Reject` 或产生 `PermissionDenied` | 在打开目标前停止；源端只看到参数无效 | `InvalidParam` |
| 目标侧响应中的 `tunnel_id` 不匹配 | 视为协议失败 | `ProtocolError` |
| 目标侧响应解码为非成功结果 | 原样转发结果，不启动 bridge | 转发后的原始结果 |
| 成功后 bridge I/O 失败 | bridge 任务退出，不重试 | 连接关闭 |
| `sfo-io` 统计上报失败但 bridge 仍可继续 | 记录日志；不改写已建立的握手结果 | 连接保持 |
| `sfo-io` 限速上下文创建失败 | 在 bridge 启动前停止；按内部错误处理 | `InternalError` |

`result_from_error` 是 relay 侧将内部 `P2pErrorCode` 映射为 `TunnelCommandResult` 的规范函数。当前实现明确把 `InvalidParam`、`PermissionDenied` 和 `Reject` 折叠成同一个对外可见的 `InvalidParam` 握手结果。未来若调整 PN 错误语义，必须同步更新该映射和对应测试。

## 实现布局

| 路径 | 职责 | 备注 |
|------|------|------|
| [p2p-frame/src/pn/service/pn_server.rs](/mnt/f/work/p2p/p2p-frame/src/pn/service/pn_server.rs) | relay listener、校验钩子、目标打开、响应转发、字节 bridge 以及 `sfo-io` 装配点 | 本补充文档对应的实现事实来源 |
| [p2p-frame/src/pn/protocol.rs](/mnt/f/work/p2p/p2p-frame/src/pn/protocol.rs) | `ProxyOpenReq`、`ProxyOpenResp`、`PnChannelKind`、`PROXY_SERVICE` | relay 消费的协议字段 |
| [p2p-frame/docs/pn_design.md](/mnt/f/work/p2p/p2p-frame/docs/pn_design.md) | 更宽范围的 PN 协议说明 | 上游参考输入，不属于数据包自有的 relay 补充文档 |
| `cyfs-p2p-test/src/main.rs` | 现有 `PnServer::new(...)` 调用点 | DV 兼容性边界 |
| `sn-miner-rust/src/main.rs` | 现有 `PnServer::new(...)` 调用点 | 集成兼容性边界 |

## 风险与回滚

- relay 当前只接受 `ProxyOpenReq` 作为第一条控制帧；畸形或意外帧会被直接丢弃，且没有替代恢复路径。
- 5 秒 open 超时同时约束目标打开和目标响应等待，因此过载 relay 可能会让健康 peer 也失败。
- 参考 PN 说明与实现对 `vport` 和 `purpose` 的使用尚未完全对齐；acceptance 必须把这一差异视为可审计的设计偏差。
- 当前工作区尚未显式引入 `sfo-io`；具体依赖版本和适配方式若与当前 runtime 特征不兼容，需要退回 design。
- 直接替换 `copy_bidirectional` 可能改变 bridge 关闭顺序；实现必须用测试锁住既有成功/失败语义。
- 如果错误地以源报文 `from` 而不是规范化后的认证身份做记账键，会直接破坏用户统计和限速隔离。
- 若两端对 TLS 模式的 tunnel 外约定不一致，错误会延后到 TLS 建立阶段才暴露；relay 无法替端点提前发现这类错配。
- 回滚应优先撤销 `pn_server.rs` 中的 relay 实现改动，同时保留本补充文档作为下一轮迭代的设计基线。
