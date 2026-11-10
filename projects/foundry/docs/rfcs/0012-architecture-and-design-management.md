# RFC 0012：架构与设计管理

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[ADR 0002 控制平面形态](../adr/0002-control-plane-form.md)、[ADR 0003 Git 存内容，Postgres 存状态](../adr/0003-git-content-postgres-state.md)、[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0009 产物血缘、基线与变更管理](0009-artifact-lineage-baselines-and-change.md)、[RFC 0010 辅助系统注册与产物契约](0010-auxiliary-systems-and-evidence.md)、[RFC 0011 需求管理与需求分析](0011-requirements-management.md)
- 作用：本文定义架构设计、详细设计、契约、质量属性、约束、组件模型、技术债和架构符合性如何被管理、版本化、验证和变更。

## 0. 结论

架构管理的核心不是画图，而是**把长期决策变成可执行、可验证、可追溯的约束**。图是模型的投影，不是第二真相。

核心决定：

1. **架构是版本化基线。** `ArchitectureBaseline` 保存某一时刻认可的组件、接口、契约、质量属性、约束和决策；基线批准后不可变。
2. **决策是一等对象。** ADR 记录背景、选项、权衡、结论、后果和证据；不可逆决策必须由人类做出。
3. **契约必须可校验。** API、schema、事件、数据库契约有版本、兼容性策略和验证命令；breaking change 必须走变更。
4. **架构模型是机器可读的。** 组件、接口、依赖、运行时单元用结构化数据保存；C4/时序图是投影，不是真相。
5. **约束进入门禁。** 依赖方向、分层、模块边界、质量预算通过 fitness function 在 CI 和门禁中检查；不能只写在文档里。
6. **实现必须引用架构基线。** 每个实现 Task 引用它遵循的架构基线和契约；偏离需要 ADR 或 Waiver。
7. **技术债是一等对象。** 技术债有位置、影响、风险、偿还计划和负责人；不能只存在于评论里。
8. **架构变更走 ChangeRequest。** 已批准架构的修改产生新版本、影响分析、迁移计划和批准（RFC 0009）。
9. **辅助系统帮助思考和验证，不代替决策。** 方案生成、权衡分析、建模、符合性检查、影响分析、仿真和基准测试是可替换能力；批准仍由人类/策略完成（RFC 0010）。
10. **详细设计按需。** 高风险、跨模块、接口复杂的变更需要 DesignDoc；小而可逆的变更可以只写任务计划，但必须引用架构约束。
11. **质量属性必须可度量。** 性能、可用性、安全、可维护性等用场景和预算表达，并有验证方式；不能写“高性能”。
12. **架构知识可追溯。** 需求 → 架构决策 → 契约 → 组件 → 任务 → 测试 → 发布形成血缘；架构偏离和过期决策可见。

## 1. 目标与非目标

### 1.1 目标

- 让架构决策可追溯、可复审、可变更，而不是散落在文档和聊天里。
- 让契约兼容性、依赖方向和模块边界可自动检查。
- 让质量属性有明确目标和验证方式。
- 让实现任务知道它遵循哪条架构决策和契约。
- 让技术债可见、可排序、可偿还。
- 让架构辅助能力可替换、可评测。
- 让架构变更能影响分析到需求、代码、测试和发布。

### 1.2 非目标

- 不实现通用绘图工具；图是模型的投影。
- 不实现完整的 UML/BPMN 建模；只保留对决策和验证有用的结构。
- 不实现自动架构生成；辅助系统可以建议，人类负责决策。
- 不实现跨项目架构共享；架构按项目隔离。
- 不实现运行时拓扑发现；可以从部署/监控接入，但不作为真相。
- 不要求所有变更都写完整架构文档；按风险和可逆性分级。
- 不把架构评审变成一次性仪式；约束必须持续可执行。
- 不实现通用配置管理；配置版本引用见 RFC 0009。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| ArchitectureBaseline | 某一时刻批准的架构快照：组件、接口、契约、质量属性、约束、决策。 |
| ADR | Architecture Decision Record：长期决策的背景、选项、权衡、结论和后果。 |
| DesignDoc | 详细设计：模块、接口、数据、错误、部署、替代方案。 |
| Contract | 系统间或模块间的接口约定：HTTP API、gRPC、JSON Schema、事件、DB schema。 |
| QualityAttribute | 可度量的质量场景：刺激、响应、度量、目标。 |
| Constraint | 架构约束：依赖方向、技术选型、合规、性能预算。 |
| ComponentModel | 机器可读的组件、接口、依赖和运行时单元。 |
| FitnessFunction | 可执行的架构检查：规则、范围、阈值、严重度。 |
| TechDebtItem | 技术债：位置、影响、风险、偿还计划、负责人。 |
| ArchitectureConformance | 代码/部署是否符合架构基线和约束。 |
| Deviation | 实现偏离架构基线；需要 ADR 或 Waiver。 |

不变量：

1. 每个项目最多一个当前 `active` 架构基线；历史基线保留。
2. 架构基线批准后不可变；变更产生新基线。
3. ADR 一旦 `accepted` 不可修改；修订产生新 ADR 或 supersede 关系。
4. 契约有版本和兼容性策略；breaking change 必须走 ChangeRequest。
5. 质量属性必须有度量、目标和验证方式；不可度量的质量属性不能进入基线。
6. 约束必须声明执行方式（manual/fitness/ci）；不可执行的约束只能作为建议，不能作为阻塞门禁。
7. 实现 Task 必须引用架构基线；没有引用的 Task 在门禁中提示或阻塞。
8. 偏离架构基线必须有 ADR 或 Waiver；不能静默偏离。
9. 技术债必须有负责人和偿还计划；高风险债务进入发布门禁。
10. 架构模型是真相；图、文档和看板是投影。
11. 架构变更必须做影响分析，覆盖需求、契约、任务、测试、发布。
12. 不可逆架构决策必须由人类批准；Agent 可以提议和提供证据。

## 3. 对象模型

### 3.1 ArchitectureBaseline

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "name": "payments-architecture-v2",
  "state": "draft|active|superseded",
  "scope": "project|service|module",
  "description": "...",
  "components": ["component-id"],
  "contracts": ["contract-id"],
  "decisions": ["adr-id"],
  "quality_attributes": ["qa-id"],
  "constraints": ["constraint-id"],
  "fitness_functions": ["ff-id"],
  "approved_by": {"kind": "human", "id": "..."},
  "approved_at": "...",
  "supersedes": "baseline-id|null",
  "created_at": "..."
}
```

规则：

- 基线只能引用同项目、已接受的 ADR/Contract/QualityAttribute/Constraint。
- 基线激活时，旧的 active 基线变为 `superseded`。
- 基线变更走 ChangeRequest；紧急变更允许先标记 `provisional`，但必须补批准。
- 基线是实现的默认引用；Task 可以引用更具体的模块基线。

### 3.2 ADR

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "title": "采用事件驱动同步退款状态",
  "status": "proposed|accepted|rejected|superseded|deprecated",
  "context": "...",
  "problem": "...",
  "options": [
    {"name": "同步调用", "pros": ["简单"], "cons": ["耦合", "超时"], "cost": "..."},
    {"name": "事件驱动", "pros": ["解耦"], "cons": ["最终一致"], "cost": "..."}
  ],
  "decision": "...",
  "consequences": ["..."],
  "evidence": ["benchmark-id", "prototype-id"],
  "reversibility": "reversible|costly|irreversible",
  "deciders": [{"kind": "human", "id": "..."}],
  "supersedes": "adr-id|null",
  "created_at": "...",
  "accepted_at": "..."
}
```

规则：

- 至少两个选项和各自代价；只有一个选项的 ADR 不能接受（除非记录“无替代方案”的理由）。
- 不可逆决策必须有人类批准。
- ADR 接受后不可修改；修订通过新 ADR + `supersedes`。
- ADR 必须关联需求、约束或事故等证据；无来源的决策标记为“内部发起”。

### 3.3 Contract

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "type": "http_api|grpc|json_schema|event|db_schema",
  "name": "refund-api",
  "version": "2.1.0",
  "spec_ref": {"type": "git", "repo": "payments", "commit": "...", "path": "openapi/refund.yaml"},
  "compatibility": "backward|forward|full|none",
  "status": "draft|active|deprecated|retired",
  "producers": ["component-id"],
  "consumers": ["component-id"],
  "validation": ["npm run lint:openapi", "foundry contract check"],
  "breaking_changes": ["..."],
  "created_at": "..."
}
```

规则：

- 契约必须有 spec 引用和验证命令；不能只写描述。
- breaking change 必须走 ChangeRequest，并列出受影响的消费者。
- 兼容性策略必须声明；`none` 只允许内部或实验契约。
- 契约版本与实现版本分离；实现引用契约版本。
- 契约校验在 CI 和发布门禁中执行。

### 3.4 QualityAttribute

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "scenario": "退款请求峰值",
  "stimulus": "每秒 500 次退款请求",
  "response": "请求在 500ms 内返回，错误率 < 0.1%",
  "measure": "p95 latency, error rate",
  "target": {"p95_ms": 500, "error_rate": 0.001},
  "priority": "must|should|could",
  "verification": ["benchmark-id", "load-test"],
  "status": "proposed|active|retired",
  "created_at": "..."
}
```

规则：

- 质量属性必须有度量、目标和验证方式。
- 不可度量的质量属性不能进入基线；可以作为讨论项。
- 质量预算进入 fitness function 和发布门禁。

### 3.5 Constraint

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "type": "dependency|technology|security|compliance|performance",
  "scope": "project|module|component",
  "rule": "domain 不能依赖 adapters",
  "enforcement": "manual|fitness|ci",
  "fitness_function": "ff-id|null",
  "owner": {"kind": "human", "id": "..."},
  "status": "active|retired",
  "created_at": "..."
}
```

规则：

- `enforcement=fitness/ci` 的约束必须有可执行检查；否则降级为 `manual`。
- 约束违反是门禁失败或警告，取决于严重度。
- 约束变更走 ChangeRequest；架构基线引用约束版本。

### 3.6 ComponentModel

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "components": [
    {
      "id": "payments-api",
      "type": "service",
      "owner": {"kind": "human", "id": "..."},
      "interfaces": ["refund-api"],
      "dependencies": ["payments-db", "event-bus"],
      "runtime": "container",
      "deploy_unit": "payments-api"
    }
  ],
  "interfaces": [
    {
      "id": "refund-api",
      "contract": "contract-id",
      "direction": "inbound|outbound"
    }
  ],
  "dependencies": [
    {"from": "payments-api", "to": "payments-db", "type": "sync", "criticality": "high"}
  ],
  "updated_at": "..."
}
```

规则：

- 组件和依赖是结构化数据；C4 图、时序图、依赖图由它生成。
- 组件必须有 owner；无主组件是技术债。
- 依赖关系用于影响分析和循环依赖检查。
- 组件模型可以按模块拆分，但同一项目内 id 唯一。

### 3.7 DesignDoc

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "title": "退款状态同步设计",
  "status": "draft|in_review|accepted|superseded",
  "scope": ["payments-api", "refund-worker"],
  "requirements": ["requirement-id"],
  "architecture_baseline": "baseline-id",
  "problem": "...",
  "design": "...",
  "interfaces": ["contract-id"],
  "data": "...",
  "errors": "...",
  "alternatives": ["..."],
  "migration": "...",
  "test_plan": "...",
  "created_at": "..."
}
```

规则：

- 高风险、跨模块、接口变化、数据迁移需要 DesignDoc。
- DesignDoc 必须引用需求、架构基线和契约。
- DesignDoc 接受后，后续变更走 ChangeRequest 或新版本。
- 小变更可以只写 Task 描述，但必须引用架构基线和契约。

### 3.8 TechDebtItem

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "title": "退款服务直接访问支付库",
  "location": ["payments-api/src/refund.rs"],
  "impact": "耦合、难以测试",
  "risk": "medium",
  "remediation": "引入 repository 接口",
  "effort": "3d",
  "priority": "high",
  "owner": {"kind": "human", "id": "..."},
  "status": "open|planned|in_progress|resolved|accepted",
  "created_at": "..."
}
```

规则：

- 技术债必须有位置、影响、风险和负责人。
- 高风险债务进入发布门禁；可以 Waiver，但需批准。
- 债务可以关联需求、任务和事故；修复后产生新的架构/代码证据。

### 3.9 FitnessFunction

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "name": "domain-no-adapters",
  "rule": "domain 包不能依赖 adapters 包",
  "scope": "workspace",
  "check": {
    "type": "command",
    "command": "cargo deny check bans",
    "expect": "exit 0"
  },
  "threshold": null,
  "severity": "error|warning",
  "constraint": "constraint-id",
  "status": "active|disabled",
  "created_at": "..."
}
```

规则：

- fitness function 必须可执行、可重复、有明确阈值。
- 在 CI 和门禁中运行；失败产生证据和阻塞原因。
- 结果写入 Evidence，带命令、版本、输入哈希和输出。
- 可以引用第三方工具（静态分析、依赖检查、性能基准）。

## 4. 生命周期与门禁

### 4.1 架构阶段

```text
需求基线
  → 架构方案（候选）
  → 权衡与决策（ADR）
  → 契约与质量属性
  → 架构基线（draft）
  → 评审（人类 + 辅助检查）
  → 基线批准（active）
  → 详细设计
```

入口条件：需求基线、关键质量属性、约束、风险。
出口条件：架构基线批准、关键契约可校验、约束可执行、技术债已记录、详细设计范围明确。

### 4.2 详细设计阶段

入口：架构基线、需求场景、契约。
出口：DesignDoc 接受（或任务计划）、接口/数据/错误/迁移方案明确、任务分解完成。

### 4.3 门禁

| 门禁 | 条件 | 阻塞 |
|---|---|---|
| architecture.baseline_approved | 基线 active、关键 ADR accepted、质量属性可度量 | 是 |
| contract.compatible | 契约兼容性检查通过 | 是 |
| constraint.enforced | 所有 `fitness/ci` 约束有可执行检查 | 是 |
| conformance.pass | 代码符合基线和约束 | 是（高风险） |
| tech_debt.threshold | 高风险债务不超过阈值或已批准 | 是（发布） |
| deviation.approved | 偏离有 ADR 或 Waiver | 是 |

## 5. 契约与兼容性

### 5.1 契约类型

| 类型 | 验证 |
|---|---|
| HTTP API | OpenAPI lint、schema diff、contract test |
| gRPC | Proto lint、buf breaking、contract test |
| JSON Schema | schema validation、diff |
| Event | schema registry、兼容性检查 |
| DB schema | migration lint、schema diff、回滚检查 |

### 5.2 兼容性策略

| 策略 | 含义 |
|---|---|
| `backward` | 新版本能读旧数据/旧客户端 |
| `forward` | 旧版本能读新数据 |
| `full` | 双向兼容 |
| `none` | 不保证；需要显式变更和迁移 |

规则：

- 默认 `backward`；`none` 需要批准。
- breaking change 必须列出受影响的消费者和迁移计划。
- 契约测试作为证据；不能只靠人工评审。
- 契约废弃有过渡期和消费者通知。

## 6. 质量属性与约束

### 6.1 质量属性场景

- 性能：吞吐、延迟、资源使用。
- 可用性：故障恢复、降级、RTO/RPO。
- 安全：认证、授权、加密、审计。
- 可维护性：模块边界、依赖方向、测试覆盖。
- 可观测性：日志、指标、追踪。
- 合规：数据保留、隐私、审计。

每个场景必须有度量、目标和验证方式；进入基线前确认可验证。

### 6.2 约束执行

- `ci`：在 CI 中运行，失败阻塞合并。
- `fitness`：作为架构检查运行，失败产生证据；严重度决定阻塞或警告。
- `manual`：人工评审，不能单独作为阻塞门禁；必须说明理由。

## 7. 架构符合性与偏离

### 7.1 符合性检查

- 依赖方向、循环依赖、模块边界。
- 契约兼容性和消费者影响。
- 质量预算（性能、资源、错误率）。
- 安全约束（加密、认证、数据访问）。
- 技术债阈值。

### 7.2 偏离

- 实现 Task 可以显式声明偏离架构基线。
- 偏离必须关联 ADR 或 Waiver；否则门禁阻塞。
- 偏离记录：范围、原因、风险、补偿措施、有效期。
- 偏离到期后自动重新评估；未处理则升级。

## 8. 技术债

- 技术债不是“以后再说”的备注；它有位置、影响、风险、偿还计划和负责人。
- 债务来源：架构偏离、临时方案、过期依赖、缺失测试、重复代码、文档缺失。
- 债务排序：风险 × 影响 × 修复成本。
- 高风险债务进入发布门禁；可以 Waiver，但需批准和补偿措施。
- 债务修复产生新的代码/测试/架构证据，并更新债务状态。

## 9. 架构辅助系统

| 能力 | 输入 | 输出 | 信任 |
|---|---|---|---|
| `architecture.options` | 需求、约束、质量属性 | 候选方案、权衡矩阵、风险 | self_reported |
| `architecture.tradeoff` | 候选方案、评分标准 | 评分、敏感性分析 | self_reported |
| `architecture.model` | 描述、代码、契约 | 组件/依赖模型草稿 | self_reported |
| `architecture.diagram` | 模型 | C4/时序图投影 | tool_verified（渲染） |
| `architecture.impact` | 变更、血缘 | 受影响组件/契约/任务 | tool_verified |
| `architecture.conformance` | 代码、基线、约束 | 符合性报告、违规项 | tool_verified |
| `contract.check` | 新旧契约 | 兼容性、breaking changes | tool_verified |
| `architecture.simulate` | 模型、负载、故障 | 性能/容量/故障结果 | tool_verified |
| `architecture.benchmark` | 候选方案、场景 | 基准数据 | tool_verified |
| `techdebt.detect` | 代码、依赖、覆盖率 | 债务候选 | self_reported |

规则：

- 辅助输出是草稿或证据，不是批准。
- 方案生成必须包含至少两个候选和代价；不能只给一个“最佳方案”。
- 模型和图的真相是结构化模型；图渲染失败不影响模型。
- 仿真和基准必须记录环境、版本和输入，保证可复现。
- 架构决策批准仍由人类/策略完成。

## 10. 追溯与变更

### 10.1 追溯链

```text
Requirement / QualityAttribute
  → ADR
  → Contract
  → ComponentModel
  → DesignDoc
  → Task / ChangeSet
  → FitnessFunction / Conformance
  → TestRun
  → Release
```

边类型见 RFC 0009：`derives_from`、`satisfies`、`refines`、`implements`、`depends_on`、`supersedes`、`verifies`。

### 10.2 架构变更

- 已批准架构的修改走 `ChangeRequest(kind=architecture)`。
- 影响分析：需求、契约、组件、任务、测试、发布、部署。
- 变更计划：迁移步骤、兼容性、回滚、验证。
- 新基线：变更批准后产生新架构基线；旧基线保留。
- 紧急变更：允许先 provisional，必须补批准和验证。

## 11. 数据模型

```sql
create table architecture_baselines (
  baseline_id   uuid primary key references baselines (id),
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  components    jsonb not null default '[]'::jsonb,
  contracts     jsonb not null default '[]'::jsonb,
  decisions     jsonb not null default '[]'::jsonb,
  quality_attributes jsonb not null default '[]'::jsonb,
  constraints   jsonb not null default '[]'::jsonb,
  fitness_functions jsonb not null default '[]'::jsonb,
  created_at    timestamptz not null default now()
);

create table architecture_decisions (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  title         text not null,
  status        text not null,
  context       text not null,
  options       jsonb not null,
  decision      text,
  consequences  jsonb not null default '[]'::jsonb,
  evidence      jsonb not null default '[]'::jsonb,
  reversibility text not null,
  deciders      jsonb not null default '[]'::jsonb,
  supersedes    uuid references architecture_decisions (id),
  created_at    timestamptz not null default now(),
  accepted_at   timestamptz
);

create table contracts (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  type          text not null,
  name          text not null,
  version       text not null,
  spec_ref      jsonb not null,
  compatibility text not null,
  status        text not null,
  producers     jsonb not null default '[]'::jsonb,
  consumers     jsonb not null default '[]'::jsonb,
  validation    jsonb not null default '[]'::jsonb,
  created_at    timestamptz not null default now()
);

create table quality_attributes (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  scenario      text not null,
  stimulus      text not null,
  response      text not null,
  measure       text not null,
  target        jsonb not null,
  priority      text not null,
  verification  jsonb not null default '[]'::jsonb,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table constraints (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  type          text not null,
  scope         text not null,
  rule          text not null,
  enforcement   text not null,
  fitness_function uuid,
  owner         jsonb not null,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table component_models (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  components    jsonb not null,
  interfaces    jsonb not null,
  dependencies  jsonb not null,
  updated_at    timestamptz not null default now()
);

create table design_docs (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  title         text not null,
  status        text not null,
  scope         jsonb not null default '[]'::jsonb,
  requirements  jsonb not null default '[]'::jsonb,
  baseline_id   uuid references architecture_baselines (id),
  content_ref   jsonb not null,
  created_at    timestamptz not null default now()
);

create table tech_debt_items (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  title         text not null,
  location      jsonb not null default '[]'::jsonb,
  impact        text not null,
  risk          text not null,
  remediation   text not null,
  effort        text,
  priority      text not null,
  owner         jsonb not null,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table fitness_functions (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  name          text not null,
  rule          text not null,
  scope         text not null,
  check         jsonb not null,
  threshold     jsonb,
  severity      text not null,
  constraint_id uuid references constraints (id),
  status        text not null,
  created_at    timestamptz not null default now()
);
```

架构基线使用 RFC 0009 的 `baselines`（`scope = architecture`）承载名称、状态、批准和取代关系；`architecture_baselines` 只存架构特有内容，不重复基线元数据。

## 12. API、MCP、CLI、Web

### 12.1 REST

```text
GET/POST /api/v1/projects/{project}/architecture/baselines
POST     /api/v1/architecture/baselines/{id}/approve
GET/POST /api/v1/projects/{project}/architecture/decisions
POST     /api/v1/architecture/decisions/{id}/accept
GET/POST /api/v1/projects/{project}/contracts
POST     /api/v1/contracts/{id}/check
GET/POST /api/v1/projects/{project}/quality-attributes
GET/POST /api/v1/projects/{project}/constraints
GET/PUT  /api/v1/projects/{project}/component-model
GET/POST /api/v1/projects/{project}/design-docs
GET/POST /api/v1/projects/{project}/tech-debt
POST     /api/v1/tech-debt/{id}/resolve
GET/POST /api/v1/projects/{project}/fitness-functions
POST     /api/v1/projects/{project}/architecture/conformance
GET      /api/v1/projects/{project}/architecture/deviations
```

### 12.2 MCP

- `architecture.baseline.get` / `architecture.baseline.diff`
- `architecture.decision.list` / `architecture.decision.get`
- `contract.get` / `contract.check`
- `architecture.constraints`
- `architecture.conformance`
- `architecture.impact`
- `architecture.options`（辅助能力）
- `design.doc.get` / `design.doc.submit`
- `techdebt.list` / `techdebt.report`

Agent 可以提出 ADR 草案、设计草案、符合性请求；不能接受 ADR、批准基线或关闭架构变更。

### 12.3 CLI

```bash
foundry architecture baseline create <project> --name v2
foundry architecture adr new <project> --title "..."
foundry architecture adr accept <adr>
foundry contract check <contract>
foundry architecture conformance <project>
foundry architecture impact <artifact>
foundry techdebt list <project>
foundry design doc new <project>
```

### 12.4 Web

- 架构基线页：组件、契约、质量属性、约束、ADR、fitness function。
- ADR 页：背景、选项、权衡、结论、证据、取代关系。
- 契约页：版本、兼容性、消费者、验证结果。
- 组件模型页：组件、依赖、owner、运行时单元；图是投影。
- 技术债页：列表、风险、负责人、偿还计划。
- 符合性页：fitness function 结果、偏离、Waiver。
- 详细设计页：设计文档、任务分解、迁移计划。

## 13. 权限与安全

- 人类架构师/技术负责人可以创建、接受 ADR、批准架构基线；Agent 可以提议和提供证据。
- 不可逆决策必须由人类批准；高风险决策需要至少两名批准人或指定委员会。
- 契约 breaking change 需要消费者 owner 确认。
- 架构模型和契约可能泄露内部结构；按项目权限过滤。
- 仿真/基准必须使用脱敏数据；生产数据需要批准。
- 所有接受、批准、偏离和 Waiver 写入审计日志。

## 14. 与现有实现的关系

- 现有 `crates/domain/src/document/{adr,design,contract}.rs` 提供领域模型；本 RFC 增加持久化、版本、契约校验和基线。
- 现有 `documents` 表存 path/git_revision/status/version；本 RFC 保留为投影，新增类型化表存可查询字段。
- 现有 `work_items` 的 `kind` 不扩展为架构对象；架构对象通过 Artifact + 类型化表管理。
- 现有 `links` 迁移为 `artifact_edges`，架构对象参与血缘。
- 现有 ADR 文档（`docs/adr/`）可以继续作为仓库内决策记录；项目级 ADR 通过 API/编辑器管理，保持同步或引用。

## 15. 实施计划

### 阶段 0：模型与内容

- 类型化表、Artifact 关联、Git 内容引用。
- 现有 ADR/DesignDoc/Contract 领域模型接入。
- 出口标准：架构对象可版本化、可查询、可追溯。

### 阶段 1：契约与 fitness function

- 契约验证命令、兼容性检查、fitness function 执行。
- 出口标准：breaking change 可检测；架构约束在 CI/门禁中运行。

### 阶段 2：基线与评审

- 架构基线创建、批准、取代；评审门禁。
- 出口标准：实现 Task 引用基线；偏离需要 ADR/Waiver。

### 阶段 3：辅助能力

- 方案生成、权衡、建模、符合性、影响分析、仿真/基准接入。
- 出口标准：候选方案可比较；符合性报告可定位；仿真可复现。

### 阶段 4：技术债与变更

- 技术债管理、发布门禁、架构 ChangeRequest、迁移计划。
- 出口标准：高风险债务可见；架构变更影响分析完整。

## 16. 验收标准

1. 架构基线可创建、批准、取代；旧基线保留。
2. ADR 有至少两个选项和代价；不可逆决策有人类批准。
3. 契约有版本、兼容性和验证命令；breaking change 被检测并阻塞。
4. 质量属性有度量、目标和验证方式；不可度量的不能进入基线。
5. 约束有执行方式；`fitness/ci` 约束在 CI 和门禁中运行。
6. 实现 Task 引用架构基线；偏离有 ADR 或 Waiver。
7. 技术债有位置、影响、风险、负责人和偿还计划。
8. 架构辅助输出可追溯、可验证、可替换；不自动批准。
9. 架构变更影响分析覆盖需求、契约、任务、测试和发布。
10. 图和文档是模型的投影；模型更新后图可重新生成。

## 17. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 建模过度 | 组件模型维护成本高 | 从契约 + 依赖规则开始；按需扩展 |
| 约束不可执行 | 文档规则无法检查 | 只把可执行规则作为阻塞门禁 |
| 架构评审形式化 | 一次评审后无人遵守 | fitness function 持续检查；偏离显式 |
| ADR 质量参差 | 选项和证据不足 | 模板检查；接受前评审 |
| 契约 breaking | 消费者被破坏 | 兼容性检查、消费者确认、迁移计划 |
| 辅助系统幻觉 | 方案看似合理但不可行 | 仿真/基准/原型证据；人类批准 |
| 技术债失控 | 债务只增不减 | 阈值、发布门禁、固定偿还配额 |
| 图与模型不一致 | 手工改图 | 图只读生成；禁止图编辑作为入口 |

**待决问题**：

1. 组件模型是否第一期就建？建议先契约 + 依赖规则，组件模型按需。
2. 架构基线粒度是项目级还是服务级？建议项目级 + 可选模块基线。
3. ADR 是否强制至少两个选项？建议高风险强制，低风险可记录“无替代”。
4. 契约校验工具选型？建议 OpenAPI/buf/schema registry 优先，统一接入 fitness function。
5. 技术债阈值如何定？建议按风险和发布时间窗，先告警后阻塞。
