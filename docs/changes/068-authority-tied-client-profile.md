# 普通画像接收绑定 UDP authority 隧道

- Status: complete
- Owner module: p2p-frame
- Task manifest: `docs/versions/v0.1/modules/p2p-frame/068-authority-tied-client-profile/task.yaml`
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/068-authority-tied-client-profile/proposal.md`
- Affected paths: `p2p-frame/src/sn/service/nat_probe_scheduler.rs`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/tests/unit/sn_tests/service/service/nat_probe_scheduler_tests.rs`
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

`handle_report_sn` 在 `observe_capable_report` 之后把 `ReportSn.net_profile` 交给 `observe_reported_profile`。此前该方法只按 peer 索引，因此同身份非 authority QUIC 或 TCP 上报仍可覆盖当前 authority 画像。

修复把画像接收绑定到当前 authority 隧道：

- `observe_reported_profile` 增加 `tunnel_id` 与 `remote_endpoint` 参数；只有 `state.authority_tunnel_id == tunnel_id` 且 `remote_endpoint.is_udp()` 时才接受画像，否则保持现状并返回空 transition。
- `observe_capable_report` 与 `observe_control` 的 authority 资格从 `Protocol::Quic` 统一为 `Endpoint::is_udp()`，使 QUIC 与其它 UDP 自定义协议隧道都能担任 authority，TCP 一律排除。
- `handle_report_sn` 把本次 `tunnel_id` 与 `observed_tunnel` 传入画像接收路径；无隧道信息时画像不再被接收。
- 调度器新增两个反例/正例测试：非 authority UDP 与 TCP 上报被忽略、非 QUIC UDP authority（`Protocol::Ext(1)`）可正常建立并接收画像。

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: yes，同一 identity 的非 authority 连接不再能覆盖服务端权威 NAT 画像；authority 资格按 UDP 族判定
- Concurrency, lifecycle, or runtime integration change: yes，同身份多连接下 profile 发布与 authority 隧道绑定
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 nat_probe_scheduler --lib` 16 项通过，含新增 `nat_probe_scheduler_client_profile_requires_udp_authority_tunnel` 与 `nat_probe_scheduler_accepts_any_udp_protocol_as_authority_client_profile`。
- `cargo test -p p2p-frame --features x509 --lib`：500 项通过。
- `cargo test -p p2p-frame --test nat_probe_logging_contract`：3 项通过。
- Result: pass
- Residual risk or follow-up: authority 资格放宽到全部 UDP 族协议，属于用户确认的语义边界；非 authority UDP/TCP 上报携带的画像会被忽略。
