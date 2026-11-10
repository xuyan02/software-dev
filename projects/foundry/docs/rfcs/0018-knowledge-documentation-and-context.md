# RFC 0018：知识、文档与上下文管理

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[ADR 0003 Git 存内容，Postgres 存状态](../adr/0003-git-content-postgres-state.md)、[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0009 产物血缘、基线与变更管理](0009-artifact-lineage-baselines-and-change.md)、[RFC 0010 辅助系统注册与产物契约](0010-auxiliary-systems-and-evidence.md)、[RFC 0013 实现、评审、测试与质量门禁](0013-implementation-review-testing-and-quality.md)
- 作用：本文定义生命周期中的知识、文档、代码理解、上下文组装和记忆。它决定 Agent 在某个任务中“知道什么、依据什么、不能看到什么”。

## 0. 结论

Agent 的产出质量很大程度上取决于上下文质量。知识管理不是“再建一个 Wiki”，而是**让正确的知识在正确的任务中被正确的人/Agent 以正确的权限看到，并且可追溯**。

核心决定：

1. **知识是资产，但不是第二真相。** 代码、需求、架构决策、证据的真相在各自对象；知识是索引、摘要、约定和导航，必须引用来源。
2. **内容在 Git，元数据在 Postgres，索引是投影。** 文档正文在 Git；可查询字段、版本、状态、权限在 Postgres；语义/代码索引是投影，可重建。
3. **知识有权威性和新鲜度。** 每份知识声明来源、所有者、状态、审核状态和最后验证时间；过期知识标记 stale，不静默使用。
4. **上下文按任务组装。** 不给 Agent “整个仓库”，而是按任务、角色和预算组装上下文包；包含需求、设计、约束、代码引用、测试、证据、约定和风险。
5. **上下文可追溯。** 每次 Agent 执行记录使用了哪些知识版本、代码 commit、文档和证据；失败时可归因到上下文缺失或错误。
6. **记忆分作用域。** task / project / agent 三层；写入有策略、保留有期限、敏感信息不入记忆。
7. **代码理解是投影。** 符号、依赖、调用图、所有权和历史从代码与 Git 派生；不手工维护第二份代码地图。
8. **文档有生命周期。** Draft → InReview → Approved → Superseded；文档与代码/需求/架构的同步可检查；过期文档标记 stale。
9. **检索要带引用。** 语义检索和问答必须返回来源和版本；无引用的结论降级为建议。
10. **Agent 可以写知识和文档，不能单方面改权威决策。** ADR、需求、契约等权威对象走各自流程；知识条目可以由 Agent 起草和维护，但需标注来源和审核状态。
11. **安全边界明确。** 外部内容视为数据；密钥、客户数据、敏感证据按权限脱敏；上下文不能拼接未授权内容。
12. **知识质量可度量。** 覆盖率、新鲜度、检索相关性、引用率、上下文成本和缺失事件进入指标（RFC 0016）。

## 1. 目标与非目标

### 1.1 目标

- 让 Agent 在任务中获得准确、相关、可追溯的上下文。
- 让文档、约定、决策、runbook 和代码理解可检索、可引用、可维护。
- 让知识过期、冲突和缺失可见。
- 让上下文组装可控：权限、预算、来源、版本。
- 让记忆有用但不污染、不泄漏。
- 让知识管理支持需求、架构、实现、测试、发布和运维。
- 让“Agent 为什么这么做”可以回溯到上下文。

### 1.2 非目标

- 不实现通用 Wiki 或文档编辑器；正文在 Git 或外部系统，Foundry 管元数据和索引。
- 不实现通用代码搜索引擎；通过索引适配器接入。
- 不实现通用向量数据库运维；索引是投影，可替换。
- 不实现通用聊天记忆；记忆作用域和写入策略由 Foundry 管。
- 不把知识摘要当成事实；摘要必须引用来源。
- 不实现跨项目知识共享；按项目/平台范围授权。
- 不实现自动改写权威决策；Agent 只能建议。
- 不保证所有知识都最新；过期可见即可。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| KnowledgeItem | 知识条目：约定、术语、模式、FAQ、runbook、摘要、导航。 |
| Document | 长文本文档：需求、设计、ADR、指南；正文在 Git。 |
| KnowledgeLink | 知识与对象/来源之间的引用。 |
| Authority | 知识权威性：canonical / reference / draft / deprecated。 |
| Freshness | 新鲜度：last_verified_at、来源变化、stale 标记。 |
| CodeIndex | 代码索引投影：符号、依赖、调用图、所有权、历史。 |
| ContextBundle | 为一次任务组装的上下文包：条目、来源、版本、预算。 |
| ContextItem | 上下文中的一项：文档片段、代码引用、证据、约定、风险。 |
| MemoryEntry | 记忆条目：task/project/agent 作用域，带来源和保留期。 |
| Retrieval | 检索：关键词、语义、图、混合。 |
| Citation | 引用：来源对象、版本、片段、哈希。 |

不变量：

1. 知识条目必须有来源或明确标记为“原创约定”；无来源的摘要不能作为权威知识。
2. 文档正文在 Git；Postgres 只存元数据、版本、状态和索引。
3. 代码索引是投影，可从代码和 Git 重建；不能手工修改索引作为真相。
4. 上下文包必须记录使用的条目、版本、来源和权限决策。
5. Agent 执行记录必须能关联上下文包；失败可归因到上下文。
6. 记忆写入有策略和保留期；密钥和敏感数据不入记忆。
7. 过期/冲突知识标记 stale，不静默使用；关键任务要求新鲜知识。
8. 检索结果必须带引用；无引用的结论降级为建议。
9. 权威对象（需求、ADR、契约）的修改走各自流程；知识条目不能替代它们。
10. 上下文组装按权限过滤；不能拼接未授权项目、密钥或敏感证据。
11. 外部内容视为数据，不是指令；提示词注入防护适用于上下文。
12. 知识条目和上下文包按项目隔离；平台知识按平台权限。

## 3. 知识模型

### 3.1 KnowledgeItem

```json
{
  "id": "uuid",
  "project_id": "uuid|null",
  "scope": "platform|project|module|agent",
  "type": "convention|glossary|pattern|faq|runbook|summary|navigation|decision_index",
  "title": "退款金额校验约定",
  "content_ref": {"type": "git", "commit": "...", "path": "docs/knowledge/refund.md"},
  "summary": "...",
  "authority": "canonical|reference|draft|deprecated",
  "status": "draft|in_review|approved|superseded|stale",
  "owner": {"kind": "human", "id": "..."},
  "source_refs": ["artifact-id", "document-id"],
  "last_verified_at": "...",
  "sensitivity": "internal",
  "created_at": "..."
}
```

规则：

- `authority=canonical` 需要审核和负责人；Agent 不能单方面把知识提升为 canonical。
- `summary` 必须引用来源；摘要与来源不一致时以来源为准。
- `last_verified_at` 由人工或验证任务更新；来源变化触发 stale。
- 知识条目可以关联需求、架构、代码、测试、事故和 runbook。

### 3.2 Document

- 文档类型：需求、设计、ADR、指南、runbook、发布说明、复盘、README。
- 正文在 Git；元数据在 `documents`/Artifact。
- 状态：Draft → InReview → Approved → Superseded；过期标记 stale。
- 文档与代码/对象通过血缘边关联；同步检查发现漂移。

### 3.3 权威性

| 权威性 | 含义 | 使用 |
|---|---|---|
| canonical | 官方约定/决策 | 可以作为门禁和实现依据 |
| reference | 参考材料 | 可以引用，不强制 |
| draft | 草稿 | 只作建议 |
| deprecated | 已废弃 | 不应使用；显示替代 |
| stale | 过期 | 需要重新验证 |

## 4. 文档管理

### 4.1 生命周期

```text
Draft → InReview → Approved → Superseded
                 ↘ Rejected
Approved → Stale（来源变化或过期）→ InReview（重新审核）
```

规则：

- 文档批准需要负责人和审核人；关键文档（需求、ADR、契约）走各自 RFC 的流程。
- 文档变更走版本或 ChangeRequest；不覆盖已批准版本。
- 文档与代码/对象不同步时标记 stale，并在门禁中提示。
- 文档删除只归档；历史保留。

### 4.2 同步检查

- 文档引用的代码/对象版本变化时，标记文档需要复审。
- README、runbook、约定与代码结构不一致时，产生 finding。
- 文档中的命令、路径、配置必须可验证；过期命令标记。
- 同步检查可以由辅助系统执行，但结论需要引用来源。

## 5. 代码理解与索引

### 5.1 索引内容

- 符号：模块、类型、函数、接口。
- 依赖：导入、调用、数据流、配置。
- 所有权：CODEOWNERS、最近修改者、负责人。
- 历史：提交、PR、变更频率、缺陷关联。
- 测试：用例、覆盖率、归属。
- 架构：组件、契约、约束（来自 RFC 0012）。

### 5.2 规则

- 索引从代码和 Git 派生，可重建；不手工维护第二份代码地图。
- 索引更新有延迟时标记 `indexed_at`；关键决策不能只依赖过期索引。
- 代码引用使用 `repo + commit + path + symbol`，避免分支漂移。
- 索引访问按项目权限；敏感仓库需要额外授权。
- 语义检索是补充，不是唯一依据；关键结论要求代码/文档引用。

## 6. 上下文组装

### 6.1 ContextBundle

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "task_id": "uuid",
  "agent_version_id": "uuid",
  "budget": {"max_tokens": 120000, "max_items": 80},
  "items": [
    {
      "type": "requirement",
      "ref": "artifact-id@3",
      "citation": "...",
      "tokens": 2400,
      "priority": "must"
    },
    {
      "type": "code",
      "ref": "repo@commit:path#symbol",
      "tokens": 8000,
      "priority": "must"
    },
    {
      "type": "convention",
      "ref": "knowledge-id@2",
      "tokens": 600,
      "priority": "should"
    }
  ],
  "excluded": [{"ref": "...", "reason": "permission|budget|stale"}],
  "created_at": "..."
}
```

### 6.2 组装规则

1. **任务优先。** 先放任务、需求、验收场景、设计、架构约束、相关代码和测试。
2. **权限过滤。** 未授权项目、holdout、密钥、敏感证据不进入上下文。
3. **预算控制。** 按 token/条目上限选择；超出时按优先级裁剪，并记录 excluded。
4. **来源可追溯。** 每项带引用和版本；Agent 结论应引用来源。
5. **新鲜度。** 过期知识降级或排除；关键任务要求新鲜。
6. **冲突处理。** 多个来源冲突时，保留冲突并标注权威性，不能静默选一个。
7. **提示词注入隔离。** 外部内容放在数据区，不作为系统指令；工具输出同样处理。
8. **可复现。** 上下文包记录版本和哈希；相同输入可重建（索引变化除外）。

### 6.3 上下文类型

| 类型 | 内容 |
|---|---|
| Task | 任务描述、验收、约束、负责人 |
| Requirement | 需求、场景、non-goals、holdout（按权限） |
| Architecture | 基线、ADR、契约、约束、质量属性 |
| Code | 相关文件、符号、依赖、历史 |
| Test | 计划、用例、覆盖率、失败 |
| Evidence | 评审、扫描、测试报告（按权限） |
| Knowledge | 约定、术语、模式、runbook |
| Risk | 已知风险、事故、技术债 |
| History | 相关提交、PR、决策 |

## 7. 记忆

### 7.1 作用域

| 作用域 | 内容 | 保留 |
|---|---|---|
| task | 当前任务的临时状态、尝试、结论 | 任务结束后按策略清理 |
| project | 项目约定、决策摘要、常见问题 | 项目生命周期 |
| agent | Agent 的偏好、有效模式、失败教训 | 版本/绑定生命周期 |

### 7.2 规则

- 记忆写入有策略：什么可写、谁可写、是否需审核。
- 密钥、客户数据、holdout、未授权证据不入记忆。
- 记忆条目有来源和版本；摘要不能替代权威对象。
- 任务记忆默认不跨任务共享；项目记忆需审核。
- 记忆过期或冲突时标记；不能静默使用旧结论。
- 记忆可删除；删除不影响权威对象和历史事件。

## 8. 检索与信任

### 8.1 检索方式

- 关键词/全文：精确匹配。
- 语义：向量检索，适合概念查找。
- 图：沿血缘/依赖检索。
- 混合：按任务组合，带权重。

### 8.2 信任规则

- 检索结果必须带来源、版本和权威性。
- canonical > reference > draft；stale 降级。
- 无引用结论只能作为建议，不能作为门禁证据。
- 多来源冲突时显示冲突，不自动合并。
- 检索质量由指标和人工抽查评估。

## 9. 辅助系统

| 能力 | 输入 | 输出 | 信任 |
|---|---|---|---|
| `knowledge.extract` | 文档、代码、决策 | 知识条目候选 | self_reported |
| `docs.generate` | 对象、代码、证据 | 文档草稿 | self_reported |
| `docs.check` | 文档、代码、对象 | 过期/冲突/漂移 | tool_verified |
| `code.index` | 仓库、commit | 代码索引投影 | tool_verified |
| `code.understand` | 仓库、查询 | 代码地图、影响面 | tool_verified |
| `context.assemble` | 任务、预算、权限 | ContextBundle | tool_verified |
| `retrieval.search` | 查询、权限 | 结果 + 引用 | tool_verified |
| `knowledge.verify` | 知识条目、来源 | 一致性/新鲜度 | tool_verified |

规则：

- 辅助输出是草稿或证据，不是权威知识。
- 摘要必须引用来源；无引用的摘要不能提升为 canonical。
- 上下文组装必须执行权限和预算规则；记录 excluded 原因。
- 辅助系统的检索和摘要质量进入评测。

## 10. 数据模型

```sql
create table knowledge_items (
  id            uuid primary key,
  project_id    uuid,
  scope         text not null,
  type          text not null,
  title         text not null,
  content_ref   jsonb not null,
  summary       text,
  authority     text not null,
  status        text not null,
  owner         jsonb not null,
  source_refs   jsonb not null default '[]'::jsonb,
  last_verified_at timestamptz,
  sensitivity   text not null default 'internal',
  created_at    timestamptz not null default now()
);

create table knowledge_links (
  knowledge_id  uuid not null references knowledge_items (id),
  target_kind   text not null,
  target_id     uuid not null,
  relation      text not null,
  primary key (knowledge_id, target_kind, target_id, relation)
);

create table context_bundles (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  task_id       uuid not null,
  agent_version_id uuid,
  budget        jsonb not null,
  items         jsonb not null,
  excluded      jsonb not null default '[]'::jsonb,
  hash          text not null,
  created_at    timestamptz not null default now()
);

create table memory_entries (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  scope         text not null,
  task_id       uuid,
  agent_version_id uuid,
  content       jsonb not null,
  source_refs   jsonb not null default '[]'::jsonb,
  retention     text not null,
  status        text not null,
  created_at    timestamptz not null default now()
);
```

索引投影可以放在外部搜索引擎/向量库，记录 `indexed_at`、`commit`、`model` 和哈希；不进入真相表。

## 11. API、MCP、CLI、Web

### 11.1 REST

```text
GET/POST /api/v1/projects/{project}/knowledge
GET      /api/v1/knowledge/{id}
POST     /api/v1/knowledge/{id}/verify
GET      /api/v1/projects/{project}/documents
POST     /api/v1/documents/{id}/review
POST     /api/v1/projects/{project}/context/assemble
GET      /api/v1/context/{bundle}
GET/POST /api/v1/projects/{project}/memory
GET      /api/v1/search?q=&project=&types=
```

### 11.2 MCP

- `knowledge.get` / `knowledge.search` / `knowledge.submit`
- `document.get` / `document.list`
- `context.assemble` / `context.get`
- `memory.read` / `memory.write`（受策略限制）
- `code.index.query` / `code.understand`
- `docs.check`

Agent 可以读取授权知识、提交草稿、请求上下文；不能把知识提升为 canonical，不能读取 holdout/密钥/未授权项目。

### 11.3 CLI

```bash
foundry knowledge list <project>
foundry knowledge search "退款校验"
foundry knowledge verify <id>
foundry docs check <project>
foundry context assemble --task <task> --budget 120000
foundry memory list <project>
```

### 11.4 Web

- 知识页：条目、权威性、来源、新鲜度、关联对象。
- 文档页：版本、审核、同步状态、diff。
- 代码理解页：符号/依赖/所有权/历史查询（引用视图）。
- 上下文页：一次任务的上下文包、排除项、预算、来源。
- 记忆页：作用域、来源、保留、删除。

## 12. 权限与安全

- 知识和文档按项目/平台权限读取；敏感知识设置 `sensitivity`。
- holdout、密钥、客户数据和未授权证据不进入上下文。
- 外部内容视为数据；提示词注入防护和输出清洗适用。
- Agent 可以提交知识草稿和文档草稿；canonical 需要人类审核。
- 上下文包和记忆有保留策略；删除请求可审计。
- 检索索引可能包含敏感内容；索引访问按权限过滤，不能通过检索绕过。
- 所有上下文组装、知识审核、记忆写入写入审计日志。

## 13. 与现有实现的关系

- 现有 `documents` 表存文档元数据；本 RFC 扩展知识条目和上下文包。
- 现有 `artifact_edges` 可以作为知识链接的一部分；知识条目通过边关联对象。
- 现有 `web/workflow-editor` 不涉及知识 UI；控制台后续增加知识/上下文页面。
- 现有 Agent runtime（RFC 0015）需要调用上下文组装；本 RFC 定义契约。
- 现有凭证和项目隔离继续适用；知识不能绕过。

## 14. 实施计划

### 阶段 0：知识与文档元数据

- KnowledgeItem/Document 元数据、权威性、来源、新鲜度。
- 出口标准：知识和文档可查询、可引用、可标记 stale。

### 阶段 1：代码索引与检索

- 代码索引投影、关键词/语义检索、引用返回。
- 出口标准：Agent 能检索并引用代码/文档。

### 阶段 2：上下文组装

- ContextBundle、预算、权限、excluded 记录。
- 接入 Agent runtime；记录上下文哈希。
- 出口标准：Agent 执行可追溯到上下文。

### 阶段 3：记忆与同步检查

- 记忆作用域、保留、写入策略；文档/代码同步检查。
- 出口标准：记忆可控；过期文档可见。

### 阶段 4：辅助能力与质量

- 知识抽取、文档生成、一致性检查、检索评测。
- 出口标准：知识质量可度量；辅助能力可替换。

## 15. 验收标准

1. 知识条目有来源、权威性、状态和新鲜度；无来源摘要不能成为 canonical。
2. 文档正文在 Git；Postgres 元数据可查询、可版本化。
3. 代码索引可重建；引用使用 commit + path + symbol。
4. 上下文包记录条目、版本、预算、排除原因和哈希。
5. 未授权内容、holdout、密钥不进入上下文。
6. Agent 执行可关联上下文包；失败可归因到上下文缺失/错误。
7. 记忆有作用域、来源、保留期；敏感信息不入记忆。
8. 检索结果带引用；无引用结论降级。
9. 过期/冲突知识标记 stale/conflict，不静默使用。
10. 知识质量、上下文成本和检索相关性可度量。

## 16. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 知识第二真相 | 摘要/索引被当成事实 | 强制引用；摘要降级；来源为准 |
| 索引过期 | 代码变化后索引延迟 | indexed_at、commit、关键决策要求新鲜 |
| 上下文过大 | token 成本高、噪音多 | 预算、优先级、裁剪、摘要 |
| 检索泄漏 | 通过检索绕过权限 | 权限过滤、索引分区、审计 |
| 记忆污染 | 错误结论被复用 | 来源、审核、过期、删除 |
| 文档漂移 | 文档与代码不一致 | 同步检查、stale、复审 |
| 提示词注入 | 外部内容变成指令 | 数据/指令分离、输出清洗 |
| 质量难评估 | 知识/检索质量主观 | 指标、抽查、用户反馈 |

**待决问题**：

1. canonical 知识由谁审核？建议领域负责人 + 平台知识负责人。
2. 代码索引用哪种存储？建议外部搜索引擎/向量库，作为投影。
3. 上下文预算按任务还是按 Agent？建议任务 + Agent 上限双重约束。
4. 记忆保留多久？建议 task 短期、project 中期、agent 版本生命周期。
5. 是否允许跨项目平台知识？建议允许平台级 canonical，项目级私有。
