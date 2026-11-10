# RFC 0011：需求管理与需求分析

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[ADR 0004 Issue 是问题空间，Requirement 是承诺空间](../adr/0004-issue-vs-requirement.md)、[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0009 产物血缘、基线与变更管理](0009-artifact-lineage-baselines-and-change.md)、[RFC 0010 辅助系统注册与产物契约](0010-auxiliary-systems-and-evidence.md)
- 取代：本 RFC 重设计现有处于早期实现的 Requirement 聚合；保留“Issue 是问题空间、Requirement 是承诺空间”的边界，以及现有 Requirement 状态机的主干。

## 0. 结论

需求管理的核心不是字段表，而是**承诺的版本化、可验证和可变更**。Issue 便宜、允许模糊；Requirement 昂贵、必须说清“为谁解决什么问题、怎样算解决、不做什么”。

核心决定：

1. **Issue 与 Requirement 严格分离。** Issue 是问题空间入口，不直接变成承诺；只有显式需求分析才创建 Requirement（ADR 0004 继续成立）。
2. **Requirement 是版本化聚合。** 字段围绕软件工程：`problem / users / acceptance / non_goals / constraints / priority / risk / owner`，另有假设、开放问题、非功能需求、来源 Issue、holdout 引用。
3. **批准是昂贵动作。** 批准要求 `users` 和 `non_goals` 非空、验收场景可执行、holdout 已建立、追溯基本完整；创建和草稿保持便宜。
4. **需求池不是 Requirement 表。** 发现阶段的候选需求可以是 Issue、候选记录或草稿 Requirement；只有通过分析并决定承诺的才进入正式 Requirement 集合。
5. **验收场景是需求的一部分。** 每个 Requirement 至少有一个可执行的验收场景；场景有 given/when/then、验证方式和是否为 holdout。
6. **holdout 与实现隔离。** holdout 场景由独立验证方持有，不进入实现 Agent 的输入、日志、上下文或工作区；实现 Agent 只能看到公开验收场景。
7. **需求变更走 ChangeRequest。** 已批准需求不能原地编辑；变更产生新版本、影响分析和批准（RFC 0009）。
8. **需求基线固定承诺。** 在批准或发布点建立需求基线；后续变更与基线比较，计算漂移。
9. **需求分析需要辅助系统。** 歧义、完整性、一致性、可测试性、场景生成、追溯建议、影响分析由可替换能力提供；输出是发现和建议，不是批准（RFC 0010）。
10. **需求必须可追溯。** 每个 Requirement 能追溯到来源 Issue、验收场景、架构决策、任务、测试和发布；覆盖缺口可见。
11. **Agent 可以写需求，不能自我批准。** Agent 可以起草、分析、建议和实现；批准由人类或策略显式授权的角色完成；实现者不能验证自己的需求。
12. **需求质量可度量。** 歧义率、验收覆盖率、变更率、返工率、holdout 泄漏事件和追溯覆盖率进入指标（RFC 0016）。

## 1. 目标与非目标

### 1.1 目标

- 让需求从“模糊想法”变成“可验证承诺”，同时不阻塞早期讨论。
- 让每个需求都有明确的用户、问题、验收标准和不做范围。
- 让需求变更可分析、可批准、可追溯，而不是散落在评论里。
- 让实现和测试能引用需求，并形成端到端追溯。
- 让 holdout 验收真正独立，降低针对测试过拟合的风险。
- 让需求分析辅助能力可替换、可评测，不绑定某个工具。
- 让需求池、基线和指标支持产品组合管理。

### 1.2 非目标

- 不实现通用工单系统；Issue 仍负责问题收集和讨论。
- 不实现产品路线图和市场判断；这些属于产品与组合阶段（RFC 0008）。
- 不实现实时协同编辑；需求草稿仍采用乐观并发和显式冲突。
- 不要求每个小需求都写完整文档；按风险和可逆性分级。
- 不实现自动批准；需求批准仍是人类/策略职责。
- 不实现跨项目需求共享；需求按项目隔离。
- 不实现通用需求管理市场或外部同步；外部系统通过适配器接入。
- 不把验收场景直接等同于测试用例；场景描述行为和验收，测试用例是实现细节。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| Issue | 问题空间入口：反馈、缺陷、问题、想法、任务请求、事故。 |
| Requirement | 承诺空间对象：为谁解决什么问题、怎样算解决、不做什么。 |
| RequirementCandidate | 需求发现阶段的候选；可以是 Issue、草稿或独立记录。 |
| RequirementPool | 项目内候选需求的集合与分诊视图。 |
| AcceptanceScenario | 可执行的验收场景：given/when/then、验证方式。 |
| Holdout | 不向实现者公开的验收场景；由独立验证方持有。 |
| Assumption | 需求依赖但尚未验证的假设。 |
| OpenQuestion | 需求中尚未回答的问题。 |
| NonFunctionalRequirement | 性能、可用性、安全、可维护性等质量要求。 |
| RequirementBaseline | 某一时刻批准的需求版本集合。 |
| RequirementQuality | 完整性、清晰度、可测试性、一致性、追溯覆盖。 |
| Disposition | Issue 的处置：转需求、转任务、关闭、拒绝、重复、暂缓。 |

不变量：

1. Issue 不直接变成 Requirement；必须经过显式分析动作。
2. Requirement 的 `problem`、`users`、`acceptance` 在创建时必填；`non_goals` 在批准时必填。
3. 每个 Requirement 至少有一个可执行验收场景；批准时至少一个公开场景和一个 holdout（低风险需求可豁免，但需记录）。
4. 已批准 Requirement 不可原地修改；变更走 ChangeRequest，产生新版本。
5. 需求版本不可变；基线引用具体版本和内容哈希。
6. holdout 场景不进入实现 Agent 的输入、日志、上下文或工作区；泄漏事件必须记录和处理。
7. 实现者不能批准自己的需求，也不能作为 holdout 的唯一验证者。
8. 每个 Requirement 必须能追溯到至少一个来源（Issue、目标或产品决策）或明确标记为“内部发起”。
9. 需求状态转换必须满足门禁；`InReview → Approved` 要求 `users`、`non_goals`、验收场景和 holdout 检查通过。
10. 需求变更必须做影响分析，覆盖架构、任务、测试、发布和证据失效。
11. 需求基线批准后不可变；变更产生新基线或新版本。
12. 跨项目需求引用被拒绝；需求查询按项目隔离。

## 3. Requirement 对象模型

### 3.1 字段

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "title": "支持部分退款",
  "problem": "客户只能整单退款，部分退款需要人工处理。",
  "users": ["电商客服", "财务"],
  "acceptance": ["scenario-uuid-1", "scenario-uuid-2"],
  "non_goals": ["不做多次部分退款合并", "不做跨币种退款"],
  "constraints": ["兼容现有支付网关", "退款审计保留 180 天"],
  "priority": "high",
  "risk": "medium",
  "owner_kind": "human",
  "owner_id": "account-uuid",
  "status": "Draft|InReview|Approved|InDesign|InDevelopment|InVerification|Done|Rejected|Deferred|Superseded",
  "version": 3,
  "source_issues": ["issue-uuid"],
  "assumptions": [{"text": "支付网关支持部分退款 API", "status": "unverified"}],
  "open_questions": [{"text": "是否支持部分退款手续费？", "owner": "..."}],
  "non_functional": [{"type": "latency", "target": "p95 < 500ms"}],
  "holdout_refs": ["holdout-uuid"],
  "baseline_refs": ["baseline-uuid"],
  "links": ["artifact-uuid"],
  "created_at": "...",
  "updated_at": "..."
}
```

### 3.2 验收场景

```json
{
  "id": "uuid",
  "requirement_id": "uuid",
  "title": "部分退款成功",
  "given": "订单已支付 100 元",
  "when": "客服发起 30 元部分退款",
  "then": "订单剩余 70 元，退款记录为 30 元，审计日志包含操作人",
  "verification": "automated|manual|holdout",
  "holdout": false,
  "priority": "must",
  "status": "active|retired"
}
```

规则：

- 场景必须可执行或可人工验证；不能只写“系统应正常工作”。
- 场景有唯一 id，可以被测试和证据引用。
- `holdout=true` 的场景单独存储和授权，不进入公开需求视图。
- 场景变更走 ChangeRequest；已批准场景不能静默修改。

### 3.3 需求类型

| 类型 | 说明 | 典型字段 |
|---|---|---|
| Functional | 功能行为 | 触发、输入、输出、状态 |
| NonFunctional | 质量属性 | 性能、可用性、安全、可维护性 |
| Constraint | 硬约束 | 兼容性、合规、技术限制 |
| Compliance | 合规要求 | 标准、审计、数据保留 |
| Data | 数据要求 | schema、保留、迁移、隐私 |

类型影响审批层级和验证强度；高风险/合规需求不能只靠自动化测试。

### 3.4 状态机

沿用现有主干：

```text
Draft → InReview → Approved → InDesign → InDevelopment → InVerification → Done
  │        │  ▲        │           │  ▲          │              │  ▲
  │        │  └────────┘           │  └──────────┘              │  └────┐
  ▼        ▼                       ▼                             ▼       │
Rejected  Rejected               Deferred                      Deferred  │
  │                                                                    │
  └──────────────► Draft ◄─────────────────────────────────────────────┘
```

允许转换见 `docs/domain/state-machines.md`。新增门禁：

| 转换 | 额外条件 |
|---|---|
| Draft → InReview | problem/users/acceptance 非空；至少一个验收场景 |
| InReview → Approved | non_goals 非空；验收场景可执行；holdout 检查通过；追溯基本完整；批准人 ≠ 创建人 |
| Approved → InDesign | 需求基线已建立（或明确豁免）；架构影响已分析 |
| InDesign → InDevelopment | 架构/契约已确认；任务已创建并引用需求 |
| InDevelopment → InVerification | 实现证据齐全；公开验收场景通过 |
| InVerification → Done | 独立验证/holdout 通过；缺陷已处理；发布或交付记录已关联 |
| Approved/InDesign/InDevelopment/InVerification → Deferred | 记录原因、恢复条件和影响 |
| Done → Superseded | 记录替代需求或变更请求 |

## 4. 需求发现与需求池

### 4.1 来源

- 客户反馈、客服工单、销售/售前、用户研究
- 监控、告警、事故、缺陷
- 内部想法、技术债、合规要求
- 产品策略、组合决策、路线图

来源通过 Issue 或 Feedback Artifact 进入；外部系统通过适配器接入，保留来源引用和原始内容哈希。

### 4.2 需求池

需求池是视图，不是新表：

```text
候选（Issue / Candidate）
  → 去重/聚类
  → 分诊（类型、价值、风险、紧急度、负责人）
  → 处置：转需求分析 / 转任务 / 关闭 / 拒绝 / 重复 / 暂缓
  → 需求分析
  → 承诺（Requirement）
```

规则：

- 候选可以合并；合并保留来源链接和讨论。
- 分诊必须记录 actor、时间、理由。
- 处置为“转需求分析”时创建草稿 Requirement，并链接来源 Issue。
- 处置为“转任务”时创建 Task，不创建 Requirement；Issue 在验证后关闭。
- 需求池支持按来源、客户、价值、风险、状态、负责人过滤。

### 4.3 优先级

优先级不是单一数字；至少包含：

- 价值（客户/收入/合规/战略）
- 成本（工作量、风险、依赖）
- 紧急度（事故、截止日期）
- 置信度（证据强度）
- 可逆性

排序结果必须可解释，不能只给一个分数。产品/组合决策由人类或明确授权的策略负责。

## 5. 需求分析

### 5.1 分析内容

| 维度 | 问题 | 产出 |
|---|---|---|
| 问题 | 要解决的真实问题是什么？ | problem 陈述 |
| 用户 | 谁受益、谁操作、谁负责？ | users、角色、场景 |
| 范围 | 做什么、不做什么？ | non_goals、constraints |
| 验收 | 怎样算解决？ | 验收场景、holdout |
| 假设 | 依赖哪些未验证前提？ | assumptions |
| 开放问题 | 还有什么没决定？ | open_questions |
| 非功能 | 质量属性是什么？ | NFR |
| 风险 | 失败会怎样？ | risk、缓解措施 |
| 依赖 | 依赖哪些系统/团队/数据？ | dependencies |
| 追溯 | 来源和相关产物是什么？ | links、source_issues |

### 5.2 质量检查

| 检查 | 失败示例 | 处理 |
|---|---|---|
| 完整性 | 没有 users 或 non_goals | 阻止批准 |
| 清晰度 | “系统应更快” | 要求可度量目标 |
| 一致性 | 两个需求互相矛盾 | 列出冲突，要求决策 |
| 可测试性 | “用户体验好” | 要求可验证场景 |
| 可追溯 | 没有来源 | 标记内部发起或补来源 |
| 范围 | 一个需求包含多个独立承诺 | 拆分为多个 Requirement |
| 风险 | 高风险但无缓解 | 要求风险计划和验证强度 |

### 5.3 验收场景与 holdout

- 公开场景：实现者和测试者可见，用于开发和自测。
- holdout：独立验证方持有，用于验收；实现者不可见。
- holdout 选择原则：覆盖关键行为、边界、失败路径、权限和安全；不能只覆盖 happy path。
- holdout 数量按风险分级：低风险至少 1 个，高风险覆盖主要失败模式。
- holdout 泄漏检测：如果 holdout 内容出现在实现者上下文、日志或代码注释中，记录泄漏事件并重新生成。
- 低风险需求可以豁免 holdout，但必须有 Waiver 和理由。

### 5.4 需求分析辅助系统

| 能力 | 输入 | 输出 | 信任 |
|---|---|---|---|
| `requirements.analyze` | 需求草稿、Issue、术语表 | 完整性/歧义/一致性发现 | self_reported |
| `requirements.scenarios` | 需求、领域知识 | 验收场景候选 | self_reported |
| `requirements.holdout` | 需求、风险模型 | holdout 候选 | self_reported |
| `requirements.testability` | 需求 | 不可测试表述、建议 | self_reported |
| `requirements.trace` | 需求、设计、任务、测试 | 候选追溯边 | self_reported |
| `requirements.impact` | 需求新版本、血缘 | 影响分析 | self_reported |
| `requirements.quality` | 需求 | 质量评分和理由 | self_reported |

规则：

- 辅助输出是草案；分析结果进入需求评审，不自动批准。
- 场景和 holdout 候选必须由人类或独立验证方确认。
- 同一 Agent 不能既生成 holdout 又实现需求。
- 辅助系统版本、模型和输入记录在 Provenance 中。

## 6. 需求基线与变更

### 6.1 需求基线

- 在 `InReview → Approved` 或发布点建立需求基线。
- 基线包含 Requirement 版本、验收场景版本和 holdout 引用。
- 基线批准后不可变；变更产生新版本和新基线。
- 发布、架构和测试引用需求基线；发布说明列出基线内容。

### 6.2 需求变更

变更来源：客户变化、事故、技术约束、架构影响、合规要求。

流程：

```text
ChangeRequest(kind=requirement)
  → 影响分析（需求、架构、契约、任务、测试、发布）
  → 评审（价值、风险、验证计划、回滚）
  → 批准
  → 新 Requirement 版本 / 新场景版本
  → 关联任务和测试
  → 验证
  → 更新基线
  → 关闭
```

规则：

- 已批准需求不能原地编辑；新版本必须引用旧版本。
- 变更必须标记受影响验收场景和 holdout；holdout 可能需要重新生成。
- 变更必须重新评估架构影响；必要时触发架构 ChangeRequest。
- 变更关闭前必须更新基线并清理 stale 证据。

## 7. 追溯

### 7.1 追溯链

```text
Goal / ProjectBrief
  → Issue
  → Requirement
  → AcceptanceScenario / Holdout
  → ArchitectureDecision / Contract
  → DesignDoc
  → Task / ChangeSet
  → TestPlan / TestCase / TestRun
  → Release
  → Deployment / Incident / Feedback
```

边类型见 RFC 0009：`derives_from`、`satisfies`、`refines`、`implements`、`verifies`、`deploys`、`causes`。

### 7.2 追溯覆盖

- 每个 Requirement 至少有一条来源边、一条实现边、一条验证边。
- 每个验收场景至少有一个测试或人工验证证据。
- 每个发布至少关联一个需求基线。
- 覆盖缺口在需求页和发布门禁中显示。
- 追溯边可以由辅助系统建议，但关键边必须确认。

## 8. 数据模型

```sql
create table requirements (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  title         text not null,
  problem       text not null,
  users         jsonb not null default '[]'::jsonb,
  non_goals     jsonb not null default '[]'::jsonb,
  constraints   jsonb not null default '[]'::jsonb,
  priority      text not null,
  risk          text not null,
  owner_kind    text not null,
  owner_id      text not null,
  status        text not null,
  version       integer not null default 1,
  source_issues jsonb not null default '[]'::jsonb,
  assumptions   jsonb not null default '[]'::jsonb,
  open_questions jsonb not null default '[]'::jsonb,
  non_functional jsonb not null default '[]'::jsonb,
  created_at    timestamptz not null default now(),
  updated_at    timestamptz not null default now()
);

create table requirement_scenarios (
  id            uuid primary key,
  requirement_id uuid not null references requirements (id),
  artifact_id   uuid not null,
  title         text not null,
  given_text    text not null,
  when_text     text not null,
  then_text     text not null,
  verification  text not null,
  holdout       boolean not null default false,
  priority      text not null,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table requirement_holdouts (
  id            uuid primary key,
  requirement_id uuid not null references requirements (id),
  scenario_id   uuid not null references requirement_scenarios (id),
  owner         jsonb not null,
  access_policy jsonb not null,
  content_ref   jsonb not null,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table requirement_baseline_items (
  baseline_id   uuid not null references baselines (id),
  requirement_id uuid not null references requirements (id),
  requirement_version integer not null,
  scenario_versions jsonb not null default '[]'::jsonb,
  primary key (baseline_id, requirement_id)
);
```

需求基线本身使用 RFC 0009 的 `baselines`（`scope = requirements`），不另建一张重复的基线表；`requirement_baseline_items` 只补需求特有的场景版本。

`requirement_scenarios.content` 长文本可存 Git；`content_ref` 指向内容。holdout 的 `access_policy` 控制读取。

## 9. API、MCP、CLI、Web

### 9.1 REST

```text
GET    /api/v1/projects/{project}/requirements?status=&owner=&q=
POST   /api/v1/projects/{project}/requirements
GET    /api/v1/requirements/{id}
POST   /api/v1/requirements/{id}/versions
POST   /api/v1/requirements/{id}/transitions
GET    /api/v1/requirements/{id}/scenarios
POST   /api/v1/requirements/{id}/scenarios
GET    /api/v1/requirements/{id}/holdouts        # 受权限限制
POST   /api/v1/requirements/{id}/analyze
GET    /api/v1/projects/{project}/requirement-pool
POST   /api/v1/issues/{id}/disposition
GET    /api/v1/projects/{project}/requirements/trace
GET    /api/v1/projects/{project}/requirements/coverage
```

### 9.2 MCP

- `requirement.list` / `requirement.get`
- `requirement.create` / `requirement.update_draft`
- `requirement.submit_review`
- `requirement.analyze`（调用辅助能力）
- `requirement.scenario.add`
- `requirement.trace`
- `requirement.coverage`
- `requirement.holdout.get`（仅独立验证方）
- `issue.disposition`

Agent 不能调用 `requirement.approve` 或 `requirement.holdout` 的读取（除非被授权为独立验证方）。

### 9.3 CLI

```bash
foundry requirement list <project>
foundry requirement create --from-issue <issue>
foundry requirement analyze <requirement>
foundry requirement scenario add <requirement> --file scenario.json
foundry requirement transition <requirement> --to InReview
foundry requirement trace <requirement>
foundry requirement coverage <project>
```

### 9.4 Web

- 需求池：候选、来源、价值/风险、分诊、处置。
- 需求详情：问题、用户、验收、non-goals、假设、开放问题、NFR、追溯、变更。
- 需求评审：质量检查、辅助发现、holdout 状态、批准原因。
- 追溯矩阵：需求 → 架构 → 任务 → 测试 → 发布。
- 覆盖率：验收覆盖、测试覆盖、追溯缺口。

## 10. 权限与安全

- 人类账户可以创建、编辑、提交评审；批准需要 Maintainer/Owner 或显式授权角色。
- Agent 可以创建草稿、提交分析、补充场景候选；不能批准需求。
- 实现 Agent 不能读取 holdout；独立验证方可以。
- holdout 渲染、导出、日志和模型上下文都要做脱敏；泄漏事件记录并触发重新生成。
- 需求内容按项目隔离；敏感需求（安全、合规、客户数据）设置 `sensitivity`。
- 变更、批准、holdout 访问和泄漏事件写入审计日志。

## 11. 与现有实现的关系

现有 Requirement 是早期实现（`work_items` + 领域模型 + 状态机）。迁移策略：

1. 保留 `work_items` 作为通用身份和查询入口；新增 `requirements` 类型化表，通过 `artifact_id`/`work_item_id` 关联。
2. 现有字段映射：`problem`、`users`、`acceptance`、`non_goals`、`constraints`、`priority`、`risk`、`owner`。
3. 现有状态机保留；新增门禁先以告警模式运行，再阻塞。
4. 现有验收场景如果是自由文本，迁移为初始 Scenario；无法结构化的保留为 `legacy` 场景，要求后续补齐。
5. 现有 `links` 迁移为 `artifact_edges`，标记 `origin=derived`。
6. 新老版本并存期间，API 返回统一视图；旧客户端可以继续使用旧字段。
7. 迁移脚本幂等、可重放，带校验和回滚说明。

## 12. 实施计划

### 阶段 0：模型与迁移

- `requirements`、`requirement_scenarios`、`requirement_holdouts` 表。
- 现有 Requirement 迁移；状态机和字段兼容。
- 出口标准：旧需求可读、可编辑，新增字段可选。

### 阶段 1：需求池与分诊

- 需求池视图、Issue 处置、候选合并、分诊记录。
- 出口标准：Issue 可以显式转为 Requirement 草稿，来源和讨论保留。

### 阶段 2：验收场景与 holdout

- 场景编辑、验证方式、holdout 存储与访问控制。
- 出口标准：批准要求公开场景 + holdout；实现者无法读取 holdout。

### 阶段 3：分析辅助与质量门禁

- `requirements.analyze`、`requirements.scenarios`、`requirements.trace` 接入。
- 质量检查、批准门禁、追溯覆盖。
- 出口标准：批准前能看到完整性/歧义/一致性发现，并定位到字段。

### 阶段 4：基线与变更

- 需求基线、ChangeRequest、影响分析、重新验证。
- 出口标准：需求变更能影响分析到架构/任务/测试/发布。

## 13. 验收标准

1. Issue 不能直接变成 Requirement；必须经过显式处置和分析。
2. 创建 Requirement 时 `problem/users/acceptance` 必填；批准时 `non_goals` 必填。
3. 每个 Requirement 至少有一个可执行验收场景；批准时 holdout 检查通过或有 Waiver。
4. 实现 Agent 的输入、日志和工作区中不存在 holdout 内容。
5. 已批准需求不能原地编辑；变更产生新版本、ChangeRequest 和影响分析。
6. 需求基线可创建、批准、diff；发布能引用基线。
7. 需求追溯覆盖可查询；缺口在需求页和发布门禁中显示。
8. 辅助分析输出可追溯、可验证、可替换；不自动批准。
9. 需求状态转换的门禁在界面上显示原因，并定位到缺失字段。
10. 迁移后旧需求可读、可编辑，历史讨论和来源链接保留。

## 14. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 表单过重 | 小需求也要填很多字段 | 按风险分级；低风险用精简模板 |
| holdout 泄漏 | 出现在日志、上下文、代码注释 | 隔离存储、脱敏、泄漏检测、重新生成 |
| 辅助分析噪音 | 歧义/冲突发现过多 | 置信度、按风险过滤、人工确认 |
| 需求膨胀 | 一个需求包含多个承诺 | 拆分规则、范围检查、non_goals |
| 追溯维护成本 | 手工边容易过期 | 自动推导 + 人工确认；stale 传播 |
| 需求变更频繁 | 基线不断变化 | 变更门槛、影响分析、发布窗口 |
| Agent 自证 | Agent 写需求又批准 | 职责分离；批准人 ≠ 创建人 |
| 产品判断缺失 | 系统能分析但不能决定做什么 | 产品/组合阶段由人类负责；系统提供证据 |

**待决问题**：

1. 低风险需求是否强制 holdout？建议至少 1 个，或 Waiver。
2. holdout 由哪个角色持有？建议独立验证方（tester/verifier），不能是实现者。
3. 需求基线在批准时建还是发布时建？建议批准时建草稿基线，发布时批准。
4. 现有自由文本验收如何迁移？建议保留 `legacy` 并逐步结构化。
5. 需求池是否支持跨项目组合视图？建议后续在组合阶段实现，不提前做。
