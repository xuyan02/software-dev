# RFC 0016：度量、成本与自举

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0013 实现、评审、测试与质量门禁](0013-implementation-review-testing-and-quality.md)、[RFC 0014 发布、部署、运维、反馈与退役](0014-release-deployment-operations-and-retirement.md)、[RFC 0015 Agent 架构与评测](0015-agent-architecture-and-evaluation.md)
- 作用：本文定义生命周期的度量、成本核算、闭环层级和自举计划。它回答“这套系统是否真的在收敛”，而不只是“完成了多少任务”。

## 0. 结论

没有度量，AI 原生开发就只是一个叙事。度量必须覆盖**质量、收敛、信任、成本和人类负担**，不能只有吞吐。

核心决定：

1. **度量回答收敛问题。** 不只看完成了多少任务，还要看返工率、缺陷逃逸率、变更失败率、证据覆盖率和人工接管率是否下降。
2. **指标有定义、有版本、有负责人。** 每个指标声明公式、单位、维度、数据来源、采样窗口和负责人；定义变更产生新版本，历史快照不重算。
3. **成本按任务归集。** 模型 token、工具调用、沙箱时间、辅助系统调用、人工评审和事故成本都要记录；不能只算模型费用。
4. **闭环分级验收。** L1 单任务、L2 单迭代、L3 单项目、L4 自举、L5 组织；每级有可测量的出口标准。
5. **自举是最强验收。** Foundry 用自己的流程、Agent 和证据管理自己的研发；如果管不好自己，就不能证明能管好别人。
6. **指标不能被单点优化。** 覆盖率、成功率、速度都可能被刷；必须组合质量、成本和人类接管指标，并定期抽查。
7. **证据覆盖率是信任指标。** 统计有多少状态转换有达到信任等级的证据；没有证据的转换即使“完成”也不计入高质量交付。
8. **Waiver 和例外要可见。** 豁免率、过期 Waiver、Stale 证据、人工接管都是系统健康信号。
9. **告警触发行动。** 指标越过阈值时触发评审、降级、回滚或改进项，而不是只上仪表盘。
10. **自举分阶段。** 先让 Foundry 管理 Foundry 的任务，再让 Agent 实现低风险任务，最后扩展到需求、设计和发布；每一步都有退出标准。
11. **不追求单一分数。** 不用一个“AI 效能分”掩盖结构性差异；按项目、阶段、角色、Agent 分解。
12. **隐私与合规。** 指标按项目隔离；个人和客户数据脱敏；成本数据按权限可见。

## 1. 目标与非目标

### 1.1 目标

- 让“是否闭环、是否收敛、是否值得”成为可测量的问题。
- 让质量、速度、成本、信任和人类负担一起被看见。
- 让 Agent 和辅助系统的效果可比较、可归因。
- 让自举有明确的阶段和退出标准。
- 让指标驱动改进，而不是驱动表演。

### 1.2 非目标

- 不实现通用 BI 平台；指标通过查询和导出接入现有 BI。
- 不实现财务核算；成本记录用于工程决策，不替代财务系统。
- 不实现个人绩效评估；指标用于系统和流程改进，不用于排名个人。
- 不承诺单一“AI 生产率”数字；度量按上下文分解。
- 不实现实时流式指标；先做分钟/小时级快照。
- 不实现跨组织对标；先做项目内和项目间可比。
- 不把自举当成完全无人；人类仍在关键决策点。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| MetricDefinition | 指标定义：名称、公式、单位、维度、来源、负责人、版本。 |
| MetricSnapshot | 某时间窗口的指标值；不可变，带来源和样本量。 |
| CostRecord | 一次成本的归集记录：模型、工具、沙箱、辅助、人工、事故。 |
| ClosureLevel | 闭环层级 L1–L5。 |
| EvidenceCoverage | 有达标证据的状态转换比例。 |
| HumanTakeoverRate | 需要人工接管的任务比例。 |
| ChangeFailureRate | 导致事故或回滚的发布比例。 |
| DefectEscapeRate | 生产发现的缺陷占全部缺陷比例。 |
| Self-hosting | 用 Foundry 管理 Foundry 自身的研发。 |
| MetricOwner | 对指标定义、口径和改进负责的人/角色。 |

不变量：

1. 每个指标有定义、版本、单位和负责人；没有定义的指标不能进入仪表盘。
2. MetricSnapshot 不可变；重算产生新快照，不覆盖旧快照。
3. 指标必须带样本量、时间窗口和数据来源；样本不足时标记不可用。
4. 成本按项目、任务、AgentVersion 和类别归集；不能只记录总额。
5. 闭环层级按项目和阶段评估；不能用一个数字代表整个组织。
6. 自举阶段有明确范围、退出标准和回滚条件。
7. 指标不能用于个人排名；用于系统、流程和能力改进。
8. 敏感数据在指标中脱敏；个人和客户标识不进入快照。
9. 告警阈值有负责人和处置动作；不能只告警不行动。
10. 指标定义变更保留历史，历史对比注明口径变化。

## 3. 指标体系

### 3.1 交付与速度

| 指标 | 公式 | 说明 |
|---|---|---|
| Lead time | 需求批准 → 生产部署 | 端到端交付时间 |
| Cycle time | 工作项开始 → 完成 | 阶段内效率 |
| Throughput | 完成工作项数 / 周期 | 吞吐 |
| Review latency | 提交评审 → 决定 | 评审等待 |
| Deployment frequency | 部署次数 / 周期 | 发布节奏 |

### 3.2 质量与收敛

| 指标 | 公式 | 说明 |
|---|---|---|
| Rework rate | 返工工作项 / 完成工作项 | 返工 |
| Defect escape rate | 生产缺陷 / 全部缺陷 | 缺陷逃逸 |
| Change failure rate | 导致事故/回滚的发布 / 发布数 | 变更失败 |
| MTTR | 事故恢复时间均值 | 恢复能力 |
| Flaky rate | flaky 用例 / 全部用例 | 测试稳定性 |
| Requirement volatility | 需求变更数 / 基线需求数 | 需求漂移 |
| Regression rate | 回归缺陷 / 发布数 | 回归 |

### 3.3 信任与证据

| 指标 | 公式 | 说明 |
|---|---|---|
| Evidence coverage | 有达标证据的转换 / 全部转换 | 证据完整性 |
| Stale evidence rate | 过期证据 / 全部证据 | 证据新鲜度 |
| Gate pass rate | 通过门禁 / 评估次数 | 门禁健康 |
| Waiver rate | 豁免次数 / 门禁评估次数 | 例外依赖 |
| Expired waiver count | 过期未处理豁免数 | 风险 |
| Traceability coverage | 有完整血缘的需求 / 全部需求 | 追溯完整性 |
| Holdout leak count | holdout 泄漏事件数 | 独立性 |

### 3.4 Agent 与辅助系统

| 指标 | 公式 | 说明 |
|---|---|---|
| Agent success rate | 成功任务 / 分配任务 | 能力 |
| Human takeover rate | 人工接管 / 分配任务 | 人类负担 |
| Cost per task | 总成本 / 完成任务 | 成本 |
| Eval pass rate | 通过评测版本 / 评测版本 | 质量门禁 |
| Drift alerts | 漂移告警数 | 稳定性 |
| Tool error rate | 工具失败 / 工具调用 | 工具健康 |
| Auxiliary agreement rate | 辅助结论与独立验证一致 / 验证数 | 辅助质量 |

### 3.5 成本

| 类别 | 记录 |
|---|---|
| 模型 | token、输入/输出、单价、费用 |
| 工具 | 调用次数、单价、外部服务费 |
| 沙箱 | CPU/内存/存储时间、单价 |
| 辅助系统 | 调用次数、费用 |
| 人工 | 评审/接管/事故工时 |
| 事故 | 停机、修复、客户影响 |

### 3.6 价值

- 交付的需求价值（按优先级/客户/收入）。
- 节省的人工时间（需谨慎估算）。
- 质量改善（缺陷、事故、返工）。
- 成本节约（基础设施、工具）。
- 不能只用“完成任务数”证明价值。

## 4. 闭环层级

### L1 单任务闭环

标准：

- Agent 从 Issue 到 PR 合并完成一次任务。
- 有独立测试/评审证据。
- 可重复执行至少 20 次样本。
- 成功率达到预设阈值（例如 ≥ 80%），人工接管率可测。
- 成本在预算内。

### L2 单迭代闭环

标准：

- 规划 → 实现 → 评审 → 测试 → 验收完整跑通。
- 需求、架构、实现、测试有追溯。
- 返工率和周期时间可测。
- 至少 3 个迭代样本。

### L3 单项目闭环

标准：

- 需求 → 架构 → 多迭代 → 发布 → 观察 → 反馈跑通。
- 发布有独立证据、批准、回滚和观察期。
- 缺陷逃逸率、变更失败率、MTTR 可测。
- 生产反馈能进入下一轮需求。

### L4 自举闭环

标准：

- Foundry 自身的需求、设计、任务、测试和发布由 Foundry 管理。
- 至少一个季度或连续 N 个迭代。
- Agent 完成比例、人工接管率、缺陷逃逸、证据覆盖可测。
- 没有因自举导致的严重事故；有则复盘并改进。

### L5 组织闭环

标准：

- 多项目组合、资源分配、优先级和成本可见。
- 跨项目依赖和共享能力可管理。
- 组织级质量和交付指标可预测。
- 权限、合规和审计满足要求。

每级达标后才对外声称支持对应范围的软件开发。

## 5. 成本核算

### 5.1 CostRecord

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "task_id": "uuid|null",
  "agent_version_id": "uuid|null",
  "category": "model|tool|sandbox|auxiliary|human|incident",
  "quantity": 18234,
  "unit": "tokens|calls|seconds|hours|usd",
  "amount": 0.42,
  "currency": "USD",
  "source": "usage_api|manual|estimate",
  "occurred_at": "..."
}
```

规则：

- 成本按任务/项目/Agent 归集；无法归集时标记 `unallocated`。
- 估算和实际分开；估算不用于结算。
- 人工成本按角色工时记录，不记录个人明细到公开指标。
- 事故成本包括停机和修复，尽量量化。
- 成本数据保留与财务/合规策略一致。

### 5.2 预算与配额

- 项目、角色、Agent、辅助能力都可以有预算。
- 超预算时拒绝或降级；拒绝原因可见。
- 预算变更走批准；关键项目可以设硬上限。
- 成本告警按天/周/月触发。

### 5.3 ROI

- ROI 需要同时看价值和成本；价值可以按需求优先级、节省时间、质量改善估算。
- ROI 是决策参考，不是唯一指标；不能为了 ROI 牺牲安全或质量。
- 估算方法和假设可见；避免把估算当事实。

## 6. 自举

### 6.1 阶段

| 阶段 | 范围 | 人工角色 | 退出标准 |
|---|---|---|---|
| A 记录 | Foundry 跟踪 Foundry 的任务和证据 | 人类执行，系统记录 | 任务和证据完整 |
| B 低风险实现 | Agent 实现低风险任务（文档、测试、小修复） | 人类评审和发布 | 成功率、返工率达标 |
| C 常规实现 | Agent 实现常规任务，人类评审 | 人类批准需求和发布 | L2/L3 指标达标 |
| D 全生命周期 | Agent 参与需求、设计、测试、发布 | 人类治理关键决策 | L4 指标达标 |

### 6.2 范围选择

- 先选自包含、可测试、低风险的模块（文档、工具链、测试、内部工具）。
- 避免一开始选安全、支付、数据迁移等高风险模块。
- 每个阶段有明确的回滚条件：质量下降、事故、成本超限时暂停。

### 6.3 度量

- 自举任务占比、Agent 完成比例、人工接管率。
- 自举项目的缺陷逃逸、变更失败、MTTR、证据覆盖。
- 自举任务成本 vs 人工基线。
- 自举产生的架构/流程问题进入改进项。

## 7. 数据模型

```sql
create table metric_definitions (
  id            uuid primary key,
  name          text not null,
  version       integer not null,
  formula       text not null,
  unit          text not null,
  dimensions    jsonb not null default '[]'::jsonb,
  source        text not null,
  owner         jsonb not null,
  status        text not null,
  created_at    timestamptz not null default now(),
  unique (name, version)
);

create table metric_snapshots (
  id            uuid primary key,
  metric_id     uuid not null references metric_definitions (id),
  project_id    uuid,
  dimensions    jsonb not null default '{}'::jsonb,
  window_start  timestamptz not null,
  window_end    timestamptz not null,
  value         numeric not null,
  sample_size   integer not null,
  source        text not null,
  created_at    timestamptz not null default now()
);

create table cost_records (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  task_id       uuid,
  agent_version_id uuid,
  category      text not null,
  quantity      numeric not null,
  unit          text not null,
  amount        numeric not null,
  currency      text not null,
  source        text not null,
  occurred_at   timestamptz not null
);

create index metric_snapshots_lookup_idx
  on metric_snapshots (metric_id, project_id, window_start);
create index cost_records_project_time_idx
  on cost_records (project_id, occurred_at);
```

## 8. API、MCP、CLI、Web

### 8.1 REST

```text
GET /api/v1/metrics
GET /api/v1/metrics/{name}/snapshots?project=&from=&to=
GET /api/v1/projects/{project}/closure-level
GET /api/v1/projects/{project}/costs?from=&to=
GET /api/v1/projects/{project}/agent-metrics
GET /api/v1/self-hosting/status
```

### 8.2 MCP

- `metrics.get` / `metrics.snapshots`
- `costs.query`
- `closure.level`
- `selfhosting.status`

Agent 可以读取指标用于改进；不能修改指标定义或关闭告警。

### 8.3 CLI

```bash
foundry metrics list
foundry metrics show lead_time --project <p> --from ...
foundry cost summary <project> --month 2026-11
foundry closure level <project>
foundry selfhosting status
```

### 8.4 Web

- 项目仪表盘：交付、质量、证据、Agent、成本。
- 闭环层级页：L1–L5 标准、当前状态、缺口。
- 自举页：阶段、范围、指标、回滚条件。
- 成本页：类别、趋势、预算、超限。

## 9. 治理

- 每个指标有负责人；定义变更需评审。
- 指标用于改进，不用于个人排名；避免激励扭曲。
- 样本量不足时标记不可用；不做小样本结论。
- 覆盖率、成功率等指标要配合质量和人工接管指标一起看。
- 定期抽查：证据真实性、holdout 独立性、成本归集准确性。
- 告警有处置动作和负责人；长期未处理升级。
- 指标数据访问按项目权限；敏感数据脱敏。

## 10. 实施计划

### 阶段 0：基础记录

- MetricDefinition/Snapshot、CostRecord。
- 先记录任务、证据、成本、人工接管。
- 出口标准：L1 指标可计算。

### 阶段 1：质量与信任指标

- 返工、缺陷逃逸、证据覆盖、Stale、Waiver、追溯覆盖。
- 出口标准：L2 指标可计算，能定位问题。

### 阶段 2：成本与 Agent 指标

- 模型/工具/沙箱/辅助/人工成本；Agent 成功率、接管率、评测通过率。
- 出口标准：成本可按任务归集，Agent 可比较。

### 阶段 3：闭环层级与自举

- L1–L5 评估、自举阶段、退出标准、回滚条件。
- 出口标准：L3/L4 状态可测，自举可运营。

### 阶段 4：告警与治理

- 阈值、告警、处置动作、指标评审、抽查。
- 出口标准：指标驱动改进，而不是只上仪表盘。

## 11. 验收标准

1. 每个指标有定义、版本、单位、来源和负责人。
2. 快照不可变；样本量不足时标记不可用。
3. 成本按项目、任务、Agent 和类别归集；估算和实际分开。
4. L1–L5 每级有明确标准和当前状态。
5. 证据覆盖率、Stale、Waiver、追溯覆盖可查询。
6. Agent 成功率、人工接管率、成本、评测通过率可查询。
7. 自举有阶段、范围、指标和回滚条件。
8. 指标告警有负责人和处置动作。
9. 指标不用于个人排名；敏感数据脱敏。
10. 指标定义变更保留历史，历史对比注明口径。

## 12. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 指标被刷 | 覆盖率/成功率优化 | 组合质量、成本、人工接管；抽查 |
| 成本归集不准 | token/工具/人工难归集 | 来源标记、估算透明、逐步改进 |
| 小样本结论 | 少量任务就下结论 | 样本量门槛、置信区间 |
| 指标过多 | 仪表盘噪音 | 分层：核心 8–10 个，下钻再看 |
| 自举偏差 | 只选容易任务 | holdout、外部基准、人工抽查 |
| 个人隐私 | 人工成本/接管含个人 | 聚合、脱敏、不排名 |
| 告警疲劳 | 阈值过多 | 关键指标、分级、抑制 |
| 价值难量化 | ROI 估算主观 | 透明假设、多指标、不单一决策 |

**待决问题**：

1. 核心指标集合是哪几个？建议 lead time、rework、defect escape、change failure、evidence coverage、human takeover、cost per task、traceability。
2. 自举先从哪个模块开始？建议文档/测试/内部工具。
3. 人工成本如何记录？建议按角色工时聚合，不记录个人明细。
4. 指标快照频率？建议小时/天级，按需触发。
5. 是否对外发布闭环层级？建议先内部评估，稳定后再对外。
