---
module: cyfs-p2p-test
version: v0.1
status: draft
approved_by:
approved_at:
---

# cyfs-p2p-test 测试

## 测试文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `testing.md` | 运行时 harness 验证基线 | 完整模块 |

## 统一测试入口
- 机器可读计划：`docs/versions/v0.1/modules/cyfs-p2p-test/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py cyfs-p2p-test unit`
- DV: `python3 ./harness/scripts/test-run.py cyfs-p2p-test dv`
- Integration: `python3 ./harness/scripts/test-run.py cyfs-p2p-test integration`

## 子模块测试
| 子模块 | 职责 | 详细测试文档 | 必需行为 | 边界/失败场景 | 测试类型 | 测试文件 |
|--------|------|--------------|----------|----------------|----------|----------|
| `cli` | 选择场景和输入 | none | 解析子命令和必需参数 | 缺少参数、目标无效 | unit | `cyfs-p2p-test/src/main.rs` |
| `config_runtime` | 加载配置和 desc 制品 | none | 正确读取配置和默认值 | 文件缺失、配置格式错误 | DV | `cyfs-p2p-test/src/main.rs` |
| `scenario_orchestration` | 运行 all-in-one、client、server 流程 | none | 启动所选场景并与栈交互 | 运行时失败、环境不匹配 | DV + integration | `cyfs-p2p-test/src/main.rs` |

## 模块级测试
| 测试项 | 覆盖边界 | 入口 | 预期结果 | 测试类型 | 测试文件/脚本 |
|--------|----------|------|----------|----------|-----------------|
| crate 本地测试 | harness 本地逻辑 | `cargo test -p cyfs-p2p-test` | 测试通过，或零测试命令成功 | unit | crate 本地测试 |
| All-in-one 运行时场景 | 端到端本地路径 | `cargo run -p cyfs-p2p-test -- all-in-one` | 场景启动并产出运行时证据 | DV | `cyfs-p2p-test/src/main.rs` |
| 工作区兼容性 | harness 仍符合工作区契约 | `cargo test --workspace` | 工作区测试通过 | integration | workspace |

## 外部接口测试
| 接口 | 职责 | 成功场景 | 失败/边界场景 | 测试类型 | 测试文档/文件 |
|------|------|----------|----------------|----------|----------------|
| 操作员 CLI 边界 | 暴露稳定的调用模型 | 子命令能选择预期流程 | 缺少 flag、路径无效 | unit + DV | `cyfs-p2p-test/src/main.rs` |
| 运行时制品边界 | 消费本地 desc/config/log 制品 | 预期的本地环境能工作 | 制品过期或缺失 | DV | 仓库本地运行时文件 |

## 回归关注点
- 场景启动流程
- 配置文件和 desc 加载
- client 模式下的目标设备路由

## 完成定义
- [ ] 运行时前置条件明确
- [ ] DV 场景仍是规范路径
- [ ] 已指出对依赖该模块的数据包造成的 integration 影响
