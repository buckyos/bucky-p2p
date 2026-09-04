---
task_manifest: task.yaml
status: approved
---

# QUIC network 并行测试端口稳定化

Risk profile: not-created

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 修复限定在 `p2p-frame/src/networks/quic/network.rs` 的现有 `#[cfg(test)]` fixture，不改变生产 QUIC/TLS 行为、公开接口、协议或依赖；但问题只在并行 socket 生命周期与 `SO_REUSEPORT` 分发下出现，需要 red-green 并行回归和完整 QUIC network 测试验证，按 bounded single-project bugfix 的 standard 处理。
- Proposal and tier confirmation: 用户回复“确认”，批准所展示的 proposal 与 standard tier；无未决问题。

## Background and Goal

用户报告 `quic_tunnel_open_stream_without_listen_stays_pending` 在 `setup_network_pair().connect()` 阶段失败，TLS 错误为 `Invalid server name`。目标用例单独运行 1/1 通过；以 21 个测试线程运行整个 `networks::quic::network::tests` 时可复现为 20/21，通过/失败用例会漂移，本次复现失败于 `quic_tunnel_control_stream_round_trip_ok`，服务端返回 `no server certificate chain resolved`。

当前 fixture 的每个 client/server listener 都绑定 `127.0.0.1:0`。`sfo-reuseport` 在 Linux UDP bind 上无条件启用 `SO_REUSEPORT`，即使业务配置的 `reuse_address` 为 false；并行且未显式协调的动态端口 listener 因而可能进入同一 reuse-port 组。QUIC Initial 被分发到另一测试的 resolver 后，若 SNI 不同则服务端无法解析证书；若同为 `quic-server`，客户端收到名称相同但身份 ID 不同的证书，最终由 `TlsServerCertVerifier` 报 `Invalid server name`。这发生在被测 stream pending 行为之前。

## Scope

### In scope

- 只调整 QUIC network 单元测试 fixture 的 loopback listener 端口分配，使同一测试进程中的并行 fixture 使用明确且不重复的非零端口。
- 保留现有单 worker runtime、3 秒连接超时、真实 QUIC/TLS 握手和现有测试语义。
- 使用用户报告用例和整个 QUIC network 测试模块做 red-green 并行验证。

### Out of scope

- 不修改生产 QUIC listener、TLS verifier/resolver、证书内容或 `sfo-reuseport` 依赖。
- 不通过延长 timeout、重试 TLS、关闭身份校验或串行化整个测试集掩盖失败。
- 不重构其他 TCP/SN/PN/TTP 测试的端口分配。

### Boundary with neighboring modules

- 端口唯一性仅服务于 `network.rs` 内现有测试进程；生产 listener 仍按调用方提供的 endpoint 工作。
- 若验证发现错误并非 fixture 端口交叉分发，而是生产 TLS/QUIC 行为缺陷，将返回 proposal 更新范围并重新确认，而不在本任务内静默扩展。

## Requirement Review

错误发生在测试前置 QUIC 握手，不是 `open_stream` 的 pending 断言。单测通过、并行模块中失败用例漂移，以及两种互补错误（服务端 SNI resolver miss；客户端证书 ID mismatch 被统一报告为 `Invalid server name`）共同指向 listener 交叉分发。最小方向是让现有真实 socket fixture 使用进程内唯一的明确端口，直接消除 reuse-port group 误合并；这比修改安全校验、增加等待或降低并行度更符合失败边界。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-056-PORT | quic_network_test_unique_listener_ports | 并行 QUIC network fixture 的 client/server listener 不共享动态 UDP reuse-port group，前置 TLS 握手稳定连接到对应身份 | 仅 `network.rs` 现有 `#[cfg(test)]` helper | 使用受控测试端口而非内核自动端口；需避免进程内重复并在 bind 冲突时明确失败 | 修复前 21-thread 模块运行复现 TLS resolver/identity 失败；修复后同命令连续多轮通过，用户原始用例精确运行通过 | 不改变生产连接、TLS 信任或超时语义 |

## Success Criteria

- Concrete user-visible or system-visible result: 用户报告用例及同模块其他用例不再因交叉 listener 的 `Invalid server name` / `no server certificate chain resolved` 在 `pair.connect()` panic。
- Required evidence: 原始用例精确运行通过；`cargo test -p p2p-frame --features x509 networks::quic::network::tests --lib -- --test-threads=21` 连续至少 6 轮通过；检查实际改动仍仅限测试 fixture。
- Explicit non-goals: 不宣称覆盖公网 QUIC、跨进程端口抢占或生产环境 reuse-port 调度；不修改生产安全边界。

## Risks

- 明确测试端口仍可能与机器上的外部进程冲突；实现应在进程内保证单调唯一，并选择与本仓库其他测试约定不冲突的范围，bind 冲突应直接暴露而非复用错误 listener。
- 测试 listener 当前可能活到测试 runtime/进程结束；本任务聚焦并行 fixture 的地址唯一性，不借机改写生产 listener 生命周期。
