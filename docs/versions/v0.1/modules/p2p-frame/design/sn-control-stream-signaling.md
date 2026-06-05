# SN Control Stream 信令设计

本补充文档定义 SN 低频信令如何消费通用 `Tunnel` control stream API。目标是在已有 tunnel 控制通道健康且远端支持对应 purpose 时，让 SN report、call、called、response 或等价小消息复用 control stream，而不是为每次小消息默认建立普通业务 stream。

## 范围

### 范围内
- SN 子模块定义固定的 SN control stream purpose，供 `listen_control_stream(...)` 和 `open_control_stream(...)` 使用。
- SN client/service 在 attach tunnel 或初始化监听时注册该 purpose 的 control stream callback。
- SN report、call、called、response 或等价低频小消息通过 control stream 发送。
- bootstrap、控制通道不可用、远端未监听 purpose、旧版本不支持或其他 control open 失败时，当前 SN 命令通道创建/发送失败，不回退到普通业务 stream。

### 范围外
- 新增 SN 大流量数据平面或通用消息总线。
- 公开 `networks::control_stream` runtime、frame、stream id、window 或 buffer 协议。
- 改变 SN 命令含义、单 SN 边界、endpoint 分类、`SnCallResp` 仅表示 SN 受理结果的语义。
- 让 SN 直接构造 raw `Data` 控制命令 payload。

## Purpose 与入口

SN 使用一个稳定 purpose，例如 `TunnelPurpose::Service(PN/SN 既有 vport)` 的 SN 专用值，或现有 `TunnelPurpose` 能表达的等价 SN 控制 vport。实现应优先复用仓库已有 SN purpose/vport 常量，避免新增与现有 SN 协议无关的公开枚举。

监听侧：
1. SN service 在获得 tunnel 后调用 `listen_control_stream(sn_purpose, callback)`。
2. callback 从返回的 `TunnelStreamRead` / `TunnelStreamWrite` 中读取一条 SN 信令消息。
3. 解析、路由、响应继续复用现有 SN command 编解码和 handler，不解析内部 control stream frame。

发送侧：
1. SN client 发送 report/call/called/response 时调用 `open_control_stream(sn_purpose)`。
2. 若 control stream 打开失败，当前 SN 请求按既有错误模型失败。
3. 在返回的 stream 上写入现有 SN command 编码。
4. 读取响应或按既有请求/响应语义完成。

## 失败边界

以下场景必须返回当前 SN 命令通道创建/发送错误，不得继续使用既有普通 stream 信令路径：
- 建链 bootstrap 阶段尚无可用 tunnel/control channel。
- `open_control_stream(...)` 返回 `PortNotListen`、`ListenerClosed`、`Interrupted` 或等价控制通道不可用错误。
- 远端未监听 SN control stream purpose。
- 远端版本未实现 SN control stream purpose。

实现不得在上述失败后调用普通 `open_stream(...)` 重试 SN 命令，也不得在 SN service 侧继续为 SN command purpose 注册普通 `listen_stream(...)`。

## 错误与关闭

- control stream 打开失败按失败边界返回。
- control stream 打开成功但读写失败时，当前 SN 请求按既有信令失败路径返回；不得把失败解释为最终 tunnel 连通性失败。
- 底层 tunnel close 或 control stream close 后，SN 不保留可继续读写的半开 control stream。
- 单条 SN 消息仍受通用 control stream `Data` payload `64 KiB` 上限和发送切分规则约束；SN 不绕过该上限。

## 实现顺序
1. 为 SN purpose 增加私有 helper 或复用现有 SN vport。
2. 在 SN service/tunnel attach 路径注册 `listen_control_stream(...)`。
3. 抽出 SN command 在任意 `TunnelStreamRead` / `TunnelStreamWrite` 上的发送/接收 helper。
4. 将默认 SN 小消息发送路径改为只调用 `open_control_stream(...)`，失败时返回错误。
5. 补充 unit 覆盖 control stream 成功，并通过代码审查确认 SN client 不调用普通 `open_stream(...)` fallback、SN service 不注册普通 `listen_stream(...)`。

## 风险与回滚

- 若实现残留 ordinary stream fallback，迁移会退化为继续默认普通 stream。
- 旧版本节点或 bootstrap 阶段缺少 SN control purpose 时会显式失败；这是本轮边界而不是兼容缺口。
- 回滚只恢复 SN 默认普通 stream 信令选择，保留通用 `Tunnel` control stream API。
