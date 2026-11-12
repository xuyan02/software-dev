# foundry-agent-runtime

Foundry 自己的 Agent 运行时。核心循环、模型网关、工具网关、上下文、轨迹、预算、取消、沙箱、评测、瞬时通知和 Agent 进程都已落地。

## 设计

```text
AgentRuntime
  ├─ ModelGateway    模型调用契约
  ├─ ToolRegistry    工具注册与执行
  ├─ ContextBundle   显式上下文，渲染进用户消息
  ├─ Budget          步数 / token / 工具调用 / 时间
  ├─ Trajectory      每一步的模型、工具、最终答案记录
  └─ Cancellation    取消信号
```

不依赖 Agent 框架。模型 provider、MCP、沙箱是薄适配器。

## 循环

```text
user/system + context
  → model
  → tool calls（逐个执行，错误回传给模型）
  → model
  → final answer
```

硬限制：最大步数、输入/输出 token、工具调用数、总时长。超过时返回 `StopReason::BudgetExceeded` / `MaxSteps`；取消返回 `Cancelled`。

## 运行

```bash
cargo test -p foundry-agent-runtime

# 脚本化 demo
cargo run -p foundry-agent-runtime --bin foundry-agent -- demo

# 真实 OpenAI 兼容端点
cargo run -p foundry-agent-runtime --bin foundry-agent -- run \
  --base-url http://127.0.0.1:11434/v1 \
  --model qwen2.5-coder \
  --api-key local-key \
  --message "查看需求文档并指出缺失项"

# 同时接入 Foundry 需求接口和本地沙箱
FOUNDRY_URL=http://127.0.0.1:8080 \
FOUNDRY_TOKEN=<token> \
FOUNDRY_PROJECT=<uuid> \
FOUNDRY_SANDBOX_ROOT=/path/to/workspace \
cargo run -p foundry-agent-runtime --bin foundry-agent -- run \
  --base-url http://127.0.0.1:11434/v1 --model qwen2.5-coder \
  --api-key local-key --message "分析需求并检查代码"

# 项目澄清：Agent 通过统一 Issue 与创建者讨论，起草并提交 ProjectBrief
FOUNDRY_URL=http://127.0.0.1:8080 \
FOUNDRY_TOKEN=<token> \
FOUNDRY_PROJECT=<uuid> \
FOUNDRY_TASK=<task-uuid> \
cargo run -p foundry-agent-runtime --bin foundry-agent -- clarify \
  --base-url http://127.0.0.1:11434/v1 --model qwen2.5-coder \
  --api-key local-key --message "帮我澄清这个项目的目标"
```

`clarify` 的交互方式：

- Agent 通过 `task_links_list` 找到 Task 上的统一澄清 Issue。
- Agent 用 `issue_get` 读取讨论，用 `issue_comment` 提问。
- 提问后 Agent 结束自己的本轮，Run 正常完成；人类回复后 control-plane 依据 Issue 评论触发新的 Run，新 Run 重新读取 Issue 继续。
- 人在 Issue 下回复；CLI 把回复作为评论发到 Issue，并恢复 Run。
- Agent 创建服务端草稿，用 `document` 的 `draft_create` / `add` / `update_text` 编辑，然后用 `task_operation` 提交；操作名使用 `task_allowed_operations` 返回的 `stage:operation` 形式，例如 `writing:completed`。
- Agent 不能批准；提交后 CLI 询问人类是否批准。

## 已实现

### Phase 0：循环骨架

- `ModelGateway` / `Message` / `ToolCall` / `Usage` / `ModelResponse`
- `Tool` / `ToolRegistry` / `ToolContext`
- `AgentRuntime` 循环、预算、取消、轨迹
- `ScriptedModel`
- `EchoTool`、`RequirementsGetTool`（静态桩）

### Phase 1：真实世界

- `OpenAiCompatibleModel`：OpenAI / DeepSeek / Ollama / vLLM，重试、超时、工具调用、usage。
- `FoundryClient` 和统一的 `document` 工具（`kind = "project"` / `"requirements"` / `"architecture"`）。项目文档、需求文档和架构设计都通过这个工具访问，不再有独立的 `project_brief_*` 工具。
- `McpClient`：initialize、tools/list、tools/call，stdio transport、JSON-RPC、错误处理。
- 轨迹使用内存 sink；管理型 Agent 不再持久化 `agent_runs` / `agent_steps`。
- `Pricing` / `Usage::cost_usd` / `RunResult.cost_usd`。
- CLI `demo` / `run` / `clarify`。

### 人类交互

- `HumanInteraction` / `human_ask`：通用同步问答工具（保留给调试和简单流程）。
- `task_links_list`：读取 Task 上的链接，找到统一澄清 Issue。
- `issue_get` / `issue_comment`：在 Issue 下讨论；评论后模型结束本轮，人类回复的通知会触发新的 Agent 执行。
- `RunOutcome`：`Finished` 或 `WaitingForIssueComment { pause, checkpoint }`。
- `AgentRuntime::resume(checkpoint, human_comments)`：通用库能力；管理型 Agent 的瞬时通知模型不使用。
- `AgentWorker`：库内参考实现（内存队列、checkpoint、评论轮询、恢复）；agentd 不使用。
- `services/agentd`：每个智能体一个常驻进程；控制平面 supervisor 负责启动/停止/重启和进程状态。
  - `PM Agent` 是平台调度者：Ready 任务进入派发队列，PM 为任务补齐每个 agent 角色（找不到就按模板创建），然后调用 `task_start` 把任务置为 `in_progress`；它不做任务本身。
  - 普通 Agent 没有 durable queue：启动、重连和收到 `foundry_task_changes`（含 `issue_commented`）时，扫描自己作为 active actor 的任务并自行决定是否执行；通知丢失不补发。
  - 执行前把 Task 摘要、角色绑定和 `allowed_operations` 注入 context，Agent 不需要先调用工具才知道当前可发哪些 operation。
  - `/agents` 只展示平台智能体和进程状态；用户不配置智能体。
  - 不再写 `agent_runs` / `agent_steps`，也没有 lease、checkpoint、运行历史或自动重试。
  - 运行和工具调用使用现有 `tracing` 日志输出；控制平面把 agentd 的 stdout/stderr 原样写入 `~/.foundry/logs/agents/<id>.log`，管理员在网页 `/agents/{id}/logs` 查看纯文本日志。
- `services/worker` 已弃用，运行时会直接报错退出。
- 澄清 Issue 由 `goal_clarification` 的 `task.created` action 创建；`clarifier` 是 agent 角色，issue 评论通知会唤醒它。
- `FoundryIssueCommentSource`：从 Issue 读取 `after` 之后的人类评论。
- 审批与回答分离：Agent 只能评论、起草和提交 ProjectBrief，人类单独批准。

### Phase 2/3：沙箱与评测（部分）

- `LocalSandbox`：`sandbox_run_command`，工作区根、cwd 逃逸检查、超时、输出截断。
- `EvaluationCase` / `EvaluationReport` / `evaluate_case` / `evaluate_suite`。

## 测试

- 15 个单元测试：provider 解析、Foundry 工具、MCP、沙箱、评测、人类提问、Worker 暂停/恢复。
- 10 个运行时集成测试。
- Postgres 流程、Agent 分配和目标澄清流程集成测试（无 `TEST_DATABASE_URL` 时跳过）。

## 后续

- AgentDefinition / AgentVersion / AgentBinding / 评测晋升。
- Docker/VM 沙箱（替换本地子进程）。
- 策略、审批、能力令牌、证据回传。
- HTTP MCP transport。
- 上下文 token 预算和检索。
- 轨迹回放和多 Agent 编排。
