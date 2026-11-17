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
  → select_item (ds status --porcelain)
      → summarize → review_file → apply_file
      → review_dir → apply_dir
      → review_all → verify
                  ↳ 不通过 → apply_all → unstage → select_item
  → verify → commit/push
```

- `prepare` 在常驻 shell 里 clone/fetch/checkout、执行 `ds source sync`、删除失效 namespace；待处理项由 `select_item` 从 `ds status --porcelain` 读取；
- 文件/目录总结由同一个 LLM 节点通过 `shell` 工具完成，并直接用 `ds edit --design-file` 写入对象；
- `review` 节点用 `ds status` / `ds diff` 检查 worktree，`apply` 节点通过则 `ds add --file` / `--directory`，不通过则带反馈回到总结节点；
- `review_all` 用 `ds diff --staged` / `ds show --staged` 检查已暂存内容，不通过时 `apply_all` 用 `ds unstage` 退回问题文件/目录；
- `verify` 执行 `ds verify` 和源码 File 检查，通过后 commit/push；失败按源文件回到总结节点，重复签名判定无进展后失败；
- 关系允许前向引用，最终由 `ds verify` 解析。

Agent 定义 `shell = true`，工具面为 `project` + `shell`。CodeSummary 平台模板的角色是
`code_summary:summarizer`。

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
