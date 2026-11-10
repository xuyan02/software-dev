# RFC 0010：辅助系统注册与产物契约

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0009 产物血缘、基线与变更管理](0009-artifact-lineage-baselines-and-change.md)、[RFC 0015 Agent 架构与评测](0015-agent-architecture-and-evaluation.md)
- 作用：本文定义“辅助系统”（需求分析、架构推理、代码理解、测试生成、发布编排等）如何注册、被调用、产出产物、形成证据，以及如何被评测和替换。所有阶段的辅助能力都遵循本契约。

## 0. 结论

生命周期里的大量智能工作不适合写进 Foundry 的领域模型：需求歧义分析、架构方案权衡、测试生成、发布风险评估。这些能力应该由**可替换的辅助系统**提供，Foundry 只管理它们的注册、调用、产物和证据。

核心决定：

1. **辅助系统是可注册的能力，不是领域对象。** 注册单位是 `AuxiliarySystem`（实现）和 `AuxiliaryCapability`（能力）；一个系统可以提供多个能力。
2. **能力有契约。** 每个能力声明输入/输出 JSON Schema、权限、成本模型、超时、确定性、默认信任等级和负责人。
3. **调用有记录。** 每次调用记录调用者、能力版本、输入哈希、输出引用、策略决策、成本、耗时和 trace id。
4. **输出是 Artifact，不是批准。** 辅助系统的输出是草稿、分析、建议或验证结果；它可以成为 Evidence，但不能替代批准。
5. **证据要验证。** 辅助输出经过独立验证、holdout 或人类确认后，才获得更高信任等级；未验证的输出只能作为参考。
6. **辅助系统可替换。** 领域模型只依赖能力和契约，不依赖具体厂商；替换实现不改变对象、流程或证据语义。
7. **辅助系统本身要被评测。** 质量、一致性、成本、延迟、漂移和失败率进入评测；低质量能力可以被降级或停用。
8. **权限最小化。** 辅助系统按能力获得项目、文件、网络、密钥和工具的访问范围；不能因为“是分析工具”就获得写权限。
9. **安全边界明确。** 辅助系统在沙箱或受控环境中运行；外部内容视为数据，不是指令；输出不能直接触发不可逆操作。
10. **先集成，后自研。** 成熟的通用能力（静态分析、扫描、监控、IaC）优先集成；AI 原生差异化的能力（需求分析、架构推理、测试 oracle、独立验证）优先自研或有意识地共建。

## 1. 目标与非目标

### 1.1 目标

- 让每个生命周期阶段都能声明“我需要哪些辅助能力”，而不绑定具体工具。
- 让辅助系统的输出可追溯、可复现、可验证、可计费。
- 让需求分析、架构推理、测试生成、发布评估等能力可替换、可比较、可演进。
- 让辅助系统不能绕过权限、门禁和审计。
- 让“这个结论是哪个工具、哪个版本、用什么输入得出的”成为一次查询。
- 让低质量或漂移的辅助系统能被发现并降级。

### 1.2 非目标

- 不实现通用插件市场或第三方计费平台。
- 不实现任意代码执行；辅助系统仍受沙箱和权限约束。
- 不把辅助系统输出直接当成领域状态；状态仍由 Foundry 的对象和流程决定。
- 不实现通用模型路由；模型调用属于 Agent runtime 或辅助系统内部，Foundry 只记录版本和成本。
- 不实现所有辅助系统；本文只定义契约和接入方式。
- 不实现自动批准；批准权仍在策略和授权的人类/Agent。
- 不实现跨项目数据共享；辅助系统按项目和能力授权。
- 不实现辅助系统之间的自由编排；编排由工作流或 Agent 显式发起，并受策略约束。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| AuxiliarySystem | 一个可注册的辅助系统实现；有 id、版本、所有者、端点、状态。 |
| AuxiliaryCapability | 系统提供的一项能力；有输入/输出 schema、权限、成本、确定性。 |
| Invocation | 一次能力调用；记录调用者、输入、输出、成本、结果。 |
| CapabilityContract | 能力的输入/输出、权限、错误、超时、信任语义。 |
| Candidate Evidence | 辅助输出中可能成为证据的部分；尚未验证。 |
| Verification | 独立检查候选证据是否成立；产生信任等级。 |
| Trust Level | 证据可信度：self_reported / tool_verified / independently_verified / human_approved / attested。 |
| Drift | 辅助系统输出质量随时间或输入分布变化而下降。 |
| Capability Evaluation | 对辅助系统能力的质量、成本、延迟、一致性评测。 |

不变量：

1. 辅助系统必须先注册能力和契约，才能被工作流或 Agent 调用。
2. 每次调用都必须有调用记录；无记录的输出不能进入血缘或证据。
3. 调用必须经过策略授权；能力和项目范围之外的操作被拒绝。
4. 输出必须写入 Artifact 或 Invocation 的输出引用；不能只存在于对话或日志。
5. 辅助系统不能直接修改领域对象状态；只能提交产物、建议或证据。
6. 候选证据必须经验证才能获得 `tool_verified` 及以上等级。
7. 同一能力的不同版本必须可区分；输出记录能力版本。
8. 辅助系统不能读取未授权项目的产物、密钥或环境。
9. 辅助系统不能批准 ChangeRequest、基线、发布或 Waiver。
10. 不可复现的输出必须标记 `partial` 或 `not_reproducible`，并降低信任等级。
11. 能力评测结果和漂移告警写入事件日志。
12. 能力停用或降级不能删除历史调用和产物；历史仍可追溯。

## 3. 架构

```text
工作流 / Agent / 人类
        │ 调用能力（带项目、任务、输入、预算）
        ▼
┌─────────────────────────────┐
│ Foundry 能力网关             │
│ 授权 · 配额 · 审计 · 记录    │
└──────────────┬──────────────┘
               │ 受控调用
┌──────────────▼──────────────┐
│ 辅助系统（沙箱/受控环境）    │
│ 需求分析 · 架构推理 · 测试   │
└──────────────┬──────────────┘
               │ 输出
┌──────────────▼──────────────┐
│ Artifact / Candidate Evidence│
│ 版本 · 哈希 · Provenance     │
└──────────────┬──────────────┘
               │ 验证
┌──────────────▼──────────────┐
│ 独立验证 / holdout / 人类     │
│ 产生 Evidence + 信任等级     │
└─────────────────────────────┘
```

组件职责：

- **能力网关**：认证、授权、项目隔离、配额、超时、重试、审计、调用记录。
- **辅助系统**：实际计算；可以在 Foundry 内、独立服务或第三方；通过 MCP/HTTP/进程调用。
- **产物层**：把输出写成 ArtifactVersion；大文件存对象存储。
- **验证层**：独立验证、holdout、交叉检查、人类确认；决定信任等级。
- **评测层**：对能力和系统做质量、成本、漂移评测。

## 4. 能力契约

### 4.1 字段

```json
{
  "id": "requirements.analyze",
  "system_id": "foundry-requirements-analyzer",
  "version": "1.2.0",
  "name": "需求分析",
  "description": "检查需求完整性、歧义、冲突和可测试性，生成验收场景建议。",
  "input_schema": { "type": "object", "required": ["requirement", "context"] },
  "output_schema": { "type": "object", "required": ["findings", "scenarios"] },
  "permissions": {
    "project_scope": "current",
    "read": ["requirement", "issue", "glossary", "architecture_baseline"],
    "write": ["artifact:draft"],
    "network": "none",
    "filesystem": "readonly",
    "secrets": []
  },
  "cost_model": { "unit": "tokens", "estimate": 20000, "currency": "USD" },
  "timeout_seconds": 600,
  "determinism": "seeded",
  "default_trust": "self_reported",
  "verification": ["independent.analyze", "human.review"],
  "owner": { "kind": "agent", "id": "..." },
  "status": "active|deprecated|disabled"
}
```

### 4.2 权限

| 维度 | 取值 | 说明 |
|---|---|---|
| `project_scope` | `current` / `explicit` | 只能访问当前项目或显式授权项目 |
| `read` | 对象类型列表 | 允许读取的产物/对象 |
| `write` | `artifact:draft` / `none` | 只能写草稿产物，不能改状态 |
| `network` | `none` / `allowlist` / `full` | 出站网络策略 |
| `filesystem` | `none` / `readonly` / `workspace` | 文件访问 |
| `secrets` | key id 列表 | 只能引用，不能读取值 |
| `tools` | 工具能力列表 | 允许调用的工具 |

### 4.3 确定性

| 值 | 含义 | 处理 |
|---|---|---|
| `deterministic` | 相同输入相同输出 | 可直接复现 |
| `seeded` | 记录随机种子可复现 | 必须记录种子 |
| `stochastic` | 输出可能不同 | 需要多次采样或人类确认；默认信任降级 |

### 4.4 典型能力

| 能力 | 输入 | 输出 | 默认信任 |
|---|---|---|---|
| `requirements.analyze` | 需求草稿、Issue、术语表、架构基线 | 发现项、验收场景、追溯建议 | self_reported |
| `requirements.trace` | 需求、设计、任务、测试 | 候选血缘边 | self_reported |
| `architecture.options` | 需求、约束、质量属性 | 候选方案、权衡矩阵、风险 | self_reported |
| `architecture.conformance` | 代码、架构基线、约束 | 符合性报告、违规项 | tool_verified |
| `contract.check` | 契约新旧版本 | 兼容性报告、breaking changes | tool_verified |
| `code.understand` | 仓库引用、查询 | 代码地图、依赖、影响面 | tool_verified |
| `review.code` | diff、设计、规范 | 评审发现、风险评分 | self_reported |
| `test.generate` | 需求、验收场景、设计、代码 | 测试用例候选 | self_reported |
| `test.run` | 仓库引用、测试计划、环境 | 测试结果、覆盖率、日志 | tool_verified |
| `verify.independent` | 验收/holdout、产物、证据 | 通过/失败、理由 | independently_verified |
| `security.scan` | 产物、依赖、镜像 | 扫描报告、SBOM | tool_verified |
| `release.plan` | 变更、证据、风险 | 发布计划、回滚计划 | self_reported |
| `deploy.execute` | 发布、环境、配置 | 部署记录 | tool_verified |
| `observe.health` | 环境、时间窗 | 健康指标、异常 | tool_verified |
| `incident.analyze` | 事故、日志、变更 | 根因候选、时间线 | self_reported |
| `docs.generate` | 产物、代码 | 文档草稿 | self_reported |

能力不是封闭列表；新增能力必须声明契约、权限和验证方式。

## 5. 调用模型

### 5.1 调用流程

```text
1. 调用者（工作流/Agent/人类）声明 capability_id、版本约束、输入、预算
2. 网关校验：注册状态、能力存在、项目隔离、角色权限、配额
3. 网关创建 Invocation（状态=pending）
4. 在沙箱/受控环境调用辅助系统
5. 系统返回输出；网关校验 schema
6. 输出写入 ArtifactVersion 或 Invocation output_refs
7. 记录成本、耗时、trace、错误
8. 标记 Invocation 状态：succeeded / failed / timed_out / rejected
9. 候选证据进入验证队列
```

### 5.2 调用记录

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "task_id": "uuid|null",
  "caller": {"kind": "agent", "id": "..."},
  "capability_id": "requirements.analyze",
  "capability_version": "1.2.0",
  "system_id": "foundry-requirements-analyzer",
  "input_ref": {"hash": "sha256:...", "artifact_ids": ["..."]},
  "output_refs": [{"artifact_id": "...", "version": 1}],
  "status": "succeeded",
  "policy_decision": {"allow": true, "policy": "...", "reason": "..."},
  "cost": {"tokens": 18342, "currency": "USD", "estimate": 0.12},
  "duration_ms": 8421,
  "trace_id": "...",
  "error": null,
  "created_at": "..."
}
```

### 5.3 幂等与重试

- 调用必须带幂等键；相同键和相同输入返回同一 Invocation。
- 超时和 5xx 可以重试，使用同一 Invocation id；4xx 不重试。
- 重试不重复计费；成本以实际执行为准，记录每次尝试。
- 长任务支持取消；取消后输出不进入证据。

### 5.4 预算与配额

- 按项目、角色、能力、Agent 设置 token/费用/并发/时间配额。
- 超限时拒绝或降级到低成本能力；拒绝原因可见。
- 调用记录进入成本汇总（RFC 0016）。

## 6. 证据与验证

### 6.1 候选证据

辅助输出中的结论、报告、场景、测试结果可以声明为候选证据：

```json
{
  "candidate_id": "uuid",
  "invocation_id": "uuid",
  "type": "requirements.coverage",
  "claims": ["每个验收场景都有对应用例"],
  "artifact_refs": ["..."],
  "trust": "self_reported",
  "status": "unverified|verified|rejected|stale"
}
```

### 6.2 验证方式

| 方式 | 说明 | 可达到的信任等级 |
|---|---|---|
| 自查 | 同一系统重新检查 | 仍为 self_reported |
| 工具验证 | 独立工具执行并复现 | tool_verified |
| 交叉验证 | 另一个辅助系统独立分析 | 最多 tool_verified |
| holdout | 独立验证方用未公开场景验证 | independently_verified |
| 人类评审 | 授权人类确认 | human_approved |
| 签名证明 | 执行面签名 + 内容寻址 + 完整 Provenance | attested |

规则：

- 同一 Agent 不能同时是执行者和唯一验证者。
- holdout 场景不能出现在实现 Agent 的输入、日志或上下文中。
- 验证结果写入 `evidence_verifications`，包含验证者、方法、时间、证据快照。
- 验证失败不删除候选证据；标记 `rejected` 并保留原因。

### 6.3 证据失效

- 上游 ArtifactVersion 变化、代码变更、依赖更新、环境或配置变化时，证据标记 `stale`。
- `stale` 证据不能用于门禁；重新验证产生新 Evidence，旧 Evidence 标记 `superseded`。
- 失效传播规则见 RFC 0009 §4.6。

## 7. 辅助系统评测

### 7.1 评测维度

| 维度 | 指标 |
|---|---|
| 正确性 | 与人工/独立验证的一致率、误报/漏报 |
| 完整性 | 覆盖的需求/场景/风险比例 |
| 一致性 | 相同输入多次输出的一致率 |
| 可复现性 | 版本、种子、配置可复现比例 |
| 成本 | 每次调用 token/费用 |
| 延迟 | P50/P95 |
| 漂移 | 随时间或输入分布的质量变化 |
| 安全 | 越权、泄漏、注入、危险操作 |

### 7.2 评测套件

- 每个能力有基准输入和期望输出（不要求逐字一致，按评分标准）。
- 评测套件版本化；不能修改已发布的基线套件。
- 评测运行记录输入、能力版本、模型/提示词版本、评分和结论。
- 评测结果用于能力晋升、降级和替换。

### 7.3 晋升与降级

```text
candidate → evaluated → active → deprecated → disabled
```

- 新能力或新版本必须先通过评测才能进入 `active`。
- 质量或漂移低于阈值时自动降级为 `deprecated`，并在门禁中提示。
- `disabled` 后仍保留历史调用和产物。
- 关键门禁不能依赖 `deprecated` 能力，除非有 Waiver。

## 8. 安全

- **沙箱**：辅助系统在容器或受控进程中运行；无 ambient credentials。
- **网络**：默认无出站；需要时使用 allowlist；记录请求域名。
- **密钥**：只注入能力需要的短期凭据；密钥值不进入输入、输出、日志或模型上下文。
- **输入信任**：外部内容（Issue、网页、工具输出）视为数据；不把其中的指令当作系统指令。
- **输出清洗**：输出写入 Artifact 前校验 schema、大小和类型；拒绝可执行内容。
- **越权检测**：调用记录审计；异常访问告警。
- **数据分类**：敏感产物限制辅助系统读取；导出需要批准。
- **供应链**：辅助系统镜像、依赖和模型来源进入扫描和 SBOM。
- **不可逆操作**：辅助系统不能直接部署、删除或发布；必须通过 Foundry 的受控流程。

## 9. 数据模型

```sql
create table auxiliary_systems (
  id            uuid primary key,
  name          text not null,
  version       text not null,
  endpoint      text not null,
  owner         jsonb not null,
  status        text not null,
  metadata      jsonb not null default '{}'::jsonb,
  created_at    timestamptz not null default now()
);

create table auxiliary_capabilities (
  id            uuid primary key,
  system_id     uuid not null references auxiliary_systems (id),
  capability    text not null,
  version       text not null,
  input_schema  jsonb not null,
  output_schema jsonb not null,
  permissions   jsonb not null,
  cost_model    jsonb not null,
  determinism   text not null,
  default_trust text not null,
  status        text not null,
  unique (system_id, capability, version)
);

create table tool_invocations (
  id                uuid primary key,
  project_id        uuid not null references projects (id),
  task_id           uuid,
  caller            jsonb not null,
  system_id         uuid not null references auxiliary_systems (id),
  capability_id     uuid not null references auxiliary_capabilities (id),
  input_hash        text not null,
  input_ref         jsonb not null,
  output_refs       jsonb not null default '[]'::jsonb,
  status            text not null,
  policy_decision   jsonb not null,
  cost              jsonb not null default '{}'::jsonb,
  duration_ms       integer,
  trace_id          text,
  error             text,
  idempotency_key   text not null,
  created_at        timestamptz not null default now(),
  unique (idempotency_key)
);

create table evidence (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  artifact_version integer,
  type          text not null,
  trust         text not null,
  status        text not null,
  content_ref   jsonb not null,
  provenance    jsonb not null default '{}'::jsonb,
  invocation_id uuid references tool_invocations (id),
  created_at    timestamptz not null default now()
);

create table evidence_verifications (
  id            uuid primary key,
  evidence_id   uuid not null references evidence (id),
  method        text not null,
  verifier      jsonb not null,
  result        text not null,
  trust         text not null,
  note          text,
  created_at    timestamptz not null default now()
);
```

## 10. API、MCP、CLI

### 10.1 REST

```text
GET    /api/v1/auxiliary-systems
POST   /api/v1/auxiliary-systems
GET    /api/v1/auxiliary-systems/{id}/capabilities
POST   /api/v1/projects/{project}/invocations
GET    /api/v1/invocations/{id}
GET    /api/v1/projects/{project}/evidence?type=&trust=
POST   /api/v1/evidence/{id}/verify
GET    /api/v1/capabilities/{id}/evaluations
```

### 10.2 MCP

- `auxiliary.list` / `auxiliary.capabilities`
- `auxiliary.invoke`
- `invocation.get`
- `evidence.submit` / `evidence.verify` / `evidence.query`
- `capability.evaluation`

### 10.3 CLI

```bash
foundry auxiliary list
foundry auxiliary capabilities <system>
foundry auxiliary invoke <capability> --project <p> --input <file>
foundry invocation get <id>
foundry evidence verify <id> --method holdout
foundry capability eval <id>
```

## 11. 与现有实现的关系

- 现有 `crates/adapters/storage` 的空壳对应大证据存储；本 RFC 不要求先实现，但输出引用必须可解析。
- 现有 `api-mcp` 是入站 MCP；本 RFC 需要出站调用能力和 MCP client，二者不混用。
- 现有 `EventSink` 继续记录调用、验证和评测事件。
- 现有 `PolicyEngine` 扩展为能力授权；`DemoPolicy` 先支持最小规则，后续迁移 OPA/Cedar。
- 现有 Agent 账户可以作为辅助系统的 owner 或 caller；但辅助系统本身不是 Agent，不参与自动批准。

## 12. 实施计划

### 阶段 0：注册与调用记录

- 表结构、能力契约校验、网关授权、调用记录。
- 先接入 1–2 个只读能力：`requirements.analyze`、`code.understand`。
- 出口标准：调用可追溯、可计费、可拒绝。

### 阶段 1：产物与证据

- 输出写 ArtifactVersion；候选证据、验证、信任等级。
- 接入 `test.run`、`security.scan`。
- 出口标准：候选证据可验证，门禁能区分信任等级。

### 阶段 2：评测与替换

- 评测套件、评分、晋升/降级、漂移告警。
- 第二个实现接入同一能力，验证可替换。
- 出口标准：能力可比较、可替换；低质量能力自动降级。

### 阶段 3：高级能力与编排

- 架构推理、测试生成、发布评估、事故分析。
- 多能力编排由工作流或 Agent 显式发起；记录每一步调用。
- 出口标准：复杂阶段能声明能力依赖并获得可验证产物。

## 13. 验收标准

1. 未注册的能力不能被调用；越权调用被拒绝并记录。
2. 每次调用有完整记录：调用者、能力版本、输入哈希、输出引用、成本、策略、耗时。
3. 辅助输出可以写入 ArtifactVersion，并出现在血缘中。
4. 候选证据经验证后获得信任等级；未验证输出不能用于关键门禁。
5. 同一能力的两个实现可以通过评测比较并替换，不修改领域模型。
6. 能力降级或停用后，历史调用和产物仍可查询。
7. 不可复现的输出被标记并降低信任等级。
8. holdout 场景不会出现在实现 Agent 的输入或日志中。
9. 辅助系统不能批准 ChangeRequest、基线、发布或 Waiver。
10. 项目隔离在调用、输出、证据和查询中全部生效。

## 14. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 能力契约过重 | 小工具也要写完整 schema | 提供最小契约模板；按风险分级 |
| 输出质量不稳定 | 辅助系统幻觉、漏报 | 评测、交叉验证、holdout、信任降级 |
| 成本失控 | 频繁调用大模型 | 预算、缓存、配额、低成本回退 |
| 权限过大 | 分析工具获得写权限 | 默认只读；写草稿也需显式授权 |
| 供应商锁定 | 领域模型依赖某家工具 | 只依赖能力 id 和契约 |
| 评测成本 | 每个能力都要基准 | 先覆盖关键能力；复用评测框架 |
| 推断边污染 | 追溯建议过多 | 置信度门槛、人工确认、不进入门禁 |
| 漂移 | 模型或数据变化导致质量下降 | 定期评测、线上指标、自动降级 |

**待决问题**：

1. 首批自研能力选哪几个？建议需求分析、独立验证、测试 oracle。
2. 辅助系统调用是否统一走 MCP，还是允许 HTTP/进程？建议统一网关，协议可多种。
3. 能力评测的评分标准由谁维护？建议领域负责人 + 平台质量负责人。
4. 第三方辅助系统是否允许读取代码？按能力授权，默认只读最小范围。
5. 辅助系统本身是否也作为 AgentDefinition 管理？不，分开：辅助系统是能力实现，Agent 是执行主体。
