---
task_manifest: task.yaml
status: approved
---

# QUIC 网络测试运行时稳定化

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 修复集中在 `p2p-frame/src/networks/quic/network.rs` 的 `#[cfg(test)]` 测试夹具，不改生产代码、协议、公开契约或部署面；但影响完整并发测试套件的稳定性且需要跨模块独立验证，按 bounded single-project 的 standard 处理而非 trivial。
- Proposal and tier confirmation: 用户回复“确认”，同意 standard tier 与提案范围。

## Background and Goal

用户报告完整测试运行中的一个偶发 panic：

```text
thread 'networks::quic::network::tests::quic_tunnel_open_datagram_without_listen_stays_pending' panicked at p2p-frame/src/networks/quic/network.rs:1061:18:
called `Result::unwrap()` on an `Err` value: p2p_frame::error::P2pErrorCode:ConnectFailed at:[p2p-frame/src/networks/quic/listener.rs:1288] quic to L4qic127.0.0.1:45665 connect failed
Caused by: Elapsed(())
```

该 panic 发生在 `setup_network_pair().connect()` 的 QUIC 握手等待上，而不是被测的 datagram pending 行为本身。单独运行该测试稳定通过（1/1，约 1.5s）。

本地复现：`cargo test -p p2p-frame --features x509 networks::quic::network::tests --lib -- --test-threads=21`。默认 16 核上 5 轮中有 3 轮各失败 1 个用例（失败用例会漂移：`quic_tunnel_open_datagram_without_listen_stays_pending`、`quic_tunnel_accept_datagram_requires_listen_first`、`quic_tunnel_datagram_round_trip_ok`），全部 panic 在 `network.rs:1061` 的 `pair.connect()`，错误均为 `ConnectFailed ... Elapsed(())`。失败偶发性表明不是被测逻辑缺陷，而是 QUIC 建立阶段的并行资源/调度问题。

根因：`new_network()` 测试夹具为每个 `QuicTunnelNetwork` 创建 `ServerRuntimeConfig::default()` 的 sfo runtime，其 worker 数等于 CPU 数（本机 16）。21 个并行 `#[tokio::test]` 各创建 client+server 两个 runtime，仅 sfo worker 就有 21×2×16=672 个独立 current-thread tokio runtime 线程，叠加 tokio 测试运行时本身（每测试 16 worker），远超 CPU 容量。QUIC 握手需要监听侧单线程 worker runtime 的 quinn driver/accept 循环被调度处理；在超配压力下该调度窗口可超过夹具的 3 秒 connect timeout，quinn 握手被饿死并返回 `ConnectFailed(Elapsed)`。

证据对照：将夹具 connect timeout 临时提高到 60 秒后，同一并行压力全部通过且握手日志均为毫秒级（0ms 级），证明不是握手固有延迟，而是启动/调度窗口超过 3 秒；将 sfo worker 固定为 1 且保留原 3 秒 timeout 后，21 线程并行连续 9 轮全部通过。

## Scope

### In scope

- 修改 `p2p-frame/src/networks/quic/network.rs` `#[cfg(test)] mod tests` 中 `new_network()`：将 `ServerRuntime::start(ServerRuntimeConfig::default())` 改为 `ServerRuntime::start(ServerRuntimeConfig::new().with_workers(1))`。
- 保留现有 3 秒 connect timeout 与所有被测行为语义不变。
- 提供并行 red→green 证据与完整套件回归证据。

### Out of scope

- 不修改 QUIC 生产路径（`network.rs` 非测试代码、`listener.rs`、`tunnel.rs`、`quinn`/sfo 依赖）。
- 不延长生产或测试 connect timeout，不串行化 QUIC 测试，不重写握手/重试逻辑。
- 不修改 TCP、SN、TTP、PN 等其他模块的 fixture（它们的同类 worker 配置问题不属于本次用户报错范围）。
- 不做 harness docs 之外的无关重构。

### Boundary with neighboring modules

- 仓库已有约定：SN 测试（`sn/tests.rs`）、QUIC listener 测试（`listener/tests.rs`、`listener.rs` 内 sfo 夹具）以及 task 032 的资源稳定化均已使用 `with_workers(1)`；本任务把 QUIC network 测试对齐到同一约定。
- TCP network 测试夹具也是 `ServerRuntimeConfig::default()`，但未观察到与本次报告相同的 panic；保留为已知的兄弟风险，不在本任务处理。

## Requirement Review

可选修复方向：(a) 延长测试 connect timeout；(b) 串行化 QUIC 测试；(c) 减少每个测试的 sfo worker 数。方向 (a) 只掩盖调度饥饿且削弱 3 秒超时语义断言；(b) 与既有多模块并行测试策略冲突；方向 (c) 消除结构性超配，与 task 032/038 及 listener 测试既有约定一致，且不改被测语义。QUIC 测试不需要多 worker 负载均衡能力（每测试只有一组 listener/client），`with_workers(1)` 在功能和覆盖上等价，因此选择方向 (c)。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-041-WORKER | quic_network_test_single_worker_runtime | QUIC 网络测试夹具不再创建每 CPU 一个 sfo worker，完整并行运行时 QUIC 握手不再被饿死超过 3 秒，`pair.connect()` 不再 panic | 仅 `network.rs` 测试夹具的 runtime 配置；生产代码与 3 秒 timeout 不变 | 单个测试不再使用 16 路 SO_REUSEPORT worker；单机 loopback 测试无负载均衡需求，覆盖不损失 | `networks::quic::network::tests` 21 线程并行连续多轮通过；用户原始用例单独运行通过；完整 `p2p-frame --features x509 --lib` 至少一轮通过 | 不改生产路径、不延长 timeout、不串行化测试 |

## Success Criteria

- Concrete user-visible result: 用户报告的 `quic_tunnel_open_datagram_without_listen_stays_pending` panic 不再出现；同一 QUIC 模块在 21 线程并行下连续多轮无 `ConnectFailed(Elapsed)`。
- Required evidence: 修复前 5 轮中 3 轮失败的并行复现记录；修复后 21 线程并行连续 ≥6 轮通过；目标用例单独运行通过；完整 `p2p-frame --features x509 --lib` 默认并发一轮通过。
- Explicit non-goals: 不宣称覆盖任意小 CPU/共享 CI runner 或公网环境；不改变 QUIC 生产连接超时语义；不处理 TCP fixture 的同类风险。

## Risks

- 与其余测试模块（SN、TCP、PN）的并发压力仍存在，极端低配机器上其他真实 socket 测试仍可能饥饿；本任务只消除 QUIC network fixture 的结构性超配。
- `with_workers(1)` 使 sfo 复用端口共用一个 worker socket；loopback 上 QUIC 协议本身仍按 DCID 路由，单 worker 下 worker-index 恒为 0，无路由歧义（现有 listener 单 worker 测试已证明该路径）。
- 本任务不新增或删除任何生产 flush/ack/告警逻辑。
