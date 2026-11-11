# RFC 0017：产品、组合与立项管理

- 状态：Draft；ProjectBrief 部分已被 RFC 0020 取代
- 日期：2026-11-09
- 关联：[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0009 产物血缘、基线与变更管理](0009-artifact-lineage-baselines-and-change.md)、[RFC 0011 需求管理与需求分析](0011-requirements-management.md)、[RFC 0016 度量、成本与自举](0016-metrics-cost-and-self-hosting.md)、[RFC 0020 需求子系统](0020-requirements-subsystem.md)

> 2026-11-11 更新：独立的 `ProjectBrief` 聚合已删除，项目级内容（定位、background：context / problems；goals / non_goals / constraints / risks、glossary）现在属于独立的项目文档，见 RFC 0020 和 `docs/api/README.md`。预算和干系人字段不再属于项目文档。本文以下关于 ProjectBrief 的描述保留为历史设计记录。

- 作用：本文补全生命周期最前端：产品判断、组合管理、项目立项和目标澄清。它回答“为什么做、做哪个、做到什么程度算成功”。

## 实现状态（2026-11-09，第一切片）

已实现 ProjectBrief（已被 RFC 0020 合并取代）：

- 领域模型：问题、目标、成功指标、范围/non-goals、约束、风险、干系人、预算、假设、开放问题。
- 状态：`draft → in_review → approved`；校验在提交和批准时执行。
- HTTP：`GET/PUT /api/v1/projects/{project}/brief/...`，提交和批准使用 `If-Match`。
- 存储：Postgres（迁移 `0012_project_briefs.sql`）和内存适配器。
- MCP：`get_project_brief`、`get_project_brief_draft`、`save_project_brief_draft`、`submit_project_brief`、`approve_project_brief`。
- Agent runtime：`task.links.list` 找统一 Issue、`issue.get`/`issue.comment` 讨论、评论后暂停/恢复、ProjectBrief 工具、`foundry-agent clarify` 交互式命令。
- 批准权限：仅人类 Maintainer/Owner；Agent 只能起草和提交。

尚未实现：成功指标实际值与达成率、组合管理、立项审批 UI、Worker 自动领取澄清任务。

## 0. 结论

软件生命周期不是从 Issue 开始的。Issue 之前还有产品判断、组合取舍和立项。系统可以管理这些决策的对象、证据和门禁，但不能替代产品判断。

核心决定：

1. **产品判断仍由人类或明确授权的产品策略负责。** Agent 可以收集、分析、建模和起草；目标取舍、预算和立项批准由人类或显式授权角色完成。
2. **组合管理是项目选择的视图和门禁。** 组合不是新工作流引擎；它是项目、预算、容量、依赖和优先级的管理层。
3. **立项有明确产物。** ProjectBrief、SuccessMetric、Constraint、Risk、Stakeholder、Scope；没有这些不进入需求阶段。
4. **目标必须可度量。** 成功指标有基线、目标值、测量方式和时间窗；不可度量的目标不能通过立项门禁。
5. **立项与根流程绑定。** 项目创建时选择根流程（RFC 0005），根流程的阶段就是项目生命周期阶段（RFC 0008）。
6. **组合决策可追溯。** 为什么选这个项目、为什么不选另一个、预算和假设是什么，都要记录；后续可以复审。
7. **产品反馈闭环。** 用户研究、市场、监控、销售、事故都可以进入组合和需求池；不是只有 Issue。
8. **Agent 可以持续扫描信号。** Agent 可以监控市场、反馈和指标，生成机会候选；不能自动立项或改变组合优先级。
9. **成本与容量可见。** 组合决策必须考虑预算、团队/Agent 容量、依赖和风险；不能只看价值。
10. **不追求通用 PMO 套件。** 只管理系统闭环需要的对象和门禁；路线图、预算和资源分配可以集成外部工具。
11. **组合指标进入度量体系。** 战略契合、ROI、决策周期、容量利用率、项目失败率进入 RFC 0016。
12. **立项可以轻量。** 小项目用精简简报；高风险/大预算项目需要完整评审。

## 1. 目标与非目标

### 1.1 目标

- 让项目选择有依据、有记录、可复审。
- 让目标、成功指标、约束和 non-goals 在立项时明确。
- 让组合层面的预算、容量、依赖和优先级可见。
- 让产品判断和 Agent 分析分工清晰。
- 让立项产物进入需求、架构和发布的血缘。
- 让“为什么做”和“怎样算成功”成为后续验收的依据。

### 1.2 非目标

- 不实现通用项目管理/PMO 套件。
- 不实现市场研究、财务核算或销售管理；通过适配器接入。
- 不自动决定产品方向或组合优先级。
- 不实现跨组织资源调度；先做单组织组合。
- 不要求所有项目都走重流程；按风险、预算和影响分级。
- 不实现通用 OKR 工具；成功指标可以和外部 OKR 对齐。
- 不实现投资回报的财务审计；ROI 是工程决策参考。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| Vision | 产品/平台的长期方向。 |
| Strategy | 当前阶段的战略选择和取舍。 |
| Portfolio | 一组项目及其预算、容量、依赖和优先级。 |
| Opportunity | 机会候选：市场、客户、技术、合规或内部改进。 |
| ProjectBrief | 立项简报：问题、目标、成功指标、范围、约束、风险、干系人。 |
| SuccessMetric | 可度量的成功标准：基线、目标、测量方式、时间窗。 |
| Constraint | 立项约束：预算、时间、合规、技术、依赖。 |
| Risk | 立项风险：概率、影响、缓解、负责人。 |
| Stakeholder | 干系人：角色、关注点、决策权。 |
| PortfolioDecision | 组合决策：选择/不选择/排序/预算。 |
| ProjectCharter | 项目章程：简报批准后的正式授权。 |

不变量：

1. 项目创建必须有批准的 ProjectBrief 或明确的精简立项记录。
2. 成功指标必须有基线、目标、测量方式和时间窗；不可度量不能批准。
3. 组合决策必须记录选项、标准、证据和决策者。
4. 立项批准人不能是唯一分析者（职责分离）；高风险项目需要多角色批准。
5. Agent 不能批准立项、组合决策或预算。
6. 项目必须选择根流程；根流程版本固定（RFC 0005）。
7. 立项产物进入血缘：Vision → Opportunity → PortfolioDecision → ProjectBrief → Requirement。
8. 预算和容量变更走 ChangeRequest 或组合决策更新。
9. 立项后的目标变更走 ChangeRequest，不静默修改。
10. 组合和立项数据按项目/组织隔离。

## 3. 对象模型

### 3.1 Vision / Strategy

```json
{
  "id": "uuid",
  "scope": "organization|product|platform",
  "vision": "...",
  "strategy": ["..."],
  "horizon": "2026-2028",
  "owner": {"kind": "human", "id": "..."},
  "status": "draft|active|superseded",
  "created_at": "..."
}
```

规则：

- Vision 和 Strategy 是长期对象；变更少见但必须有记录。
- 不进入每个项目的日常流程，但项目立项要引用。
- Agent 可以分析和建议，不能批准。

### 3.2 Opportunity

```json
{
  "id": "uuid",
  "project_id": null,
  "source": "customer|market|monitoring|sales|internal|compliance",
  "title": "中小客户需要部分退款",
  "description": "...",
  "evidence": ["feedback-id", "analytics-id"],
  "estimated_value": {"type": "revenue|cost|risk|strategic", "amount": "..."},
  "confidence": "low|medium|high",
  "status": "new|assessed|selected|deferred|rejected",
  "created_at": "..."
}
```

规则：

- Opportunity 可以跨项目，属于组合层。
- 来源和证据必须可追溯；不能只写“客户需要”。
- 评估结果进入组合决策；未选中的机会保留原因。

### 3.3 ProjectBrief

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "title": "部分退款能力",
  "problem": "客服只能整单退款，人工处理成本高。",
  "goals": ["支持部分退款", "降低人工处理 30%"],
  "success_metrics": ["metric-id"],
  "scope": {"in": ["部分退款"], "out": ["多次退款合并", "跨币种"]},
  "constraints": ["支付网关兼容", "审计保留 180 天"],
  "risks": [{"risk": "网关不支持", "probability": "medium", "impact": "high", "mitigation": "..."}],
  "stakeholders": [{"role": "产品", "id": "..."}, {"role": "客服", "id": "..."}],
  "budget": {"amount": 50000, "currency": "USD"},
  "root_workflow": "project",
  "status": "draft|in_review|approved|superseded",
  "approved_by": {"kind": "human", "id": "..."},
  "created_at": "..."
}
```

规则：

- 必须包含 problem、goals、success_metrics、scope、constraints、risks、stakeholders。
- `scope.out` 就是项目级 non-goals；后续需求变更要与之比较。
- `root_workflow` 决定项目生命周期流程；创建后固定版本。
- 批准后不可原地修改；变更走 ChangeRequest。

### 3.4 SuccessMetric

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "name": "人工退款处理量下降",
  "baseline": 1000,
  "target": 700,
  "unit": "cases/month",
  "measurement": "support dashboard",
  "window": "3 months after launch",
  "owner": {"kind": "human", "id": "..."},
  "status": "active|retired",
  "created_at": "..."
}
```

规则：

- 必须有基线、目标、单位、测量方式和时间窗。
- 测量方式必须可执行或可审计；不能写“看情况”。
- 目标未达成时，项目复盘必须解释；不能静默改目标。

### 3.5 Portfolio / PortfolioDecision

```json
{
  "id": "uuid",
  "portfolio_id": "uuid",
  "title": "2026 Q4 组合",
  "items": [
    {"project_id": "uuid", "priority": 1, "budget": 120000, "capacity": "2 teams"},
    {"opportunity_id": "uuid", "decision": "deferred", "reason": "依赖未就绪"}
  ],
  "criteria": ["战略契合", "ROI", "风险", "依赖", "容量"],
  "deciders": [{"kind": "human", "id": "..."}],
  "status": "draft|approved|superseded",
  "created_at": "..."
}
```

规则：

- 组合决策必须记录选项、标准、证据和决策者。
- 未选中的机会保留原因；后续可以复审。
- 组合变更有版本和影响分析；预算和容量变化要可见。
- Agent 可以生成候选排序，不能批准。

## 4. 组合管理

### 4.1 组合视图

- 按战略目标、产品线、客户、项目类型分组。
- 显示预算、容量、依赖、风险、进度和成功指标。
- 支持“如果增加/减少预算会怎样”的模拟，但模拟不是承诺。

### 4.2 选择标准

| 标准 | 说明 |
|---|---|
| 战略契合 | 是否支持 Vision/Strategy |
| 价值 | 收入、成本、风险、合规、战略 |
| 置信度 | 证据强度、假设 |
| 成本 | 预算、人力/Agent、时间 |
| 风险 | 技术、依赖、合规、交付 |
| 容量 | 团队/Agent 并发和技能 |
| 可逆性 | 失败能否低成本停止 |

### 4.3 组合决策

```text
机会/项目候选
  → 证据收集
  → 评分和排序
  → 容量与依赖检查
  → 组合决策（选择/延期/拒绝）
  → 预算和授权
  → 立项
```

规则：

- 决策必须可解释；不能只给一个排名。
- 预算和容量是硬约束；超配需要显式批准。
- 组合决策定期复审；环境变化可以重开。

## 5. 立项与目标澄清

### 5.1 流程

```text
Opportunity / 组合决策
  → 创建项目（选择根流程）
  → 起草 ProjectBrief
  → 目标澄清（goal_clarification）
  → 风险/可行性分析
  → 成功指标确认
  → 立项评审
  → 批准（ProjectCharter）
  → 进入需求阶段
```

### 5.2 门禁

| 门禁 | 条件 |
|---|---|
| initiation.brief | ProjectBrief 必填字段完整 |
| initiation.goals | 目标可度量、成功指标有基线/目标/测量 |
| initiation.scope | in/out 明确；non-goals 已记录 |
| initiation.risk | 主要风险有缓解和负责人 |
| initiation.stakeholders | 关键干系人确认 |
| initiation.budget | 预算和容量批准 |
| initiation.workflow | 根流程已选择且版本固定 |

### 5.3 轻量与完整立项

| 类型 | 条件 | 要求 |
|---|---|---|
| 轻量 | 低风险、可逆、预算小 | 精简简报；目标、范围、成功指标、负责人 |
| 完整 | 高风险、不可逆、预算大、合规 | 完整简报 + 评审 + 多角色批准 |

## 6. 辅助系统

| 能力 | 输入 | 输出 | 信任 |
|---|---|---|---|
| `market.research` | 市场、竞品、公开数据 | 机会、趋势、风险 | self_reported |
| `feedback.aggregate` | 客户反馈、工单、评论 | 主题、频次、影响 | tool_verified |
| `opportunity.score` | 机会、标准、证据 | 评分、敏感性 | self_reported |
| `portfolio.simulate` | 组合、预算、容量 | 情景模拟 | self_reported |
| `feasibility.analyze` | 目标、约束、技术 | 可行性、风险、依赖 | self_reported |
| `metric.baseline` | 指标、数据源 | 基线、测量方式 | tool_verified |
| `brief.draft` | 机会、证据 | 简报草稿 | self_reported |

规则：

- 辅助输出是草稿和证据，不是批准。
- 市场/竞品数据必须标注来源和时间；过期数据降级。
- 评分标准公开、可复现；不能黑箱排序。
- 组合模拟不构成承诺；实际决策仍需批准。

## 7. 追溯与指标

### 7.1 追溯

```text
Vision / Strategy
  → Opportunity
  → PortfolioDecision
  → ProjectBrief
  → SuccessMetric
  → Requirement
  → 架构/实现/测试/发布
  → 指标结果
```

边类型见 RFC 0009：`derives_from`、`satisfies`、`refines`、`verifies`。

### 7.2 指标

- 组合：战略契合度、ROI、预算利用率、容量利用率、项目失败率。
- 立项：决策周期、目标清晰度、指标可度量率。
- 交付：目标达成率、成功指标达成率、范围漂移。
- 这些指标进入 RFC 0016。

## 8. 数据模型

```sql
create table visions (
  id            uuid primary key,
  scope         text not null,
  vision        text not null,
  strategy      jsonb not null default '[]'::jsonb,
  horizon       text,
  owner         jsonb not null,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table opportunities (
  id            uuid primary key,
  source        text not null,
  title         text not null,
  description   text not null,
  evidence      jsonb not null default '[]'::jsonb,
  estimated_value jsonb not null default '{}'::jsonb,
  confidence    text not null,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table project_briefs (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  title         text not null,
  problem       text not null,
  goals         jsonb not null default '[]'::jsonb,
  success_metrics jsonb not null default '[]'::jsonb,
  scope         jsonb not null,
  constraints   jsonb not null default '[]'::jsonb,
  risks         jsonb not null default '[]'::jsonb,
  stakeholders  jsonb not null default '[]'::jsonb,
  budget        jsonb,
  root_workflow text not null,
  status        text not null,
  approved_by   jsonb,
  created_at    timestamptz not null default now()
);

create table success_metrics (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  name          text not null,
  baseline      numeric,
  target        numeric,
  unit          text not null,
  measurement   text not null,
  window        text not null,
  owner         jsonb not null,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table portfolio_decisions (
  id            uuid primary key,
  title         text not null,
  items         jsonb not null,
  criteria      jsonb not null,
  deciders      jsonb not null,
  status        text not null,
  created_at    timestamptz not null default now()
);
```

## 9. API、MCP、CLI、Web

### 9.1 REST

```text
GET/POST /api/v1/visions
GET/POST /api/v1/opportunities
POST     /api/v1/opportunities/{id}/assess
GET/POST /api/v1/portfolios/{id}/decisions
GET/POST /api/v1/projects/{project}/brief
POST     /api/v1/projects/{project}/brief/approve
GET/POST /api/v1/projects/{project}/success-metrics
GET      /api/v1/projects/{project}/metrics/attainment
```

### 9.2 MCP

- `vision.get`
- `opportunity.list` / `opportunity.submit`
- `portfolio.candidates` / `portfolio.simulate`
- `brief.draft` / `brief.get`
- `metrics.attainment`

Agent 可以提交机会、生成简报草稿和模拟；不能批准组合决策或立项。

### 9.3 CLI

```bash
foundry opportunity list
foundry opportunity submit --source customer --title "..."
foundry portfolio simulate --budget 500000
foundry brief draft <project>
foundry brief approve <project>
foundry metrics attainment <project>
```

### 9.4 Web

- 组合页：项目、预算、容量、依赖、风险、优先级。
- 机会页：来源、证据、评分、决策。
- 立项页：简报、目标、成功指标、风险、干系人、批准。
- 目标达成页：成功指标、基线、目标、实际、趋势。

## 10. 权限与安全

- 组合决策和立项批准需要人类或显式授权的产品/组合角色。
- Agent 可以分析和起草；不能批准、不能改预算、不能改组合优先级。
- 预算、客户和合同数据按权限过滤；敏感机会脱敏。
- 组合决策和立项变更写入审计日志。
- 跨项目机会共享需要组合权限；项目数据仍隔离。

## 11. 与现有实现的关系

- 现有 `Project` 和 `goal_clarification` 流程是立项阶段的实现基础；本 RFC 增加 ProjectBrief、SuccessMetric 和组合对象。
- 项目创建必须选择根流程（RFC 0005 已要求）；本 RFC 把 ProjectBrief 和根流程绑定。
- 现有 Issue 继续作为问题空间入口；Opportunity 是组合层的候选，可以转化为 Issue/Requirement。
- 现有 `project_review` 流程与成功指标达成复盘衔接。
- 现有 `documents`/Artifact 承载简报内容；类型化表存可查询字段。

## 12. 实施计划

### 阶段 0：简报与目标

- ProjectBrief、SuccessMetric 表；精简/完整立项门禁。
- 出口标准：新项目有目标、成功指标、范围、风险和批准。

### 阶段 1：机会与组合

- Opportunity、PortfolioDecision；评分和决策记录。
- 出口标准：组合决策可解释、可追溯；未选项保留原因。

### 阶段 2：辅助能力

- 市场/反馈聚合、机会评分、可行性、组合模拟接入。
- 出口标准：辅助输出可追溯、可验证、可替换。

### 阶段 3：目标达成与复盘

- 成功指标实际值、达成率、复盘。
- 出口标准：项目完成时能回答“是否达到目标”。

## 13. 验收标准

1. 新项目有 ProjectBrief，必填字段完整；轻量项目有精简记录。
2. 成功指标有基线、目标、单位、测量方式和时间窗。
3. 立项批准人不是唯一分析者；高风险项目有多角色批准。
4. 项目根流程已选择并固定版本。
5. 组合决策记录选项、标准、证据和决策者；未选项保留原因。
6. Agent 不能批准立项或组合决策。
7. 机会和立项产物进入血缘，能追溯到需求。
8. 目标变更走 ChangeRequest，不静默修改。
9. 项目完成时能查询成功指标达成率。
10. 敏感机会/预算数据按权限过滤。

## 14. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 产品判断被自动化 | Agent 评分替代人类决策 | 明确 Agent 只建议；人类批准 |
| 指标不可测 | 目标写得好听但无法测 | 门禁要求测量方式；不可测不批准 |
| 组合数据过期 | 市场和预算变化 | 数据时间戳、定期复审 |
| 流程过重 | 小项目填很多表 | 轻量模板；按风险分级 |
| 预算/容量不准 | 估算偏差 | 透明假设；定期校准 |
| 机会噪音 | 反馈/市场信号过多 | 聚合、评分、阈值 |
| 目标漂移 | 项目进行中改目标 | ChangeRequest + 影响分析 |
| 跨组织组合 | 多组织资源调度 | 后续阶段；先单组织 |

**待决问题**：

1. 轻量立项的预算阈值如何定？建议按组织配置。
2. 组合决策复审频率？建议季度或重大变化时。
3. 成功指标由谁维护？建议项目 Owner + 产品负责人。
4. 是否接入外部 OKR/财务系统？建议通过适配器，不重造。
5. 机会是否必须关联项目？不必须；组合层可以存在未立项机会。
