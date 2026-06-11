# SN 模块：分布式目录与跨 SN NAT 信令方案

> 状态：模块方案草案，尚未进入仓库 proposal/design 审批流程。
>
> 适用范围：`p2p-frame` 中的 SN 子模块，以及 `sn-miner` 的部署/启动配置边界。

## 1. 模块目标

SN 模块需要从单 SN 服务模型扩展为多 SN 分布式服务模型：

- 单节点 SN 必须是多节点模型的退化形态，同一套协议和数据结构应能在 `owner_set = [self]` 时保持当前单 SN 行为。
- 部署应支持从单 SN 平滑扩展到多 SN，而不是要求用户、SN operator 或上层调用方切换到另一套协议。
- 用户可以连接任意 SN 节点。
- 配置了同一套 OwnerMembership 的 SN 节点可以查询同一 owner 域内某个用户当前连接的是哪个 SN。
- NAT 穿透信令可以跨 SN 转发。
- 用户详细 NAT 信息只保存在当前连接的 serving SN 上。
- 全局目录只保存轻量 `peer_id -> serving_sn` 归属关系。
- 具备目录 owner 能力的 SN 节点由 Owner SN 启动配置中的 owner 列表决定。

本方案不要求所有 SN 都保存用户外网地址、SN 观察地址或完整 endpoint 列表。

## 2. 单节点到多节点扩展模型

SN 分布式目录应按渐进式部署设计：

```text
单节点部署：
  serving SN = owner SN = 当前 SN
  owner_set(peer_id) = [self]
  ServingLease 写入本地目录
  现有 PeerManager/CachedPeerInfo 状态保存在本地
  call/called 仍走本机 SN 流程
  不配置 owner 时不向任何远端 owner 发布，也不接受跨 SN 目录查询

多节点部署：
  serving SN 可以是任意 Edge SN
  owner_set(peer_id) 由 OwnerMembership + HRW 计算
  ServingLease 写入 owner set
  现有 PeerManager/CachedPeerInfo 状态仍保存在 serving SN
  call/called 可跨 SN relay
  只有配置了相同 OwnerMembership 的 Edge SN 才属于同一查询域
```

因此，`ServingLease`、现有 `PeerManager/CachedPeerInfo` 状态、owner resolver 和 inter-SN relay 都应允许在单节点模式下退化为本地调用。第一版实现可以先落地本地 `ServingDirectory`，再把 backend 扩展为 owner set 远程复制，而不改变 SN report/call/called 的上层语义。

扩展路径：

```text
Phase 0: 单 SN，本地目录，owner_set = [self]
Phase 1: 多 SN，共享 OwnerMembership，但只发布 ServingLease
Phase 2: 跨 SN query serving 和 relay call
Phase 3: owner quorum、gossip hint、动态 membership
```

## 3. 角色划分

### 3.1 Edge SN / Serving SN

Serving SN 是用户实际连接的 SN。

职责：

- 接收用户连接、report 和 heartbeat。
- 根据真实连接来源观察公网地址。
- 本地保存用户 NAT/endpoint 详情。
- 未配置 OwnerMembership 时，所有 `ServingLease` 和现有 `PeerManager/CachedPeerInfo` 状态都只保存在本地，只支持本机查询和本机 SN call/called。
- 配置 OwnerMembership 时，使用本地 owner 列表计算该用户对应的 owner set，并向该 owner set 发布轻量归属 lease。
- 只有配置了相同 OwnerMembership 的 Edge SN 才能通过 owner set 相互查询用户归属并发起跨 SN relay。
- 接收同一 owner 域内其他 SN 转发来的 call 请求。
- 向本机连接用户投递 called 信令。

### 3.2 Owner SN / Directory SN

Owner SN 是启动配置中的 owner 列表声明为目录节点的 SN。

职责：

- 启动时加载完整的 Owner SN 列表或等价 OwnerMembership 配置。
- 只认可该 owner 列表中的 SN 节点；不在列表中的 SN 不具备 owner 身份，也不能作为 owner set 成员被接受。
- 保存 `peer_id -> serving_sn` 的短 TTL 归属记录。
- 响应同一 OwnerMembership 查询域内其他 SN 的归属查询。
- 接收 Edge SN 已按 OwnerMembership 计算出的 owner set 写入请求，并返回本 owner 本地保存的索引数据。
- 不保存完整 NAT 详情。
- 可不直接承载大量用户连接。

Owner SN 不替 Edge SN 计算某个用户的 owner set。用户归属的 owner set 由 Edge SN 使用相同 OwnerMembership 和 HRW 规则在本地计算；Edge SN 再把索引数据上传到计算出的 owner set，或从该 owner set 查询索引数据。

单节点或未配置 owner 部署中，当前 SN 只具备本地目录能力，不向远端 owner 发布，也不对其他 SN 提供全局 owner 查询能力；一旦进入多节点部署，owner 资格必须由启动配置中的 OwnerMembership 明确声明。不同 OwnerMembership 表示不同查询域，Edge SN 只有配置相同 owner 域时才能互查归属。

## 4. 数据结构

### 4.1 ServingLease

Owner SN 保存的轻量目录记录。

```text
ServingLease {
  peer_id
  serving_sn_id
  serving_sn_endpoint
  sequence
  updated_at
  expires_at
  serving_sn_signature
  peer_signature optional
}
```

字段说明：

- `peer_id`：用户身份。
- `serving_sn_id`：用户当前连接的 SN。
- `serving_sn_endpoint`：用于其他 SN 访问 serving SN 的地址。
- `sequence`：防止乱序更新覆盖新归属。
- `expires_at`：lease 过期时间。
- `serving_sn_signature`：证明该归属由 serving SN 发布。
- `peer_signature`：可选，用于证明用户确认当前 serving SN。

更新规则：

```text
只接受 sequence 更大的 lease
相同 sequence 使用确定性裁决规则
expired lease 不参与查询结果
offline/delete 不能删除 sequence 更新的 lease
```

### 4.2 Serving Peer Cache

Serving SN 本地保存的用户详细状态应尽量沿用现有 `PeerManager` / `CachedPeerInfo` 结构，而不是为分布式目录重新定义一套 peer detail。现有字段已经覆盖 `ReportSn`、`SnQuery` 和 `SnCall` 需要的主要信息。

```text
CachedPeerInfo / serving peer cache {
  peer_id
  desc
  map_ports
  local_eps
  is_wan
  last_send_time
  last_call_time
}
```

其中：

- `desc` 对应现有 `peer_info` / encoded cert，是 `SnQueryResp.peer_info` 和 `SnCallResp.to_peer_info` 的来源。
- `map_ports` 和 `local_eps` 来自现有 `ReportSn`，用于候选构造。
- `is_wan` 继续沿用现有 WAN 判断语义；SN 观察端点分类仍按现有 `Wan` / `ServerReflexive` 规则处理。
- `last_send_time` / `last_call_time` 继续作为本地活跃度和 call 去重/节流相关状态。

这些信息只保存在 serving SN 上。其他 SN 需要时向 serving SN 查询，或通过 serving SN 转发 NAT call。分布式目录新增的 `ServingLease` 只索引 `peer_id -> serving_sn`，不得替代或复制现有 `PeerManager` 的完整状态。

### 4.3 OwnerMembershipManifest

Owner SN 成员视图。

```text
OwnerMembershipManifest {
  network_id
  epoch
  previous_epoch
  valid_from
  valid_until
  owner_members: [
    {
      sn_id
      public_key
      endpoints
      region
      weight
      status: Active | Draining
    }
  ]
  signatures
}
```

Owner set 只从 `owner_members` 中计算，不从全部 Edge SN 中计算。
Owner SN 启动时必须配置完整 `owner_members` 列表或等价 manifest；只有该列表中的 SN 节点才被认可为 owner。Edge SN 也必须使用同一份 owner 列表计算用户的 owner set。

## 5. Owner Set 计算

推荐使用带 epoch 的 Rendezvous Hashing / HRW。

```text
owner_set(peer_id, epoch) =
  top K Owner SNs by score(hash(peer_id, owner_sn_id, epoch))
```

计算职责：

- Edge SN 负责为本机连接用户计算 `owner_set(peer_id)`。
- Edge SN 将 `ServingLease` 上传到计算出的 owner set。
- Edge SN 查询其他用户归属时，同样先本地计算该用户的 owner set，再向这些 Owner SN 查询索引。
- Owner SN 只校验请求方和 OwnerMembership 是否匹配，并读写本 owner 本地索引数据；不负责替 Edge SN 选择 owner set。

建议默认：

```text
replication_factor = 3
```

优点：

- Owner SN 增减时只迁移少量用户归属 key。
- 所有 SN 在同一个 membership epoch 下可算出一致 owner set。
- 普通 Edge SN 动态扩缩容不会影响 owner keyspace。
- 单节点部署可直接返回 `[self]`，避免引入不必要的远程目录依赖。

## 6. Owner 列表配置

Owner SN 集合由 Owner SN 和 Edge SN 的启动配置决定。配置中必须包含完整 owner 列表或等价 `OwnerMembershipManifest`，同一 owner 域内所有 SN 必须使用相同列表计算 owner set 和校验请求。

### 6.1 静态或外部分发配置

适合第一版。

```text
配置文件或外部分发系统提供完整 Owner SN 列表
Owner SN 启动时加载该列表，并只认可列表中的 owner
Edge SN 启动时加载同一列表，并用它计算 owner set
不同 owner 列表表示不同查询域
```

如果使用签名 manifest，签名只用于验证配置完整性和版本，不改变“有哪些 Owner SN 由启动配置决定”的原则。

### 6.2 可选演进：Quorum 签署配置版本

如果后续需要去中心化更新 owner 列表，可以让当前 owner 列表中的节点共同签署下一版本配置。

```text
Owner SN authority set 共同签署 membership epoch
达到 majority 或 BFT quorum 后 manifest 生效
manifest 通过 gossip 分发
```

即便采用该演进，Edge SN 仍必须先加载并接受某个具体配置版本，再用该版本计算 owner set。不建议让纯 gossip membership 直接参与 owner 计算。纯 gossip 会导致不同 SN 短期看到不同成员集合，从而对同一个 `peer_id` 算出不同 owner set。

## 7. 用户上线与刷新

用户上线：

```text
U -> SN-A: connect/report
SN-A: 更新现有 PeerManager/CachedPeerInfo(U)
SN-A: 计算 owner_set(U)
SN-A -> owner_set(U): publish ServingLease(U -> SN-A)
```

在线刷新：

```text
SN-A -> owner_set(U): refresh ServingLease
```

未配置 OwnerMembership 时，上述 publish/refresh 只更新本地目录，不产生 inter-SN 网络请求；其他 SN 也不能通过 owner set 查询该节点上的用户归属。

建议：

```text
refresh interval = 20s
lease ttl = 60s
```

离线通知只作为优化，不能作为正确性前提。客户端崩溃、断电、网络切换或 SN 故障时，离线消息可能缺失，最终必须靠 TTL 收敛。

## 8. 用户切换 SN

用户从 `SN-A` 切换到 `SN-B`：

```text
U -> SN-B: connect/report with sequence=N+1
SN-B -> owner_set(U): ServingLease(U -> SN-B, seq=N+1)
```

Owner SN 更新规则：

```text
如果新 lease sequence 更大，覆盖旧 lease
如果旧 SN-A 的 offline 消息迟到，不得删除 SN-B 的新 lease
如果多个 serving SN 都有 fresh lease，按 sequence 和策略返回一个或多个候选
```

如果需要更强防伪，客户端应签名：

```text
PeerServingClaim {
  peer_id
  serving_sn_id
  sequence
  expires_at
  peer_signature
}
```

## 9. 用户归属查询

配置了同一套 OwnerMembership 的 SN 查询用户当前所属 SN：

```text
SN-X: owner_set = HRW(U, current_epoch from the same OwnerMembership)
SN-X -> owner_set: query ServingLease(U)
owner_set -> SN-X: U served by SN-A
```

查询策略：

- 并发查询 owner set 中多个 SN。
- 验证签名、TTL 和 sequence。
- 选择 fresh 且 sequence 最大的 lease。
- 如果 current epoch 查不到，可在迁移窗口内查询 previous epoch。

SN 可以缓存查询结果：

```text
peer_id -> serving_sn
```

缓存 TTL 应短于 lease TTL。缓存命中后可直接访问 serving SN；失败后回 owner set 查询。

未配置 OwnerMembership 时，归属查询应直接命中本地目录或当前 `PeerManager` 缓存，不改变既有单 SN call 路径，也不允许其他 SN 通过目录查询本机用户。

## 10. 跨 SN NAT Call

示例：

```text
A connected to SN-A
B connected to SN-B
SN-X receives call(B)
```

流程：

```text
SN-X -> owner_set(B): query ServingLease(B)
owner_set(B) -> SN-X: B served by SN-B
SN-X -> SN-B: relay called(B, A candidates)
SN-B -> B: called(A, candidates)
B -> A: direct/reverse connect
```

约束：

- `SnCallResp` 仍只表示 SN 已受理或已转发，不表示最终 tunnel 连通成功。
- 现有 `PeerManager/CachedPeerInfo` 状态只由 serving SN 使用。
- Owner SN 不需要保存完整外网地址。
- relay call 必须做 SN 间身份校验和授权检查。
- 当 caller 和 callee 属于同一个 serving SN 时，不需要 inter-SN relay，应退化为本地 SN call/called。

## 11. 全 SN 轻量广播的取舍

也可以让所有 SN 保存轻量目录：

```text
peer_id -> serving_sn_id
```

优点：

- 查询最快。
- 任意 SN 本地可知道用户所属 SN。
- 实现简单。

缺点：

- 写放大是 `在线用户数 * SN 节点数`。
- SN 节点越多越不可控。
- 仍然需要 TTL、refresh、sequence、签名和乱序处理。

建议：

```text
全 SN 广播只作为 gossip hint/cache
权威归属仍保存在 owner set
```

## 12. DHT/Gossip 替代方案

开放式 SN 网络可以考虑 DHT/gossip。

```text
sn_id = hash(sn_public_key)
peer_lease_key = hash("sn-peer-lease", peer_id)
owner_set = DHT keyspace 上 closest K SN
```

流程：

```text
serving SN PUT peer_lease_key -> ServingLease
query SN GET peer_lease_key -> ServingLease
```

该方案更分布式，但问题更多：

- routing table 不一致。
- 查询多跳，延迟更高。
- 记录污染风险更大。
- 需要更严格签名和准入策略。

建议受控基础设施优先采用 OwnerMembership + HRW；开放第三方 SN 网络再考虑 DHT/gossip。

## 13. 模块接口建议

SN 模块可抽象以下内部能力。

### 13.1 Serving Directory

```text
ServingDirectory.put(ServingLease)
ServingDirectory.get(peer_id) -> Vec<ServingLease>
ServingDirectory.expire(peer_id, sequence)
```

第一版 backend：

```text
LocalDirectory
OwnerSetDirectory
```

后续可替换为：

```text
FullReplicationDirectory
DhtDirectory
```

### 13.2 Owner Resolver

```text
OwnerResolver.current_epoch()
OwnerResolver.owner_set(peer_id) -> Vec<OwnerSn>
OwnerResolver.previous_owner_set(peer_id) -> Vec<OwnerSn>
```

`OwnerResolver` 运行在 Edge SN 本地，用启动配置中的 OwnerMembership 计算 owner set。Owner SN 可以使用同一 resolver 做请求校验或调试，但不作为用户 owner set 的决策方。

单节点模式下：

```text
OwnerResolver.owner_set(peer_id) -> [self]
```

### 13.3 Inter-SN Relay

```text
InterSnRelay.query_serving(peer_id)
InterSnRelay.query_detail(peer_id)
InterSnRelay.relay_call(peer_id, call_context)
```

Inter-SN 请求必须基于 SN 身份证书认证，不接受普通 peer 伪造；请求双方还必须处于同一 OwnerMembership 查询域。

## 14. 失败与收敛规则

### 14.1 Serving SN 故障

如果 serving SN 不可达：

- owner lease 仍可能短时间存在。
- 查询方访问 serving SN 失败后，应重新查询 owner set。
- 如果没有其他 fresh lease，则认为该 peer 当前 SN 服务不可用。
- 不应由 owner SN 返回过期 NAT detail。

### 14.2 Owner SN 故障

因为 owner set 有多个副本：

- 查询并发访问多个 owner。
- 任意 fresh lease 可用于路由。
- 下一次 refresh 会写入当前 owner set。

### 14.3 Membership 变更

OwnerMembership epoch 变化时：

- 新 report/refresh 写入 current epoch owner set。
- 查询短期兼容 previous epoch owner set。
- 旧 owner lease 依赖 TTL 自然过期。

### 14.4 网络分区

如果使用 quorum membership：

- 只有多数派或 quorum 分区能产生新 epoch。
- 少数派可服务已有 lease，但不能发布新 membership epoch。
- peer lease 依赖 TTL 收敛。

## 15. 安全边界

- SN 之间必须使用 mTLS 或等价证书身份认证。
- Owner SN 只接受受信 serving SN 发布的 lease。
- `observed_endpoints` 只能由 serving SN 根据真实连接来源生成，不能信任客户端自填。
- `ServingLease` 必须签名。
- 如果允许 peer signed claim，owner 应验证 peer 签名和 sequence。
- 普通 Edge SN 不自动获得 owner 权限。
- Owner SN membership 必须可验证，不能由普通 gossip 状态直接决定。

## 16. 推荐实施阶段

### 阶段 0：单节点等价目录

- 引入 `ServingLease`，并明确它与现有 `PeerManager/CachedPeerInfo` 状态的边界。
- 实现本地 `ServingDirectory`。
- `owner_set(peer_id)` 在无 OwnerMembership 时返回 `[self]`，只用于本地目录，不触发远端发布或远端查询。
- 保持当前单 SN report/call/called 行为等价。

### 阶段 1：多节点目录归属

- 引入 Owner SN 角色。
- 引入 OwnerMembershipManifest。
- 实现 HRW owner set。
- 实现 `ServingLease` 发布与查询。

### 阶段 2：跨 SN 信令

- 实现 inter-SN query serving。
- 实现 inter-SN relay call。
- serving SN 投递 called 给本机用户。

### 阶段 3：缓存与动态变更

- 增加本地短 TTL cache。
- 增加 gossip hint。
- 支持 current/previous epoch 查询。

### 阶段 4：可靠性和治理

- 增加 peer signed serving claim。
- 增加 owner quorum 写入或查询。
- 增加 Draining、异常下线和审计日志。

## 17. 非目标

- 不要求 owner 保存完整 NAT/endpoint 详情。
- 不要求所有 SN 全量保存所有用户状态。
- 不改变 `SnCallResp` 只表示受理/转发的语义。
- 不把 SN owner 目录设计成强一致用户会话数据库。
- 不把 gossip membership 直接作为 owner 计算输入。
- 不在第一版引入完整 DHT 或开放式第三方 SN 准入。
- 不把单节点 SN 和多节点 SN 做成两套不兼容协议。

## 18. 建议结论

SN 模块推荐采用：

```text
单节点是多节点模型的退化形态，owner_set = [self]
Owner SN 由启动配置中的 owner 列表决定
OwnerMembershipManifest + epoch + HRW
owner set 保存 peer_id -> serving_sn 的短 TTL ServingLease
serving SN 保存现有 PeerManager/CachedPeerInfo 状态
同一 owner 域内的 SN 查询 owner set 后向 serving SN relay call
gossip/full broadcast 只作为缓存 hint
```

该结构比全 SN 广播更可扩展，比 DHT/gossip 更稳定，也符合“详细 NAT 信息只保存在上线 SN 上”的边界。
