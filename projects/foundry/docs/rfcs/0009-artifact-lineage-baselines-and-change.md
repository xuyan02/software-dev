# RFC 0009：产物血缘、基线与变更管理

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0010 辅助系统注册与产物契约](0010-auxiliary-systems-and-evidence.md)、[ADR 0003 Git 存内容，Postgres 存状态](../adr/0003-git-content-postgres-state.md)、[ADR 0004 Issue 是问题空间，Requirement 是承诺空间](../adr/0004-issue-vs-requirement.md)
- 作用：本文定义生命周期中所有对象共享的产物模型、血缘图、基线和变更管理。需求、架构、测试、发布等 RFC 引用本文，不各自另造一套版本和追溯机制。

## 0. 结论

Foundry 的贯穿主线不是“阶段”，而是**产物及其血缘**。阶段只是产物产生和消费的位置；没有血缘，阶段之间就无法做影响分析、变更控制和证据审计。

核心决定：

1. **Artifact 是生命周期的连接件。** 需求、架构决策、契约、设计、任务、补丁、测试报告、发布记录都是 Artifact；每个 Artifact 有类型、所有者、状态和至少一个不可变版本。
2. **内容与元数据分离。** 长文本、代码和文档内容存 Git；大证据（报告、截图、trace、覆盖率包、SBOM）存 S3 兼容对象存储；Postgres 只存元数据、状态、索引和内容引用。引用使用 SHA-256 或 `git commit + path`，保证内容寻址。
3. **版本不可变。** ArtifactVersion 一旦创建不可修改；变更产生新版本。已批准或已进入基线的版本不能原地编辑。
4. **血缘边是有类型的、可查询的。** `derives_from`、`satisfies`、`verifies`、`implements`、`refines`、`depends_on`、`supersedes`、`deploys`、`causes`、`mitigates` 等关系决定影响分析的方向和强度。
5. **影响分析由血缘图驱动，不靠人工维护清单。** 改一个需求，系统能沿图找出受影响的架构决策、契约、任务、测试、发布和部署；并标记需要重新验证的证据。
6. **基线是某一时刻被批准的产物版本集合。** 需求基线、架构基线、发布基线都用同一套模型；基线批准后不可变，变更产生新基线。
7. **变更必须走 ChangeRequest。** 已批准产物的修改、范围变化、配置变更、紧急修复都要有 ChangeRequest；ChangeRequest 自带影响分析、验证计划、回滚计划和批准。
8. **配置和版本是可复现性的一部分。** 代码、配置、环境、数据 schema、模型、提示词、工具版本通过 `config_refs` 记录；关键证据缺少版本清单时降低信任等级。
9. **外部事实只引用不复制。** Commit、PR、CI run、镜像 digest、监控指标存引用和哈希；内容留在外部系统，避免第二真相。
10. **血缘图是投影，不是编辑入口。** 手工可以补充和修正边，但领域对象和事件仍是真相；图查询结果不能直接修改对象。
11. **保留与审计。** Artifact、版本、血缘、基线、变更和批准按项目保留；删除只允许归档，不允许物理删除已批准版本（合规场景按保留策略处理）。

## 1. 目标与非目标

### 1.1 目标

- 让每个生命周期产物可版本化、可追溯、可复现。
- 让需求变更、架构偏离、缺陷修复、配置变更都能做影响分析。
- 让基线成为发布、审计和合规的依据。
- 让 ChangeRequest 成为受控变更的唯一入口，而不是散落在评论和聊天里。
- 让外部执行面的事实以引用方式进入血缘，不复制内容。
- 让 Agent 和人类使用同一套产物、版本和变更语义。
- 让“这个发布包含哪些需求和证据”“这个改动影响哪些测试”成为一次查询，而不是人工盘点。

### 1.2 非目标

- 不实现通用文档管理系统；Git 仍是长文本内容的真相。
- 不实现通用图数据库；血缘用关系表 + 递归查询实现，规模扩大后再评估专用存储。
- 不实现自动代码合并、CRDT 或实时协同。
- 不实现物理删除和版本回滚 API；回滚通过新版本和新基线表达。
- 不把血缘图当作领域对象状态的替代；图是查询视图。
- 不自动推断所有血缘；推断边必须标记来源和置信度，并允许人工修正。
- 不实现跨项目血缘；跨项目默认不可见。
- 不实现通用配置中心；配置引用指向外部系统或 Git，Foundry 只记录引用和版本。
- 不要求所有 Artifact 都走同一张宽表；类型化对象可以有类型化表，Artifact registry 只存共享元数据。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| Artifact | 生命周期中可版本化的产物；有 id、类型、所有者、状态和版本序列。 |
| ArtifactVersion | Artifact 的一次不可变快照；内容寻址。 |
| ContentRef | 内容引用：Git commit + path，或对象存储 key + SHA-256。 |
| LineageEdge | 两个 Artifact（或版本）之间的有类型关系。 |
| Upstream / Downstream | 沿血缘方向的上游来源和下游派生。 |
| Baseline | 某一时刻被批准的一组 ArtifactVersion。 |
| BaselineItem | 基线中的一项：Artifact + 版本 + 内容哈希。 |
| ChangeRequest | 对基线或已批准产物的受控变更提案。 |
| ImpactAnalysis | 沿血缘图计算的受影响对象、风险和验证需求。 |
| ConfigRef | 代码、配置、环境、数据、模型、提示词、工具等版本引用。 |
| Provenance | 产物或证据的生成/验证版本清单：工具、模型、提示词、配置、数据。 |
| Stale | 上游变化导致下游产物或证据需要重新验证的状态。 |

不变量：

1. 每个 Artifact 至少有一个 ArtifactVersion；版本号在 Artifact 内单调递增，从 1 开始。
2. ArtifactVersion 不可变；内容哈希与 ContentRef 必须一致。
3. 已批准版本、已进入基线的版本、已发布版本不能原地修改。
4. 每条血缘边必须有 `source`、`target`、`relation`、`origin`（manual/derived/inferred）和 `created_at`。
5. 血缘边不能跨项目；跨项目引用在写入时拒绝。
6. 影响分析结果必须带路径和关系；不能只返回一个无解释的列表。
7. Baseline 批准后不可变；变更产生新 Baseline 或新 ChangeRequest。
8. ChangeRequest 必须有目标产物/基线、原因、影响分析、验证计划和批准；紧急变更可以后补批准，但必须记录。
9. 配置引用缺失或不可解析时，相关证据的信任等级降级，并在门禁中显示。
10. 外部事实（commit、CI run、部署）只存引用；引用失效时标记 Stale，不静默使用。
11. 删除 Artifact 只允许归档；已批准版本和基线项保留。
12. 所有变更、批准、豁免和影响分析写入事件日志。

## 3. Artifact 模型

### 3.1 字段

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "kind": "requirement",
  "title": "支持部分退款",
  "owner_kind": "human",
  "owner_id": "account-uuid",
  "state": "draft|active|approved|superseded|archived",
  "current_version": 3,
  "sensitivity": "internal",
  "tags": ["payments"],
  "created_at": "...",
  "updated_at": "..."
}
```

```json
{
  "artifact_id": "uuid",
  "version": 3,
  "content_ref": {
    "type": "git",
    "repo": "payments",
    "commit": "abc123",
    "path": "docs/requirements/partial-refund.md"
  },
  "content_hash": "sha256:...",
  "message": "增加部分退款场景",
  "based_on_version": 2,
  "created_by": {"kind": "human", "id": "..."},
  "created_at": "...",
  "provenance": {
    "tool_invocations": ["..."],
    "model_configs": ["..."],
    "prompt_versions": ["..."],
    "config_refs": ["..."]
  }
}
```

### 3.2 类型

| 组 | 类型 |
|---|---|
| 产品与立项 | `vision`、`portfolio_decision`、`project_brief`、`success_metric`、`risk` |
| 需求 | `issue`、`requirement`、`scenario`、`holdout`、`requirement_baseline`、`change_request` |
| 架构与设计 | `architecture_baseline`、`adr`、`design_doc`、`contract`、`quality_attribute`、`constraint`、`component_model`、`tech_debt_item`、`fitness_function` |
| 实现 | `task`、`changeset`、`patch`、`commit_ref`、`pull_request_ref` |
| 评审与测试 | `review`、`review_finding`、`test_plan`、`test_case`、`test_run`、`test_report`、`coverage_report`、`holdout_result` |
| 质量与安全 | `quality_gate_result`、`scan_report`、`sbom`、`provenance_attestation`、`waiver` |
| 发布 | `release`、`release_candidate`、`release_note`、`deployment`、`config_version`、`health_signal`、`rollback_record` |
| 运维与反馈 | `incident`、`postmortem`、`feedback`、`improvement_action`、`experiment` |
| 退役 | `retirement_plan`、`migration_record`、`archive_record` |
| Agent | `agent_definition`、`evaluation_suite`、`evaluation_run`、`trajectory` |
| 辅助 | `analysis_report`、`option_set`、`simulation_result`、`benchmark_result` |

类型不是封闭枚举；新增类型必须归入某个组，并声明默认状态机、所有者、证据要求和可见性。

### 3.3 状态

| 状态 | 含义 | 允许操作 |
|---|---|---|
| `draft` | 可编辑草稿 | 编辑、提交评审、删除（未批准） |
| `active` | 已生效、可被引用 | 新版本、变更请求、归档 |
| `approved` | 已批准、可进入基线 | 新版本（经 ChangeRequest）、归档 |
| `superseded` | 被新版本或新对象取代 | 读取、追溯 |
| `archived` | 归档，只读 | 读取、审计 |

状态转换由领域对象自己的状态机决定；Artifact registry 只保存共享状态，不替代领域状态机。

### 3.4 类型化对象与 Artifact 的关系

- Requirement、ADR、DesignDoc、Contract、TestPlan、Release、AgentDefinition 等可以有类型化表，保存可查询字段和状态机。
- Artifact registry 保存它们的共享身份、版本、内容引用和血缘；类型化表通过 `artifact_id` 关联。
- 不允许两份内容：类型化表存结构和状态，内容通过 ContentRef 指向 Git/S3；JSONB 快照只用于重建，不作为第二真相。
- 一次性结果（TestRun、Deployment、ScanResult）作为 Record，可以只有 Artifact 元数据和内容引用，不需要类型化聚合。

## 4. 血缘图

### 4.1 边模型

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "source_artifact_id": "uuid",
  "source_version": 3,
  "target_artifact_id": "uuid",
  "target_version": 1,
  "relation": "derives_from",
  "origin": "manual|derived|inferred",
  "confidence": 1.0,
  "note": "由需求分析辅助系统建议，人工确认",
  "created_by": {"kind": "agent", "id": "..."},
  "created_at": "..."
}
```

方向约定：`source` 是上游，`target` 是下游。例如 Requirement `satisfies` ProjectBrief、Task `implements` Requirement、TestRun `verifies` Requirement。

### 4.2 关系类型

| 关系 | 方向 | 含义 |
|---|---|---|
| `derives_from` | 下游 → 上游 | 由某产物派生 |
| `satisfies` | 下游 → 上游 | 满足某个目标/需求 |
| `refines` | 下游 → 上游 | 细化上游 |
| `implements` | 下游 → 上游 | 实现某个设计/需求 |
| `verifies` | 下游 → 上游 | 验证某个需求/设计 |
| `depends_on` | 下游 → 上游 | 依赖某个产物或组件 |
| `conflicts_with` | 双向 | 与某产物冲突 |
| `supersedes` | 新 → 旧 | 取代旧版本/旧对象 |
| `documents` | 下游 → 上游 | 记录某个对象 |
| `deploys` | 下游 → 上游 | 部署某个发布 |
| `causes` | 下游 → 上游 | 导致事故/缺陷 |
| `mitigates` | 下游 → 上游 | 缓解事故/风险 |
| `evidenced_by` | 对象 → 证据 | 由某证据支撑 |

### 4.3 边的来源

| 来源 | 说明 | 信任 |
|---|---|---|
| `manual` | 人或 Agent 显式建立 | 高 |
| `derived` | 从对象字段或结构推导（如 Task.requirement_id） | 中高 |
| `inferred` | 辅助系统基于语义建议 | 低，需确认 |

规则：

- `inferred` 边不能单独满足门禁，必须人工确认或由高置信度任务确认。
- 边的创建、删除和修正都写入事件日志。
- 边可以指向具体版本；版本为空表示“当前版本”，查询时解析为最新版本并标记。
- 删除对象时边归档，不物理删除。

### 4.4 查询

```text
upstream(artifact, relations?, depth?)
downstream(artifact, relations?, depth?)
paths(from, to, max_depth?)
impact(artifact_version, change_type)
coverage(project, requirement)
```

- 默认深度 5，最大 20；超过限制返回截断标记。
- 默认排除 `inferred` 边，除非显式请求。
- 结果按关系类型和 Artifact 类型分组，带路径和版本。
- 跨项目边不存在；查询不能越界。

### 4.5 影响分析

```text
输入：ArtifactVersion + 变更类型（modify/delete/supersede/config）
1. 找到所有下游边（implements/verifies/depends_on/satisfies/refines/deploys）
2. 按关系强度排序：implements > verifies > depends_on > satisfies > refines
3. 收集受影响的 Artifact、当前状态、负责人
4. 标记受影响的 Evidence 为 stale
5. 识别受影响的 Baseline、Release、Deployment
6. 输出：对象列表、路径、影响理由、建议验证范围、风险等级
```

影响分析必须可解释：每条影响带路径（例如 Requirement → Contract → Task → TestPlan）和关系。不得只返回“受影响 17 项”。

### 4.6 失效传播

- 上游 ArtifactVersion 变化时，沿 `implements`、`verifies`、`depends_on` 向下游标记 `stale`。
- `stale` 的证据不能用于门禁，直到重新验证。
- 重新验证产生新的 Evidence，不修改旧 Evidence；旧 Evidence 标记 `superseded`。
- 代码变更、依赖更新、环境变化、配置变化都可以触发失效传播。

## 5. 基线

### 5.1 模型

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "name": "requirements-v1.0",
  "scope": "requirements|architecture|release|project",
  "state": "draft|approved|superseded",
  "description": "...",
  "created_by": {"kind": "human", "id": "..."},
  "approved_by": {"kind": "human", "id": "..."},
  "created_at": "...",
  "approved_at": "..."
}
```

BaselineItem：

```json
{
  "baseline_id": "uuid",
  "artifact_id": "uuid",
  "artifact_version": 3,
  "content_hash": "sha256:...",
  "note": "..."
}
```

### 5.2 规则

1. 基线在 `draft` 时可以增删项；`approved` 后不可变。
2. 基线只能包含同项目、已批准或 active 的 ArtifactVersion。
3. 基线必须声明 scope；跨 scope 的产物不能混入。
4. 基线 diff 按 Artifact 类型分组：新增、删除、版本变化、内容变化。
5. 发布必须引用一个 release 基线；需求变更必须与需求基线比较。
6. 新基线不修改旧基线；旧基线保留用于追溯。
7. 基线批准是门禁的一部分；批准人和创建人不能是同一人（高风险场景）。
8. 基线项可以引用外部事实（commit、镜像 digest），但仍需内容哈希。

### 5.3 基线 diff

```text
baseline.diff(base, target):
  added: target 中有、base 中没有
  removed: base 中有、target 中没有
  changed: 同一 Artifact 版本不同或内容哈希不同
  unchanged: 版本和哈希都相同
  按 kind 分组；每组带负责人和影响提示
```

## 6. ChangeRequest

### 6.1 字段

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "title": "需求变更：支持部分退款",
  "kind": "requirement|architecture|design|config|emergency|retirement",
  "state": "draft|impact_analysis|review|approved|implementing|verifying|closed|rejected|cancelled",
  "reason": "...",
  "requester": {"kind": "human", "id": "..."},
  "approver": {"kind": "human", "id": "..."},
  "target_baseline_id": "uuid|null",
  "target_artifacts": [{"artifact_id": "uuid", "from_version": 2, "to_version": null}],
  "impact_summary": "...",
  "validation_plan": "...",
  "rollback_plan": "...",
  "deadline": "...",
  "created_at": "...",
  "approved_at": "...",
  "closed_at": "..."
}
```

### 6.2 生命周期

```text
draft
  → impact_analysis（生成影响分析）
  → review（评审影响、验证计划、回滚计划）
  → approved（批准）
  → implementing（关联 Task/ChangeSet）
  → verifying（验证证据）
  → closed（更新基线、关闭）
  ↘ rejected / cancelled
```

紧急变更：

- 允许从 `draft` 直接到 `implementing`，标记 `emergency=true`。
- 必须在 24 小时内补齐影响分析和批准，否则自动升级并阻塞后续阶段。
- 紧急变更不能跳过安全门禁；可以并行验证，但不能无证据关闭。

### 6.3 影响分析

- 调用 RFC 0009 §4.5 的影响分析。
- 必须列出：受影响需求、架构/契约、任务、测试、发布/部署、证据失效项。
- 必须给出：风险等级、建议验证范围、预计工作量、回滚影响。
- 影响分析结果写入 `change_impacts`，带血缘路径和来源。
- 影响分析可以由辅助系统生成，但必须标记来源和置信度；批准前需要人工确认关键项。

### 6.4 批准与职责分离

- 创建人不能批准自己的 ChangeRequest。
- 高风险变更（架构、发布、安全、数据）需要至少两名批准人，或指定角色批准。
- Agent 可以发起、分析和实现 ChangeRequest；不能批准，除非策略显式授权且留痕。
- 变更关闭前必须：更新受影响 Artifact 的新版本、更新基线、产生验证证据、清理或豁免 stale 证据。

### 6.5 与工作项的关系

- ChangeRequest 可以创建 Task/ChangeSet；Task 必须引用 ChangeRequest。
- ChangeRequest 关闭不依赖所有 Task 完成，但必须依赖验证证据和基线更新。
- 发布可以包含多个 ChangeRequest；Release 基线引用它们产生的产物版本。

## 7. 配置与版本管理

### 7.1 ConfigRef

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "kind": "code|config|environment|data_schema|model|prompt|tool|dependency",
  "ref": "repo@commit|config-key|schema-version|model-id|prompt-version|tool-version",
  "hash": "sha256:...|null",
  "scope": "project|environment|task",
  "artifact_id": "uuid|null",
  "created_at": "..."
}
```

### 7.2 规则

1. 关键证据必须带 Provenance：使用的工具、模型、提示词、配置、数据版本。
2. 代码版本用 `repo + commit`；配置版本用配置项 + 哈希；环境版本用环境 id + 配置快照；数据 schema 用迁移版本；模型和提示词用版本 id。
3. 引用不可解析时，证据信任等级降级；门禁显示“不可复现”。
4. 配置变更走 ChangeRequest（`kind=config`），并触发影响分析。
5. 发布固定配置版本；部署记录引用配置版本。
6. 环境配置和密钥分离：配置版本记录非敏感配置；密钥只引用密钥管理系统的 key id，不存值。

### 7.3 可复现性

```text
provenance = {
  code: [repo@commit],
  config: [config_version_id],
  environment: [env_id + snapshot_hash],
  data: [schema_version, fixture_version],
  model: [model_config_id],
  prompt: [prompt_version_id],
  tool: [tool_definition_id],
  auxiliary: [tool_invocation_id]
}
```

- 可复现性评分：全部可解析 = `reproducible`；部分缺失 = `partial`；关键缺失 = `not_reproducible`。
- `not_reproducible` 的证据不能用于不可逆发布门禁。

## 8. 门禁集成

- 门禁评估读取 Artifact、Baseline、Evidence、Provenance。
- 门禁结果记录：`gate_id`、`stage`、`artifact_versions`、`evidence_ids`、`policy_decision`、`waiver_id`、`result`。
- 基线批准、ChangeRequest 批准、Waiver 批准都是门禁的输入。
- Stale 证据自动阻塞相关门禁，直到重新验证。
- 门禁失败必须返回可定位的原因：缺哪个产物、缺哪个证据、哪个信任等级不够、哪个策略拒绝。

## 9. 数据模型

```sql
-- 共享产物注册表
create table artifacts (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  kind          text not null,
  title         text not null,
  owner_kind    text not null,
  owner_id      text not null,
  state         text not null,
  current_version integer not null default 1,
  sensitivity   text not null default 'internal',
  tags          jsonb not null default '[]'::jsonb,
  created_at    timestamptz not null default now(),
  updated_at    timestamptz not null default now()
);

create table artifact_versions (
  artifact_id   uuid not null references artifacts (id),
  version       integer not null,
  content_ref   jsonb not null,
  content_hash  text not null,
  message       text,
  based_on_version integer,
  provenance    jsonb not null default '{}'::jsonb,
  created_by    jsonb not null,
  created_at    timestamptz not null default now(),
  primary key (artifact_id, version)
);

create table artifact_edges (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  source_artifact_id uuid not null references artifacts (id),
  source_version integer,
  target_artifact_id uuid not null references artifacts (id),
  target_version integer,
  relation      text not null,
  origin        text not null,
  confidence    numeric(3,2) not null default 1.0,
  note          text,
  created_by    jsonb not null,
  created_at    timestamptz not null default now(),
  unique (source_artifact_id, source_version, target_artifact_id, target_version, relation)
);

create table baselines (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  name          text not null,
  scope         text not null,
  state         text not null,
  description   text,
  created_by    jsonb not null,
  approved_by   jsonb,
  created_at    timestamptz not null default now(),
  approved_at   timestamptz,
  unique (project_id, name)
);

create table baseline_items (
  baseline_id   uuid not null references baselines (id),
  artifact_id   uuid not null references artifacts (id),
  artifact_version integer not null,
  content_hash  text not null,
  note          text,
  primary key (baseline_id, artifact_id)
);

create table change_requests (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  title         text not null,
  kind          text not null,
  state         text not null,
  reason        text not null,
  requester     jsonb not null,
  approver      jsonb,
  target_baseline_id uuid references baselines (id),
  impact_summary text,
  validation_plan text,
  rollback_plan text,
  emergency     boolean not null default false,
  deadline      timestamptz,
  created_at    timestamptz not null default now(),
  approved_at   timestamptz,
  closed_at     timestamptz
);

create table change_impacts (
  change_request_id uuid not null references change_requests (id),
  artifact_id   uuid not null references artifacts (id),
  artifact_version integer,
  relation_path text not null,
  impact        text not null,
  risk          text not null,
  origin        text not null,
  primary key (change_request_id, artifact_id, relation_path)
);

create table config_refs (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  kind          text not null,
  ref           text not null,
  hash          text,
  scope         text not null,
  artifact_id   uuid references artifacts (id),
  created_at    timestamptz not null default now()
);

create index artifacts_project_kind_idx on artifacts (project_id, kind, state);
create index artifact_edges_source_idx on artifact_edges (source_artifact_id);
create index artifact_edges_target_idx on artifact_edges (target_artifact_id);
create index change_requests_project_state_idx on change_requests (project_id, state);
```

递归查询使用 `WITH RECURSIVE`，深度上限 20；大项目可以加物化视图或缓存，但视图不是真相。

## 10. API、MCP、CLI、Web

### 10.1 REST

```text
GET    /api/v1/projects/{project}/artifacts?kind=&state=&q=
POST   /api/v1/projects/{project}/artifacts
GET    /api/v1/artifacts/{id}
GET    /api/v1/artifacts/{id}/versions
POST   /api/v1/artifacts/{id}/versions
GET    /api/v1/artifacts/{id}/lineage?direction=upstream|downstream&relations=&depth=
POST   /api/v1/artifacts/{id}/edges
DELETE /api/v1/artifacts/{id}/edges/{edge}
GET    /api/v1/artifacts/{id}/impact?version=
GET    /api/v1/projects/{project}/baselines
POST   /api/v1/projects/{project}/baselines
POST   /api/v1/baselines/{id}/approve
GET    /api/v1/baselines/{id}/diff?target=
GET    /api/v1/projects/{project}/change-requests
POST   /api/v1/projects/{project}/change-requests
POST   /api/v1/change-requests/{id}/analyze
POST   /api/v1/change-requests/{id}/approve
POST   /api/v1/change-requests/{id}/close
```

### 10.2 MCP

- `artifact.get` / `artifact.list` / `artifact.submit`
- `artifact.lineage` / `artifact.impact`
- `baseline.create` / `baseline.get` / `baseline.diff`
- `change.request` / `change.analyze` / `change.status`
- `config.ref` / `provenance.get`

MCP 调用必须带项目和角色上下文；不能跨项目查询；Agent 不能批准 ChangeRequest。

### 10.3 CLI

```bash
foundry artifact list <project> --kind requirement
foundry artifact lineage <artifact> --direction downstream
foundry artifact impact <artifact> --version 3
foundry baseline create <project> --name requirements-v1 --scope requirements
foundry baseline diff <baseline> <target>
foundry change request <artifact> --kind requirement --reason "..."
foundry change analyze <change-request>
foundry change approve <change-request>
```

### 10.4 Web

- 产物页：类型、版本、内容预览、所有者、状态、Provenance。
- 血缘页：上下游图、路径、影响分析、Stale 标记。
- 基线页：项列表、diff、批准、发布引用。
- 变更页：影响、验证计划、回滚计划、批准、关联任务、验证证据。

## 11. 权限与安全

- 读取产物遵循项目隔离和角色；Viewer 只读，Member 可提交草稿，Maintainer 可批准非高风险变更，Owner/Admin 可批准高风险变更。
- Agent 可以提交 ArtifactVersion、建立 `derived`/`inferred` 边、发起 ChangeRequest；不能批准 ChangeRequest、不能批准基线、不能删除已批准版本。
- 内容引用必须校验哈希；ContentRef 指向不可达内容时标记无效，不能作为证据。
- 敏感产物（安全、密钥引用、合规）设置 `sensitivity`，限制可见性和导出。
- 血缘和影响分析结果可能泄露架构信息；按项目权限过滤。
- 所有写操作幂等，带 `client_save_id` 或 `Idempotency-Key`；并发更新使用版本号或 ETag。

## 12. 与现有实现的关系

- 现有 `links` 表保留为兼容视图；新的 `artifact_edges` 成为血缘真相。迁移时把 `links` 转换为边，保留原 id 和备注。
- 现有 `documents` 表（path、git_revision、status、version）成为 Artifact 的一种类型化投影；文档内容仍在 Git。
- 现有 `work_items` 和 `tasks` 通过 `artifact_id` 关联；不复制正文到 Artifact。
- Issue 通过 `derives_from` 或 `satisfies` 连接到 Requirement；Requirement 通过 `implements`/`verifies` 连接到架构、任务和测试。
- 现有事件日志继续使用；新增 `artifact.created`、`artifact.versioned`、`edge.created`、`baseline.approved`、`change.approved` 等事件类型。
- 旧数据迁移：先建 Artifact 记录并回填版本；边从现有外键和 links 推导，标记 `origin=derived`；人工确认后再提升信任。

## 13. 实施计划

### 阶段 0：模型与迁移

- 建表、迁移脚本、事件类型。
- 把 Project/Issue/Requirement/Task/Document 映射为 Artifact 和初始版本。
- `links` 转换为 `artifact_edges` 的兼容迁移。
- 出口标准：任意现有对象可查询版本和基本血缘。

### 阶段 1：版本与基线

- ArtifactVersion 写入、内容寻址、Provenance。
- 基线创建、批准、diff。
- 出口标准：需求基线和发布基线可用；diff 可解释。

### 阶段 2：变更与影响

- ChangeRequest 状态机、影响分析、批准、关联任务、验证、关闭。
- Stale 传播和门禁集成。
- 出口标准：需求变更能影响分析到任务/测试/发布，并能阻塞过期证据。

### 阶段 3：辅助系统与推断边

- 辅助系统建议边（`inferred`）、人工确认、置信度。
- 基于语义检索的血缘补全。
- 出口标准：推断边可解释、可确认、不能单独满足门禁。

### 阶段 4：规模与运维

- 递归查询性能、物化视图、缓存。
- 保留策略、归档、审计导出。
- 出口标准：万级 Artifact 的查询和影响分析在可接受延迟内。

## 14. 验收标准

1. 任一 Artifact 可以查询全部版本、内容哈希、Provenance 和当前状态。
2. 修改一个 Requirement 版本后，影响分析返回受影响的架构、契约、任务、测试、发布和失效证据，并带路径。
3. 基线批准后不可修改；基线 diff 按类型列出新增、删除、修改。
4. ChangeRequest 从创建到关闭完整走通，批准人不能是创建人。
5. Stale 证据阻止相关门禁；重新验证后恢复。
6. 外部 commit/CI run 只存引用；引用失效时标记无效，不静默使用。
7. 未记录配置/模型/提示词版本的证据降低信任等级，并在门禁中可见。
8. 跨项目血缘写入被拒绝；查询按项目隔离。
9. 迁移后旧 `links` 数据可查询，原备注和关系保留。
10. 全局搜索能按产物类型、状态、负责人、标签和版本过滤。

## 15. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 图规模 | 递归查询在大量边时变慢 | 先限制深度和关系；必要时物化视图或专用图存储 |
| 推断边噪音 | 辅助系统建议过多，人工确认成本高 | 只建议高置信度；批量确认；门禁不接受未确认边 |
| 版本爆炸 | 每次小改动都建版本 | 按语义版本或批准点建版本；草稿和临时产物不建正式版本 |
| 迁移复杂 | 旧 links、documents、work_items 映射 | 分步迁移、兼容视图、回滚脚本 |
| 基线粒度 | 需求/架构/发布基线 scope 不同 | 明确 scope；跨 scope 引用用边表达，不混入基线 |
| 外部引用失效 | commit 被 force-push、镜像被删除 | 标记无效并降级；关键证据要求不可变引用 |
| 配置引用缺失 | 模型/提示词版本未记录 | 默认降级；关键门禁强制要求 |
| 权限泄露 | 血缘暴露架构和发布信息 | 按项目权限过滤；敏感产物单独标记 |

**待决问题**：

1. 版本号采用单调整数还是语义版本？建议内部单调整数，展示层可映射语义版本。
2. 基线是否支持部分批准？建议不支持，整体批准；部分范围用多个基线。
3. 影响分析的默认关系集合是哪几类？建议 `implements/verifies/depends_on/satisfies`。
4. 推断边是否进入基线证据？不进入，只作提示。
5. 旧 `links` 是否保留写入口？建议只读兼容，新写入统一走 `artifact_edges`。
