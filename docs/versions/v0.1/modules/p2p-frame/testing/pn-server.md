# pn_server 测试补充

本补充文档定义直接 `pn` 子模块中 `pn_server` 的 relay 侧验证重点。它把 [docs/versions/v0.1/modules/p2p-frame/design/pn-server.md](/mnt/f/work/p2p/docs/versions/v0.1/modules/p2p-frame/design/pn-server.md) 中的设计，以及 [p2p-frame/docs/pn_design.md](/mnt/f/work/p2p/p2p-frame/docs/pn_design.md) 中的 PN 参考说明落地为可验证内容。

## 范围

- relay 侧 listener 启动与 accept-loop 行为
- `ProxyOpenReq` 的规范化、校验与转发
- `ProxyOpenResp` 的转发与 bridge 激活
- bridge 启动前 relay 失败的结果映射
- 成功握手后 bridge payload 的用户流量统计
- 成功握手后 bridge payload 的用户级限速
- `sfo-io` 接入失败或背压引发的 relay 行为边界

## 规范入口

- Unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`
- DV: `python3 ./harness/scripts/test-run.py p2p-frame dv`
- Integration: `python3 ./harness/scripts/test-run.py p2p-frame integration`

`pn_server` 不定义自己的独立执行入口。它继承 [testing.md](/mnt/f/work/p2p/docs/versions/v0.1/modules/p2p-frame/testing.md) 中的模块级命令。

## Unit 覆盖矩阵

| 行为 | 当前证据 | 预期断言 |
|------|----------|----------|
| 目标流获取通过注入的 factory 进行委派 | [p2p-frame/src/pn/service/pn_server.rs](/mnt/f/work/p2p/p2p-frame/src/pn/service/pn_server.rs) 中的 `pn_service_uses_injected_target_stream_factory` | relay 打开请求的目标，重写 `from`，转发 `ProxyOpenReq`，转发成功响应，并在双向上传输字节 |
| validator 拒绝会短路目标打开 | [p2p-frame/src/pn/service/pn_server.rs](/mnt/f/work/p2p/p2p-frame/src/pn/service/pn_server.rs) 中的 `pn_service_rejects_proxy_open_when_validator_rejects` | validator 能看到规范化上下文，不会打开目标流，源端收到 `InvalidParam` |
| 通过 `PnServer.start()` 完成 listener 注册和端到端 bridge | [p2p-frame/src/pn/service/pn_server.rs](/mnt/f/work/p2p/p2p-frame/src/pn/service/pn_server.rs) 中的 `pn_server_listens_and_bridges_proxy_stream` | relay 在 `PROXY_SERVICE` 上监听，接收来流，转发握手，并桥接 payload |
| TLS-over-proxy 不要求 relay 理解模式字段 | 待补 `pn_server` TLS 透明桥接测试 | relay 不终止 TLS，也不携带 TLS 模式字段；两端在 open 成功后进入 TLS 时，relay 只继续桥接握手字节和密文 |
| 成功 bridge 会按规范化后的 `from` 记账 | 待补 `pn_server` 统计测试 | 只统计成功转发的 payload 字节；控制帧不入账；统计主体等于已认证远端 peer id |
| 同一用户多条 bridge 共享限速预算 | 待补 `pn_server` 限速测试 | 多条来自同一规范化 `from` 的 bridge 会观察到共享背压，而不是按连接各自无限速 |
| 默认构造路径保持兼容 | 待补 `PnServer::new(...)` 兼容性测试 | 未显式注入限速配置时，现有调用点仍能启动并桥接，且不要求外部立即提供 `sfo-io` 细节 |

## Validator 特定预期

- `PnServer::new(...)` 通过使用 allow-all validator 保持向后兼容行为。
- 当 relay 准入策略需要检查 `from`、`to`、`tunnel_id`、`kind` 和 `purpose` 时，必须使用 `PnServer::new_with_connection_validator(...)`。
- validator 观察到的 `from` 必须是 relay 用已认证连接元数据重写后的值，而不是源端原始报文中的 `from`。
- 当前预期 validator 侧的 `Reject` 会表现为握手结果 `InvalidParam`，而不是一个独立的线协议权限错误码。
- 统计与限速使用的用户键必须与 validator 观察到的规范化 `from` 一致；测试不允许出现 validator 与记账键使用两套身份来源。

## 统计与限速特定预期

- 流量统计只覆盖成功握手之后的 payload bridge，不统计 `ProxyOpenReq` / `ProxyOpenResp`。
- 若请求选择 TLS-over-proxy，relay 统计口径切换为成功转发的 TLS record 字节数，而不是应用明文字节数。
- 统计口径基于“relay 成功写到对端”的字节数；读到但未成功写出的字节不得入账。
- 限速只作用于成功握手后的 bridge 数据路径，不改变 open-result 映射。
- `sfo-io` 集成失败若发生在 bridge 前，测试应断言源端看到 `InternalError` 或文档声明的等价内部错误结果。
- `sfo-io` 统计上报失败若发生在 bridge 中，测试应断言连接不会被额外改写为新的线协议结果，而是按 bridge I/O 语义退出并记录诊断。

## 必需失败覆盖

| 条件 | 预期结果 | 当前状态 |
|------|----------|----------|
| 打开目标流超时 | 源端收到 `Timeout`；bridge 不启动 | 尚无专门的 unit 测试覆盖 |
| 打开目标流返回 `NotFound` 或被中断 | 源端收到映射后的非成功结果；bridge 不启动 | 尚无专门的 unit 测试覆盖 |
| 自定义 validator 构造路径使用了注入策略 | 注入的 validator 收到规范化上下文并控制准入 | 通过 `PnService` 级 validator 测试部分覆盖；尚无专门的 `PnServer::new_with_connection_validator(...)` 构造测试 |
| relay 会先规范化 `from` 再执行 validator | validator 上下文中的 `from` 等于已认证远端 peer id，而不是报文原值 | 通过 `pn_service_rejects_proxy_open_when_validator_rejects` 部分覆盖 |
| 目标侧响应中的 `tunnel_id` 不匹配 | relay 将其视为协议失败且不进行 bridge | 尚无专门的 unit 测试覆盖 |
| 第一条控制帧意外 | relay 丢弃连接且不 panic | 尚无专门的 unit 测试覆盖 |
| 成功后 bridge 侧 I/O 关闭 | bridge 任务干净退出，不进入重试风暴 | 仅被成功路径测试间接覆盖 |
| payload 统计误把控制帧或失败写入计入流量 | 统计值只反映成功转发 payload 字节 | 尚无专门的 unit 测试覆盖 |
| 限速键错误地使用原始报文 `from` | 同一已认证用户的多连接共享预算，伪造报文 `from` 不会绕过限速 | 尚无专门的 unit 测试覆盖 |
| `sfo-io` 限速上下文创建失败 | bridge 不启动，源端收到内部错误结果 | 尚无专门的 unit 测试覆盖 |
| `sfo-io` 统计/限速背压改变关闭顺序 | bridge 退出仍然干净，不出现重试风暴或悬挂任务 | 尚无专门的 unit 测试覆盖 |

这些缺口仅在模块数据包仍为 `draft` 时才可接受。进入批准前，应对每条未覆盖的 relay 侧失败路径做显式处置。

## DV 与 Integration 继承

- DV 证据仍然是模块级 all-in-one 运行时场景。对于 `pn_server`，成功意味着 PN relay 启动和 proxy 流不会阻塞场景完成，并且默认构造路径在接入 `sfo-io` 后仍能正常工作；若场景显式启用 TLS-over-proxy，relay 仍应只桥接 TLS 字节而不终止 TLS。
- Integration 证据仍然是工作区级测试套件。对于 `pn_server`，成功意味着 relay 侧 PN 改动不会破坏 `cyfs-p2p`、`cyfs-p2p-test` 或 `sn-miner-rust` 的兼容性，尤其不能要求现有 `PnServer::new(...)` 调用点立即理解新的限速配置细节。

## `pn_server` 的完成定义

- relay 启动、请求规范化、校验、响应转发和成功 bridge 激活都被 unit 测试覆盖。
- relay 在 bridge 前失败时，对非成功 open 结果的映射已被记录并文档化。
- relay 在成功 bridge 后的统计准确性、限速共享预算和背压退出行为都被 unit 测试覆盖。
- 模块级 DV 与 integration 入口继续与 [testplan.yaml](/mnt/f/work/p2p/docs/versions/v0.1/modules/p2p-frame/testplan.yaml) 保持一致。
- PN 参考说明与 `pn_server` 实现之间的任何协议字段差异，都在 acceptance 中被显式指出。
