# pn proxy TLS-over-proxy 测试补充

本补充文档定义直接 `pn` 子模块中 proxy tunnel `stream` 的 TLS-over-proxy 验证重点。它把 [docs/versions/v0.1/modules/p2p-frame/design/pn-proxy-encryption.md](/mnt/f/work/p2p/docs/versions/v0.1/modules/p2p-frame/design/pn-proxy-encryption.md) 中的设计落地为可验证内容。

## 范围

- proxy tunnel `stream` 的显式“明文 / TLS-over-proxy”模式选择
- tunnel 外预先约定 TLS 模式时的双端一致性要求
- active 侧在 proxy stream 建立成功后发起 TLS client 握手
- passive 侧在返回成功的 `ProxyOpenResp` 后发起 TLS server 握手
- TLS 身份校验绑定到预期 `remote_id`
- TLS 握手失败、证书校验失败、双端模式错配以及明文兼容路径
- `datagram` 在本轮不提供 TLS-over-proxy 语义，但必须忽略 `stream` 加密模式并继续保持明文兼容

## 规范入口

- Unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`
- DV: `python3 ./harness/scripts/test-run.py p2p-frame dv`
- Integration: `python3 ./harness/scripts/test-run.py p2p-frame integration`

## Unit 覆盖矩阵

| 行为 | 当前证据 | 预期断言 |
|------|----------|----------|
| passive 明文兼容 | `passive_stream_accept_returns_channel_and_success_response` | `stream_security_mode == Disabled` 时，被动侧返回可用明文 proxy stream |
| passive 逻辑 tunnel 可连续消费多个入站 stream channel | `passive_tunnel_accepts_multiple_stream_channels` | 同一 passive `PnTunnel` 在第一条 channel 被 `accept_stream()` 消费后，后续相同逻辑 tunnel 的 stream channel 能继续通过 `accept_stream()` 交付，而不是重新创建 tunnel 或报 already consumed |
| active 逻辑 tunnel 可消费反向入站 stream channel | `active_tunnel_accepts_inbound_stream_channel` | 本端已存在 active `PnTunnel` 时，远端经同一逻辑 tunnel 反向 `open_stream()` 产生的入站 channel 能投递到该 active tunnel，并由其 `accept_stream()` 消费 |
| 显式 `TlsRequired` 会在 proxy stream 建立后叠加 TLS | `passive_stream_accept_wraps_stream_with_tls_when_requested` | `ProxyOpenReq` / `Resp` 成功后继续完成 TLS client/server 握手，调用方只拿到 TLS 保护后的可用 stream |
| TLS server 身份绑定到预期 `remote_id` | `client_tls_wrapper_rejects_wrong_remote_identity` | 证书合法但身份不匹配时，TLS 建立失败 |
| passive `datagram` 忽略 `stream` 加密模式 | `passive_datagram_ignores_tls_mode_and_returns_read_and_success_response` | 对 datagram 请求选择 `TlsRequired` 时，被动侧仍返回可用明文 datagram |
| client 级或 per-tunnel 显式模式不会把本地 datagram 直接拒绝为 `NotSupport` | `create_tunnel_with_options_sets_tls_mode_without_local_datagram_rejection` | `PnTunnelOptions.stream_security_mode == TlsRequired` 时，本地 `open_datagram()` 不因 TLS 模式直接返回 `NotSupport` |
| 默认通用接口 active `create_tunnel*` + `open_stream()` 明文兼容 | 尚无 direct unit test | 未显式配置的 `PnClient` 上，active 通用建链路径仍返回可用明文 proxy stream |
| active `open_stream()` 在 `TlsRequired` 下完成 client 握手 | 尚无 direct unit test | active 侧在拿到 `ProxyOpenResp::Success` 后进入 TLS client 握手并只返回 TLS stream |

## 必需失败覆盖

| 条件 | 预期结果 | 当前状态 |
|------|----------|----------|
| active 侧选择 `TlsRequired`，passive 侧保持明文 | 当前 channel 在 TLS 握手或后续 I/O 上失败，不暴露明文 stream | 尚无 direct unit test 覆盖真实 `PnTunnel::open_stream()` 路径 |
| `ProxyOpenResp` 成功但 TLS client 握手失败 | 当前 channel 失败并关闭 | 尚无 direct unit test 覆盖真实 `PnTunnel::open_stream()` 路径；当前只有 wrapper 级失败覆盖 |
| passive 侧 TLS server 握手失败 | 当前 channel 失败并关闭，不暴露明文 stream | 尚无 direct unit test 覆盖真实 `PnTunnel::accept_stream()` 的 server 失败路径 |
| TLS 证书可解析但 `remote_id` 不匹配 | TLS 校验失败 | 已由 `client_tls_wrapper_rejects_wrong_remote_identity` 在 wrapper 级覆盖 |
| 调用方只使用旧接口 | 行为与当前明文 PN tunnel 一致 | 被动侧已有明文兼容测试；active 通用 `create_tunnel*` + `open_stream()` 仍缺 direct unit test |
| 调用方对同一 tunnel 选择 `TlsRequired` 后继续打开或接受 `datagram` | datagram 仍按当前明文语义工作，不因 TLS 模式新增失败 | 已由 `passive_datagram_ignores_tls_mode_and_returns_read_and_success_response` 与 `create_tunnel_with_options_sets_tls_mode_without_local_datagram_rejection` 部分覆盖；active 真实对端联通路径仍缺 direct unit test |
| 显式 client 级默认模式被用于 stack 默认 proxy client | 通过通用 trait 创建的 tunnel 和被动 accept 的 tunnel 继承该模式快照 | 已由 `create_default_proxy_client_enables_tls_for_proxy_streams` 覆盖本地配置与 datagram 非拒绝边界；运行时联通仍由 DV 承接 |

## DV 与 Integration 继承

- DV 证据仍然是模块级 all-in-one 运行时场景。当前场景代码显式调用 `set_proxy_stream_encrypted(true)`，因此 DV 直接覆盖的是 stack 默认 proxy client 的 client 级 TLS 模式配置路径；成功意味着这条显式 TLS 配置不会破坏现有场景启动。
- Integration 证据仍然是工作区级测试套件。对于 TLS-over-proxy，成功意味着新增 PN 专有接口不会破坏 `cyfs-p2p`、`cyfs-p2p-test` 或 `sn-miner-rust` 的现有编译与测试路径。
- 对 `datagram` 边界，DV 与 integration 的成功还意味着显式 `TlsRequired` 或 client 级默认 TLS 模式不会把现有明文 datagram 调用点回归成 `NotSupport`。

## 完成定义

- proxy stream 的明文兼容路径、TLS 成功路径和 TLS 失败路径都被 unit 测试覆盖。
- TLS 身份校验已明确绑定到预期 `remote_id` 并有失败断言。
- `datagram` 忽略 `stream` 加密模式并保持明文兼容的边界已被显式记录并有对应测试。
- 模块级 DV 与 integration 入口继续与 [testplan.yaml](/mnt/f/work/p2p/docs/versions/v0.1/modules/p2p-frame/testplan.yaml) 保持一致。
