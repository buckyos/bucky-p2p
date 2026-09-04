---
task_manifest: task.yaml
status: approved
---

# 失效 Direct 缓存条目的降级与失效 Proposal

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- rationale: 改动集中在 p2p-frame 单模块的 `TunnelManager` 缓存使用与 endpoint 排分路径：连续失败阈值、缓存条目删除、单 endpoint 预试跳过和排分降级，并给公开缓存 trait 增加带默认实现的 `remove`（向后兼容）。这与 042（rendezvous 失败编排、internal concurrency/lifecycle 修复归 standard）同型：内部运行时连接编排变化、无 wire/协议/配置/数据/依赖/发布/安全面影响，targeted verification 可用；不满足 trivial（涉及并发/生命周期/runtime 集成与连接超时行为），也无 high-risk 的具体触发证据。若用户希望使用 high-risk 暂存生命周期，可在确认时替换。
- triggered_boundaries: 若实现发现必须改变 `P2pConnectionInfo` wire 编码或 `P2pConnectionInfoCache` 的必选 API（非默认方法）才能满足验收，属于需求范围变更，需回到 proposal 复核；本提案默认仅增加带默认实现的 `remove`，保持现有实现全部编译。
- confirmation_statement: 用户于 2026-09-03 在任务描述中给出问题定位（`open_known_tunnel_with_options` 单 endpoint 预试 + preferred 排分永不衰减）和建议方向（连续失败 N 次后降级/删除 cache 的 Direct 条目，或失败时直接进入全候选矩阵），并回复“确定，按建议修改吧”。该回复确认按建议组合实现：连续失败达到阈值后删除缓存 Direct 条目、不恢复排分加成、后续 open 跳过单 endpoint 预试直接进入全候选矩阵；tier 按默认分类 standard 记录，用户未给出显式 tier 覆盖。

Risk profile: not-created (replace with ./risk-profile.yaml only after high-risk confirmation)

## Background and Goal

`conn_info_cache` 中的 `ConnectDirection::Direct` 条目一旦写入就不会失效：`open_known_tunnel_with_options`（`p2p-frame/src/tunnel/tunnel_manager.rs`）每次 open 都先把 cache 指向的单个 endpoint 单独试一遍；该 endpoint 在 NAT 重启/换端口后必然失败，于是每次 open 都先等满一个完整传输层连接超时，再进入全候选矩阵。

失败后该 endpoint 仍因两个不衰减的加分项继续排在首位：
- cache 条目带来的 `+10,000` preferred 分；
- `EndpointScore.last_success_at > 0` 带来的 `+2,000` 分（只增不减）；
- 失败惩罚封顶 `min(fail_count, 20) * 300 = -6,000`。

净分仍可能是 `+6,000`，所以 stale endpoint 在后续 open 中始终保持第一优先级。

目标：Direct 缓存失效有确定性机制——连续失败达到阈值后把该条目视为失效并删除；删除后不再做单 endpoint 预试、不再享受 preferred/last_success 排分加成，open 直接进入全候选矩阵（矩阵仍包含该 endpoint，若对端恢复正常端口映射可重新命中并恢复缓存）。

## Scope

### In scope

- 对 `EndpointScore.fail_count` 增加失效阈值语义：同一 endpoint 自上次成功以来的连续失败数达到 `DIRECT_CACHE_MAX_FAILURES`（本提案取 2）后，判定其作为 Direct 缓存条目已失效。
- Direct 缓存失效处理：
  - `open_known_tunnel_with_options` 读取到已失效的 Direct 条目时，先从 `P2pConnectionInfoCache` 删除该条目；
  - 单 endpoint 预试失败并使失败数达到阈值时，立即删除该条目；
  - 已失效/已删除后，后续 open 不再对 cache 指向的旧 endpoint 做单 endpoint 定点预试，直接进入 `preferred_direct_endpoints` 全候选矩阵。
- 排分降级：endpoint 失败数达到阈值后，`preferred_direct_endpoints` 不再给它 `+10,000` preferred 加分，也不再因历史 `last_success_at` 给 `+2,000`；static WAN `+500` 与失败惩罚不变。
- `P2pConnectionInfoCache` 增加 `async fn remove(&self, conn_id: &P2pId)`，带空默认实现以保证第三方实现兼容；`DefaultP2pConnectionInfoCache` 真正删除条目。
- 回归测试覆盖：健康缓存仍优先；两次失败后排分降级；降级后跳过预试；只剩失效 endpoint 时缓存条目被真正删除；单次 open 内的预试+矩阵累计失败可触发阈值。

### Out of scope

- 不改变 `Reverse`/`Proxy` 缓存条目的生命周期语义。
- 不做 `last_success_at` 时间衰减（例如按 TTL 或指数衰减）；本任务用连续失败阈值解决“永不衰减”的后果，不新增时间维度配置。
- 不修改 wire/协议、`P2pConnectionInfo` 编码、`ConnectDirection`、SN/PN/网络建立语义、`conn_timeout` 或 042 的总 deadline。
- 不改 endpoint 分数表的内存淘汰/容量策略。
- 不整理或归属工作区中其他既有未提交改动（harness refresh、030/031、042 相关等）。

### Boundary with neighboring modules

- `p2p-frame/src/tunnel/connection_info.rs`：仅新增带默认实现的 `remove`，现有 `get`/`add` 语义不变；第三方实现无需修改即可编译。
- `p2p-frame/src/tunnel/tunnel_manager.rs`：只在 `open_known_tunnel_with_options` 的 Direct 分支与 `preferred_direct_endpoints` 排分处改动；reverse/proxy 分支、NAT-aware/rendezvous 流程与 `open_direct_path` 并发语义不变（后者已有的 `on_direct_connect_result` 失败/成功计数被复用）。

## Requirement Review

需求合理：cache 的 Direct 条目保存的是“上一次成功直连的 endpoint”，但没有失效机制，NAT 重启/换端口后会把必然失败的 endpoint 当成首选，每次 open 多付一个完整连接超时。连续失败阈值直接复用既有的 `EndpointScore.fail_count`（每次失败 `saturating_add(1)`、每次成功清零），语义正是“自上次成功以来连续失败 N 次”。

选择的权衡：
- 阈值取 `N=2`：一次瞬时失败不会永久降级一个可能恢复的 endpoint；两次连续失败（含单次 open 内预试失败后再一次矩阵失败，两者都计入 endpoint 失败计数）足以判失效。代价是对 NAT 重启后的首次/前两次 open 仍可能付出既有超时代价；阈值到达后的 open 不再有定点预试。
- 删除条目与排分降级双管齐下：即使第三方 cache 实现的 `remove` 是默认空实现，`preferred_direct_endpoints` 仍按失败计数降级，不会重新把失效 endpoint 排到首位。
- 矩阵仍保留失效 endpoint：它已经与其余候选并发执行，不增加等待；若旧端口恢复，可立即重新成功并恢复 cache。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-DCACHE-1 | stale_direct_cache_invalidation | 连续失败达到 `DIRECT_CACHE_MAX_FAILURES` 后，Direct 缓存条目被判定失效：`open_known_tunnel_with_options` 读取时删除、预试失败达阈值时删除，且删除后不再对旧 endpoint 做单 endpoint 定点预试，直接进入全候选矩阵 | 只作用于 `ConnectDirection::Direct` 缓存条目与 `open_known_tunnel_with_options`；`Reverse`/`Proxy` 分支不变 | 复用既有 per-endpoint 失败计数，代价是阈值到达前仍按既有行为预试 | 回归用例断言降级后旧 endpoint 单次 open 仅被矩阵尝试一次（无定点预试）、仅剩失效 endpoint 时缓存条目被删除 | 不按时间或 TTL 衰减，不做 cache 容量淘汰 |
| P-DCACHE-2 | stale_direct_cache_scoring_demotion | 失败计数达到阈值后，`preferred_direct_endpoints` 不再给该 endpoint `+10,000` preferred 分，也不再给予 `last_success_at` 的 `+2,000`；`+500` static WAN 与失败惩罚保持不变 | 只改排分输入，不改候选集合与 `open_direct_path` 并发语义 | 直接消除“净分仍 +6,000 恒居首位”的来源，同时保留一次成功即恢复（成功清零失败计数） | 单测断言“成功一次后连续两次失败”的 endpoint 排在无分数候选之后 | 不引入时间衰减系数或新配置 |
| P-DCACHE-3 | stale_direct_cache_regression_tests | 新增回归测试：健康缓存仍首选；两次失败后排分降级；降级后跳过定点预试且只进矩阵；失效条目被真正删除；既有 `conn_info_cache_direct_preferred_on_reconnect` 等用例保持通过 | 仅修改 p2p-frame 测试与最小辅助；沿用 `MockDialNetwork` 家族 | 用 per-endpoint 拨号计数与缓存内容断言行为，避免只靠时间推断 | 点名的 lib 测试与既有相关用例通过；`cargo test -p p2p-frame --features x509 --lib` 全绿 | 不新增合成 fake 网络冒充真实 socket 证据，不引入固定 sleep 依赖 |

## Success Criteria

- Concrete system-visible result: NAT 重启/换端口后，失效的 Direct 缓存条目在连续失败达到阈值后被删除；此后每个 open 不再先为旧 endpoint 单独等满一个完整连接超时，而是直接进入全候选矩阵；排分不再因 `+10,000`/`+2,000` 把失效 endpoint 恒置首位；对端旧端口恢复时可重新直连并恢复缓存。
- Required evidence: 实现后的调用链 diff 与回归测试证据：阈值判定、缓存移除、预试跳过、排分降级三个断言族，以及既有 tunnel/cache 用例保持通过；写明 loopback/mock 单元证据不是公网 NAT 或真实跨网络证据。
- Explicit non-goals: 不改变 reverse/proxy 缓存语义、不引入时间衰减/新配置、不改 wire/公开必选 API、不把本任务成果误写为已部署公网 NAT 证据。

## Risks

- 阈值 `N=2` 会使首次/第二次失效 open 仍付出既有预试超时代价；接受并记录权衡（之后的 open 不再重复），若用户希望“首次失败即降级”可在确认时替换为 N=1。
- 单次 open 内预试失败本身会计数一次，随后矩阵再失败一次即达到阈值；这会让“一次 open”同时贡献两次连续失败。该语义与“连续失败”定义一致（自上次成功以来每个失败拨号都计数），并在回归测试中显式固定。
- 增加 trait 默认方法 `remove` 属公开 API 增量；默认空实现保持第三方编译兼容，但若第三方实现不复盖 `remove`，其内部缓存可能仍保留条目；排分降级与预试跳过不依赖 `remove` 是否真正删除，因此行为不回归。
- 工作区存在大量既有未提交修改；lower-tier baseline 与 stage-scope 证据绑定本任务 manifest，避免把无关改动归入 043。

## Approval Record

- approver:
- approval_date: 2026-09-03
- user_statement: "确定，按建议修改吧"
