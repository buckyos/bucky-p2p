# Chg-036: stabilize QUIC owner abort and TTP readiness observers

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/036-fix-test-lifecycle-readiness/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/036-fix-test-lifecycle-readiness/proposal.md
- Affected paths: p2p-frame/src/ttp/client.rs, p2p-frame/src/ttp/runtime/handle.rs, p2p-frame/src/ttp/runtime/server.rs, p2p-frame/src/networks/quic/listener/tests.rs, p2p-frame/src/pn/service/pn_server/tests/reverse_tcp_proxy_tests.rs
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

`TtpServer::has_cached_tunnel_for_test` 不再复用会清理缓存项的 production
`find_existing_tunnel_in_multi`，改为 `#[cfg(test)]` 的锁内只读 bucket 扫描；它复用
`is_tunnel_available` 和 `match_target`，对 `Connecting` tunnel 返回 false 但不删除。
QUIC owner-lifecycle 回归测试把固定 20 次调度让步改为 1 秒有界轮询，以观察
`AbortOnDropTask` 在 owner future 被取消时对 worker-runtime task 的 abort；生产
abort 行为未改变。

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: yes
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

并发/生命周期风险已限定在测试观察边界和测试同步：production TTP
cache/state machine、QUIC connect/punch 生产路径均未修改。确认后的变更仍在
standard 选档范围内，残余风险为宽套件调度窗口仍由 1 秒有界等待观察，而不是
取消同步事件本身。

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib -- ttp::tests::server_ udp_punch_quic_nat reverse_tcp_proxy_tests -- --nocapture`
- Result: pass
- Residual risk or follow-up: loopback/unit evidence only; no public NAT, multi-host, or deployed PN evidence
