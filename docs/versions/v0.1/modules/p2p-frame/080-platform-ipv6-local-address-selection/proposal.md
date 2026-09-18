---
task_manifest: task.yaml
status: approved
---

# Platform IPv6 Local Address Selection Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: standard
- Final tier: high-risk
- Tier rationale / triggered boundaries: 变更面为 `p2p-frame` 客户端本地地址采集（`DefaultSnLocalIpProvider` → 平台化 provider），只改“哪些本地地址进入既有上报装配面”，不改 wire/协议/命令字段、不改持久化数据、不改 SN 服务端 `sanitize_reported_endpoints` 语义（079 已冻结）、不新增 crate（Windows 复用既有 `winapi` 依赖的 `iptypes`/`iphlpapi` feature）。按孤立评估它与 078 同量级（`standard`）。最终升为 `high-risk` 的原因是用户在同一确认中显式要求“自动完成”，即启动 auto-pipeline：`harness/rules/task-entry-gate-rules.md` 规定显式 auto-pipeline 请求按 `high-risk` 工作流执行，`harness/rules/auto-pipeline-rules.md` 同时要求 design/implementation/testing/acceptance 以独立子任务与运行时状态推进，并产出 `pipeline/plan.md`、`risk-profile.yaml`、`testplan.yaml`、`acceptance-report.md`。因此本任务使用 high-risk 全流程与自动流水线，不另行请求升级确认。
- Proposal and tier confirmation: 用户于 2026-09-18 确认本提案及 `自动完成` 启动语句。启动语句原文：`确认，自动完成`。确认同时表示接受提案正文中列出的建议裁决（原“待裁决问题”1–4），记录见文末“确认与裁决记录”。

## 确认与裁决记录
- 平台范围：本任务包含 Linux（运行时验证）与 Windows（编译验证，`cfg(target_os = "windows")`）两条采集源；macOS/其他维持 base 语义。
- 依赖方式：Windows 使用既有 `winapi` 依赖新增 `iptypes`/`iphlpapi` feature，不引入 `windows` crate，不改动依赖集合。
- 范围拆分：P3 活性探测、地址变化触发即时上报、`endpoint_scores` 增加 local-source 维度均拆为后续独立任务，本任务不做；本任务只做采集/过滤/排序/上报内容。
- 端点截断公平性：保持 078 已确认的 `report_on_send` 装配与 32 截断现状，本任务不改。
- tier：`high-risk`（依据上文 auto-pipeline 触发边界）。

## Background and Goal

现状：客户端本地 IPv6 采集只做“名称黑名单 + loopback/unspecified/multicast”过滤，然后把网卡上的**全部**地址交给上报面。

- `p2p-frame/src/sn/client/sn_service.rs:456-460`：`SnLocalIpProvider` trait（保持不变）。
- `sn_service.rs:462-527`：`DefaultSnLocalIpProvider::filter_local_ips` 按 `should_ignore_interface` 名称黑名单过滤虚拟/隧道网卡，再按 loopback/unspecified/multicast 过滤，`take(MAX_LOCAL_IP_COUNT=32)`（`sn_service.rs:464`、`516`）。
- `sn_service.rs:582`：`SNClientService::new` 的构造点固定为 `Arc::new(DefaultSnLocalIpProvider)`；`stack.rs:582` 的 `set_local_ip_provider` 是保留的自定义注入口。
- `sn_service.rs:1976-1991`：`report_on_send` 对每个 unspecified 监听器展开 `local_ips × listeners`，再对**端点**列表 `truncate(32)`（`127` 个地址 × 2 监听器会先展开再截断）。

本机实测（`company-wgr`，`/proc/net/if_inet6` + `ip -6 addr show eth1` 交叉验证）证明今天确实在上报失效地址：

| 地址 | /proc flags | `ip -6 addr` 状态 |
|------|-------------|-------------------|
| `240e:3bc:308f:24c0:c7f7:43da:1d1:d30a` | `a2` | `deprecated`，`preferred_lft 0sec` |
| `fdc8:b144:c39b:0:d27c:e2e8:997f:1138` | `a2` | `deprecated`，`preferred_lft 0sec` |
| `240e:3bc:308f:24c0::77`、`240e:...:1031:5a3:3ece:306`、`fdc8:b144:c39b::77`、`fdc8:...:e816:bb17:faac:8ce9` | `82` | permanent，`preferred_lft forever` |

目标（沿用用户方案的成功信号）：
1. 端点数收敛：每接口 1–2 个稳定首选地址进入上报；
2. `deprecated` / `tentative` / DAD 失败地址不再出现在报告中；
3. Linux 与 Windows 同样增强，macOS/其他维持现状（回退）；
4. 不执行 `ip -6 addr del`，不改内核地址表。

## Scope

### In scope
- `SnLocalIpProvider` trait 与 `stack.rs::set_local_ip_provider` 保持不变；新增 `EnhancedLocalIpProvider`（`base: DefaultSnLocalIpProvider` + `source: PlatformSource`），在 `SNClientService::new` 按 `cfg(target_os)` 选择，其他平台仍为 base 语义。
- 平台无关的 `AddressMeta`（flags / dad_state / preferred_lft / valid_lft / prefix_source / temporary）与**纯函数**过滤 + 确定性排序，作为 Linux/Windows 共用判定面。
- Linux 数据源：解析 `/proc/net/if_inet6`（无新依赖），bits 按内核真实定义（见 Requirement Review 的更正）；解析与判定写成可注入字符串的纯函数。
- Windows 数据源：`GetAdaptersAddresses`，**复用既有 `winapi` 依赖**（新增 `iptypes`/`iphlpapi` feature），把 `IP_ADAPTER_UNICAST_ADDRESS_LH` 转成 `AddressMeta`；转换函数为纯逻辑、可用 mock 数据单测。
- 其他平台（macOS 等）：`EnhancedLocalIpProvider` 只走“步骤 1 基础过滤 + 步骤 4 截断”，并在模块文档注明平台 API 限制。
- 不变量：增强后的输出集合必须是旧输出的**子集**（只少不增）；`MAX_LOCAL_IP_COUNT=32` 语义保留。

### Out of scope
- P3 活性探测（向 SN/回显端点发探针识别“内核 preferred 但上游不路由”的残留前缀）。
- 地址变化（`RTM_NEWADDR`/Windows 地址变更通知）触发即时 `report_on_send`。
- `endpoint_scores` 增加 local-source 维度的直连记忆（现 key 只有 `(protocol, addr)`，见 `tunnel_manager.rs:137-152`）。
- SN 服务端 `sanitize_reported_endpoints` 分类语义（079 已确认冻结）、rendezvous/punch 的 IPv4-only 候选资格。
- 任何形式的 `ip -6 addr del` / 系统地址表写操作。

### Boundary with neighboring modules
- 078 的网卡名称黑名单与 32 上限保留，本次只在“地址选择”层面增强。
- `NetManager::listener_info_entries`（`networks/net_manager.rs:184`）与监听器注册路径不改。
- 上报装配面（`report_on_send` 的监听器展开与 32 截断）本次不改；是否修正“跨监听器截断公平性”见待裁决问题 4。

## Requirement Review

方向成立：用内核状态（DAD/lifetime/flags）取代名称猜测来选择上报地址，是把“10+ 个端点/接口”收敛到“1–2 个可用首选”的正确办法。但方案中有三处必须先更正，否则目标 2 无法达成：

1. **Linux flags 位映射错误（阻塞级）**。方案写“0x04 deprecated、0x08 tentative、0x40 anycast、0x80 global”。内核真实定义（`/usr/include/linux/if_addr.h:45-58`）是：`0x01 SECONDARY/TEMPORARY`、`0x02 NODAD`、`0x04 OPTIMISTIC`、`0x08 DADFAILED`、`0x10 HOMEADDRESS`、`0x20 DEPRECATED`、`0x40 TENTATIVE`、`0x80 PERMANENT`；且该字段**没有 anycast 位**（anycast 是 `IFA_ANYCAST` 属性，不是 flag）。按方案给定的映射实现，本机两条 `0xa2` 地址（`0x80|0x20|0x02`）会因为“没有 0x04/0x08”而被完整保留，成功信号 2 直接落空。本机实测 `a2 ⇔ deprecated`、`82 ⇔ permanent` 与内核定义一致，可直接作为回归基准。
2. **flags 字段必须按 u32 解析**。`/proc/net/if_inet6` 第 5 列以 `%02x` 打印内核 `ifa_flags`，高位例如 `IFA_F_MANAGETEMPADDR(0x100)` / `IFA_F_STABLE_PRIVACY(0x800)` 会输出成 3 位十六进制（如 `100`/`800`）。用 `u8::from_str_radix` 解析会在这些行上失败甚至整行丢弃，实现必须用 `u32`。
3. **Windows 不需要新 crate**。方案要求“用 windows crate”，但 `p2p-frame` 已经依赖 `winapi = 0.3.9`（`p2p-frame/Cargo.toml:18`）。本地 registry 源码验证：`winapi-0.3.9/src/um/iptypes.rs:77-88` 的 `IP_ADAPTER_UNICAST_ADDRESS_LH` 已包含 `PrefixOrigin`/`SuffixOrigin`/`DadState`/`ValidLifetime`/`PreferredLifetime`/`OnLinkPrefixLength`，`src/um/iphlpapi.rs:337` 有 `GetAdaptersAddresses`，且 `iphlpapi`、`iptypes` 都是该 crate 的现成 feature。用 `winapi` 可避免新增依赖与锁文件变动（构建图/供应链触发面）。

其他取舍：
- `deprecated` 判定用 `/proc` 的 `0x20` 位已足够（等价于 `preferred_lft == 0`）；解析 `/proc/net/ipv6_route` 或 netlink `IFA_CACHEINFO` 取 lifetime 属可选加强，建议本任务不做（收益低、解析面大）。
- 排序是“选择优先级”，不是过滤器：实现需保证不因排序/截断把某网卡唯一可用地址挤掉；网卡级至少保留一个通过过滤的地址。
- 过滤后仍保留 `fe80::/10`（链路本地）与 `fc00::/7`（ULA）——与 079 的 Lan 分类一致，属现状保持。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-local-ip-metadata-pipeline | 新增平台无关 `AddressMeta` 与纯函数过滤/确定性排序；输出为旧输出子集，网卡级不空 | `sn_service.rs` 内新增模块；不改 `SnLocalIpProvider` trait 签名 | 排序规则引入固定字典序决胜，保证可复现 | 表驱动单测：空列表、全部失效、混合场景；子集不变量用例 | 不做活性探测/网络实测判定 |
| P-002 | CHG-linux-if-inet6-source | Linux 解析 `/proc/net/if_inet6`（u32 flags，真实 IFA_F 位），过滤 `DEPRECATED(0x20)`/`TENTATIVE(0x40)`/`DADFAILED(0x08)` | 纯函数接受字符串输入；不引入新依赖 | 容器/命名空间内 `/proc` 缺失需回退 base | 单测：本机实测样本行（`a2` 被过滤、`82` 保留）+ 构造 tentative/dadfailed 行 | 不读 netlink、不改内核 |
| P-003 | CHG-windows-adapters-source | Windows `GetAdaptersAddresses` → `AddressMeta`，过滤 `DadState ∈ {Invalid,Tentative,Duplicate,Deprecated}` 且 `OperStatus != Up` 的接口 | 复用 `winapi` 既有依赖（新增 `iptypes`/`iphlpapi` feature）；转换函数纯逻辑 | 本机只能编译验证，无 Windows 运行时证据 | `cargo check -p p2p-frame --target x86_64-pc-windows-gnu` 通过 + mock adapter 单测 | 不做 Windows 实机/CI 运行验证 |
| P-004 | CHG-fallback-platform-parity | macOS/其他走 base 等价实现（步骤 1+4），能力差距写入模块文档 | 仅 `sn_service.rs` + `docs/modules/p2p-frame.md` 说明 | 非 Linux/Windows 无 DAD 感知，保持现状 | 编译期 `cfg` 分支检查；文档更新 | 不改 macOS 采集行为 |
| P-005 | CHG-enhanced-provider-wiring | `SNClientService::new` 按 `cfg(target_os)` 构造 `EnhancedLocalIpProvider`；`set_local_ip_provider` 与 `cyfs-p2p-test` 注入路径保持可用 | 仅默认构造点 | 默认实现变为平台相关，需保证自定义注入优先级不变 | 现有注入相关测试不回归；`sn_service.rs:2570` 起的 provider 测试保留 | 不改自定义 provider 语义 |

## Success Criteria

- 可见结果：本机（`company-wgr`）provider 输出不再包含两条 `deprecated` 地址；SN 侧收到的 `local_eps` 中 IPv6 端点数量与网卡地址数收敛，且不含 `deprecated`/`tentative`/DAD 失败地址；`fe80`/ULA 现状保持。
- 所需证据：
  - 纯函数单测（Linux 样本行 + `AddressMeta` 表驱动 + 子集不变量）通过；
  - `cargo test -p p2p-frame --features x509 --lib` 相关子集通过；`cargo check --workspace` 通过；
  - Windows 目标 `cargo check --target x86_64-pc-windows-gnu` 通过（编译证据，非运行时证据）；
  - 078 既有测试 `filter_local_ips_drops_virtual_tunnel_and_loopback`（`sn_service.rs:2518`）、`filter_local_ips_keeps_physical_nics`、`filter_local_ips_caps_count_at_max` 不回归；
  - 实机观察（记录为辅助证据）：重启 `bucky-vpn` 后比对 SN 返回端点不再含 `d005` 类 `deprecated` 残留。
  - 高-risk 流程证据：`pipeline/plan.md` 校验通过、`testplan.yaml` 与 `harness/scripts/test-run.py p2p-frame/080-<task> all` 运行记录、独立 `acceptance-report.md`。
- 显式非目标：不声明 Windows 运行时/多机部署级连通验证；不声明“上游不可路由的 preferred 残留”（`d00e` 型）已被识别（那是 P3 探测的能力）；不改 IPv4 上报语义。

## Risks

- 过滤过激导致可用源被删（最高影响面）：某接口全部全球地址都处于 `deprecated` 时，本次会全部过滤。缓解：网卡级保留首个通过过滤的地址；`fe80`/ULA 保持上报；不为“仍 preferred 但上游不路由”的地址做保留（该场景由后续 P3 探测任务覆盖）。
- 平台证据不对称：Windows 只有编译证据，无运行时验证；若 CI/实机不可用，需在 completion report 中显式记录该缺口。
- 解析脆弱性：`/proc/net/if_inet6` 格式变化、`flags` 宽度、`/proc` 在容器中不可用（需回退 base 而非返回空）。
- 与 078/079 的边界漂移：本任务只改“地址选择”，不得顺手改 `report_on_send` 装配或服务端 sanitizer，否则需要重新确认提案。
- 排序/截断交互：若保留现有“按监听器展开后 `truncate(32)`”，多监听器下仍可能出现“地址挑选了但端点被截掉”的观感；是否一并修正见待裁决问题 4。

## 变更历史
- 2026-09-18：提案初次登记为 `standard`（draft），列出 5 个待裁决问题。
- 2026-09-18：用户以 `确认，自动完成` 确认提案并启动 auto-pipeline；final tier 定为 `high-risk`，待裁决问题按正文建议裁决记录，状态置 `approved`。
