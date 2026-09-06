# Completion Report: 063-review-tunnel-nat-traversal

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: trivial
- Change record: not-applicable

## Delivery Summary

- Outcome: 已完成当前工作区 tunnel NAT 穿透只读审查，发现两项 P2 正确性问题。检查包含已有未提交的 060/061/062 相关代码；未修改生产或测试代码。
- Handoff: 本报告为审查交付，两个发现未实施修复；任务交付 accepted 不表示被审查的生产逻辑无缺陷。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-review-nat-traversal | 当前建链调用链、两端动作、候选、预测、SN 授权、并发、超时和回退只读审查 | proposal.md Scope、Success Criteria | 下文调用链、F-001/F-002 因果链、定向测试记录与证据局限 | 交付缺陷及可复核源码位置，修复未纳入已确认范围 | pass |

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 063-F-001 | P2 | p2p-frame/src/tunnel/tunnel_manager.rs:1132,1443,1460；p2p-frame/src/sn/protocol/sn.rs:120,134；p2p-frame/src/sn/client/sn_service.rs:860 | ReverseConnectOnly 发送端产生 TCP/QUIC 混合候选，协议却要求整个数组同协议，导致请求在本地 validate 时失败并回退 legacy | no |
| 063-F-002 | P2 | p2p-frame/src/tunnel/tunnel_manager.rs:1475,1594,1709,1792；p2p-frame/src/networks/quic/network.rs:537,687；p2p-frame/src/networks/network.rs:26 | 预测的 listener generation 没有绑定到最终 connect/punch，校验后 listener 替换或连接切换至另一个 listener 时，仍沿用旧源 socket 的预测协调结果 | no |

这里的 blocking 描述是否阻塞只读审查交付，不代表生产问题已修复。

### 063-F-001：混合传输候选与协议约束不一致

触发条件：caller 有静态公网可达证据、callee 非公网且双方 profile 有效，策略选择 ReverseConnectOnly；caller 收集出的合格候选同时有 TCP 与 QUIC（例如公网双 listener，或 QUIC 外网观察地址加 TCP 映射端口）。

`rendezvous_base_endpoints` 对非 punching 操作接受 TCP 与 QUIC，并直接放入同一个数组；`new_rendezvous_request` 随后调用 `request.validate()`，而 `validate_rendezvous_endpoints` 将第一个候选的 protocol 作为整个数组的约束。第二种 protocol 导致 InvalidParam；失败发生在发出 rendezvous 之前。`open_nat_aware_tunnel` 捕获后进入 legacy SnCall。

影响：本应支持的公网反向连接组合无法走 rendezvous；legacy 可能补偿成功，因此不能声称所有连接都失败。用户观察上可能表现为持续回退或由 legacy 决定成功率。

反证检查：已运行的 `reverse_connect_request_candidates_accept_public_wan_and_mapped` 明确断言混合输出合法，但未将该输出交给请求校验；协议的同传输约束也适用于 ReverseConnectOnly，不存在此操作的例外。建议发送端与协议统一候选契约（选定传输/分组协调，或明确调整协议支持范围），并覆盖“候选构造→请求校验”组合。

### 063-F-002：预测与执行 socket 的绑定在协调后丢失

触发条件：使用本端预测的 rendezvous 进行中，预测校验之后、执行 connect/punch 之前，QUIC listener 被关闭并在不同端口重建；或存在多个同地址族 listener，连接在首个 listener 失败后尝试另一个 listener。

预测由 `predict_traversal_endpoints` 选择第一个同地址族 listener，返回其 generation；TunnelManager 在发送请求前（接收方在启动 action 前）校验一次。随后跨 SN 的异步协调结束，执行端仅传普通 `TunnelConnectIntent`，其字段不包含 listener、local endpoint 或 prediction generation。`create_tunnel_with_intent` 重新读取 listener 列表并依次尝试；`punch_only` 也重新选择第一个同地址族 listener。

因此 listener 重建后会从新 socket 发包；多 listener fallback 也会换 socket。对端仍根据旧 socket 的预测端点打洞。在依赖源公网 IP:port 的 NAT 映射/过滤场景下，这组预测不能描述新 socket 的映射，导致本轮打洞失配。已有校验函数可以拒绝旧 generation，但执行阶段没有再次调用，也没有把本轮动作固定到已校验 listener。

本项为完整调用链支持的静态交错缺陷，未新增或运行换 listener 的专门重现测试。现有 generation 测试只直接调用校验函数；真实单 listener 稳定运行不触发此缺陷。建议动作持有原 listener 的绑定并在失效时终止/重新协调，禁止带着旧预测切换源 socket；仅补一次无持有关系的校验仍存在后续竞争窗口。

## Reviewed Call Chain

- `open_tunnel_from_id` → `DefaultDeviceFinder::get_peer_info` → `open_tunnel_from_lookup`。从同一次 Query 取得所用 SN、双方 profile 和目标候选；缺少或失效 profile 走 known/legacy 路径。
- `select_connect_plan`：非对称/非对称由 caller 连接、callee 打洞等待；非对称/对称由 callee 反连、caller 打洞等待；对称/非对称由 caller 连接、callee 打洞等待；双对称由 caller 连接，按两端预测可用性分别选 Base/Predicted。缺预测为 bounded best effort；公网策略独立于 profile 内的 observation，但建链入口先要求 profile 有效。
- `open_nat_aware_tunnel` → `open_rendezvous_tunnel` → SN request/notify/response。SN 请求端使用实际命令 tunnel 的观察 IP 校验；预测响应端由 serving SN 检查目标的实际观察 IP；响应 seq 和形状有校验。response 仅代表 action armed，不是 tunnel 已完成。
- QUIC 探测和辅助 punch 使用 listener 的 UDP socket；普通 QUIC 握手负责实际建链。探测 token、响应来源及签名有独立校验。端口预测有 8 个上限、IP 一致性、有效期和整数边界约束。
- waiter 区分 remote、tunnel、方向；owner 用 token 防止旧完成/清理删除新状态；target action 启动受 attach 屏障保护；caller collision 让位后等待 winner completion。
- rendezvous 失败进入 legacy SnCall，legacy 本地动作失败再尝试 PN。rendezvous 和 legacy 共享外层绝对预算；预算耗尽会终止整个尝试，因此 PN 并不保证每次获得执行时间。未将该既定预算边界列作缺陷。

## Independent Defect Discovery

本节是在初步调用链判断及定向测试之后进行的独立反证轮次；未把“已有测试通过”作为两个发现的消除依据。

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | nat_connect_plan.rs 的四种组合和公网分支、候选构造、sn.rs 校验 | 从混合 TCP/QUIC 输出追到 new_rendezvous_request 的实际 validate，再追到 legacy 捕获点 | F-001 因果链成立；报告限定为 rendezvous 被拒，不夸大为连接必然失败 | pass |
| boundaries-and-failure-paths | network.rs 的 generation 校验、listen/close_all_listener、create_tunnel_with_intent、punch_only | 校验之后替换 listener；检查 intent、owner 和后续 action 是否存在绑定或重新校验；反查测试是否覆盖该交错 | F-002 执行缺少绑定；现有校验测试未覆盖交错。标明静态发现及单 listener 稳态不受影响 | pass |
| regression-and-side-effects | 当前工作区 baseline、既有未提交差异、真实 socket 矩阵成功条件 | 核对无生产/测试编辑；检查 symmetric 行 require_connected=false、跨 SN 用例只声称 armed | 审查未引入生产变更；测试范围和结论边界已明确，未把套件通过写成公网对称 NAT 成功 | pass |

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib tunnel::nat_connect_plan -- --test-threads=1`（5/5）；`cargo test -p p2p-frame --features x509 --lib rendezvous -- --test-threads=1`（39/39）；`cargo test -p p2p-frame --features x509 --test tunnel_rendezvous_protocol -- --test-threads=1`（7/7）；`cargo test -p p2p-frame --features x509,test-real-socket-matrix --test real_p2p_tunnel_flow -- --test-threads=1`（7/7，运行 27.60 秒）。合计本次执行 58 项测试断言入口全部通过，不同过滤套件可能包含重复覆盖。
- Result: pass
- Exception reason: 不新增测试、不部署公网环境；F-001/F-002 以当前源码因果链定位。已有定向测试不等价于这两项反例的专门回归验证。

## Evidence Limits

- 策略矩阵中的三条 symmetric 行在 strategy_matrix.rs:690、701、712 设置 `require_connected: false`，可以在 bounded error 下通过；人工 reflector 返回模拟映射观察，不是真实 NAT 路由器。
- 跨 SN rendezvous 用例验证请求到达与 action armed，不声称端到端穿透及 payload 完成。
- 本次没有真实双公网 NAT、listener 更换期间穿透或双传输 ReverseConnectOnly 的专门端到端重现证据。

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 已交付确认范围内的只读审查，两个生产正确性问题已说明触发条件、完整因果链和证据边界；未实施未经授权的修复。审查交付完成与生产逻辑仍有问题分别记录。
