# 触发规则

## 目标
- 定义哪些类型的变更需要额外检查。

## 范围
- 适用于整个工作区所有模块；对 Tier 0 和 Tier 1 模块执行更严格的升级规则。

## 契约或协议变更
- 触发条件：
  - `p2p-frame/src/networks/**`、`p2p-frame/src/tunnel/**`、`p2p-frame/src/ttp/**`、`p2p-frame/src/sn/**` 或 `p2p-frame/src/pn/**` 下的文件发生变化
  - `p2p-frame/docs/*design*.md`、`*protocol*.md` 或 `*test_cases*.md` 发生变化
- 额外检查：
  - unit
  - DV
  - integration
  - 更新后的 design/testing 数据包评审
- 评审关注点：
  - 线协议兼容性
  - 状态迁移
  - 下游适配层假设

## 持久数据或制品变更
- 触发条件：
  - `desc-tool/**` 中 descriptor 或密钥处理发生变化
  - 生成的 desc/sec/device 制品语义发生变化
- 额外检查：
  - unit
  - 面向操作员的 DV
- 评审关注点：
  - 文件兼容性
  - 签名正确性
  - 意外数据破坏

## 运行时或集成变更
- 触发条件：
  - `cyfs-p2p-test/**`、`sn-miner-rust/**`、运行时 feature flag 或启动/配置默认值发生变化
- 额外检查：
  - DV
  - integration
- 评审关注点：
  - 启动行为
  - 场景假设
  - 配置与制品前置条件

## 安全或身份变更
- 触发条件：
  - `p2p-frame/src/tls/**`、`p2p-frame/src/x509*`、`p2p-frame/src/p2p_identity.rs` 或安全敏感的 `desc-tool` 代码发生变化
- 额外检查：
  - unit
  - 定向 design 评审
  - acceptance 评审必须显式点出安全敏感范围
- 评审关注点：
  - 握手正确性
  - 证书校验
  - feature-gated 行为

## 阶段门禁
- 在 DV 之前：
  - 已记录 unit 命令与运行时前置条件
- 在 integration 之前：
  - 对影响运行时的变更，已声明 DV 路径
- 在 acceptance 之前：
  - 已显式记录触发的额外检查，以及任何有意延后的证据

## 输出要求
- 哪些触发类别被命中
- 实际运行了哪些额外检查
- 哪些高成本检查仍待完成
- 残余风险
