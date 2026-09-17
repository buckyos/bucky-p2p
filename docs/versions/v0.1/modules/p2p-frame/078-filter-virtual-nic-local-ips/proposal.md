---
task_manifest: task.yaml
status: approved
---

# Filter Virtual NIC Addresses and Cap Local IP Count in DefaultSnLocalIpProvider Proposal

Risk profile: not-created

## Revision 2026-09-17 (returned to draft)
- 修订原因：用户在已批准提案基础上明确，32 个数量上限必须落实在 `report_on_send` 实际上报的 IP 端点数据（`local_eps`）上，而不仅是 `DefaultSnLocalIpProvider::get_local_ips` 的返回值。
- 技术背景：`report_on_send`（`sn_service.rs:1958`）按 `net_manager.listener_info_entries()` 逐监听器展开端点——监听器绑定 unspecified 地址时会把每个本地 IP 展开为一条端点；TCP/QUIC 多监听器并存时，即使 provider 已限 32，`local_eps` 仍可能超过 32（例如双监听器展开 2×32=64 条）。
- 状态：本方案已退回 draft，等待用户对修订后的提案重新确认；确认前不修改项目代码。用户已于 2026-09-17 确认按修订执行，final tier 维持 standard，本提案恢复 approved。

## Workflow Tier Judgment
- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 修改集中在 `p2p-frame` 的 `p2p-frame/src/sn/client/sn_service.rs` `DefaultSnLocalIpProvider` 与 `report_on_send` 上报装载面（虚拟网卡名称过滤增强 + 本地 IP/上报端点数量上限 32），属 bounded 单模块行为修正。不改协议/命令 wire 字段、版本、签名或持久化数据，不新增依赖，不引入并发/生命周期结构。但它改变运行时上报候选（本地 IP 集合影响 SN 上报与 unspecified 监听器的端点展开），存在可预见的连接性影响面，因此定为 standard 而非 trivial；不进入 high-risk。
- Proposal and tier confirmation: 用户于 2026-09-17 确认本提案及 standard tier 执行；识别方式征询结论（已确认）：保持 `if-addrs` 名称过滤扩展，不引入 Linux netlink `IFLA_INFO_KIND` 精确识别或其它平台定制依赖。用户补充确认：IP 列表取前 32 个地址上报，保持 SN 正常上报。2026-09-17 用户进一步明确：`report_on_send` 实际上报的 IP 数据最多 32 个；用户在提案退回 draft 后再次确认，final tier 维持 standard。

## Background and Goal
`p2p-frame/src/sn/client/sn_service.rs:462` 的 `DefaultSnLocalIpProvider` 负责收集本机非回环 IP 列表，供 `report_on_send`（`sn_service.rs:1939`）在监听器绑定到 unspecified 地址时展开成本地端点集合上报给 SN。当前实现有两个问题：

1. 虚拟网卡（VPN、虚拟化、隧道、容器桥接等）的名称过滤不完整：`should_ignore_interface`（`sn_service.rs:465`）只覆盖 `VMware`/`VirtualBox`/`ZeroTier`/`zt`/`Tun`/`tun`/`utun`/`docker`/`lo`/`veth`/`feth`/`V-M`/`br-`/`vEthernet`。常见的 `TAP`、`virbr`、`vmnet`、`vboxnet`、`dummy`、`wg`、`tailscale`、`hamachi` 以及系统隧道适配器（`sit`/`ip6tnl`/`teredo`/`isatap`/`6to4` 等）仍会被当作可用本地 IP 上报。
2. `get_local_ips` 对返回数量无上限：在存在大量网卡（尤其容器/多租户环境）时，上报的本地 IP 列表可能异常膨胀。
3. 仅截断 provider 返回值不足以约束上报载荷：`report_on_send` 按监听器展开 `local_eps`（unspecified 监听器 × 本地 IP），多监听器并存时实际上报端点数可超过 32。

目标：`DefaultSnLocalIpProvider::get_local_ips` 返回非回环、非虚拟网卡的本地 IP，且数量不超过 32 个；`report_on_send` 构造的上报端点 `local_eps` 最多保留前 32 个，保证实际上报 IP 数据不超过 32。

## Scope
### In scope
- `p2p-frame/src/sn/client/sn_service.rs`：扩展 `should_ignore_interface` 的名称过滤集合以覆盖更全的虚拟/隧道/容器适配器模式；将 `get_local_ips` 的结果截断上限收敛为常量（32）。
- `p2p-frame/src/sn/client/sn_service.rs`：保持 `report_on_send` 原有内联组装位置不变，在组装 `local_eps` 后追加 `local_eps.truncate(MAX_LOCAL_IP_COUNT)` 截断至 32，保证实际上报的本地 IP 端点不超过 32；`map_ports`（映射端口）不属于 IP 数据、不参与该上限。按用户 2026-09-17 指示，不抽取独立组装函数。
- 提供可测试的纯函数形态（例如接收 `&[if_addrs::Interface]` 的过滤函数），以便在不依赖真实网卡环境的前提下对虚拟网卡过滤与 32 上限做单元验证。

### Out of scope
- 不修改 `ReportSn`/`SnCall`/wire 字段、命令版本、签名、证书或编解码。
- 不改变 `cyfs-p2p-test` 的 `EmptySnLocalIpProvider` 与自定义 `SnLocalIpProvider` 注入机制。
- 不引入新的接口枚举依赖（保持 `if-addrs`），不做 IPv6 link-local 的另行处理（不属本需求）。
- 不改 SN 服务端接收/去重逻辑，不上报去重/排序策略变化。

### Boundary with neighboring modules
- 仅在 `p2p-frame` 客户端侧 provider 收口；`stack.rs` 的 `set_local_ip_provider` 注入面与 `cyfs-p2p-test` 的 override 机制保持兼容。

## Requirement Review
需求成立且与现有实现方向一致：`DefaultSnLocalIpProvider` 本就把回环与若干虚拟网卡排除在外，本次是收口「虚拟网卡过滤面」并追加数量上限。方案采用名称过滤增强（不引入按 OS flags/网卡类型判断的新依赖），原因：`if-addrs` 0.13 的 `Interface` 结构仅暴露 `name`/`addr`/`index`（Windows 另有 `adapter_name`），无虚拟属性标志可用；名称模式匹配与现有代码同一机制，风险可控。32 上限取已过滤结果的前 32 个；若未来需要优先级策略（如优先 IPv4/物理网卡），属独立需求。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-local-ip-virtual-filter | 扩展 `DefaultSnLocalIpProvider::should_ignore_interface`，过滤更多常见虚拟/隧道/容器/VPN 网卡名称（覆盖 `TAP`/`virbr`/`vmnet`/`vboxnet`/`dummy`/`wg`/`tailscale`/`hamachi` 及系统隧道适配器模式），不误伤物理网卡 | 只改 `sn_service.rs` 内过滤面；命名仍为含子串匹配，与现机制一致 | 纯名称匹配无法覆盖未知/私有网卡类型；过宽匹配可能误过滤名称恰含关键字的物理网卡，需在过滤集合上谨慎定界 | 纯函数单元用例：对构造的 `Interface` 列表，虚拟网卡条目被过滤、普通物理网卡保留 | 不引入按 OS API flags 判断虚拟网卡的新依赖 |
| P-002 | CHG-local-ip-max-32-cap | `get_local_ips` 返回的非回环、非虚拟 IP 数量不超过 32，常量集中命名（如 `MAX_LOCAL_IP_COUNT = 32`）；`report_on_send` 组装的上报端点 `local_eps` 总数不超过 32 | 过滤后再截断；provider 返回点与 `report_on_send` 组装点均应用同一 32 常量；`report_on_send` 保持原有内联位置、仅追加截断；`map_ports` 及协议/端口语义不变 | 超过 32 时丢弃末尾 IP，丢弃顺序与当前枚举顺序绑定；多监听器展开场景由 `report_on_send` 组装点兜底 | 单元用例证明构造 40 个合法 IP 时 provider 仅返回 32 个；`report_on_send` 内联截断经代码审查与全量 lib 套件验证 | 不做 IP 优先级排序/去重策略设计 |

## Success Criteria
- 系统可见结果：`DefaultSnLocalIpProvider::get_local_ips` 不再包含常见虚拟/隧道/VPN 网卡地址；返回数量恒 <= 32。`report_on_send` 实际发送的 `ReportSn.local_eps` 恒 <= 32。
- 所需证据：
  - 新增纯函数单元用例通过：虚拟网卡名称过滤、物理网卡保留、数量截断至 32；`report_on_send` 上报上限以原位置内联 `truncate(MAX_LOCAL_IP_COUNT)` 落实，经代码审查与全量 lib 套件验证。
  - `cargo test -p p2p-frame --features x509 --lib` 通过（含 `sn_service` 现有用例）。
  - `cargo check --workspace` 通过。
- 非目标：不声明生产多宿主/公网环境的完整连通矩阵验证；不验证对未知网卡类型的判定。

## Risks
- 名称过滤集合扩展有误过滤风险：若某物理网卡名含目标关键字（如物理口名含 `tap`/`dummy`），会被误判为虚拟网卡，导致该接口本地 IP 不上报 SN。通过单元用例对常见物理网卡名称（`eth`/`enp`/`wlan`/`en0` 等）做保留断言降低该风险。
- `if-addrs` 平台差异：Windows `adapter_name` 与部分隧道适配器在非 Windows 平台命名不同，过滤集合需保持跨平台可编译并与现平台行为一致。
- 数量截断只保证 <=32，不保证保留哪些 IP；若调用方依赖特定 IP 顺序，需在实现/记录中说明截断发生在既有枚举顺序上。
- `report_on_send` 组装点截断会丢弃后半部分端点（多监听器展开时可能截掉后一个协议的条目）：若未来需要按协议/地址优先级保留端点，属独立需求；本次固定保留既有枚举顺序的前 32 条。
