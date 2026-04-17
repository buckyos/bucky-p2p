# 设计文档规则

## 目标
- 定义 `design.md` 与 `design/` 的最低结构和审批要求。

## 范围
- `docs/versions/<version>/modules/<module>/design.md`
- `docs/versions/<version>/modules/<module>/design/`
- `docs/modules/<module>.md` 中必须同步的长期边界说明

## 必需元数据
- `module`
- `version`
- `status`
- `approved_by`
- `approved_at`

## 必需内容
- 直接子模块拆分
- 实现顺序
- 接口与依赖摘要
- 实现布局
- 文档索引

## 护栏
- Design 必须落实已批准 proposal 的意图，不能静默改变范围。
- Design 任务不得修改测试策略、验收标准或实现代码。
- 如果数据包外已有设计说明属于当前设计证据链，就必须显式建立索引。
- 如果 design 发现 proposal 存在歧义，应退回 proposal，而不是就地修补范围。
