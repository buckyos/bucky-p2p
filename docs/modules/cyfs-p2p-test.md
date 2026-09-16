# cyfs-p2p-test

## 类型
- 运行时验证二进制

## 职责
- 提供可运行的本地场景，覆盖 all-in-one、client 和 server 流程，以验证整个工作区栈。

## 关键边界
- 范围内：
  - `cyfs-p2p-test/src/main.rs`
  - 测试 harness 使用的本地配置和生成的运行时制品

## 依赖
- `p2p-frame`
- 本地 config 制品与动态生成的 x509 身份

## 模块级别
- Tier 1：对验证至关重要的运行时 harness

## 必需的验证倾向
- 该模块按 `custom-rules/cyfs-p2p-test-stage-ban-rules.md` 禁止作为正式测试/验收证据；它只作为运行时场景驱动器。
- x509 身份正确性由 `p2p-frame` 的 `--features x509` 测试面验证。
- 对 CLI、配置加载、运行时编排或场景语义的改动，需要在依赖它的数据包中更新 testing 计划。
