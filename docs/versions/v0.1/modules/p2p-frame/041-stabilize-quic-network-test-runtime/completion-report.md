# Completion Report: 041-stabilize-quic-network-test-runtime

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/041-stabilize-quic-network-test-runtime.md

## Delivery Summary

- Outcome: QUIC network 测试夹具不再按 CPU 数创建 sfo worker。`new_network()` 使用 `with_workers(1)`，消除完整并行套件下 21×2×16=672 个单线程 runtime 的超配；QUIC 握手不再被饿死超过 3 秒，`pair.connect()` 不再出现 `ConnectFailed ... Elapsed(())` panic。生产代码、3 秒 connect timeout 与全部被测语义保持不变。
- Handoff: 仅改动 `p2p-frame/src/networks/quic/network.rs` 的 `#[cfg(test)] new_network()` 一行 runtime 配置；用户报告的 `quic_tunnel_open_datagram_without_listen_stays_pending` 及其同模块测试在并行压力下稳定通过。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| quic_network_test_single_worker_runtime | QUIC 网络测试夹具不再创建每 CPU 一个 sfo worker；并行运行时 `pair.connect()` 不再 panic | proposal.md P-041-WORKER 行 | `new_network()` 使用 `with_workers(1)`；21 线程并行连续 9 轮通过；完整 lib 套件 427/427 通过 | 修复前 5 轮 3 败（失败用例漂移、panic 位置一致）；修复后零失败 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | `new_network()` 夹具、`open_or_connect`/`connect_with_ep` 超时语义、`with_workers(1)` 的 sfo 单 worker socket/CID 路由 | 检查单 worker 下 `quic_packet_worker_index`（workers=1 恒 0）与 `linux_reuseport_select` 路由是否歧义 | sfo 单 worker 路径与 listener 测试既有夹具一致，无路由歧义；握手日志证明 3 秒超时是被饿死而非握手延迟 | pass |
| boundaries-and-failure-paths | 客户端无 listener fallback（`create_tunnel_no_listener_*`）、`close_all_listener`、`listen` 重复/失败路径 | 21 线程并行中这些用例均覆盖；单独重跑目标用例 | 全部通过；单 worker 只影响 fixture，不改变 quinn client endpoint fallback 或 listener 生命周期 | pass |
| regression-and-side-effects | 生产 QUIC listener/worker 多路复用路径、TCP/SN/PN 套件、既有 `with_workers(1)` 约定（sn、listener tests） | 全量 lib 套件默认并发跑一轮并检查既有 029/037/038 用例 | 427/427 通过；生产默认 worker 路径仍由非本测试代码与其它模块测试覆盖 | pass |

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 networks::quic::network::tests --lib -- --test-threads=21`（9 轮）；`quic_tunnel_open_datagram_without_listen_stays_pending` 单独复跑；`cargo test -p p2p-frame --features x509 --lib` 完整套件
- Result: pass
- Exception reason: n/a

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 041-F-001 | none | 21 线程并行 9 轮 + 完整 lib 套件 427/427 | 未发现阻塞性缺陷；TCP network 测试夹具仍使用默认 worker 数，属文档化兄弟风险但无对应失败证据 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 复现证据（5 轮 3 败）与修复证据（并行 9 轮全过 + 全量 427/427）闭合；独立缺陷发现未找到生产或测试回归；交付与已批准提案一致。
