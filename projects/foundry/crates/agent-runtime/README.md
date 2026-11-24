# foundry-agent-runtime

Foundry 的 Agent 运行时。图执行、模型消息协议、模型 Provider、工具契约和 LLM 工具循环都由 `graph-agent` 提供；Foundry 只保留 Agent 定义、提示词、Foundry 工具和业务集成。

## 结构

```text
AgentRuntime
  ├─ graph-agent Agent/Graph    一次 Run = 一个 LLM 节点
  ├─ ModelGateway               模型调用（graph-agent 定义，OpenAI 兼容 / Scripted）
  ├─ Foundry ToolRegistry       Foundry 工具实现
  ├─ graph_tools                把 Foundry 工具适配进 graph-agent
  └─ ContextBundle              显式上下文，渲染进用户消息
```

`AgentRuntime::run` 把系统提示词和用户消息交给 graph-agent 的 LLM 节点，返回 `RunResult { final_message }`。模型返回工具调用时，由 graph-agent 执行工具并把结果继续送回模型，直到模型给出最终答案。

Foundry 工具的 `spec()` 直接使用 graph-agent 的 `ToolDefinition`。`adapt_registry` 把整个 Foundry `ToolRegistry` 包装成 graph-agent 的注册表，因此固定图节点和 LLM 节点使用同一套工具。工具失败以错误码和消息回传给模型。

## 当前工具面

Agent 定义新增 `shell` 字段。`shell = true` 时，agentd 为该 Agent 创建并持有一个常驻 shell，跨任务复用，进程退出时关闭；`shell = false` 时不创建。

当前 agentd 只注册两个工具：

- `project`：`action = get`；
- `shell`：执行一条命令。

`issue`、`task`、`agent`、`requirements`、`workspace`、`document`、`human_ask` 等工具暂时对所有 Agent 禁用，实现保留在 crate 中，后续再决定是否恢复。

## CodeSummary

`codesummary` 模块是一个固定图：

```text
prepare
  → select_item (arche status --porcelain --detail)
      → 无 sections → summarize → review_file → apply_file
                                  → review_dir  → apply_dir
      → 有 sections → fix ────────→ review_file / review_dir
      → 没有工作项 → review_all → verify
                           ↳ 不通过 → apply_all → unstage → select_item
  → verify → commit/push
```

- `prepare` 在常驻 shell 里 clone/fetch/checkout、执行 `arche source sync` 生成 Directory/File 对象；待处理项由 `select_item` 从 `arche status --porcelain --detail` 读取；
- 文件/目录总结由 `summarize` LLM 节点通过 `shell` 工具完成：用 `arche create` 创建对象，用 `arche doc` 写设计字段，不直接返回设计文本；
- 修改由独立的 `fix` LLM 节点完成：Review 不通过时只告诉 `fix` “未通过，请查看评论列表”；`fix` 自己找到当前工作项相关文档的 open 评论，重新核对代码和设计后修改文档或对象。Review 输出的 Section 只用于内部路由，不进入 `fix` 提示词；
- CodeSummary 没有按节点加载不同技能的能力；`arche`、`design-writing`、`code-summary` 作为共享技能提供给所有阶段，当前阶段提示词追加在共享技能之后；
- `review` 节点用 `arche status --porcelain --detail` / `arche diff` / `arche show` 检查 worktree；阻塞问题写成对应对象的 `arche comment add`，JSON 只返回 `passed` 和 `sections` 路由。`apply` 节点通过则 `arche add --file` / `arche add --directory`，不通过则进入 `fix`；进入 `fix` 的提示词只说明未通过并查看评论，不传递 Review 的问题正文；
- `review_all` 用 `arche diff --staged` / `arche status --porcelain --detail` 检查已暂存内容，阻塞问题同样写成评论；它返回多个文件/目录，不直接进入单工作项的 `fix`。不通过时 `apply_all` 用 `arche unstage` 退回问题文件/目录，并把对象列表交给 `select_item`，由 `select_item` 分发到 `fix`；
- `verify` 执行 `arche check`、`arche doc check`、`arche structure check`，通过后 commit/push；失败则流程失败；
- 关系允许前向引用，最终由 `arche check` 解析。

Agent 定义 `shell = true`，工具面为 `project` + `shell`，技能为 `arche`、
`design-writing` 和 `code-summary`。CodeSummary graph 由 `code_summary_group:summarizer` 使用。

### 组模式

`code_summary_group` 使用同一套 CodeSummary graph，但任务 input 包含：

```json
{
  "directories": ["src"],
  "files": ["src/a.rs"],
  "target_branch": "design-consistency/<root>",
  "source_git_commit": "<git-sha>"
}
```

组模式只处理 `files` 中的文件，不执行全仓 `source sync`；从集成分支创建组分支，提交后推送组分支，并用 `merge_request` 工具创建或刷新关联当前任务的 DS MR。`design-consistency` 技能描述组审查、自动合入、冲突解决和集成流程。

## 人工交互

没有 checkpoint、暂停和恢复。澄清时 Agent 通过工具在统一 Issue 下发表评论，然后正常结束本轮；人类回复后由 control-plane 触发新的 Run，新 Run 重新读取 Issue 继续。

- Agent 通过 `task_links_list` 找到 Task 上的澄清 Issue。
- Agent 用 `issue_get` 读取讨论，用 `issue_comment` 提问。
- Agent 创建服务端草稿，用文档工具编辑，再用 `task_operation` 提交。
- Agent 不能批准；审批由人类完成。

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

# 同时接入 Foundry 接口和本地沙箱
FOUNDRY_URL=http://127.0.0.1:8080 \
FOUNDRY_TOKEN=<token> \
FOUNDRY_PROJECT=<uuid> \
FOUNDRY_SANDBOX_ROOT=/path/to/workspace \
cargo run -p foundry-agent-runtime --bin foundry-agent -- run \
  --base-url http://127.0.0.1:11434/v1 --model qwen2.5-coder \
  --api-key local-key --message "分析需求并检查代码"

# 项目澄清
FOUNDRY_URL=http://127.0.0.1:8080 \
FOUNDRY_TOKEN=<token> \
FOUNDRY_PROJECT=<uuid> \
FOUNDRY_TASK=<task-uuid> \
cargo run -p foundry-agent-runtime --bin foundry-agent -- clarify \
  --base-url http://127.0.0.1:11434/v1 --model qwen2.5-coder \
  --api-key local-key --message "帮我澄清这个项目的目标"
```

## 组成

- `AgentRuntime` / `AgentRunRequest` / `RunResult`：一次 Agent 执行。
- `Tool` / `ToolRegistry` / `ToolContext` / `ToolSpec`：Foundry 工具契约。
- `graph_tools`：Foundry 工具到 graph-agent 工具系统的适配。
- `ContextBundle`：按 kind/source 引用并渲染进用户消息的上下文。
- `FoundryClient`：需求、项目、Anchor 文档、Issue 等 Foundry HTTP 接口。
- MCP 客户端、本地沙箱、评测、提示词、技能。
- `services/agentd`：每个智能体一个常驻进程；执行只写日志，不写运行历史。

## 测试

```bash
cargo test -p foundry-agent-runtime
```

包含工具适配、LLM 工具循环、上下文渲染、工具错误回传、评测、沙箱、MCP 等测试。
