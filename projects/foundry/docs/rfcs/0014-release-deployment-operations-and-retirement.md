# RFC 0014：发布、部署、运维、反馈与退役

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0009 产物血缘、基线与变更管理](0009-artifact-lineage-baselines-and-change.md)、[RFC 0010 辅助系统注册与产物契约](0010-auxiliary-systems-and-evidence.md)、[RFC 0011 需求管理与需求分析](0011-requirements-management.md)、[RFC 0013 实现、评审、测试与质量门禁](0013-implementation-review-testing-and-quality.md)
- 作用：本文定义发布、部署、环境、配置、观察、事故、反馈、改进和退役的对象、流程、门禁与证据。

## 0. 结论

发布不是“部署成功”，而是**在证据、批准和观察都通过后，确认变更可以安全留在生产环境**。开环发布不算闭环。

核心决定：

1. **ReleaseCandidate 与 Release 分离。** Candidate 是可被验证的候选构建；Release 是被批准、可部署、可追溯的版本；Deployment 是 Release 在某个环境的实际部署记录。
2. **Release 必须引用基线。** 发布引用需求基线、架构基线、变更请求和证据；发布说明自动汇总，但需要人类确认。
3. **部署必须固定配置版本。** 环境、配置、密钥引用、数据迁移和镜像 digest 都要记录；不可复现的部署不能作为证据。
4. **观察期是发布的一部分。** 部署完成不等于发布完成；观察窗口内的健康指标、错误率、延迟和业务指标决定发布是否完成或回滚。
5. **回滚计划必须可执行。** 发布前验证回滚路径；回滚也走门禁和证据，不是临时操作。
6. **事故是一等对象。** Incident 是 IssueKind::Incident，但有独立响应流程、指挥官、时间线和复盘；事故可以自动从告警创建。
7. **反馈闭环回到需求。** 用户反馈、监控、事故和改进项必须能进入 Issue/Requirement，形成下一轮输入。
8. **退役是生命周期阶段。** 退役有计划、依赖检查、数据保留/迁移、归档和批准；不能直接删除。
9. **Agent 可以执行，不能自我批准。** Agent 可以准备发布、执行部署、观察、起草复盘；发布批准、不可逆部署、事故关闭由人类或策略授权。
10. **证据分级。** 发布门禁要求独立测试、安全扫描、人类批准和可复现的部署证据；自述不够。
11. **变更失败和 MTTR 可度量。** 发布频率、变更失败率、回滚率、MTTR、错误预算进入指标（RFC 0016）。
12. **不重造部署/监控。** 通过适配器接入 K8s、云平台、监控和告警；Foundry 管状态、门禁、证据和血缘。

## 1. 目标与非目标

### 1.1 目标

- 让发布有完整证据链和批准，而不是“CI 绿了就上”。
- 让部署可复现、可观察、可回滚。
- 让事故响应、复盘和改进形成闭环。
- 让生产反馈进入需求和下一轮迭代。
- 让退役安全、可审计、不留下孤儿依赖和数据。
- 让 Agent 和人类在同一套发布和运维流程下协作。
- 让发布风险、成本和失败率可度量。

### 1.2 非目标

- 不实现部署引擎、K8s 控制器或监控系统；通过适配器接入。
- 不实现通用告警平台；接入现有告警并做状态同步。
- 不实现自动批准发布；不可逆操作仍由人类或显式策略批准。
- 不实现生产数据访问；调试数据需要批准和脱敏。
- 不实现通用实验平台；实验能力通过辅助系统接入。
- 不要求每次发布都人工批准；低风险、可逆、有充分证据的发布可以按策略自动批准。
- 不实现跨项目发布编排；组合发布后续在组织阶段考虑。
- 不实现物理删除退役系统；退役是归档和依赖清理。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| ReleaseCandidate | 可验证的候选构建：commit、镜像、基线、证据。 |
| Release | 被批准、可部署、可追溯的版本。 |
| ReleaseNote | 发布说明：变更、需求、风险、回滚。 |
| Environment | 部署环境：dev/staging/prod 或自定义。 |
| ConfigVersion | 环境配置的版本快照（非敏感）。 |
| Deployment | Release 在某个环境的部署记录。 |
| Rollout | 发布策略：一次性、灰度、金丝雀、蓝绿。 |
| HealthSignal | 观察期健康指标：错误率、延迟、可用性、业务指标。 |
| Incident | 事故：IssueKind::Incident，有独立响应流程。 |
| Postmortem | 复盘：时间线、根因、影响、改进项。 |
| Feedback | 用户/监控/事故反馈，进入需求池。 |
| ImprovementAction | 复盘或反馈产生的改进任务。 |
| RetirementPlan | 退役计划：依赖、数据、迁移、归档、批准。 |
| ErrorBudget | 错误预算：SLO 允许的失败额度。 |

不变量：

1. Release 必须引用一个 ReleaseCandidate 和至少一个基线。
2. ReleaseCandidate 必须绑定 commit、构建产物、镜像 digest 和证据。
3. Deployment 必须绑定 Release、Environment、ConfigVersion 和部署工具版本。
4. 部署完成不等于发布完成；必须有观察期和健康证据。
5. 回滚路径必须在发布前验证；回滚产生新的 Deployment 记录，不覆盖旧记录。
6. 事故必须有关闭条件和复盘；高风险事故没有复盘不能关闭。
7. 反馈必须能追溯到来源和处置；不能只存在聊天里。
8. 退役必须有依赖检查、数据保留/迁移、归档和批准。
9. 发布批准、不可逆部署、事故关闭必须由人类或显式授权策略完成。
10. 所有发布、部署、回滚、事故、复盘和退役写入事件日志。
11. 观察期健康证据缺失或不可复现时，发布不能标记完成。
12. 配置和密钥分离；密钥只引用 key id，不存值。

## 3. 发布模型

### 3.1 ReleaseCandidate

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "release_id": "uuid",
  "version": "1.4.1",
  "commit": "def456",
  "artifacts": [
    {"type": "container", "name": "refund-api", "digest": "sha256:..."},
    {"type": "binary", "name": "refund-worker", "hash": "sha256:..."}
  ],
  "baselines": ["requirements-v1.4", "architecture-v2"],
  "evidence": ["test-run-id", "scan-id", "sbom-id"],
  "status": "building|verifying|ready|rejected",
  "created_by": {"kind": "agent", "id": "..."},
  "created_at": "..."
}
```

规则：

- Candidate 必须可重复构建；构建环境、依赖锁和工具版本记录。
- 证据绑定 candidate 的 commit 和产物 digest；commit 变化后证据失效。
- Candidate 被拒绝后保留原因；修复产生新 candidate。

### 3.2 Release

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "version": "1.4.1",
  "candidate_id": "uuid",
  "state": "planned|candidate|verifying|approved|deploying|deployed|observing|completed|rolled_back|cancelled",
  "release_note": "note-id",
  "change_requests": ["cr-id"],
  "requirements_baseline": "baseline-id",
  "architecture_baseline": "baseline-id",
  "rollback_plan": "...",
  "approved_by": {"kind": "human", "id": "..."},
  "approved_at": "...",
  "created_at": "..."
}
```

规则：

- Release 只包含已批准的 ChangeRequest 和已通过门禁的 Candidate。
- Release 说明由系统汇总，但必须人类确认关键内容。
- Release 批准是部署的前提；紧急发布可以后补批准，但必须记录。
- Release 可以部署到多个环境；每个环境有独立 Deployment。
- 回滚产生新状态和 Deployment，不删除历史。

### 3.3 发布门禁

| 门禁 | 条件 | 阻塞 |
|---|---|---|
| release.candidate_ready | Candidate ready、构建可复现 | 是 |
| release.evidence | 独立测试、安全扫描、SBOM 齐全 | 是 |
| release.baseline | 需求/架构基线引用完整 | 是 |
| release.rollback | 回滚计划可执行并验证 | 是 |
| release.approval | 人类/策略批准，职责分离 | 是 |
| release.no_open_blockers | 无开放 blocker 缺陷/安全发现 | 是 |

## 4. 部署与环境

### 4.1 Environment

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "name": "prod",
  "type": "dev|staging|prod|custom",
  "owner": {"kind": "human", "id": "..."},
  "config_version": "config-id",
  "secret_refs": [{"key": "payments/db", "ref": "vault:..."}],
  "status": "active|maintenance|retired",
  "created_at": "..."
}
```

规则：

- 环境有独立配置版本和密钥引用；不能把 prod 配置复制到 staging 时带入敏感值。
- 环境变更走 ChangeRequest；高风险环境需要批准。
- 环境健康状态接入 HealthSignal；不可用环境不能部署。

### 4.2 ConfigVersion

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "environment_id": "uuid",
  "version": "2026-11-09.1",
  "content_ref": {"type": "git", "commit": "...", "path": "config/prod.yaml"},
  "hash": "sha256:...",
  "created_by": {"kind": "human", "id": "..."},
  "created_at": "..."
}
```

规则：

- 配置版本内容在 Git；敏感值只存密钥引用。
- 部署固定配置版本；配置变化产生新版本和 ChangeRequest。
- 配置漂移检测：实际配置与 ConfigVersion 不一致时告警。

### 4.3 Deployment

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "release_id": "uuid",
  "environment_id": "uuid",
  "config_version_id": "uuid",
  "strategy": "rolling|canary|blue_green|recreate",
  "status": "pending|running|succeeded|failed|rolled_back",
  "started_at": "...",
  "finished_at": "...",
  "tool": {"name": "...", "version": "..."},
  "migrations": ["migration-id"],
  "health_window": "PT30M",
  "health_signals": ["signal-id"],
  "rollback_of": "deployment-id|null",
  "executed_by": {"kind": "agent", "id": "..."}
}
```

规则：

- 部署记录不可变；失败和回滚保留。
- 数据迁移必须可回滚或有前向修复计划；迁移失败不能静默继续。
- 部署工具版本和配置版本记录在 Provenance 中。
- 部署完成后进入观察期；观察期通过才标记 Release `completed`。

### 4.4 Rollout 策略

| 策略 | 适用 | 观察 |
|---|---|---|
| rolling | 常规无状态服务 | 持续健康检查 |
| canary | 高风险变更 | 小流量对比 |
| blue_green | 需要快速回滚 | 双环境切换 |
| recreate | 不可并行 | 接受停机 |

规则：

- 策略由 Release 风险决定；高风险默认 canary/blue-green。
- 观察指标和阈值在发布前定义；不能事后挑指标。
- 自动回滚需要明确阈值；触发后记录事件和证据。

### 4.5 回滚

- 回滚计划在发布前验证：镜像/版本可回溯、数据库可回退、配置可恢复。
- 回滚是新的 Deployment，引用 `rollback_of`。
- 回滚后必须验证健康、更新事故/发布状态、产生复盘或改进项。
- 回滚不能删除故障版本的历史；保留用于分析。

## 5. 运维与事故

### 5.1 SLO / SLI

- 每个服务定义 SLI（可用性、延迟、错误率、饱和度）和 SLO。
- ErrorBudget 用于发布门禁：预算耗尽时限制高风险发布。
- 指标来自监控系统；Foundry 存引用和快照，不复制原始时序数据。

### 5.2 HealthSignal

```json
{
  "id": "uuid",
  "deployment_id": "uuid",
  "name": "http_5xx_rate",
  "value": 0.002,
  "threshold": 0.005,
  "window": "5m",
  "status": "healthy|degraded|critical",
  "source": "prometheus",
  "observed_at": "..."
}
```

规则：

- 观察指标和阈值在发布前定义并冻结。
- 健康信号异常触发告警、暂停 rollout 或自动回滚。
- 信号必须可追溯来源和查询；不可复现的信号降级。

### 5.3 Incident

Incident 是 `IssueKind::Incident`，扩展字段：

```json
{
  "issue_id": "uuid",
  "severity": "s1|s2|s3|s4",
  "commander": {"kind": "human", "id": "..."},
  "status": "detected|triaged|mitigating|mitigated|resolved|closed",
  "started_at": "...",
  "mitigated_at": "...",
  "resolved_at": "...",
  "affected_environments": ["prod"],
  "affected_releases": ["release-id"],
  "timeline": ["event-id"],
  "customer_impact": "..."
}
```

规则：

- 告警可以自动创建 Incident；去重和合并。
- Incident 有指挥官和明确状态；Agent 可以协助，但关闭需要人类。
- 时间线记录检测、分诊、止损、修复、恢复；事件只追加。
- 事故关联 Release、Deployment、ChangeRequest、Issue 和证据。
- 高危事故必须复盘；复盘有改进项和负责人。

### 5.4 Postmortem

```json
{
  "id": "uuid",
  "incident_id": "uuid",
  "summary": "...",
  "impact": "...",
  "root_causes": ["..."],
  "contributing_factors": ["..."],
  "timeline": ["..."],
  "what_went_well": ["..."],
  "what_went_wrong": ["..."],
  "actions": ["improvement-action-id"],
  "status": "draft|reviewed|accepted",
  "owners": ["..."],
  "created_at": "..."
}
```

规则：

- 复盘不追责个人，聚焦系统和流程。
- 根因必须区分直接原因、贡献因素和系统性原因。
- 改进项有负责人、截止时间和验证方式。
- 复盘接受后，改进项进入需求池或任务系统。

## 6. 反馈与改进

### 6.1 Feedback

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "source": "customer|support|monitoring|analytics|incident|internal",
  "content": "...",
  "classification": "bug|feature|question|praise|other",
  "linked_issue": "issue-id|null",
  "linked_requirement": "requirement-id|null",
  "status": "new|triaged|closed",
  "created_at": "..."
}
```

规则：

- 反馈进入需求池或 Issue，不直接变成 Requirement。
- 反馈保留来源、原始内容哈希和隐私标记。
- 反馈分类和优先级可以由辅助系统建议，人类确认。

### 6.2 改进闭环

```text
反馈/事故/指标
  → 分类和影响分析
  → Issue / Requirement / ImprovementAction
  → 优先级和计划
  → 实现、验证、发布
  → 指标观察
  → 下一轮反馈
```

规则：

- 改进项有负责人、截止时间和验证指标。
- 复盘改进项和产品需求一样进入基线和发布。
- 指标改善需要证据；不能只标记“已改进”。

### 6.3 实验

- 实验有假设、指标、样本、周期和停止条件。
- 实验变更走发布流程；不能绕过安全和权限。
- 实验结果进入 Decision 和需求池；失败实验也保留结论。
- 实验平台通过辅助系统接入。

## 7. 退役与归档

### 7.1 RetirementPlan

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "scope": ["service", "component", "data"],
  "reason": "...",
  "dependencies": [{"target": "...", "owner": "...", "status": "migrated|blocked"}],
  "data_retention": {"policy": "...", "duration": "...", "archive": "..."},
  "migration": "...",
  "communication": "...",
  "rollback": "...",
  "approved_by": {"kind": "human", "id": "..."},
  "status": "draft|approved|executing|completed|cancelled",
  "created_at": "..."
}
```

规则：

- 退役前必须做依赖检查：调用方、数据消费方、监控、合同、合规。
- 数据保留和迁移必须明确；不能直接删库。
- 退役执行有步骤、验证和回滚；关键步骤需要批准。
- 归档保留文档、架构决策、发布记录和事故；代码按保留策略归档。
- 退役完成后更新血缘、架构基线和项目状态。

### 7.2 退役门禁

| 门禁 | 条件 |
|---|---|
| retirement.dependencies | 所有依赖已迁移或有批准的计划 |
| retirement.data | 数据保留/迁移/删除策略批准 |
| retirement.communication | 干系人通知完成 |
| retirement.rollback | 回滚路径可用 |
| retirement.approval | 人类批准 |

## 8. 证据与门禁

| 证据 | 最低信任 | 说明 |
|---|---|---|
| 构建产物 | tool_verified | 可复现构建、digest |
| 测试报告 | tool_verified/independently_verified | 关键发布要求独立验证 |
| 安全扫描/SBOM | tool_verified | 工具版本和规则集记录 |
| 发布批准 | human_approved | 职责分离 |
| 部署记录 | tool_verified | 工具、配置、环境、迁移 |
| 健康指标 | tool_verified | 来源、窗口、阈值 |
| 事故时间线 | human_approved | 指挥官确认 |
| 复盘 | human_approved | 改进项和验证 |

规则：

- 发布门禁不能只接受自述证据。
- 部署记录必须绑定 Release/Candidate/ConfigVersion。
- 健康信号必须可追溯来源；不可复现则降级。
- 事故和复盘证据进入血缘，关联发布和变更。

## 9. 辅助系统

| 能力 | 输入 | 输出 | 信任 |
|---|---|---|---|
| `release.plan` | 变更、证据、风险 | 发布计划、风险、回滚计划 | self_reported |
| `release.notes` | 变更、需求、缺陷 | 发布说明草稿 | self_reported |
| `deploy.execute` | Release、Environment、Config | Deployment 记录 | tool_verified |
| `observe.health` | 环境、时间窗 | HealthSignal | tool_verified |
| `incident.analyze` | 事故、日志、变更 | 根因候选、时间线 | self_reported |
| `postmortem.generate` | 事故、时间线 | 复盘草稿 | self_reported |
| `feedback.classify` | 反馈 | 分类、影响、优先级建议 | self_reported |
| `experiment.analyze` | 实验数据 | 结论、显著性 | tool_verified |
| `retirement.check` | 系统、依赖、数据 | 退役检查报告 | tool_verified |

规则：

- 辅助输出是草稿或证据，不是批准。
- 发布说明、复盘和事故根因必须人类确认。
- 部署执行受策略约束；Agent 不能绕过发布门禁。
- 观察和实验数据必须记录来源、窗口和工具版本。

## 10. 数据模型

```sql
create table releases (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  version       text not null,
  candidate_id  uuid,
  state         text not null,
  release_note  jsonb,
  requirements_baseline uuid,
  architecture_baseline uuid,
  rollback_plan text,
  approved_by   jsonb,
  approved_at   timestamptz,
  created_at    timestamptz not null default now(),
  unique (project_id, version)
);

create table release_candidates (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  release_id    uuid not null references releases (id),
  commit_sha    text not null,
  artifacts     jsonb not null default '[]'::jsonb,
  baselines     jsonb not null default '[]'::jsonb,
  evidence      jsonb not null default '[]'::jsonb,
  status        text not null,
  created_by    jsonb not null,
  created_at    timestamptz not null default now()
);

create table environments (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  name          text not null,
  type          text not null,
  owner         jsonb not null,
  config_version uuid,
  secret_refs   jsonb not null default '[]'::jsonb,
  status        text not null,
  created_at    timestamptz not null default now(),
  unique (project_id, name)
);

create table config_versions (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  environment_id uuid not null references environments (id),
  version       text not null,
  content_ref   jsonb not null,
  hash          text not null,
  created_by    jsonb not null,
  created_at    timestamptz not null default now()
);

create table deployments (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  release_id    uuid not null references releases (id),
  environment_id uuid not null references environments (id),
  config_version_id uuid not null references config_versions (id),
  strategy      text not null,
  status        text not null,
  started_at    timestamptz not null,
  finished_at   timestamptz,
  tool          jsonb not null,
  migrations    jsonb not null default '[]'::jsonb,
  health_window text,
  rollback_of   uuid references deployments (id),
  executed_by   jsonb not null
);

create table health_signals (
  id            uuid primary key,
  deployment_id uuid not null references deployments (id),
  name          text not null,
  value         numeric,
  threshold     numeric,
  window        text not null,
  status        text not null,
  source        text not null,
  observed_at   timestamptz not null
);

create table postmortems (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  incident_id   uuid not null references issues (id),
  summary       text not null,
  impact        text not null,
  root_causes   jsonb not null default '[]'::jsonb,
  contributing_factors jsonb not null default '[]'::jsonb,
  timeline      jsonb not null default '[]'::jsonb,
  actions       jsonb not null default '[]'::jsonb,
  status        text not null,
  owners        jsonb not null default '[]'::jsonb,
  created_at    timestamptz not null default now()
);

create table feedback (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  source        text not null,
  content       text not null,
  classification text,
  linked_issue  uuid references issues (id),
  linked_requirement uuid,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table improvement_actions (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  source        text not null,
  title         text not null,
  owner         jsonb not null,
  due_at        timestamptz,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table retirement_plans (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  scope         jsonb not null,
  reason        text not null,
  dependencies  jsonb not null default '[]'::jsonb,
  data_retention jsonb not null,
  migration     text,
  rollback      text,
  approved_by   jsonb,
  status        text not null,
  created_at    timestamptz not null default now()
);
```

## 11. API、MCP、CLI、Web

### 11.1 REST

```text
GET/POST /api/v1/projects/{project}/releases
GET      /api/v1/releases/{id}
POST     /api/v1/releases/{id}/candidates
POST     /api/v1/releases/{id}/approve
GET/POST /api/v1/projects/{project}/environments
GET/POST /api/v1/projects/{project}/config-versions
POST     /api/v1/releases/{id}/deployments
GET      /api/v1/deployments/{id}
POST     /api/v1/deployments/{id}/rollback
GET      /api/v1/deployments/{id}/health
GET/POST /api/v1/projects/{project}/incidents
POST     /api/v1/incidents/{id}/timeline
GET/POST /api/v1/incidents/{id}/postmortem
GET/POST /api/v1/projects/{project}/feedback
GET/POST /api/v1/projects/{project}/improvement-actions
GET/POST /api/v1/projects/{project}/retirement-plans
```

### 11.2 MCP

- `release.list` / `release.get` / `release.candidate.create`
- `release.evidence`
- `deployment.create` / `deployment.get` / `deployment.rollback`
- `health.query`
- `incident.create` / `incident.timeline` / `incident.close`
- `postmortem.submit`
- `feedback.submit`
- `improvement.list`
- `retirement.check`

Agent 可以准备发布、执行部署、观察、起草复盘；不能批准发布、不能关闭事故、不能批准退役。

### 11.3 CLI

```bash
foundry release list <project>
foundry release candidate <release> --commit <sha>
foundry release approve <release>
foundry deploy <release> --env prod --strategy canary
foundry deployment health <deployment>
foundry deploy rollback <deployment>
foundry incident create --severity s2 --title "..."
foundry incident timeline <incident>
foundry postmortem submit <incident> --file postmortem.md
foundry retirement plan <project>
```

### 11.4 Web

- 发布页：候选、证据、批准、发布说明、回滚计划。
- 部署页：环境、配置、进度、健康信号、回滚。
- 运维页：SLO、错误预算、告警、事故、时间线。
- 复盘页：根因、贡献因素、改进项、验证。
- 反馈页：来源、分类、关联 Issue/Requirement。
- 退役页：依赖、数据、迁移、归档、批准。

## 12. 权限与安全

- 发布批准、不可逆部署、事故关闭和退役批准需要人类或显式授权策略。
- Agent 可以准备、执行、观察、起草；不能自我批准。
- 环境访问按角色和项目隔离；prod 权限更高。
- 密钥只引用 key id；部署工具从密钥管理系统取值，不进入 Foundry。
- 生产数据访问需要批准和脱敏；调试数据有期限。
- 发布说明和事故时间线可能包含敏感信息；按项目权限过滤。
- 所有发布、部署、回滚、事故、复盘和退役写入审计日志。

## 13. 与现有实现的关系

- 现有 `release` 内置流程是协调壳；本 RFC 增加 Release/ReleaseCandidate/Deployment 对象和门禁。
- 现有 `IssueKind::Incident` 继续使用；本 RFC 增加事故扩展字段、时间线和复盘。
- 现有 `Issue` 的 `source_kind` 支持 monitoring/incident/analytics；反馈接入复用 Issue 和 Feedback。
- 现有 `events` 记录发布/部署/事故事件；新增事件类型。
- 现有部署/监控适配器为空；本 RFC 需要 K8s/云平台/监控适配器。
- 现有 `project_review` 流程与复盘衔接；复盘改进项进入需求池。

## 14. 实施计划

### 阶段 0：发布对象与部署记录

- Release/Candidate/Environment/Deployment 表、API、基本门禁。
- 手工记录部署和健康信号。
- 出口标准：发布可追溯、部署可查询、回滚记录完整。

### 阶段 1：部署适配与观察

- K8s/云平台适配器、配置版本、健康信号接入。
- 观察窗口、自动回滚阈值。
- 出口标准：部署可执行、可观察、可回滚。

### 阶段 2：事故与复盘

- 告警接入、Incident 创建/去重、时间线、复盘、改进项。
- 出口标准：事故从告警到复盘闭环；改进项可追踪。

### 阶段 3：反馈与实验

- 反馈接入和分类、实验平台、改进闭环。
- 出口标准：生产反馈能进入需求池并影响下一轮。

### 阶段 4：退役与指标

- 退役计划、依赖检查、数据迁移、归档。
- 发布/事故指标（变更失败率、MTTR、回滚率、错误预算）。
- 出口标准：退役可审计；发布健康可度量。

## 15. 验收标准

1. Release 引用 Candidate 和基线；部署引用 Release、Environment、ConfigVersion。
2. 发布门禁要求独立测试、安全扫描、SBOM、回滚计划和批准。
3. 部署完成进入观察期；健康证据缺失时发布不能完成。
4. 回滚是新的 Deployment 记录；历史保留。
5. 事故从告警创建、去重、时间线、复盘和改进项完整闭环。
6. 高危事故没有复盘不能关闭；改进项有负责人和验证。
7. 反馈能进入 Issue/Requirement 并追溯到来源。
8. 退役有依赖检查、数据保留/迁移、归档和批准。
9. Agent 不能批准发布、关闭事故或批准退役。
10. 发布/部署/事故/复盘事件可重放。

## 16. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 发布证据不足 | CI 绿了就发布 | 独立测试、安全、SBOM、批准 |
| 观察期形式化 | 部署完就标记完成 | 预定义指标和窗口；自动判定 |
| 回滚不可用 | 计划未验证 | 发布前演练；回滚证据 |
| 告警噪音 | 事故泛滥 | 去重、分级、抑制 |
| 复盘流于形式 | 改进项无人跟进 | 负责人、截止、验证指标 |
| 反馈丢失 | 反馈停在聊天 | 统一入口、分类、关联 |
| 退役遗漏 | 依赖和数据未处理 | 依赖检查、门禁、归档 |
| 成本 | 环境和监控成本 | 环境生命周期、配额、按需 |
| 权限过大 | Agent 可部署 prod | 最小权限、批准、审计 |

**待决问题**：

1. 低风险发布是否允许自动批准？建议允许，但需证据和策略。
2. 观察期长度默认多少？建议按风险 15–60 分钟，关键服务更长。
3. 事故关闭是否需要复盘先行？建议高危必须，低危可简化。
4. 退役数据保留策略由谁定？建议合规/Owner 共同批准。
5. 是否接入现有 SLO 平台？建议接入，不在 Foundry 重造。
