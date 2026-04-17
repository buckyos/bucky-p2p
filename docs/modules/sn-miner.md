# sn-miner

## 类型
- 服务启动二进制

## 职责
- 基于 desc/sec 制品和运行时配置，使用 CYFS P2P 栈启动 SN 服务行为。

## 关键边界
- 范围内：
  - `sn-miner-rust/src/main.rs`
  - `sn-miner-rust/config/**`

## 依赖
- `cyfs-p2p`
- 仓库本地路径下的 device 制品

## 模块级别
- Tier 1：面向协议、带有运行时副作用的操作型二进制

## 必需的验证倾向
- 启动默认值、desc 加载、PN/SN 启动或生成制品的改动都需要 DV 证据。
- 配置默认值的改动会触发 trigger-rule 升级。
