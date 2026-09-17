---
task_manifest: task.yaml
status: approved
---

# Classify IPv6 Reported Endpoints Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 修改 SN 服务端 `sanitize_reported_endpoints` 的上报端点保留/分类策略（安全相邻边界），同时收口客户端本地 IP 上报过滤。IPv6 Wan 端点将基于客户端自报保留（上报隧道走 IPv4，SN 无法对 IPv6 做 exact-observed 校验），并进入 peer cache `local_eps`，其中部分会继续流向 legacy SnCall 反向端点数组；这是对 task 053 报备边界行为面的改变，属安全/兼容影响，需完整生命周期的独立验证，因此按 high-risk 提报。
- Proposal and tier confirmation: 用户于 2026-09-17 确认 high-risk 提案并启动自动完成流水线；裁决 1) legacy SnCall 反向端点数组不排除 IPv6 Wan 端点（保持现状消费）；2) 客户端过滤只在 `DefaultSnLocalIpProvider` 添加。启动语句：`确认，1不排除，2只在DefaultSnLocalIpProvider中添加，自动完成`。

## Background and Goal
`p2p-frame/src/sn/service/service.rs` 的 `sanitize_reported_endpoints`（service.rs:581）目前对 IPv6 只有两分支：ULA（`fc00::/7`）与链路本地（`fe80::/10`）标为 `Lan`，其余 IPv6 全部落入 `_ => continue` 被丢弃；没有对标 IPv4 的 `is_non_lan_ipv4_addr + observed_ip 匹配 -> Wan` 分支。结果全球单播 IPv6（如 `240e:...`）、IPv4-mapped（`::ffff:a.b.c.d`）、文档段（`2001:db8::/32`）、基准段（`2001:2::/48`）都进不了 peer cache。

目标行为（用户已确认）：
1. 非局域网 IPv6（全球单播等非特殊用途地址）在服务端归类为 `Wan`；由于上报连接走 IPv4，SN 观察不到 IPv6 源地址，因此 IPv6 Wan **不**带 IPv4 分支的 `observed_ip` 匹配条件，记录为显式取舍。
2. loopback、unspecified、multicast 在服务端和客户端上报路径均过滤。
3. IPv4-mapped、文档段、基准段保留并归类为 `Lan`。

Rust 工具链现状：Rust 1.98.0（2026-08-20）Stabilized APIs 列表不包含 `Ipv6Addr::is_global` / `is_unicast_global`；本地 stable 1.96.0 编译同样拒绝它们及 `is_ipv4_mapped`（E0658，`ip` feature #27709）。因此实现必须用稳定方法（`is_loopback` / `is_unspecified` / `is_multicast` / `is_unique_local` / `is_unicast_link_local`）加手写前缀/段位判定，不使用 nightly API。

## Scope
### In scope
- `p2p-frame/src/endpoint.rs`：新增稳定可用的 IPv6 分类 helper（对标 `is_non_lan_ipv4_addr`），覆盖 Lan（ULA/链路本地/IPv4-mapped/文档段/基准段）、Wan（其余非特殊 IPv6）、排除（loopback/unspecified/multicast）。
- `p2p-frame/src/sn/service/service.rs`：`sanitize_reported_endpoints` 增加 IPv6 Wan 分支；IPv6 分支不依赖 `observed_ip`（上报走 IPv4，无法观察 IPv6）；保留 IPv4 现有 exact-observed 语义。
- `p2p-frame/src/sn/client/sn_service.rs`：仅在 `DefaultSnLocalIpProvider` 本地 IP 收集面过滤 loopback、unspecified、multicast（IPv4/IPv6 对称），保留 IPv4-mapped/文档/基准段；不改 `report_on_send` 组装面。
- 服务端与客户端新增单元用例：IPv6 分类表驱动用例（全球单播 -> Wan、ULA/链路本地/IPv4-mapped/文档/基准 -> Lan、loopback/unspecified/multicast 丢弃、IPv4 行为不变）。

### Out of scope
- 不修改 IPv4 的 exact-observed 公网校验语义。
- 不修改 rendezvous/punch 的 IPv4-only 候选资格（`rendezvous_ipv4_eligible` 等保持现状）。
- 不修改任何 wire/protocol/命令字段、公共 API、依赖或持久化数据。
- 不引入 Rust nightly API 或新 crate。

### Boundary with neighboring modules
- 仅 `p2p-frame` 内实现；SN 授权/response-owner 校验仍以请求隧道 exact-observed IP 为准，本次不触碰 `rendezvous_endpoints_owned_by` / `validate_rendezvous_response_owner`。
- 遗留 SnCall 反向端点数组会消费 sanitized `local_eps`，IPv6 Wan 端点随之进入该面；用户裁决不排除、保持现状（见 Risks）。

## Requirement Review
需求成立：IPv6 完全被丢弃造成 IPv4/IPv6 分类不对称，且服务端/客户端对 loopback、unspecified、multicast 的过滤面不一致。方向修正为「非特殊 IPv6 进 Wan、特殊用途段进 Lan、不可路由/保留类丢弃」合理。

主要取舍：
- IPv6 Wan 为客户端自报（SN 经 IPv4 隧道无法观察 IPv6），与 IPv4 exact-observed 规则不对称；这是用户明确选择的取舍，提案按该边界记录。
- `is_global`/`is_unicast_global`/`is_ipv4_mapped` 在 Rust 1.98 仍未稳定，只能用稳定方法 + 手写判定，分类点保持集中、可测试。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-ipv6-sanitizer-area | SN 服务端 IPv6 分类：非特殊 IPv6 -> Wan，ULA/链路本地/IPv4-mapped/文档/基准 -> Lan，loopback/unspecified/multicast 丢弃；IPv4 语义不变 | `endpoint.rs` helper + `service.rs` sanitizer；IPv6 Wan 不带 observed-IP 条件；IPv4 仍精确观察校验 | IPv6 Wan 为客户端自报，无 SN 观察证据（用户确认接受） | 单元用例：全球单播 -> Wan；五类 Lan 保留；三类丢弃；IPv4 分支回归 | 不改 rendezvous/punch IPv4-only 门槛 |
| P-002 | CHG-client-ip-report-filter | 客户端上报路径过滤 loopback/unspecified/multicast（IPv4/IPv6 对称），保留 IPv4-mapped/文档/基准段 | `sn_service.rs` 本地 IP 收集/上报端点组装面；不影响 listener 注册与协议候选 | loopback 已过滤，补 unspecified/multicast；防止客户端把不可路由/保留类地址推进 `local_eps` | 单元用例：构造含 loopback/unspecified/multicast/合法地址的集合，过滤后仅保留合法项 | 不改非上报用途的本地地址枚举语义 |

## Success Criteria
- 系统可见结果：SN 收到含全球单播 IPv6 的 `ReportSn.local_eps` 后，该端点在 peer cache 中为 `Wan`（无需 observed 匹配）；含 IPv4-mapped/文档/基准段为 `Lan`；loopback/unspecified/multicast 不进入缓存。客户端上报前即过滤上述三类地址。
- 所需证据：
  - 服务端 sanitizer 单元用例与客户端过滤单元用例通过（覆盖 IPv6 分类表 + IPv4 回归）。
  - `cargo test -p p2p-frame --features x509 --lib` 相关子集通过；`cargo check --workspace` 通过。
  - 现有 053 及 SN 相关测试（loopback 过滤、exact-observed、rendezvous 授权）不回归。
- 显式非目标：不声明公网多机/部署级连通验证；不改变 IPv4 公网校验与 rendezvous/punch IPv4-only 资格。

## Risks
- 安全（已知妥协）：IPv6 Wan 由客户端自报进入复用 `local_eps` 的 legacy SnCall 反向端点数组，可能把不可观察的全球 IPv6 引入该面；rendezvous 授权仍由 exact-observed 隧道校验兜底，本项按用户确认保留并记录。
- 兼容：旧版服务端/客户端仍会丢弃 IPv6；升级后新增端点分类不改变 wire 形状，但缓存内容变化可能影响依赖 `local_eps` 的旧逻辑，需通过现有回归验证。
- 工具链：`is_global`/`is_unicast_global`/`is_ipv4_mapped` 在 Rust 1.98 仍为 nightly，手写判定需与 std 未来稳定化保持一致（集中 helper + 单测锁语义）。
- 回归：IPv4 分支若被重构波及会破坏 053 边界，必须保持独立分支与 exact-observed 校验。
