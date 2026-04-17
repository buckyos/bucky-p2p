# pn_server 设计补充

本补充文档聚焦于直接 `pn` 子模块内 relay 侧 `pn_server` 的设计。它来源于 [p2p-frame/docs/pn_design.md](/mnt/f/work/p2p/p2p-frame/docs/pn_design.md)，并与当前 [p2p-frame/src/pn/service/pn_server.rs](/mnt/f/work/p2p/p2p-frame/src/pn/service/pn_server.rs) 中的 relay 实现保持对齐。

## 范围

### 范围内
- relay 侧 `PROXY_SERVICE` 流接收与生命周期
- `ProxyOpenReq` 的首帧解析
- 打开目标侧之前的 relay 准入钩子
- 目标流获取、响应转发和双向桥接
- relay 侧超时与错误到结果码的映射

### 范围外
- 已有 `TtpServer` tunnel 之外的 relay 发现与维护
- client 侧 `PnClient`、`PnListener` 和 `PnTunnel` 的责任归属
- 替换 `p2p-frame/docs/` 下现有 PN 协议说明

## 运行时角色

| 组件 | 职责 | 入口点 | 输出 |
|------|------|--------|------|
| `PnServer` | 负责 listener 注册、accept-loop 生命周期和任务监管 | `new`、`start`、`stop` | `PROXY_SERVICE` 上活跃的 relay listener |
| `PnService` | 负责单连接级请求处理 | `handle_proxy_connection`、`handle_proxy_open_req` | 已转发的 open 请求、已转发的 open 响应、字节 bridge |
| `PnConnectionValidator` | 在打开目标侧之前允许或拒绝一个已规范化的 relay 请求 | `validate` | `ValidateResult::Accept` 或 `ValidateResult::Reject` |
| `PnTargetStreamFactory` | 打开下游 `S -> B` 流，避免测试与具体 `TtpServer` 耦合 | `open_target_stream` | 目标侧 `(read, write)` 流对 |

## 构造与默认值

| 构造器 / helper | 当前契约 |
|-----------------|----------|
| `PnServer::new(ttp_server)` | 便捷构造器；安装 `allow_all_pn_connection_validator()` 作为默认准入策略 |
| `PnServer::new_with_connection_validator(ttp_server, validator)` | 面向需要 relay 侧准入控制的部署场景的显式构造器 |
| `allow_all_pn_connection_validator()` | 默认 validator 实现；始终返回 `ValidateResult::Accept` |

因此，当前实现默认保持既有 relay 行为不变，同时允许在不改变 bridge 管线本身的情况下注入策略。

## 新增校验逻辑

- relay 不再直接信任源端报文里的 `req.from`；它总是先用已接收流元数据中的已认证远端 peer id 做规范化覆盖。
- 规范化后的请求会被收敛成 `PnConnectionValidateContext { from, to, tunnel_id, kind, purpose }`，并在打开目标侧流之前交给 `PnConnectionValidator`。
- `PnServer::new(...)` 通过默认的 allow-all validator 保持兼容；只有 `PnServer::new_with_connection_validator(...)` 才会启用部署方自定义准入策略。
- validator 返回 `Reject`，或校验过程返回 `PermissionDenied` 时，relay 对源端暴露的握手结果统一是 `TunnelCommandResult::InvalidParam`。
- 这层校验是 relay 的前置 admission gate，不替代目标端收到 `ProxyOpenReq` 之后的最终业务接受判定。

## 控制流

1. `PnServer.start()` 注册 `TtpServer.listen_stream(PROXY_SERVICE)` listener，并启动 accept loop。
2. 每条被接收的 relay 流都会交给 `PnService.handle_proxy_connection`。
3. `handle_proxy_connection` 读取第一帧控制头，目前只处理 `ProxyOpenReq`。
4. `handle_proxy_open_req` 会把 `req.from` 重写为来自已接收流元数据中的已认证远端 peer id，而不是信任请求体中的原始值。
5. 规范化后的请求会通过 `PnConnectionValidator` 校验，使用的上下文是 `PnConnectionValidateContext { from, to, tunnel_id, kind, purpose }`。
6. 如果校验通过，relay 会在 `PN_OPEN_TIMEOUT` 限制下，通过 `PnTargetStreamFactory` 打开目标侧流。
7. relay 将 `ProxyOpenReq` 转发到目标流，等待 `ProxyOpenResp`，并检查返回的 `tunnel_id` 是否与请求一致。
8. relay 再把得到的 `ProxyOpenResp` 写回源端。
9. 只有在 `TunnelCommandResult::Success` 时，relay 才会进入两个字节流的 `copy_bidirectional` bridge。
10. 任何失败都会在 bridge 启动前终止，两端只完成 open-result 交换。

## 协议契约

| 项目 | 当前 relay 契约 |
|------|-----------------|
| 控制服务 | 以 `TunnelPurpose` 表达的 `PROXY_SERVICE` |
| 首个命令 | `ProxyOpenReq` |
| 结果命令 | `ProxyOpenResp` |
| 身份来源 | relay 使用已接收流的已认证 peer id 作为 `from` |
| bridge 启动条件 | `ProxyOpenResp.result == TunnelCommandResult::Success` |
| 成功后的数据路径 | 透明字节转发；握手后 relay 不再解析 payload frame |

旧版 PN 说明仍然使用 `vport` 描述服务选择。当前 relay 实现则把下游服务选择放在 `ProxyOpenReq.purpose: TunnelPurpose` 中。这是一个显式、与实现对齐的差异，在参考 PN 说明完成统一前，必须持续留在证据链中。

## 依赖与边界

- `PnServer` 依赖 `TtpServer` 完成 listener 注册和按 peer id 打开目标流。
- `PnService` 依赖网络命令辅助逻辑来解析和发送 PN 控制帧。
- relay 可以在打开目标侧之前拒绝请求，但在目标侧接收到请求之后，它不负责做最终业务接受判定。
- bridge 成功启动后，relay 只承担传输职责，不再检查应用 payload，也不会在初始 `kind` 之外区分 `Stream` 与 `Datagram` 语义。

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

`result_from_error` 是 relay 侧将内部 `P2pErrorCode` 映射为 `TunnelCommandResult` 的规范函数。当前实现明确把 `InvalidParam`、`PermissionDenied` 和 `Reject` 折叠成同一个对外可见的 `InvalidParam` 握手结果。未来若调整 PN 错误语义，必须同步更新该映射和对应测试。

## 实现布局

| 路径 | 职责 | 备注 |
|------|------|------|
| [p2p-frame/src/pn/service/pn_server.rs](/mnt/f/work/p2p/p2p-frame/src/pn/service/pn_server.rs) | relay listener、校验钩子、目标打开、响应转发、字节 bridge | 本补充文档对应的实现事实来源 |
| [p2p-frame/src/pn/protocol.rs](/mnt/f/work/p2p/p2p-frame/src/pn/protocol.rs) | `ProxyOpenReq`、`ProxyOpenResp`、`PnChannelKind`、`PROXY_SERVICE` | relay 消费的协议字段 |
| [p2p-frame/docs/pn_design.md](/mnt/f/work/p2p/p2p-frame/docs/pn_design.md) | 更宽范围的 PN 协议说明 | 上游参考输入，不属于数据包自有的 relay 补充文档 |

## 风险与回滚

- relay 当前只接受 `ProxyOpenReq` 作为第一条控制帧；畸形或意外帧会被直接丢弃，且没有替代恢复路径。
- 5 秒 open 超时同时约束目标打开和目标响应等待，因此过载 relay 可能会让健康 peer 也失败。
- 参考 PN 说明与实现对 `vport` 和 `purpose` 的使用尚未完全对齐；acceptance 必须把这一差异视为可审计的设计偏差。
- 回滚应优先撤销 `pn_server.rs` 中的 relay 实现改动，同时保留本补充文档作为下一轮迭代的设计基线。
