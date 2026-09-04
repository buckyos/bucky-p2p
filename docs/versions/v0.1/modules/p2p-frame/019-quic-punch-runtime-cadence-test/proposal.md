---
module: p2p-frame
task_name: 019-quic-punch-runtime-cadence-test
submodule: 019-quic-punch-runtime-cadence-test
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# QUIC Punch 生产调度路径测试修正 Proposal

## Background and Goal

`017-quic-nat-traversal-improvement` 中名为 `udp_punch_quic_nat_two_sides_continue_beyond_one_second_with_one_connect_future` 的测试使用两个独立计数循环模拟 active/reverse punch。该测试能覆盖 owner future 的生命周期，但没有调用 `QuicTunnelListener::run_udp_punch_burst`，因此不能证明生产 punch 调度在一秒后仍按预期运行。

`018-quic-punch-skip-missed-ticks` 已修正错过 tick 后的调度推进，并用 `udp_punch_next_offset` 的纯函数断言覆盖关键算术。然而这些断言仍未验证生产循环是否实际调用该推进逻辑，也未验证真实循环在 backdated/stalled `started_at` 下不会追赶式连续发送。

本任务只修正测试归属与覆盖：让 cadence 回归测试直接驱动实际 `run_udp_punch_burst`，并把原模拟计数测试收窄为其真正覆盖的 owner/single-connect-future 语义。允许在 `listener.rs` 增加仅 `#[cfg(test)]` 生效的最小发送观测 seam，以便确定性记录发送尝试；release 构建、公开接口和生产调度行为不得改变。

## Scope

### In scope

- 在 `listener.rs` 增加仅测试构建可见的最小发送观测 seam；观测点必须位于 `run_udp_punch_burst` 的实际发送路径，且测试可用它记录发送尝试并避免依赖公网 UDP 环境。
- 在专用 `listener/tests.rs` 中新增或改写确定性测试，直接调用 `QuicTunnelListener::run_udp_punch_burst`，覆盖 active/reverse intent、超过一秒的 attempt elapsed、跨多个错过 interval 的恢复，以及 deadline 内不会历史追赶式连续发送。
- 保留 `udp_punch_next_offset` 的纯函数边界测试，但不得再把纯函数测试单独作为生产循环已经正确接线的充分证据。
- 将 `network/punch_owner_tests.rs` 中模拟 active/reverse 计数循环的测试改名并收窄断言，使名称和结论只声明实际覆盖的 owner lifecycle、晚于一秒的 connect 完成和 single connect future 语义。
- 在 task-local 测试设计与注册中包含实际承载测试编译/执行的模块声明、专用测试文件与统一 runner 映射。

### Out of scope

- 修改 `run_udp_punch_burst` 的 cadence 算法、`udp_punch_next_offset` 算术、`50ms` interval、active/reverse 首包偏移或 connect deadline。
- 修改 punch payload、candidate policy、UDP source socket、send-error best-effort 语义、listener close、connect owner cancellation 或 Quinn connect 行为。
- 新增 release 可见的测试 hook、公开 API、feature flag、环境变量、配置项或依赖。
- 将模拟 owner 测试继续表述为真实 UDP punch cadence、真实 active/reverse 协同发送或生产网络集成证据。
- 修改 `018-quic-punch-skip-missed-ticks` 已完成 packet 或其验收结论；本任务以 sibling correction packet 独立记录测试充分性修正。

### Boundary with neighboring modules

- `p2p-frame/src/networks/quic/listener.rs` 仍拥有生产 punch 循环；本任务只允许加入 test-only 观测边界，不改变 release 路径语义。
- `p2p-frame/src/networks/quic/listener/tests.rs` 拥有对实际 listener punch 循环的确定性验证。
- `p2p-frame/src/networks/quic/network/punch_owner_tests.rs` 只验证 `connect_with_owned_udp_punch` 的组合与生命周期，不再声称验证 listener cadence。
- 其他 crate、wire protocol、SN/PN、tunnel publish 与外部网络环境不在本任务范围内。

## Requirement Review

- 用户选择修正问题 2 而不扩大到其他评审发现，范围合理；需要把“测试运行了一个相似循环”和“测试运行了生产循环”明确分开。
- 仅增加更多 `udp_punch_next_offset` 断言不足以关闭问题，因为生产循环可能遗漏调用、传错 elapsed/deadline，或在发送/终止分支中绕开 helper。
- 直接向文档地址发送 UDP 并计数外部收包会引入路由、平台和 CI 网络差异。最小 test-only 发送观测 seam 可在不改变 release 构建的前提下，让测试确定性经过生产循环的候选检查、时间判断、发送分支和 next-offset 更新。
- test-only seam 必须默认关闭、按 listener/test 实例隔离，并复用生产发送调用边界；不得形成跨测试共享的可变全局状态或让测试实现复制生产循环。
- 原“two sides”测试仍有 owner 价值：它证明 connect future 在一秒后完成时只被驱动一次，且完成会 drop 所属 punch future。应保留该价值，但移除虚构 cadence 的计数器、active/reverse 命名和发送次数断言。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-QPRCT-1 | quic_punch_runtime_cadence_direct_test | 通过最小 `#[cfg(test)]` 发送观测 seam，让确定性测试直接运行 `run_udp_punch_burst`，证明 backdated/stalled attempt 对 active/reverse 各不会补发历史 tick，并验证实际循环使用 deadline 与 next-offset 推进 | 限于 test-only seam、`listener/tests.rs` 与任务测试注册；release 构建和生产方法签名/行为不变 | 测试构建会多一个内部观测分支，但换取无公网依赖、低抖动且能覆盖真实生产控制流的证据 | 测试直接调用实际方法；观测到的发送尝试数量/时序边界能区分“跳过历史 tick”与“零等待追赶”；release 编译与 focused test 均通过 | 不重新实现或修正生产 cadence 算法 |
| P-QPRCT-2 | quic_punch_owner_test_claim_accuracy | 收窄原模拟计数测试的名称、fixture 和断言，使其只证明 owner future 在晚于一秒的 connect 完成时保持 single connect future 并清理 punch future | 限于 `network/punch_owner_tests.rs`；不改变 `connect_with_owned_udp_punch` 生产实现 | 不再由此测试展示 active/reverse 发送计数，但消除错误覆盖声明并保留真实生命周期价值 | 测试不包含自建 cadence 循环或伪发送计数；名称与断言只对应 owner/single-connect/drop 行为 | 不把 owner 测试升级为真实双端 NAT 网络测试 |

## Success Criteria

- 至少一个确定性自动测试直接调用 `QuicTunnelListener::run_udp_punch_burst`，而不是复制或近似其循环。
- direct test 通过 test-only 观测点确认：对 elapsed 已超过一秒并跨过多个 interval 的 active 与 reverse attempt，生产循环不会为每个历史 tick 连续触发发送；测试能在恢复 catch-up 逻辑时失败。
- direct test 仍经过真实 candidate policy、deadline 检查、发送分支和 `udp_punch_next_offset` 接线，且不依赖公网路由、真实远端收包或长时间 sleep。
- 原模拟测试不再使用 active/reverse punch counter 来声明生产 cadence；它只验证一个 connect future、晚于一秒完成和 owned punch future drop。
- release 构建不包含可配置的 observer，不新增公开接口、依赖、配置或生产状态；现有生产调度 diff 在本任务中保持不变。
- task-local `testplan.yaml` 与统一 runner 能定位并执行两个 `change_id` 对应的实际测试，且 evidence inputs 包含测试声明与注册路径。

## Risks

- 如果 seam 放在生产发送函数之外或直接绕过生产循环，测试仍可能产生假阳性；design 必须把观测点绑定到同一个实际发送分支。
- 如果 observer 使用进程级全局状态，并行测试可能互相污染；应采用实例级或严格作用域内的 test-only 注入与自动清理。
- 如果测试只断言最终总数而没有为 deadline/elapsed 选择区分性边界，旧的 catch-up 实现可能也能通过；测试参数必须让旧逻辑产生明显多次发送，而当前逻辑产生有界次数。
- 如果为了测试修改 release-visible 结构、方法签名或发送错误语义，会超出用户授权；后续 design 和 testing scope check 必须明确证明这些边界未变。

## Approval Record

- approver:
- approval_date:
- user_statement: ""
