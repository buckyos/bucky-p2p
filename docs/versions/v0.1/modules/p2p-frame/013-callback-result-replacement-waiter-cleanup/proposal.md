---
module: p2p-frame
task_name: 013-callback-result-replacement-waiter-cleanup
submodule: 013-callback-result-replacement-waiter-cleanup
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# Callback Result Replacement Waiter Cleanup Fix Proposal

## Background and Goal

已批准任务 `sn-client-qa-correlation-fix` 为 crates.io `callback-result 0.2.4` 增加了本地 drop cleanup：`ResultFuture` 在完成前被丢弃时，会从 `CallbackWaiter.result_notifies` 删除对应 `callback_id`，避免未轮询 future 留下 tombstone。

当前实现只按 `callback_id` 删除，没有确认 map 中的当前注册是否仍属于执行 cleanup 的旧 future。旧 waiter 收到结果后，`set_result` 会把同一 key 的值取成 `None`；此时 API 允许同一 `callback_id` 注册 replacement waiter。若旧 future 随后完成或被 drop，它的 cleanup 会无条件删除该 key，从而误删 replacement waiter。之后给 replacement 发送结果会返回 `WaiterError::NoWaiter`，造成可确定复现的丢唤醒和请求超时。

本任务是对已批准 `sn-client-qa-correlation-fix` 的序号化 sibling fix。目标是让每个 cleanup 只删除自己拥有的 waiter 注册实例：旧 future 的完成、超时或 drop 不得删除同 ID 的后继注册，同时保留取消前未轮询 future 的同步清理能力。

## Scope

### In scope

- 修正 vendored `callback-result::CallbackWaiter` 的 waiter 注册所有权，使 cleanup 同时校验 `callback_id` 与具体注册实例/代次。
- 覆盖 `create_result_future` 与 `create_timeout_result_future` 两种构造路径。
- 保证旧 future 正常完成、结果已送达后再 drop、未首次 poll 即取消、timeout、以及 replacement 注册与旧 cleanup 交错时，只有旧注册被清理。
- 保持 `set_result`、`set_result_with_cache`、FIFO cache、`AlreadyExist`、`NoWaiter` 与 `Timeout` 的既有公开语义。
- 增加确定性回归测试，直接复现“旧 waiter 已收结果 -> 同 ID replacement 注册 -> 旧 future drop/完成 -> replacement 仍收到结果”的顺序。

### Out of scope

- 不修改 `p2p-frame` 的 SN client/service、command QA framing、业务 response 校验或 tunnel 行为。
- 不改变 `callback-result` 的公开类型、方法签名、crate 名称、版本或依赖版本。
- 不修改 `SingleCallbackWaiter`；当前缺陷来自 keyed `CallbackWaiter` 对同一 `callback_id` 的 replacement 删除。
- 不重写 `notify-future`，不新增异步 runtime、后台清理任务、全局 generation allocator 或定时扫描。
- 不借本任务处理既有 cache 容量策略、callback ID 分配/回绕或其他未复现的通用 command runtime 行为。

### Boundary with neighboring modules

- `third-party/callback-result/src/lib.rs` 拥有 keyed waiter 注册、结果交付和 cleanup 所有权。
- `third-party/callback-result/tests/drop_cleanup.rs` 在 post-implementation testing 阶段承载 replacement、drop、ready、timeout 与既有 bounded-retention 回归。
- 根 `Cargo.toml` / `Cargo.lock` 已把 crates.io 包指向同名同版本 vendored crate；本修复不需要改变该解析边界。
- `sfo-cmd-server` 与 `p2p-frame` 继续仅消费现有公开 API，不感知内部注册 token/代次。

## Requirement Review

- 用户要求修复该问题是合理且必要的：现有实现解决了未轮询 future 的残留，却没有满足已批准 design 明确要求的“旧 cleanup 不得删除 replacement callback”。
- 只禁止同 ID replacement 或延迟 cleanup 会改变公开行为并掩盖竞态，不是合适修复。
- 选定方向是为每次 map 注册建立私有、不可混淆的所有权身份，并让完成/drop cleanup 采用条件删除；具体 token 表示和条件删除结构由 design 决定。
- cleanup 必须保持同步且有界，不能依赖旧 future 内部 notify 先被 drop、调度顺序或固定等待时间。
- 该修复影响通用依赖行为，testing 除原始内存保留用例外，必须覆盖同 ID replacement 的两个旧 future 终态：未消费结果时 drop，以及消费到 Ready 后的正常完成。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-CRRWC-1 | callback_result_replacement_waiter_cleanup | `CallbackWaiter` 的每个 cleanup 只能移除自己拥有的注册；同 ID replacement 一旦成功注册，旧 future 的 ready/timeout/drop 均不得删除或完成 replacement，且未轮询取消仍须立即释放旧注册 | vendored `callback-result` keyed waiter 内部状态与 task-owned regression tests；公开 API 和所有消费者保持不变 | 需要为 map entry 引入私有注册身份并执行条件删除，内部状态略增；接受该复杂度以消除确定性丢唤醒 | 确定性测试覆盖普通与 timeout 构造器、旧 future drop 与 ready 两条清理路径、replacement 成功交付、late old cleanup 不影响新 waiter，以及原 bounded-retention/normal/timeout 测试继续通过；`p2p-frame` 消费者编译或任务级验证通过 | 不禁止 replacement、不改 callback ID 分配、不改 SN/QA 协议、不重写 cache 或 `SingleCallbackWaiter` |

## Success Criteria

- Concrete user-visible or system-visible result: 旧 waiter 收到结果或被取消后，同一 `callback_id` 注册的新 waiter 不会再被旧 future 的 cleanup 删除；第二次 `set_result` 成功，replacement future 收到正确结果而不是超时或 `NoWaiter`。
- Required evidence: approved design 定义注册身份、条件删除原子性、ready/drop/timeout 顺序和具体 `Scope Paths`；post-implementation testing 具备当前最小复现的 red-green 用例、两种 future 构造器覆盖、既有 cleanup 回归及实际消费者编译/任务入口证据。
- Explicit non-goals: 不改公开 API、crate 版本、SN 业务代码、QA wire、callback ID 生成、cache 策略、`SingleCallbackWaiter` 或 Harness 规则。

## Risks

- 如果条件删除的身份可复用或比较不完整，旧 cleanup 仍可能误删 replacement，丢唤醒问题会保留。
- 如果 normal-ready 路径先无条件删除再解除 drop cleanup，仍可能与同 ID replacement 交错；design 必须让“确认 owner 并删除”在同一 mutex 临界区内完成。
- 如果 `set_result` 取走 notifier 后过早释放注册身份，replacement 可能在旧 future 尚未拥有安全清理能力时覆盖 slot；状态转换必须明确区分已交付但尚未消费与已清理。
- vendored patch 被工作区所有 `callback-result 0.2.4` 消费者共享；即使公开 API 不变，也需要实际消费者编译证据和回滚说明。
- 仅保留 4096 个不同 ID 的内存测试无法发现同 ID replacement 竞态；若未新增顺序测试，验收不得以旧测试全绿判定完成。

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `third-party/callback-result/src/lib.rs` 的 `CallbackWaiter` 公开方法允许结果交付后再次注册同一 callback ID；本修复改变内部 cleanup 对 replacement 的语义正确性，但保持 API 形状不变 | design 记录公开行为不变、owner 条件删除和调用方影响；testing 覆盖正向 replacement、旧 cleanup 负向隔离与 error 语义 | proposal 已用当前源码和确定性最小复现确认旧 cleanup 会删除 replacement | owner: design/testing; reason: 条件删除结构与可运行测试属于后续阶段；acceptance impact: 缺失同 ID replacement 证据时不得接受 | 其他消费者可能依赖相同 ID 的快速复用 |
| data/schema | no | 改动只涉及进程内 `HashMap` waiter 状态，不涉及持久化数据、序列化格式、cache key 格式或迁移 | design/diff 确认无持久化路径进入 Scope Paths | proposal 已检查 vendored crate 状态仅为内存结构 | owner: none; reason: not applicable; acceptance impact: none | none |
| security/privacy/permission | no | waiter 不处理身份、权限、secret、PII 或租户边界；结果类型与 callback ID 信任边界不变 | source/diff 审查确认未扩展输入或日志 | proposal 已限定为内部所有权修复 | owner: none; reason: not applicable; acceptance impact: none | none |
| runtime/integration | yes | `ResultFuture::poll/drop`、`CallbackWaiter::set_result` 与 replacement 注册可并发/交错，当前可导致丢唤醒和 timeout | design 描述 mutex 内原子性和所有终态；testing 覆盖 ready/drop/timeout/replacement 顺序、重复结果与实际消费者 | proposal 已确定性复现 replacement 被旧 drop 删除并得到 `NoWaiter` | owner: design/testing; reason: 实现前不能生成 post-implementation 测试；acceptance impact: 任一 owner/drop 顺序未覆盖则阻塞 | 极端调度顺序可能暴露遗漏的 owner 转移 |
| build/dependency/config/deployment | yes | 根 `Cargo.toml` 的 `[patch.crates-io]` 将工作区消费者绑定到 `third-party/callback-result 0.2.4` | design 记录同名同版本兼容与回滚；testing 运行 vendored crate 测试并验证至少一个真实 workspace 消费者编译 | proposal 已确认不需要版本、lockfile或依赖解析变化 | owner: design/testing; reason: clean/relevant compile 属于实现后验证；acceptance impact: vendored crate 或消费者不编译则阻塞 | 本地 patch 必须与仓库一起交付 |
| ui/datamodel/workflow | no | 仓库无该依赖对应的 UI、展示模型、可访问性或前端 workflow 修改 | changed-path 和消费者审查确认无 UI surface | proposal scope 排除 UI 和业务 workflow | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | 本任务消费现有 packet/admission/testing/acceptance 流程，不修改 `harness/**`、模板、checker、CI 或规则 | 后续只运行现有 task-scoped checks | proposal 文件与任务索引是治理制品，不改变治理行为 | owner: none; reason: not applicable; acceptance impact: none | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
