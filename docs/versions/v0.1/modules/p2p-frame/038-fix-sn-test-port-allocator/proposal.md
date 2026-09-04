---
task_manifest: task.yaml
status: approved
---

# SN 测试端口分配器避开 OS 临时端口区间

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- Final tier confirmation: 用户以连续 `继续完成/继续` 指令确认推进并自动完成
- Tier rationale / triggered boundaries: 修复集中在 p2p-frame 测试资源分配函数与一个回归断言，不改变生产代码、公开契约、协议、依赖或部署面；但影响整套并发测试的稳定性与跨模块共享端口空间，故按 bounded single-project 的 standard 处理而非 trivial。
- Proposal and tier confirmation: task 037 收尾时发现的残留失败 `sn_profile_flow_tests::tcp_only_registration_never_receives_or_executes_probe`，根因是 `sn/tests.rs::next_port()` 从 42000 起顺序分配且不避开本机 OS 临时端口区间（当前 43255-47350），完整并发套件中 SN 固定端口会与其它模块临时端口相撞；用户要求继续完成，故创建 sibling task 038。

## Background and Goal

完整 `p2p-frame --features x509 --lib` 默认并发运行时偶现：

```text
thread 'sn::tests::sn_profile_flow_tests::tcp_only_registration_never_receives_or_executes_probe' panicked
Caused by: Socket(Os { code: 98, kind: AddrInUse, message: "Address already in use" })
```

`sender` 侧证据：`sn/tests.rs` 的 `next_port()` 从 `42000` 递增分配，`NEXT_PORT` 在完整套件下进入 `43255-47350`（Linux 默认临时端口区间），而其它真实 socket 测试通过 `bind(0)` 取得同一区间端口。固定端口与临时端口在同一进程并发时互相踩踏，`AddrInUse` 出现于 `sn_service.start()` 等真实 bind 点，无法用单测隔离复现。

修复方向：让 `next_port()` 始终返回 OS 临时端口区间之外、且与仓库既有固定测试端口区间不重叠的端口。Linux 下读取 `/proc/sys/net/ipv4/ip_local_port_range` 取 `ephemeral_start`，在 `25025..min(43100, ephemeral_start-1)` 内单调分配；macOS/Windows 的临时端口从 49152 起，统一上限 43100 天然避开。这样其它测试的 `bind(0)` 不可能选中 SN 测试端口，跨模块踩踏在结构上消失。

## Scope

### In scope

- 修改 `sn/tests.rs::next_port()`：端口分配恒低于当前平台 OS 临时端口起点（Linux 动态读取；其它平台使用 43100 上限），且高于既有固定测试端口占用（25025 下限），在同一区间内单调分配、回绕可复用。
- 增加一个回归断言：连续分配一组端口后全部位于 `test_port_low()..test_port_high()` 区间（尤其低于该平台 ephemeral start），证明该分配器不会进入临时端口区间。
- 保持现有 `SETUP_MAX_RETRY`/`is_addr_bind_conflict` 语义不变，作为外部进程或极端配置下的兜底重试。

### Out of scope

- 不修改生产 SN/TCP/QUIC/PN 行为、协议、配置或错误映射。
- 不改变其它测试模块的端口使用方式，不把 037 的 guard/reverse 修复改动回退或改写。
- 不为每个 SN 用例增加整拓扑重建脚本；沿用并保留现有 bounded retry 作为兜底。
- 不修改 `cyfs-p2p-test/**`，不使用其产物作为证据。

### Boundary with neighboring modules

- 只改 `p2p-frame/src/sn/tests.rs` 的 `next_port()` 与其回归断言；`sn_profile_flow_tests`、`five_by_five_command_matrix_tests`、`sn_same_sn_tests` 通过该函数间接获得安全端口，无需逐用例改写。
- 仓库其它固定端口（20001-25024、23101、23901 等）仍由各自测试独占；新区间从 25025 起避免重叠。
- task 032 的 `sn_test_bind_conflict_recovery`（P-032-2）与本任务重叠但 032 未完成；本任务作为 sibling 交付该缺陷的端口分配主修复，032 保留原状。

## Requirement Review

修复合理。可选路径包括“bind 冲突后整拓扑重建”和“分配器避开临时区间”。前者已有多个 helper 部分实现，但失败点直接出现在 `next_port()` 固定端口与 `bind(0)` 临时端口相撞，先消除结构性冲突更简单且更稳定；现有 bounded retry 保留为兜底，不会把非 bind 错误吞掉。若未来 OS 配置使新区间也失效，回归断言会立即暴露。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-038-PORT | sn_test_port_allocator_ephemeral_safe_range | SN 测试端口分配恒低于 OS 临时端口起点并高于既有固定端口区间，完整并发套件不再出现 SN 侧 `AddrInUse` | 仅 `sn/tests.rs` 分配函数与回归断言；生产行为与既有 bounded retry 不变 | 用平台感知端口区间换取结构上的无冲突，Linux 动态读取，其它平台用 43100 上限 | 回归断言验证 64 个端口全部低于 ephemeral start；`sn_profile_flow` 组与完整 lib 套件默认并发多次通过 | 不整拓扑重建全部用例、不吞非 bind 错误、不改其它模块 |

## Success Criteria

- Concrete system-visible result: `sn_profile_flow_tests` 及完整 `p2p-frame --features x509 --lib` 默认并发运行多次不再出现 SN 固定端口 `AddrInUse`；037 的两个点名 reverse 用例保持通过。
- Required evidence: 端口区间回归断言 red→green；`sn_profile_flow` 组重复运行通过；完整套件默认并发与 4 线程各至少一轮通过；保留用户原始失败日志与 037 的验证结果。
- Explicit non-goals: 不宣称跨机器/共享 CI runner 之外的 OS 配置全部覆盖；不改变生产端口/绑定语义。

## Risks

- 若某环境把 ephemeral_start 配置到 25025 以下或读取失败，回退上限 43100 仍可能进入临时区间；回归断言会失败并暴露，需要该环境单独处理。
- 固定测试区间可能与外部进程抢占；belonging retry（现有 `SETUP_MAX_RETRY`/`is_addr_bind_conflict`）继续作为兜底，但本任务不新增吞错逻辑。
- 0 与 65535 边界、回绕复用极端情形下可能出现同进程内二次分配；设计为连续多轮套件不会耗尽区间（约 7700-18000 端口）。

## Approval Record

- approver: user
- approval_date: 2026-09-02
- user_statement: "继续完成（连续多次继续指令）"
