# Tunnel NAT 打洞优化测试补充

本补充文档把 `design/tunnel-nat-traversal.md` 中的单 SN NAT 打洞优化落地为可运行验证。真实 NAT 成功率无法在 unit 层完全断言，因此 unit 重点覆盖调度、候选、退避和评分规则；DV/integration 负责确认现有运行时和下游兼容性不被破坏。

## Unit 验证矩阵

| 行为 | 建议或现有测试 | 预期结果 |
|------|----------------|----------|
| hedged direct/reverse 统一短延迟 | 待补 `nat_quic_candidates_use_short_reverse_delay` | direct 与 reverse 使用同一 `tunnel_id`，reverse 调度延迟固定为 300ms，不再按 endpoint 类型分成 2 秒窗口 |
| QUIC UDP punch 单次连接开关默认关闭 | 待补 `tunnel_connect_intent_controls_udp_punch_per_connection_with_default_off` | 默认 `TunnelConnectIntent` 不发送 punch，`TunnelManager` 只有在 SN service 存在且候选符合策略时才为本次 candidate intent 开启 punch |
| QUIC/UDP NAT 候选发送同源 UDP punch | 待补 `quic_nat_candidates_send_same_socket_udp_punch` | punch 从 QUIC listener 同一本地端口发送，只对非 WAN UDP 候选启用，并带有发送次数和间隔上限 |
| UDP punch 发送失败不改变建链结果 | 待补 `udp_punch_send_failure_is_best_effort` | punch 失败只产生诊断，不让 `connect_with_ep(...)` 提前失败，也不改变后续 QUIC handshake 成败判定 |
| UDP punch 不引入接收解析或业务载荷 | 待补 `udp_punch_has_private_payload_and_no_receive_path` | punch payload 只包含 magic/version 与 tunnel/candidate 关联信息，不被上层读取，不新增 raw UDP receive/ack 路径 |
| `SnCall` 携带本次建链候选 | 待补 `reverse_path_passes_reverse_endpoints_to_sn_call` | `SNClientService::call(...)` 收到非空 `reverse_endpoint_array`，候选来自本地 listener / SN observed WAN / 映射端口组合集合 |
| SN called 保留调用方候选并扩展单 SN 观察候选 | 待补 `sn_call_preserves_caller_reverse_endpoints_before_observed_endpoints` | `SnCalled.reverse_endpoint_array` 前段保留调用方传入候选，后续可追加单 SN 观察和映射端点 |
| proxy 新建后进入短窗口升级 | 待补 `proxy_upgrade_starts_with_short_nat_retry_window` | 新 proxy candidate 的下一次升级时间约为 15 秒，而不是 5 分钟，且不抢占 PN 首次 open 的 5 秒响应窗口 |
| proxy 短窗口耗尽后进入有上限退避 | 待补 `proxy_upgrade_short_window_falls_back_to_capped_backoff` | 15s/30s/60s/120s 后进入指数退避，最大不超过 2 小时 |
| 升级路径不把 proxy 视为成功 | 现有 `stored_proxy_upgrade_dials_direct_instead_of_reusing_proxy` 继续承担，必要时补充断言 | 脱代理尝试调用 direct/reverse，不能通过再次创建 proxy 清理 upgrade 状态 |
| endpoint 评分按协议隔离失败 | 待补 `endpoint_score_isolated_by_protocol` | TCP endpoint 失败不降低同地址 QUIC/UDP endpoint 的排序 |
| 无多 SN fanout | 待补 `nat_traversal_uses_single_sn_call_path` | 单次 reverse path 不并发向多个 SN 发送 call，不依赖跨 SN 观察结果 |

## DV 验证

DV 继续使用 `python3 ./harness/scripts/test-run.py p2p-frame dv`，即 `cargo run -p cyfs-p2p-test -- all-in-one`。

通过标准：
- 现有 all-in-one 场景仍能完成。
- 默认 proxy 兜底仍可用。
- NAT-aware 调度不要求环境存在真实 NAT，也不要求 DV 断言公网打洞成功率。
- 同源 UDP punch 默认关闭；SN service 存在且本次 candidate intent 开启时，DV 只要求不破坏 all-in-one 场景，真实 NAT 映射是否因此改善不作为自动断言。

## Integration 验证

Integration 继续使用 `python3 ./harness/scripts/test-run.py p2p-frame integration`，即 `cargo test --workspace`。

通过标准：
- `cyfs-p2p`、`cyfs-p2p-test`、`sn-miner-rust` 不需要实现多 SN 或新的公开 tunnel trait。
- `SNClientService::call(...)` 的兼容调用路径仍可用，未传候选时保持当前字段语义。
- `TunnelNetwork` / `Tunnel` trait 不新增 NAT 专用参数。
- QUIC UDP punch 不要求下游使用方处理 raw UDP payload，也不要求下游新增 socket；默认关闭，只有 `TunnelManager` 在 SN service 存在且候选符合策略时才为本次 `TunnelConnectIntent` 开启。

## 缺口与边界

- unit 只能验证策略，不证明任意真实 NAT 环境下成功率提升。
- 如果实现新增可配置延迟或候选新鲜度参数，必须补充默认值和边界值测试；当前设计不维护候选新鲜度窗口。
- 如果实现需要改变 `SnCall` 字段编码或 `SnCallResp` 语义，必须退回 proposal；本测试文档不覆盖该类协议重写。
- 如果实现无法在 QUIC listener 同一 socket/端口上发送 punch，必须记录缺口或退回 design；测试不得接受“新建不同源端口 UDP socket”作为等价覆盖。
