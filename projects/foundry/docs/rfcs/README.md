# RFC 索引

本目录记录 Foundry 的设计决策和实现方案。RFC 是设计稿，不表示已实现；实现状态见各 RFC 的“实施计划”和代码。

## 阅读顺序

1. 先读 [RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)：阶段、对象分类、门禁、证据。
2. 再读两个连接件：[RFC 0009 产物血缘、基线与变更](0009-artifact-lineage-baselines-and-change.md) 和 [RFC 0010 辅助系统与证据](0010-auxiliary-systems-and-evidence.md)。
3. 然后按生命周期阶段读专项 RFC：产品/需求 → 架构/设计 → 实现/测试 → 发布/运维 → Agent/度量。
4. 最后读贯穿性 RFC：知识/上下文、治理/安全/数据。

## 总览

| RFC | 标题 | 状态 | 范围 | 主要依赖 |
|---|---|---|---|---|
| [0001](0001-foundry-mvp.md) | Foundry MVP 纵向切片 | 已实现 | 控制平面 MVP | — |
| [0002](0002-project-issue-workflow-and-policy.md) | 项目流程、Issue 流程与权限策略 | 部分被取代 | 流程/策略 | 0001 |
| [0003](0003-task-workflow-model.md) | Task Workflow 模型与项目运行 | 部分被取代 | Task/流程 | 0002 |
| [0004](0004-durable-event-bus.md) | 持久事件总线 | 已实现 | 事件总线 | 0003 |
| [0005](0005-workflow-definitions.md) | Workflow 定义、版本与可视化编辑 | 已实现 | 流程定义 | 0003/0004 |
| [0006](0006-workflow-variables.md) | Workflow 变量、Operation、Event 与转移 | 已实现 | 流程语义 | 0005 |
| [0007](0007-workflow-script-syntax.md) | Workflow 脚本语法 | 已实现 | 脚本语言 | 0006 |
| [0008](0008-software-lifecycle-model.md) | 软件生命周期模型 | Draft | 生命周期总纲 | 0005/0006 |
| [0009](0009-artifact-lineage-baselines-and-change.md) | 产物血缘、基线与变更管理 | Draft | 贯穿连接件 | 0008 |
| [0010](0010-auxiliary-systems-and-evidence.md) | 辅助系统注册与产物契约 | Draft | 辅助能力/证据 | 0008/0015 |
| [0011](0011-requirements-management.md) | 需求管理与需求分析 | Draft | 需求阶段 | 0008/0009/0010 |
| [0012](0012-architecture-and-design-management.md) | 架构与设计管理 | Draft | 架构/设计阶段 | 0008/0009/0010/0011 |
| [0013](0013-implementation-review-testing-and-quality.md) | 实现、评审、测试与质量门禁 | Draft | 实现/验证阶段 | 0008/0009/0010/0011/0012 |
| [0014](0014-release-deployment-operations-and-retirement.md) | 发布、部署、运维与退役 | Draft | 交付/运营阶段 | 0008/0009/0010/0013 |
| [0015](0015-agent-architecture-and-evaluation.md) | Agent 架构与评测 | Draft | Agent 领域 | 0008/0010/0013 |
| [0016](0016-metrics-cost-and-self-hosting.md) | 度量、成本与自举 | Draft | 度量/自举 | 0008/0013/0014/0015 |
| [0017](0017-product-portfolio-and-initiation.md) | 产品、组合与立项管理 | Draft | 产品/立项阶段 | 0008/0009/0011 |
| [0018](0018-knowledge-documentation-and-context.md) | 知识、文档与上下文管理 | Draft | 贯穿连接件 | 0008/0009/0010/0013 |
| [0019](0019-governance-security-compliance-and-data.md) | 治理、安全、合规与数据 | Draft | 贯穿约束 | 0008/0009/0015 |
| [0020](0020-requirements-subsystem.md) | 需求子系统 | Draft | 需求分析结构与文档 | 0008/0009/0010/0011 |

## 专项关系

```text
0008 生命周期总纲
 ├─ 0009 产物血缘/基线/变更 ──┐
 ├─ 0010 辅助系统/证据 ───────┤
 ├─ 0017 产品/组合/立项      │
 ├─ 0011 需求管理 ───────────┤
 ├─ 0012 架构与设计 ─────────┤
 ├─ 0013 实现/评审/测试/质量 ─┤
 ├─ 0014 发布/部署/运维/退役 ─┤
 ├─ 0015 Agent 架构/评测 ────┤
 ├─ 0016 度量/成本/自举 ─────┘
 ├─ 0020 需求子系统（结构化需求文档）
 ├─ 0018 知识/文档/上下文
 └─ 0019 治理/安全/合规/数据
```

## 状态约定

- `已实现`：代码和测试已落地。
- `部分被取代`：主体已实现，部分设计被后续 RFC 修订。
- `Draft`：设计稿，待评审；不代表已实现。
- `Proposed`：ADR 的状态；等待接受。

## 相关文档

- [领域模型](../domain/model.md)
- [状态机](../domain/state-machines.md)
- [术语表](../domain/glossary.md)
- [流程定义 AST](../domain/workflow-ast.md)
- [内置流程](../domain/builtin-workflows.md)
- [API / MCP](../api/README.md)
- [ADR 索引](../adr/)
