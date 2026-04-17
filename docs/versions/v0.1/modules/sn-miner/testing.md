---
module: sn-miner
version: v0.1
status: draft
approved_by:
approved_at:
---

# sn-miner 测试

## 测试文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `testing.md` | 服务启动验证基线 | 完整模块 |

## 统一测试入口
- 机器可读计划：`docs/versions/v0.1/modules/sn-miner/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py sn-miner unit`
- DV: `python3 ./harness/scripts/test-run.py sn-miner dv`
- Integration: `python3 ./harness/scripts/test-run.py sn-miner integration`

## 子模块测试
| 子模块 | 职责 | 详细测试文档 | 必需行为 | 边界/失败场景 | 测试类型 | 测试文件 |
|--------|------|--------------|----------|----------------|----------|----------|
| `cli` | 选择 desc 输入路径 | none | 解析默认和显式 desc 路径 | 路径输入错误 | unit | `sn-miner-rust/src/main.rs` |
| `artifact_loading` | 加载/创建 desc 和 sec 文件 | none | 在缺失时创建默认值、规范化 endpoint | 文件格式错误、生成失败 | unit + DV | `sn-miner-rust/src/main.rs` |
| `service_startup` | 创建服务并启动运行时 | none | 启动路径能到达服务初始化 | 运行时启动失败 | DV + integration | `sn-miner-rust/src/main.rs` |

## 模块级测试
| 测试项 | 覆盖边界 | 入口 | 预期结果 | 测试类型 | 测试文件/脚本 |
|--------|----------|------|----------|----------|-----------------|
| crate 本地测试 | 启动本地逻辑 | `cargo test -p sn-miner` | 测试通过，或零测试命令成功 | unit | crate 本地测试 |
| CLI DV 探测 | 二进制入口仍可调用 | `cargo run -p sn-miner -- --help` | 进程成功退出并输出帮助文本 | DV | `sn-miner-rust/src/main.rs` |
| 工作区兼容性 | 运行时使用方仍符合工作区契约 | `cargo test --workspace` | 工作区测试通过 | integration | workspace |

## 外部接口测试
| 接口 | 职责 | 成功场景 | 失败/边界场景 | 测试类型 | 测试文档/文件 |
|------|------|----------|----------------|----------|----------------|
| 操作员启动边界 | 暴露稳定的启动入口 | 默认/帮助路径工作正常 | desc 路径缺失、制品错误 | DV | `sn-miner-rust/src/main.rs` |
| 适配层边界 | 正确消费 `cyfs-p2p` 的启动路径 | 可以编译并集成 | 适配层假设变化导致启动失效 | integration | workspace |

## 回归关注点
- 默认制品创建
- endpoint 规范化
- 启动到 SN 服务和 PN server 的路径

## 完成定义
- [ ] 启动和制品假设明确
- [ ] 配置/默认值改动会触发更强评审
- [ ] 运行时行为具备 DV 证据
