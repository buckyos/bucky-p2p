---
module: p2p-frame
task_name: 014-callback-result-published-release-migration
submodule: 014-callback-result-published-release-migration
version: v0.1
status: approved
approved_by: user
approved_at: 2026-08-26
approved_content_sha256: 1cc8e80b08ab9a46436d8e5f6979eb822ad677713a0400f12d7571542e377202
---

# Callback Result Published Release Migration Proposal

## Background and Goal

任务 `sn-client-qa-correlation-fix` 与 `013-callback-result-replacement-waiter-cleanup` 为 crates.io 尚未包含的 waiter drop/replacement 修复维护了 `third-party/callback-result`，并由根 `Cargo.toml` 的 `[patch.crates-io]` 强制所有消费者使用该本地副本。

crates.io 已发布 `callback-result 0.2.5`。当前源码核对确认：发布版的 keyed `CallbackWaiter` 已包含本地 0.2.4 patch 的注册身份、条件删除、未轮询 drop cleanup 和同 ID replacement 保护，并且发布包包含对应 replacement 回归测试。因此本仓库不再需要继续拥有该依赖源码。

本任务目标是让工作区显式依赖 crates.io `callback-result 0.2.5`，删除根 patch 和 `third-party/callback-result`，刷新 lockfile，并证明真实消费者仍使用包含所需修复的 registry 发布版。

## Scope

### In scope

- 将 `p2p-frame/Cargo.toml` 的 `callback-result` 最低依赖版本提升并固定表达为 `0.2.5`，防止重新解析到不含修复的旧版。
- 删除根 `Cargo.toml` 中指向 `third-party/callback-result` 的 `[patch.crates-io]` 覆盖；若该表删除后为空，则一并删除空表。
- 更新 `Cargo.lock`，使 `callback-result 0.2.5` 记录 crates.io source 与 checksum，并保持无重复旧版解析。
- 删除完整的 `third-party/callback-result/**` 本地副本，包括其源码、manifest、license 和本地回归测试。
- 验证 registry 0.2.5 的 keyed `CallbackWaiter` 仍覆盖 drop cleanup 与 replacement ownership，并验证 `p2p-frame`、`sfo-cmd-server` 依赖闭包能够解析和编译。
- 在后续 testing 阶段基于迁移后的代码与发布包事实重新设计任务级验证；不继续把已删除 vendor 内的测试路径作为当前任务测试入口。

### Out of scope

- 修改 `callback-result` 的公开 API、回调 ID 分配、结果缓存策略或发布包源码。
- 修改 p2p-frame 的 SN、QA、tunnel、transport 或业务逻辑。
- 在仓库中保留第二份 registry 0.2.5 源码、重新 vendor 发布包，或用 Git/path 依赖替代 crates.io。
- 修改已验收的 013 task packet 或其历史证据；历史文档继续如实描述当时使用的本地 patch。
- 在 proposal 阶段删除目录、修改 Cargo 文件、更新 lockfile、修改测试或运行 implementation/testing 验证。

### Boundary with neighboring modules

- `p2p-frame/Cargo.toml` 拥有工作区对 `callback-result` 的直接最低版本要求。
- 根 `Cargo.toml` 与 `Cargo.lock` 拥有工作区级依赖来源覆盖和可复现解析结果。
- `third-party/callback-result/**` 是本任务要移除的临时依赖源码，不转移到其他模块或目录。
- `sfo-cmd-server 0.4.0` 通过 `CallbackWaiter<u128, CmdBody>` 消费同一 crate；它是依赖闭包验证对象，不在本任务中修改。
- `cyfs-p2p`、`cyfs-p2p-test` 与 `sn-miner-rust` 通过 p2p-frame 间接受影响，仅作为编译/集成闭包，不获得新的业务行为。

## Requirement Review

- 删除本地副本是合理的：所需修复已经进入正式发布版，继续维护 `[patch.crates-io]` 会造成重复所有权、发布遗漏风险和不必要的源码漂移。
- 不能只删除 `third-party/callback-result`。根 patch 仍指向该目录，直接删除会使 Cargo 解析失败；依赖版本、patch、lockfile 和目录必须作为一次原子迁移处理。
- `p2p-frame` 当前声明 `callback-result = "0.2.3"`。虽然 Cargo 可能在刷新后选择 0.2.5，但继续保留该下界允许未来解析到不含修复的 0.2.3/0.2.4，因此本任务选择显式提升到 `0.2.5`。
- 已检查 crates.io 0.2.5：keyed `CallbackWaiter` 与本地修复在忽略换行格式后一致，发布包还包含 replacement 测试。0.2.5 额外为 `SingleCallbackWaiter` 引入相同的 drop/replacement ownership 修复；仓库源码和当前 `sfo-cmd-server 0.4.0` 均未消费该类型，但 design/testing 仍需记录该上游行为差异和依赖风险。
- 删除 vendor 会同时删除仓库内的 `drop_cleanup.rs` 和 `replacement_waiter.rs`。后续 testing 不能引用不存在的测试目标，必须以 registry 发布包核对、依赖解析证据和真实 consumer 验证覆盖迁移目标。
- 回滚应是一个整体：恢复本地目录、根 patch、旧依赖声明和对应 lockfile，不能只恢复其中一部分。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-CRPRM-1 | callback_result_registry_release_migration | 工作区必须直接解析 crates.io `callback-result 0.2.5`，移除本地 patch 与 `third-party/callback-result/**`，同时保留已依赖的 keyed waiter drop/replacement 语义和真实消费者兼容性 | `Cargo.toml`、`p2p-frame/Cargo.toml`、`Cargo.lock` 与 `third-party/callback-result/**`；不修改 p2p-frame 或上游 crate 生产源码 | 放弃仓库内直接修改和直接运行 vendor 测试的能力，换取单一上游所有权和标准 registry 发布；接受 0.2.5 中未被当前消费者使用的 `SingleCallbackWaiter` 同类修复，但要求明确依赖审查 | `Cargo.lock` 唯一解析到带 registry source/checksum 的 0.2.5；根 patch 和本地目录均不存在；发布包源码/测试核对证明 keyed 修复存在；任务级 dependency tree、消费者编译及适当回归验证通过 | 不修改 callback-result API/源码，不改变 SN/QA 协议或 callback ID/cache 语义，不引入 Git/path 替代源，不改写历史 013 证据 |

## Success Criteria

- Concrete user-visible or system-visible result: 仓库不再包含或引用 `third-party/callback-result`，所有工作区消费者从 crates.io 获取 `callback-result 0.2.5`。
- Required dependency evidence: `p2p-frame/Cargo.toml` 要求 0.2.5；根 patch 已移除；`Cargo.lock` 的唯一 `callback-result` 条目为 0.2.5 且包含 registry source/checksum；依赖树不再显示本地 path source。
- Required behavior evidence: 核对 registry 0.2.5 的 keyed waiter owner-conditional cleanup 与发布包 replacement tests，并通过任务统一入口执行真实 p2p-frame/sfo-cmd-server 消费闭包验证。
- Required cleanup evidence: `third-party/callback-result` 下的 manifest、源码、license 和测试全部删除，没有其他生产 Cargo 配置引用该目录。
- Explicit non-goals: 不修改上游或 p2p-frame runtime 行为，不保留 vendored fallback，不扩展到无关依赖升级，不修改已完成 task packet 的历史陈述。

## Risks

- 只删除目录而未同时删除 root patch 会立即破坏依赖解析；四个迁移面必须在同一 implementation 中保持一致。
- 宽松的 `0.2.3` 版本下界不能表达本任务对修复版本的要求，必须提升到 0.2.5 并检查 lockfile 唯一解析结果。
- registry 0.2.5 除 keyed waiter 修复外还改变了 `SingleCallbackWaiter` 的取消/replacement cleanup。当前代码检索未发现工作区或 `sfo-cmd-server 0.4.0` 使用它，但未来/feature-gated 消费者仍是依赖审查关注点。
- 删除 vendor 测试会减少仓库直接拥有的依赖内部回归入口；验收必须确认发布包携带对应测试，并以 consumer 级验证覆盖本仓库责任边界。
- crates.io 可用性成为 clean resolve/build 的外部前置条件；已有 lockfile 与 Cargo cache 只能缓解，不能等同于继续 vendor。
- 回滚若只恢复 path 或只回退 lockfile会产生 source/version 不一致；design 必须给出成组回滚顺序。

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `p2p-frame/Cargo.toml` 直接依赖 `callback-result`，`sfo-cmd-server 0.4.0/src/client/mod.rs` 使用 `CallbackWaiter<u128, CmdBody>`；迁移改变依赖来源和最低版本但不改变 API 形状 | design 记录依赖 API/行为兼容性、caller 闭包和版本策略；testing 执行正向 consumer 编译及 keyed waiter 行为/发布测试核对 | proposal 已比较本地 keyed 实现与 registry 0.2.5，并定位真实消费者 | owner: design/testing; reason: 具体检查与可运行任务入口属于后续阶段；acceptance impact: 缺少 consumer 兼容或 keyed 语义证据时不得接受 | 发布版中的非 keyed 行为变化可能影响未发现的条件消费者 |
| data/schema | no | 计划路径仅为 Cargo manifests、lockfile 和依赖源码删除；不涉及持久化数据、序列化格式、cache key、迁移或保留策略 | design/diff 确认没有持久化路径进入 Scope Paths | proposal 已检查目标路径与依赖职责 | owner: none; reason: not applicable; acceptance impact: none | none |
| security/privacy/permission | no | `callback-result` 只提供进程内 future/waiter 协调；本任务不改变身份、认证、授权、密钥、TLS、输入信任或日志边界 | dependency review 确认无新增依赖和安全边界变化 | proposal 已检查 crate manifest 与公开实现边界 | owner: none; reason: not applicable; acceptance impact: none | registry supply-chain 风险由 checksum 和精确发布版本审查缓解，但不新增安全语义 |
| runtime/integration | yes | `CallbackWaiter` 的 drop、timeout、ready 和 same-ID replacement 是 `sfo-cmd-server` 响应关联的运行时生命周期；0.2.5 还更新 `SingleCallbackWaiter` cleanup | design 记录新旧行为差异与 failure/rollback；testing 覆盖 keyed waiter 修复存在、consumer 编译和适当任务级回归 | proposal 已确认 keyed 实现等价、额外 SingleCallbackWaiter 差异以及当前直接消费者 | owner: design/testing; reason: implementation 后才能对最终解析结果运行测试；acceptance impact: 生命周期语义或消费闭包未验证则阻塞 | 上游发布内容与本地测试拥有关系变化可能掩盖未来回归 |
| build/dependency/config/deployment | yes | 根 `Cargo.toml` `[patch.crates-io]`、`p2p-frame/Cargo.toml` 版本、`Cargo.lock` source/checksum 与 `third-party/callback-result/**` 将同时变化 | design 给出原子迁移、clean/reproducible resolve、唯一版本检查和整体回滚；testing 验证 dependency tree、lock source/checksum、无残留 path 引用及 consumer build | proposal 已从 crates.io 获取 0.2.5 元数据并确认发布包内容 | owner: design/testing/release; reason: 最终 lockfile 和删除结果尚未实现；acceptance impact: 本地 path 残留、非 0.2.5 解析或不可复现构建均阻塞 | clean 构建依赖 registry 可用性，错误的宽松解析可能回退旧版 |
| ui/datamodel/workflow | no | 目标文件不包含 UI、展示状态、表单、可访问性、本地化或前后端数据模型 | scope/diff 检查确认无 UI 路径 | proposal 边界限定为 Rust 依赖来源迁移 | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | 本任务使用现有 packet/checker/runner，不修改 `harness/**`、模板、AGENTS、CI 或检查器 schema | 仅运行现有阶段门禁并维护任务自身证据 | proposal packet 按现有序号和 scope manifest 创建 | owner: downstream stages; reason: 正常阶段检查由各阶段执行；acceptance impact: 缺少常规门禁证据会阻塞 | none |

## Approval Record

- approver: user
- approval_date: 2026-08-26
- user_statement: "确认，按简单任务修复就好"
