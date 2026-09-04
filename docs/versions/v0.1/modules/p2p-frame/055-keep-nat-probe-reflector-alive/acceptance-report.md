# NAT Probe Reflector 发送错误恢复 Acceptance Report

## Findings
| ID | Severity | Owning Stage | Correctness Category | Evidence | Problem | Blocking |
|----|----------|--------------|----------------------|----------|---------|----------|
| F-055-A1-000 | none | none | overall | 独立 reviewer 检查 `NatProbeReflector::run`、SN service owner、专用回归测试、testplan 及 red/green artifacts 后未发现任务范围内缺陷 | no finding | no |

## Object and Scope
- Task manifest: `task.yaml`
- Review date: 2026-09-04
- In-scope implementation: `p2p-frame/src/sn/nat_probe.rs` 的 `NatProbeReflector::run` 与 `send_response`；`p2p-frame/src/sn/service/service.rs` 的既有 task owner；`p2p-frame/tests/unit/sn_tests/nat_probe/tests.rs` 的失败后存活回归；当前 proposal、pipeline plan、risk profile、testplan 与 task-run artifacts
- Review mode: independent falsification；reviewer 未参与 implementation/testing，先检查当前源码和反例，再选择结论

## Requirement Coverage
| change_id | Requirement or Boundary | Source | Implementation Evidence | Finding | Status |
|-----------|-------------------------|--------|-------------------------|---------|--------|
| CHG-keep-nat-probe-reflector-alive | 单次 UDP response 发送失败只丢弃当前响应并继续同一 reflector/socket；不重试该响应；后续合法请求仍得到有效签名响应；recv、协议、签名限流及 service supervisor 不变 | `proposal.md` P-001、Scope、Success Criteria；`pipeline/plan.md` Acceptance Baseline、State Ownership、Failure Flows | `nat_probe.rs` 的实际 send error 分支记录 local/target/error 后 `continue`；`recv_from` 仍以 `?` 返回；`service.rs` 启动/持有逻辑未改；`reflector_survives_one_udp_send_failure_without_retrying_it` 在同一 JoinHandle 上验证 error 后第二个真实 UDP signed response 与 attempts==2；red artifact exit 101、green artifact exit 0 | 未发现缺失行为或越界改动 | pass |

## Independent Defect Discovery
| Category | Applicable Scope | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|------------------|--------------------|-------------------|----------------------------------|--------|
| requirement-and-behavior | P-001 的 request-local send recovery 与明确非目标 | approved proposal；pipeline Acceptance Baseline；`nat_probe.rs:256-297` | 搜索 send error 仍向外返回、当前响应被重试、外层被改为重启，以及 recv error 被误放宽的反例 | send Err 只记录并继续；当前 response 不重试；service 与 recv terminal 边界未改 | pass |
| logic-and-control-flow | receive、validate、sign、send、下一轮 receive 的全部相关分支 | `NatProbeReflector::run` 与 `send_response` | 跟踪 send success、send Err、sign reject、sign Err、invalid request 和 recv Err 的后继路径 | 只有 send Err 从终止改为下一轮；成功自然下一轮，其他分支保持原语义 | pass |
| boundary-and-input | 任意 `std::io::Error`、合法/非法 PNAT 请求、IPv4 与固定包长 | `nat_probe.rs:259-296`；回归的两个固定长度 token | 检查错误种类是否漏接、输入校验顺序是否改变、响应 B 是否可能来自 A | 所有 send I/O Err 进入同一恢复分支；校验仍在发送前；测试断言 token B、source、observed 与长度 | pass |
| state-and-data-integrity | reflector socket/task 状态和测试注入状态 | `NatProbeReflector` fields；`send_response`；回归测试 | 检查失败后换 socket、重复响应、生产共享状态、测试状态泄漏到非测试构建 | 同一 socket/task 继续；attempts==2 排除重试；注入字段和方法全部受 `cfg(test)` 限制 | pass |
| error-handling-and-recovery | send、local_addr logging fallback、recv 与 signing 错误 | `nat_probe.rs:259-297` | 检查日志辅助失败覆盖原始 send 错误、错误被吞后任务不可用、recv 故障忙循环 | local_addr 失败有退化日志且仍保留 send error；send 后继续可用；recv 仍返回防止无退避自旋 | pass |
| resource-lifetime-and-cleanup | bound socket、run future、service task handle 与测试 abort | `nat_probe.rs` reflector ownership；`service.rs:2018-2041`；回归 JoinHandle | 检查 send Err 后 socket/task 被 drop、重复 spawn、detached retry 或取消失效 | 分支不退出 scope、不 spawn；原 task handle 与 stop ownership 未改；测试在结束时 abort owner | pass |
| concurrency-and-ordering | 串行 receive/sign/send 顺序与测试观测同步 | production loop；AtomicBool/AtomicUsize test-only seam；回归等待逻辑 | 检查第二请求先于首个 error、计数可见性、并发 retry 或新锁死锁 | 测试等待 attempts==1 后才发 B；SeqCst 观测明确；生产未新增锁、并发任务或 retry | pass |
| interface-and-compatibility | public `NatProbeReflector::run` 与现有 caller | `pipeline/plan.md` Exported Interfaces；`service.rs:2034-2037`；QUIC listener test caller；green x509 lib build | 检查签名、导出、wire、调用方式和返回类型变化 | 公开 run 方法的参数、返回类型和导出路径未变；现有调用方编译；仅 send error 的运行时语义按 proposal 放宽 | pass |
| security-and-capacity | amplification、签名预算、持续故障与日志/重试容量 | 固定 1200-byte 常量；共享签名 budget；send error branch | 检查失败响应被无限重试、绕过签名限流、放大包或新增无界状态 | 不重试；每次 send 前仍受共享 128/sec 签名 admission 和固定包长约束；无新增生产队列/状态 | pass |
| test-adequacy | error、recovery、normal、lifecycle、compatibility 与证据真实性 | dedicated test；testplan；red `20260904T074931Z`；green `20260904T075049Z` | 检查零测试、旧 `?` 仍能通过、只断言 handle、首响应冒充次响应、隐式 retry 与 feature 未启用 | 精确 x509 test 真执行 1 case；旧终止语义 exit 101；green 验证第二 token 的 source/observed/signature 且 attempts==2，同一 handle 前后均存活 | pass |

## Document Consistency
| Document | Source | Implementation Consistency | Finding | Status |
|----------|--------|----------------------------|---------|--------|
| design | `pipeline/plan.md` | 实现遵守 request-local recovery、同一 socket owner、无 retry、recv terminal、公开签名兼容及最小 scope | 未发现 design-to-code mismatch | pass |
| testing | `testplan.yaml`、pipeline runtime coverage、red/green artifacts | 测试命令、change_id、unit/DV 注册、x509 feature、实现文件和实际 artifact inputs 一致；未使用 `cyfs-p2p-test` | 未发现零测试、过宽证据或文档/测试不一致 | pass |

## Result Summary
- Overall result: accepted
- Outcome: 一次 NAT probe UDP response 发送错误不再结束 reflector；同一任务和 socket 可继续响应下一合法请求，失败数据报不重试
- Blocking issues: none
- Residual validation boundary: 本地确定性 loopback 测试未复现真实公网 ENOBUFS、接口撤销或路由故障，也不证明部署环境的网络恢复；它直接验证这些错误到达同一 I/O Err 分支后的生命周期语义
- Next action: parent-orchestrator 可记录 accepted runtime state、验证完整 pipeline 并移除 unfinished-task index entry

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: 独立缺陷搜索未发现 requirement、control-flow、recovery、resource、compatibility、capacity 或 test-adequacy 缺陷；red/green 证据能区分旧终止行为与当前失败后继续行为，且没有超出批准边界
