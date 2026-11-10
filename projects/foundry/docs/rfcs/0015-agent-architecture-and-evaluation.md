# RFC 0015：Agent 架构与评测

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0010 辅助系统注册与产物契约](0010-auxiliary-systems-and-evidence.md)、[RFC 0013 实现、评审、测试与质量门禁](0013-implementation-review-testing-and-quality.md)、[RFC 0016 度量、成本与自举](0016-metrics-cost-and-self-hosting.md)
- 作用：本文定义 Agent 作为一等公民的架构：定义、版本、模型/提示词、工具、绑定、评测、观测和生命周期。

## 实现状态（2026-11-09）

已实现 `projects/foundry/crates/agent-runtime` 的运行时：

- 模型：`OpenAiCompatibleModel`（OpenAI/DeepSeek/Ollama/vLLM），重试、超时、工具调用、usage。
- 工具：`ToolRegistry`、`EchoTool`、`LocalSandbox`、Foundry 需求文档工具。
- MCP：`McpClient`（initialize/tools list/tools call）、`StdioTransport`。
- 循环：模型 → 工具 → 观察 → 最终答案；预算、取消、轨迹。
- 轨迹：`InMemoryTrajectory`、`PostgresTrajectorySink`、`agent_runs`/`agent_steps` 迁移。
- 成本：`Pricing`、`Usage::cost_usd`、`RunResult.cost_usd`。
- 评测：`EvaluationCase`、`EvaluationReport`、`evaluate_case`、`evaluate_suite`。
- 人类交互：`HumanInteraction`、`human.ask`（保留）；统一 Issue 讨论：`task.links.list`、`issue.get`、`issue.comment`。
- 暂停/恢复：`RunOutcome::WaitingForIssueComment`、`AgentCheckpoint`、`AgentRuntime::resume`。
- Agent 进程：`services/agentd` 每个 Agent 一个常驻进程，只领取分配到该定义的 Run；控制平面 supervisor 启动/停止/重启并记录进程状态。`/agents` 管理定义、绑定和进程。
- 事件驱动恢复：agentd 消费 `IssueCommented` 事件；只有人类评论会把等待中的 Run 重新置为 queued。
- `services/worker` 已弃用。
- 澄清 Issue 由 `goal_clarification` 的 `task.created` action 创建；`clarifier` 声明为 agent 角色，任务池调度器为未绑定的 agent 角色创建 AgentRun。
- ProjectBrief 工具：`project.brief.get/get_draft/save_draft/submit`。
- CLI：`foundry-agent demo|run|clarify`。

尚未实现：AgentDefinition/版本/绑定、评测晋升、Docker/VM 沙箱、策略/审批/证据、HTTP MCP transport、上下文 token 预算、多 Agent。

## 0. 结论

Agent 不只是 `AccountKind::Agent`。账户是身份，**AgentDefinition 是版本化的能力配置**；没有定义、评测和绑定，Agent 只是一个能登录的账户。

核心决定：

1. **身份与配置分离。** `Account(kind=agent)` 是稳定身份；`AgentDefinition` 描述它是什么、能做什么、用什么模型/提示词/工具、受什么限制；`AgentVersion` 是不可变配置快照。
2. **定义版本化。** Agent 定义像 Workflow 一样有草稿、版本、发布和固定引用；任务运行时固定 `(agent_definition, agent_version)`，后续升级不改变历史任务。
3. **能力由绑定决定。** Agent 能承担哪个工作流角色，由 `AgentBinding` 决定；绑定必须经过评测和批准。账户能登录不等于能执行任何任务。
4. **工具是一等对象。** 工具注册表记录工具类型、schema、权限、成本、确定性、超时和所有者；Agent 只能调用被授权的工具。
5. **评测前置。** 没有通过评测的 AgentVersion 不能绑定到关键角色；评测覆盖基准、回归、holdout、对抗、安全和成本。
6. **评测是持续的。** 上线后继续用 canary、shadow、A/B 和漂移监控评估；质量下降自动降级或回退。
7. **观测是一等能力。** 轨迹、工具调用、模型调用、token/成本、延迟、失败分类、人工接管都要记录；不能只看最终结果。
8. **Agent 不能自我修改。** Agent 不能创建、修改、发布自己的定义、工具权限或评测标准；这些属于人类管理员或明确授权的平台角色。
9. **多 Agent 协作后置。** 委派、团队、交接和冲突解决是后续能力；第一期先保证单 Agent 可评测、可观测、可回退。
10. **自举是验收。** Foundry 自己的需求、设计、实现、测试和发布逐步由 Foundry 管理的 Agent 完成；这是 L4 闭环。
11. **安全和成本是硬约束。** 工具权限最小化、密钥隔离、提示词注入防护、预算和配额是 Agent 运行的前提。
12. **不追求“通用 Agent 框架”。** 只管理 Foundry 生命周期需要的 Agent 能力；不发明第二套工作流或工具协议。

## 1. 目标与非目标

### 1.1 目标

- 让 Agent 可定义、可版本化、可评测、可绑定、可观测、可回退。
- 让 Agent 的能力和权限显式化，而不是隐含在提示词或账户角色里。
- 让模型、提示词、工具和运行限制的变更可追溯、可复现。
- 让评测成为 Agent 上线的门禁，而不是事后验证。
- 让 Agent 的失败可归因：模型、提示词、工具、上下文、环境还是任务本身。
- 让多 Agent 协作有清晰的角色和交接边界（后续阶段）。
- 让自举可度量：哪些任务由 Agent 完成，人工接管率多少。

### 1.2 非目标

- 不实现通用 Agent 编排框架或自主目标生成。
- 不实现模型训练或微调；只管理模型配置和版本引用。
- 不实现通用工具市场；工具注册面向 Foundry 场景。
- 不实现 Agent 自我复制、自我授权或自我修改。
- 不实现跨项目 Agent 共享；定义可以平台级，但绑定按项目。
- 不实现完全自主的多 Agent 组织；协作是后续能力。
- 不把 Agent 定义写进 Workflow AST；两者分离，通过绑定关联。
- 不实现通用提示词 IDE；只提供版本、变量和评测。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| AgentDefinition | Agent 的定义：名称、所有者、能力、角色、默认版本。 |
| AgentVersion | 不可变配置快照：模型、提示词、工具、限制、策略、运行时。 |
| ModelConfig | 模型提供商、模型 id、参数、回退和成本。 |
| PromptVersion | 系统提示词、任务模板、变量和版本。 |
| ToolDefinition | 工具注册：类型、schema、权限、成本、确定性、超时。 |
| AgentBinding | AgentVersion 与工作流角色/项目范围的绑定。 |
| AgentTrajectory | 一次任务执行的轨迹：步骤、工具调用、模型调用、成本、结果。 |
| EvaluationSuite | 评测套件：用例、评分标准、阈值、holdout。 |
| EvaluationRun | 一次评测运行：版本、结果、分数、成本、结论。 |
| CapabilityStandard | 某个工作流角色的能力标准：成功率、质量、成本、安全。 |
| Human Takeover | 人工接管：Agent 失败或主动升级时人类接手。 |
| Shadow Mode | 影子运行：Agent 执行但不产生副作用，用于评测。 |

不变量：

1. Agent 定义和版本不可变；修改产生新版本。
2. 每个 AgentVersion 必须绑定模型配置、提示词版本、工具集和资源限制。
3. 任务运行时固定 AgentVersion；后续发布不改变历史任务。
4. AgentBinding 必须引用已通过评测的 AgentVersion 和明确的项目范围。
5. Agent 不能修改自己的定义、工具权限、评测标准或绑定。
6. 工具调用必须经过工具网关授权；未授权的工具不可调用。
7. 模型、提示词、工具和配置版本必须记录在 Provenance 中。
8. 评测结果必须可追溯到套件版本、输入、环境和评分标准。
9. 关键角色绑定要求评测通过和批准；低风险角色可以放宽，但需记录。
10. 轨迹必须记录工具调用、模型调用、成本、延迟和结果；密钥不进入轨迹。
11. 质量下降或漂移触发降级/回退；历史任务保留原版本。
12. 多 Agent 协作必须有明确角色、交接物和冲突解决；不能共享隐式状态。

## 3. 对象模型

### 3.1 AgentDefinition

```json
{
  "id": "uuid",
  "name": "implementer-v2",
  "owner": {"kind": "human", "id": "..."},
  "account_id": "account-uuid",
  "description": "负责实现和自测的编码 Agent",
  "capabilities": ["code.implement", "test.run", "code.understand"],
  "default_version": 3,
  "status": "active|deprecated|retired",
  "created_at": "..."
}
```

规则：

- `account_id` 是登录身份；一个账户可以绑定多个定义（例如不同项目/角色）。
- 定义属于账户或平台；平台定义由管理员维护。
- 定义不包含具体项目绑定；绑定由 `AgentBinding` 表达。
- 定义停用不删除历史版本和轨迹。

### 3.2 AgentVersion

```json
{
  "id": "uuid",
  "definition_id": "uuid",
  "version": 3,
  "model_config_id": "uuid",
  "prompt_version_id": "uuid",
  "tools": ["tool-id"],
  "limits": {
    "max_tokens_per_task": 200000,
    "max_cost_usd_per_task": 5.0,
    "max_runtime_seconds": 1800,
    "max_concurrency": 4
  },
  "policies": {
    "allowed_roles": ["implementer"],
    "forbidden_actions": ["release.approve", "waiver.approve"],
    "human_approval_required": ["delete", "publish"]
  },
  "memory": {"scopes": ["task", "project"], "retention": "task"},
  "runtime": {"sandbox": "container", "network": "allowlist"},
  "evaluation": {"suite_id": "uuid", "last_run_id": "uuid", "status": "passed"},
  "created_by": {"kind": "human", "id": "..."},
  "created_at": "..."
}
```

规则：

- 版本内容不可变；变更产生新版本。
- 限制是硬约束，不是建议；运行时强制执行。
- 策略声明禁止动作和需要人类批准的动作；与项目策略叠加。
- 评测状态是绑定的前提。

### 3.3 ModelConfig

```json
{
  "id": "uuid",
  "provider": "anthropic|openai|local|custom",
  "model": "claude-...",
  "params": {"temperature": 0.2, "max_tokens": 8000},
  "fallbacks": [{"provider": "...", "model": "..."}],
  "cost": {"input_per_mtok": 3.0, "output_per_mtok": 15.0},
  "status": "active|deprecated",
  "created_at": "..."
}
```

规则：

- 模型和参数版本化；评测绑定具体配置。
- 回退链明确；回退事件记录。
- 成本用于预算和指标；不准确时标记。

### 3.4 PromptVersion

```json
{
  "id": "uuid",
  "name": "implementer-system",
  "version": 7,
  "system_prompt_ref": {"type": "git", "commit": "...", "path": "prompts/implementer.md"},
  "task_templates": {"implement": "...", "fix": "..."},
  "variables": ["task.title", "requirement", "architecture_baseline"],
  "evaluation": {"suite_id": "uuid", "last_run_id": "uuid"},
  "created_at": "..."
}
```

规则：

- 提示词内容在 Git；版本和哈希在 Postgres。
- 提示词不能绕过权限或门禁；它是行为指导，不是授权。
- 提示词变更必须评测；关键角色要求回归通过。
- 提示词中的变量必须来自受控上下文；不能拼接未授权内容。

### 3.5 ToolDefinition

```json
{
  "id": "uuid",
  "name": "git.create_branch",
  "type": "mcp|http|function",
  "description": "在仓库创建分支",
  "input_schema": {"type": "object", "required": ["repo", "branch", "base"]},
  "output_schema": {"type": "object"},
  "permissions": {
    "scopes": ["repo:write"],
    "project_scope": "current",
    "network": "allowlist",
    "secrets": []
  },
  "cost": {"unit": "calls", "estimate": 1},
  "determinism": "deterministic",
  "timeout_seconds": 60,
  "owner": {"kind": "human", "id": "..."},
  "status": "active|deprecated|disabled"
}
```

规则：

- 工具必须有 schema、权限、成本和超时。
- 工具调用记录输入/输出哈希、状态、耗时和错误。
- 写工具和不可逆工具需要额外授权；默认 Agent 只读。
- 工具版本变化影响评测；绑定固定工具版本或范围。
- 工具不能绕过 Foundry 的发布、批准和证据门禁。

### 3.6 AgentBinding

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "agent_version_id": "uuid",
  "workflow_id": "iteration",
  "role": "implementer",
  "status": "proposed|active|suspended|revoked",
  "evaluation_run_id": "uuid",
  "approved_by": {"kind": "human", "id": "..."},
  "scope": {"tasks": "assigned", "repositories": ["payments"]},
  "created_at": "..."
}
```

规则：

- 绑定声明项目、工作流、角色和范围；不能跨项目隐式生效。
- 绑定必须引用通过的评测运行。
- 绑定可以有上限：并发、任务数、成本、时间。
- 绑定撤销后新任务不再分配；运行中的任务按策略完成或取消。

### 3.7 EvaluationSuite / EvaluationRun

```json
{
  "id": "uuid",
  "name": "implementer-regression",
  "version": 2,
  "cases": [
    {
      "id": "case-1",
      "task": "修复部分退款金额校验",
      "input_ref": "...",
      "expected": "...",
      "rubric": "tests_pass_and_review_clean",
      "holdout": true,
      "weight": 1.0
    }
  ],
  "metrics": ["success_rate", "quality_score", "cost", "latency", "safety"],
  "thresholds": {"success_rate": 0.9, "quality_score": 0.8, "safety": 1.0},
  "owner": {"kind": "human", "id": "..."},
  "created_at": "..."
}
```

```json
{
  "id": "uuid",
  "suite_id": "uuid",
  "agent_version_id": "uuid",
  "environment": "eval-sandbox",
  "results": [{"case_id": "case-1", "score": 0.95, "status": "passed"}],
  "summary": {"success_rate": 0.93, "quality_score": 0.86, "cost": 12.4},
  "status": "passed|failed|error",
  "started_at": "...",
  "finished_at": "..."
}
```

规则：

- 套件版本化；已发布套件不可修改。
- 评测用例包含 holdout，执行 Agent 不可见。
- 评测环境固定模型、工具、数据和配置版本。
- 评测结果用于绑定、晋升、降级和回退。
- 评测失败必须给出失败分类和样例；不能只给一个分数。

## 4. 生命周期与晋升

```text
draft → evaluating → active → deprecated → retired
                ↘ rejected
```

规则：

- `draft`：可编辑，不可绑定。
- `evaluating`：运行评测套件；不可绑定关键角色。
- `active`：评测通过并批准，可绑定。
- `deprecated`：不再新绑定，已有绑定可继续或迁移。
- `retired`：不可绑定；历史轨迹保留。
- `rejected`：评测或审批失败；保留原因。

晋升条件：

1. 评测套件通过阈值。
2. 安全评测通过（无越权、无密钥泄漏、无危险操作）。
3. 成本/延迟在预算内。
4. 人类管理员批准。
5. 关键角色需要至少一个 holdout 通过。

降级/回退：

- 线上成功率、质量或安全低于阈值时自动降级。
- 降级后新任务停止分配；运行中任务按策略完成或取消。
- 回退到上一个通过评测的版本；历史任务保留原版本。
- 降级/回退写入事件日志和指标。

## 5. 工具与能力

### 5.1 能力标准

每个工作流角色定义能力标准：

| 角色 | 能力标准 |
|---|---|
| clarifier | 需求澄清质量、问题覆盖率、人工接管率 |
| analyst | 需求完整性、歧义发现、验收场景质量 |
| architect | 方案质量、权衡完整性、符合性 |
| implementer | 测试通过率、评审通过率、返工率、成本 |
| reviewer | 缺陷发现率、误报率、评审延迟 |
| tester | 覆盖、holdout 通过、flaky 率 |
| release_manager | 发布证据完整性、回滚成功率 |
| operator | 事故响应时间、MTTR、误操作率 |

标准由领域负责人维护；绑定必须满足标准。

### 5.2 工具权限

- 默认只读；写工具需要显式授权。
- 工具权限按项目、仓库、环境和动作范围授权。
- 不可逆工具（部署、删除、发布）需要人类批准或策略授权。
- 工具调用审计；异常调用告警。
- 工具 schema 变化影响评测；绑定固定版本或范围。

## 6. 评测

### 6.1 评测类型

| 类型 | 目的 |
|---|---|
| 基准 | 与已知基线比较 |
| 回归 | 防止新版本退化 |
| holdout | 防止过拟合 |
| 对抗 | 提示词注入、越权、危险操作 |
| 安全 | 密钥、数据、权限、供应链 |
| 成本 | 预算和效率 |
| 一致性 | 相同输入多次输出稳定 |

### 6.2 评分

- 每个用例有 rubric；评分可以是自动、人工或混合。
- 自动评分必须可复现；人工评分有评审记录。
- 分数不能只看最终答案；要看过程、证据和安全性。
- 失败分类：理解、规划、工具、代码、测试、环境、安全、成本。
- 评测结果进入 AgentVersion 的评测记录；绑定引用具体运行。

### 6.3 持续评测

- Canary：小流量任务使用新版本，对比旧版本。
- Shadow：新版本执行但不产生副作用。
- A/B：按任务分流，比较质量和成本。
- 漂移：输入分布、模型行为或工具变化导致质量下降时告警。
- 线上失败可以回流为评测用例；避免只评测离线样本。

## 7. 观测

### 7.1 AgentTrajectory

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "task_id": "uuid",
  "agent_version_id": "uuid",
  "started_at": "...",
  "finished_at": "...",
  "steps": [
    {"type": "model", "model": "...", "tokens": 1200, "cost": 0.02},
    {"type": "tool", "tool": "git.create_branch", "status": "ok", "duration_ms": 120},
    {"type": "tool", "tool": "test.run", "status": "failed", "duration_ms": 30000}
  ],
  "outcome": "succeeded|failed|cancelled|human_takeover",
  "failure_category": "test|tool|context|model|environment|safety",
  "human_takeover": false,
  "cost": {"tokens": 18234, "usd": 0.42}
}
```

规则：

- 轨迹记录模型调用、工具调用、成本、延迟和结果；不记录密钥和敏感内容。
- 轨迹可以用于回放和评测；回放不产生副作用。
- 失败分类用于改进提示词、工具和评测。
- 人工接管记录触发原因和接管者。
- 轨迹保留策略按项目；敏感任务脱敏。

### 7.2 指标

- 任务成功率、人工接管率、返工率。
- token/费用、延迟、工具调用次数。
- 失败分类分布、安全事件、越权尝试。
- 绑定覆盖率、评测通过率、漂移告警。
- 这些指标进入 RFC 0016 的度量体系。

## 8. 运行时与绑定

- Runtime 在沙箱中执行；工具通过工具网关调用。
- 上下文由受控的 Artifact/需求/架构/证据组装；不能随意读取未授权内容。
- 记忆作用域：task、project、agent；保留和清除策略明确。
- 长任务支持 checkpoint/resume；取消和超时强制生效。
- 任务分配按绑定、并发、预算和技能匹配；不能超过绑定范围。
- 运行时固定 AgentVersion、WorkflowVersion、模型、提示词、工具和配置版本。

## 9. 多 Agent 协作（后续）

设计边界：

- 角色明确：每个 Agent 有工作流角色和绑定。
- 交接物是 Artifact/Evidence，不是隐式对话状态。
- 委派有上限：深度、并发、成本、时间。
- 冲突解决：人类或指定协调者；不能无限互相修改。
- 共享工作区需要锁/租约；避免并发写冲突。
- 多 Agent 不能绕过权限和门禁；每个 Agent 独立授权。

第一期不实现多 Agent 自主编排；先保证单 Agent 可评测、可观测、可回退。

## 10. 安全

- 最小权限：工具、项目、网络、密钥按绑定授权。
- 密钥隔离：不进入提示词、轨迹、日志或产物。
- 提示词注入：外部内容视为数据；关键动作必须经过结构化门禁。
- 输出清洗：工具输出、代码和命令校验；拒绝危险操作。
- 权限提升：Agent 不能修改自己的绑定、工具权限或评测标准。
- 供应链：模型、工具、镜像、依赖进入扫描和 SBOM。
- 审计：定义、版本、绑定、评测、工具调用、轨迹和降级可追溯。
- 成本：预算、配额、超限拒绝或降级。

## 11. 数据模型

```sql
create table agent_definitions (
  id            uuid primary key,
  name          text not null,
  owner         jsonb not null,
  account_id    uuid not null,
  description   text,
  capabilities  jsonb not null default '[]'::jsonb,
  default_version integer,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table agent_versions (
  id            uuid primary key,
  definition_id uuid not null references agent_definitions (id),
  version       integer not null,
  model_config_id uuid not null,
  prompt_version_id uuid not null,
  tools         jsonb not null default '[]'::jsonb,
  limits        jsonb not null,
  policies      jsonb not null,
  memory        jsonb not null default '{}'::jsonb,
  runtime       jsonb not null default '{}'::jsonb,
  evaluation    jsonb not null default '{}'::jsonb,
  created_by    jsonb not null,
  created_at    timestamptz not null default now(),
  unique (definition_id, version)
);

create table model_configs (
  id            uuid primary key,
  provider      text not null,
  model         text not null,
  params        jsonb not null default '{}'::jsonb,
  fallbacks     jsonb not null default '[]'::jsonb,
  cost          jsonb not null default '{}'::jsonb,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table prompt_versions (
  id            uuid primary key,
  name          text not null,
  version       integer not null,
  system_prompt_ref jsonb not null,
  task_templates jsonb not null default '{}'::jsonb,
  variables     jsonb not null default '[]'::jsonb,
  evaluation    jsonb not null default '{}'::jsonb,
  created_at    timestamptz not null default now(),
  unique (name, version)
);

create table tool_definitions (
  id            uuid primary key,
  name          text not null,
  type          text not null,
  description   text,
  input_schema  jsonb not null,
  output_schema jsonb not null,
  permissions   jsonb not null,
  cost          jsonb not null default '{}'::jsonb,
  determinism   text not null,
  timeout_seconds integer not null,
  owner         jsonb not null,
  status        text not null,
  created_at    timestamptz not null default now(),
  unique (name)
);

create table agent_bindings (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  agent_version_id uuid not null references agent_versions (id),
  workflow_id   text not null,
  role          text not null,
  status        text not null,
  evaluation_run_id uuid,
  approved_by   jsonb,
  scope         jsonb not null,
  created_at    timestamptz not null default now(),
  unique (project_id, agent_version_id, workflow_id, role)
);

create table evaluation_suites (
  id            uuid primary key,
  name          text not null,
  version       integer not null,
  cases         jsonb not null,
  metrics       jsonb not null,
  thresholds    jsonb not null,
  owner         jsonb not null,
  created_at    timestamptz not null default now(),
  unique (name, version)
);

create table evaluation_runs (
  id            uuid primary key,
  suite_id      uuid not null references evaluation_suites (id),
  agent_version_id uuid not null references agent_versions (id),
  environment   text not null,
  results       jsonb not null default '[]'::jsonb,
  summary       jsonb not null default '{}'::jsonb,
  status        text not null,
  started_at    timestamptz not null,
  finished_at   timestamptz
);

create table agent_trajectories (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  task_id       uuid not null,
  agent_version_id uuid not null references agent_versions (id),
  steps         jsonb not null default '[]'::jsonb,
  outcome       text not null,
  failure_category text,
  human_takeover boolean not null default false,
  cost          jsonb not null default '{}'::jsonb,
  started_at    timestamptz not null,
  finished_at   timestamptz
);
```

## 12. API、MCP、CLI、Web

### 12.1 REST

```text
GET/POST /api/v1/agent-definitions
POST     /api/v1/agent-definitions/{id}/versions
GET      /api/v1/agent-definitions/{id}/versions
POST     /api/v1/agent-versions/{id}/evaluate
GET      /api/v1/agent-versions/{id}/evaluations
GET/POST /api/v1/projects/{project}/agent-bindings
POST     /api/v1/agent-bindings/{id}/revoke
GET/POST /api/v1/tools
GET      /api/v1/tools/{id}
GET      /api/v1/agent-trajectories?task=
GET/POST /api/v1/evaluation-suites
```

### 12.2 MCP

- `agent.definition.get` / `agent.version.get`
- `agent.binding.list`
- `agent.trajectory.get`
- `evaluation.run` / `evaluation.result`
- `tool.list` / `tool.permissions`

Agent 不能调用修改定义、绑定、工具权限或评测标准的接口。

### 12.3 CLI

```bash
foundry agent definition list
foundry agent version new <definition> --model <config> --prompt <version>
foundry agent evaluate <version> --suite <suite>
foundry agent bind <version> --project <p> --workflow iteration --role implementer
foundry agent binding revoke <binding>
foundry agent trajectory <task>
foundry tool list
```

### 12.4 Web

- Agent 定义页：版本、模型、提示词、工具、限制、策略。
- 评测页：套件、运行、分数、失败分类、holdout。
- 绑定页：项目、工作流、角色、范围、状态。
- 轨迹页：步骤、工具调用、模型调用、成本、失败分类、人工接管。
- 工具页：schema、权限、成本、超时、所有者。

## 13. 与现有实现的关系

- 现有 `AccountKind::Agent` 继续作为身份；本 RFC 增加 AgentDefinition/AgentVersion/Binding。
- 现有 Workflow role 通过 AgentBinding 绑定 Agent；不修改 Workflow AST。
- 现有 `api-mcp` 入站工具继续使用；工具网关是出站能力，二者分离。
- 现有 Policy 扩展为工具/动作授权；DemoPolicy 先支持最小规则。
- 现有 Task 固定 workflow 版本；本 RFC 增加固定 AgentVersion。
- 现有事件日志记录定义、版本、绑定、评测、轨迹和降级。

## 14. 实施计划

### 阶段 0：定义与版本

- AgentDefinition/AgentVersion/ModelConfig/PromptVersion/ToolDefinition 表。
- 最小绑定和运行时固定版本。
- 出口标准：Agent 可定义、可版本化、可运行。

### 阶段 1：工具网关与轨迹

- 工具授权、调用记录、轨迹、成本。
- 出口标准：工具调用可审计；轨迹可回放。

### 阶段 2：评测

- 评测套件、运行、评分、holdout、晋升门禁。
- 出口标准：未通过评测的版本不能绑定关键角色。

### 阶段 3：持续评测与降级

- Canary/shadow/A-B、漂移监控、自动降级/回退。
- 出口标准：线上质量下降能发现并回退。

### 阶段 4：多 Agent 协作

- 委派、交接、共享工作区、冲突解决。
- 出口标准：多 Agent 任务可追踪、可审计、不越权。

## 15. 验收标准

1. Agent 定义和版本可创建、查询、固定；历史任务保留原版本。
2. 未通过评测的版本不能绑定关键角色。
3. 绑定有项目、工作流、角色、范围和批准。
4. 工具调用经过授权和记录；未授权工具不可调用。
5. 轨迹记录模型/工具调用、成本、延迟、结果和失败分类；密钥不出现。
6. 评测套件版本化、holdout 隔离、评分可复现。
7. 降级/回退生效；新任务不再分配，历史可查。
8. Agent 不能修改自己的定义、绑定、工具权限或评测标准。
9. 自举任务可以追溯到 AgentVersion、WorkflowVersion、模型和提示词版本。
10. 多 Agent 协作（后续）有明确角色、交接物和冲突解决。

## 16. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 评测成本高 | 每个版本都要跑套件 | 分层评测：冒烟、回归、全量 |
| 过拟合 | 针对评测优化 | holdout、线上回流、对抗用例 |
| 版本爆炸 | 提示词/工具频繁变 | 语义版本、合并变更、固定范围 |
| 工具权限过大 | Agent 可写生产 | 默认只读、最小权限、不可逆需批准 |
| 轨迹敏感 | 轨迹含代码/数据 | 脱敏、保留策略、访问控制 |
| 漂移 | 模型升级导致行为变化 | 持续评测、canary、自动回退 |
| 多 Agent 混乱 | 互相修改、责任不清 | 角色、交接物、上限、协调者 |
| 自举偏差 | 只评测容易任务 | holdout、人工抽查、外部基准 |

**待决问题**：

1. 关键角色的评测阈值由谁定？建议领域负责人 + 质量负责人。
2. 是否允许 Agent 定义跨项目复用？建议平台定义可复用，绑定按项目。
3. 轨迹保留多久？建议按项目合规策略，默认 90 天，敏感任务脱敏。
4. 自动降级的阈值？建议成功率/安全/成本三组阈值，先告警后降级。
5. 多 Agent 协作何时进入？建议 L3 之后，先保证单 Agent 闭环。
