---
task_manifest: task.yaml
status: approved
---

# 稳定 reverse TCP 测试：端口 guard 与 PN cache readiness 同步

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- Final tier confirmation: 2026-09-02 用户确认 `确认，按 standard 自动完成`
- Tier rationale / triggered boundaries: 修复集中在 p2p-frame 测试面和一个可选的 `#[cfg(test)]` 只读/事件观察边界；但涉及跨模块（TCP network、TTP、PN）的并发时序与真实 socket 生命周期，且仓库已有多个同模块未完成任务（032-035）可能重叠，不能按单文件 trivial 处理。不改变任何生产协议、缓存 prune、状态机或公开契约；无 high-risk 触发边界，故提报 standard。
- Proposal and tier confirmation: 用户原始请求为 `修复错误`；2026-09-02 用户确认 `确认，按 standard 自动完成`，采用 sibling task 037，未完成 task 032 保持原状。

## Background and Goal

用户给出的宽套件运行出现两个失败：

1. `networks::tcp::network::construction_tests::reverse_data_first_claim_tests::tcp_data_control_direction_priority_falls_back_for_active_and_passive_opens`
   `connect_with_optional_local` bind local `127.0.0.1:44104` 得到 `AddrInUse`（`OS Error 98`）。
2. `pn::service::pn_server::tests::traffic_manager_tests::reverse_tcp_proxy_tests::tcp_reverse_data_first_claim_pn_proxy_stream_uses_real_reverse_tcp_target`
   `PN should cache B control tunnel before the proxy request: Elapsed(())`（等 5 秒后超时）。

复现与证据（均基于当前工作区）：

- 两个用例单独运行均通过；`reverse_data_first_claim` 过滤（20 个用例）3 次并发运行均通过；完整 `p2p-frame` lib 套件在 `--test-threads=4` 与默认并发下重复失败于同一 PN 用例，`--skip quic/ttp/sn/tunnel/pn::client` 均无法消除，`--test-threads=2` 时 7 测试组合连续 3 次通过。
- 端口侧根因：`reverse_tcp_proxy_tests.rs` 的 `b_reusable_ep` 和 `reverse_data_first_claim_tests.rs` 的 `client_local_ep` 都采用“先 `TcpListener::bind(127.0.0.1:0)` 占端口，随后 `drop` 再让网络 socket 重新 bind 同一端口”的模式。端口在 drop 与真正 bind 之间是空闲窗口，其他并发测试线程可通过 `bind(:0)` 或顺序固定端口（如 `sn/tests.rs` 的 42000+ 序列）抢走该端口，产生 `AddrInUse`。
- 本地实验确认 Linux 语义：两个非 listen、且都设置 `SO_REUSEADDR` 的 TCP socket 可同时 bind 同一端口；listen socket 会阻止第二个 bind；连接中/已 bind 的 socket 与另一个 listen socket 可共存。因此“全程持有不 listen 的 guard socket，直到网络连接 socket 完成 bind”可以消除窗口且不改变网络 socket 行为。
- PN 侧：task 029/036 已把 readiness 观察改成只读 bucket 扫描，但当前 5 秒 deadline 在 8 线程并发下仍稳定超时，且失败时没有任何生命周期状态输出。`has_cached_tunnel_for_test` 的布尔轮询无法区分“未 attach/未 cache”“已 cache 但 `Connecting`”“已 closed/error”，也不能事件驱动地等待 TCP 被动侧 `PassiveReady -> Connected` 提升。仓库内未完成 task 032 的 `P-032-1 pn_reverse_tcp_readiness_synchronization` 覆盖同一问题（不延长 deadline、不固定 sleep、一次 proxy request），可作为本任务对齐目标，但本任务按当前证据新建 sibling packet 而不是改写 032 的已批准范围。

目标是消除两类不稳定：端口分配不再存在可被并发抢占的空窗；PN 用例在唯一 `ProxyOpenReq` 前以可诊断、事件可等待的方式确认 B control tunnel 已 cache 且处于 production 可用状态，失败时报告具体生命周期边界。

## Scope

### In scope

- 新增（或复用）测试专用 TCP 端口 guard：用 `socket2` 创建非 listen、设置 `SO_REUSEADDR` 的 TCP socket，`bind(127.0.0.1:0)` 拿到端口后**持有到网络连接 socket 完成同端口 bind**，再释放 guard；用于 `reverse_tcp_proxy_tests.rs::b_reusable_ep` 与 `reverse_data_first_claim_tests.rs::client_local_ep` 的分配。
- 让 PN reverse TCP 用例的 cache-ready 等待改为基于真实 tunnel 状态的有界等待：等待目标 identity 匹配的 cache 项出现并达到 `Connected`，使用 Notify 或低频率事件轮询而非 `yield_now` 忙轮询；超时断言输出 cache 快照（是否 cache、状态、closed 标志、endpoint 匹配结果）。
- 如诊断证实 5 秒 bound 在并发负载下不足以等待真实 `PassiveReady -> Connected`，则把 bound 调整为有界且与负载无关的方式（例如事件等待 + 15 秒环境上限），并在 change record 中记录修订理由；不得退化为无限等待或固定 sleep。
- 只在 `#[cfg(test)]` 增加必要的只读/快照测试面；production TTP cache、lookup/prune、TCP 状态机、PN 请求语义保持原样。
- 为两个缺陷各增加可重复的 red/green 或稳定压力证据，并重新执行点名 PN 用例、并发压力用例和相关 TCP reverse 用例。

### Out of scope

- 不改变 production `TtpTunnelCache`、`is_tunnel_available`、`match_target`、`PassiveReady -> Connected` 提升、reverse first-claim、PN relay 或 TCP wire 行为。
- 不把 `AddrInUse` 当作可忽略错误吞掉；guard 方案必须消除窗口，而不是在失败后重试/隐藏。
- 不延长测试 deadline 作为唯一修复手段；任何 bound 调整必须伴随事件驱动或状态前置关系。
- 不新增 `ProxyOpenReq` 重试、target-open 重试、固定 sleep 或测试 hook 推进 tunnel 状态。
- 不修改 `cyfs-p2p-test/**`，不使用其产物作为证据。
- 不整理工作区其他既有未提交改动，也不接管未完成 task 032-035 的其余 scope。

### Boundary with neighboring modules

- `p2p-frame/src/networks/tcp/connection.rs`、`network.rs` 生产 bind/reuse 语义不变；guard 只在测试代码中使用。
- `p2p-frame/src/ttp/**` 只允许 `#[cfg(test)]` 只读快照/等待 seams；`TtpServer`/`TtpRuntime` production 方法不变。
- `p2p-frame/src/pn/service/pn_server/tests/reverse_tcp_proxy_tests.rs` 保持唯一 request、关闭直接 listener、双向字节断言。

## Requirement Review

修复请求合理。端口 failure 是测试资源分配的真实竞态，必须修复才能让宽套件稳定；PN readiness failure 在 task 029/036 修复观察器副作用后仍复发，说明还需可诊断与事件驱动的等待，而不是再次增大超时时间。采用“guard 端口 + 事件/快照 readiness”方向：前者从根源消除 `AddrInUse`，后者把不可诊断的布尔轮询改为建立在真实状态变化上的有界等待。若实施中诊断表明 PN 侧从未 cache 或隧道已 closed，将按证据返回 proposal 补充 scope，不静默扩大修复。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-037-PORT | reverse_tcp_test_port_guard | 两个 reverse TCP 测试的 local endpoint 在“选定端口”到“网络 socket 完成 bind”之间必须由 guard 持续持有，消除其他并发测试抢占该端口的窗口 | 仅测试代码与一个 `#[cfg(test)]` 共享 helper；生产 socket/bind/reuse 行为不变 | 新增小 helper 换取端口分配确定性；依赖 Linux `SO_REUSEADDR` 非 listen socket 共存语义，平台边界记录在案 | 并发运行 reverse TCP + PN 用例不再出现 `AddrInUse`；保留用户失败日志作为 red 证据 | 不做 bind 失败重试、不吞 EADDRINUSE、不改为固定端口池 |
| P-037-PN | pn_cache_readiness_deterministic_sync | PN 用例在唯一 proxy request 前必须等到匹配的 B control tunnel 已 cache 且 production 可用；超时失败必须输出可定位的生命周期快照 | 仅 `#[cfg(test)]` 等待/seam 与 `reverse_tcp_proxy_tests.rs`；不改变 production cache/lookup/TCP/PN 语义 | 有界事件等待替换忙轮询，失败可诊断；若需调整 bound，以诊断证据为依据并记录 | 8 线程 7 测试组合与完整 lib 套件不再出现 `Elapsed(())`；快照超时断言能报告明确边界；点名 PN 精确/并发压力用例通过 | 不无限等待、不加固定 sleep、不重试请求 |

## Success Criteria

- Concrete system-visible result: 用户在日志中给出的两个点名用例在串行、受控并行（`--test-threads=2/8`）与完整 `p2p-frame --features x509 --lib` 运行中不再出现 `AddrInUse` 或 readiness `Elapsed(())`。
- Required evidence: 保留原始失败输出；端口 guard 与 PN readiness 各一份 red/green 证据；`reverse_data_first_claim` 过滤组、PN 精确用例、PN 12-case 并发压力用例重复通过；standard 任务完成 `docs/changes/037-stabilize-reverse-tcp-tests.md` 与独立缺陷发现 completion report。
- Explicit non-goals: 不宣称多机、公网 NAT、Docker/GHCR 或部署环境证据；不改变生产协议与缓存契约；不处理 032-035 的其他未完成 scope。

## Risks

- guard 依赖当前运行平台的 `SO_REUSEADDR` 语义；若 CI 换到不同 OS，需要按平台条件编译或记录限定。当前目标环境为 Linux/WSL 已验证。
- 事件等待若绑定错误状态对象，可能制造假成功；必须复用 `is_tunnel_available`/`match_target` 或等价的真实状态来源，任何新 seam 都不得推进状态。
- 如果诊断显示 readiness 卡在 `Connecting` 是生产初始 Ping 调度/时序问题，而不是测试观察问题，应停止并返回 proposal 扩大范围，不静默改等待时间。
- 仓库存在未完成 task 032-035，特别是 032 与本任务 PN 项重叠；本任务为 sibling 修复，范围内不与 032 的批准 scope 冲突。

## Approval Record

- approver: user
- approval_date: 2026-09-02T12:00:00+08:00
- user_statement: "确认，按 standard 自动完成"
