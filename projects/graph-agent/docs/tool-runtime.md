# graph-agent 工具系统

- 状态：核心、Foundry 适配、LLM tool loop 和 OpenAI 兼容 provider 已实现
- 日期：2026-11-16
- 适用范围：`graph-agent`、`foundry-agent-runtime`、CodeSummary

## 1. 目标

1. 固定节点和 LLM 节点使用同一套工具；
2. 固定节点可以 async 调用工具，不依赖模型；
3. LLM 节点只拿到允许暴露的工具定义；
4. 工具错误有稳定的机器码；
5. 凭据、容器句柄不进入模型上下文。

## 2. 核心类型

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}
```

```rust
pub enum ToolOutput {
    Text(String),
    Json(Value),
}
```

`Text` 只用于文本；结构化结果必须用 `Json`，不允许在文本里塞 JSON。

```rust
pub struct ToolError {
    pub code: u32,
    pub message: String,
}
```

- `code`：给代码看的稳定标识；
- `message`：给人/Agent 看的说明；
- 公共 code：`1..=999`；
- 工具自己的 code：从 `1000` 开始；
- 工具 code 只在 `(tool, code)` 组合内有意义，不要求全局唯一。

```rust
pub struct ToolContext {
    pub run_id: String,
    pub agent_id: Option<String>,
    pub task_id: Option<String>,
    pub project_id: Option<String>,
}
```

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    async fn call(
        &self,
        arguments: Value,
        context: &ToolContext,
    ) -> Result<ToolOutput, ToolError>;
}
```

## 3. Registry

```rust
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), RegistryError>;
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>>;
    pub fn names(&self) -> Vec<String>;
    pub fn definition(&self, name: &str) -> Result<ToolDefinition, ToolError>;
    pub fn definitions(&self, names: &[String]) -> Result<Vec<ToolDefinition>, ToolError>;
    pub async fn call(
        &self,
        name: &str,
        arguments: Value,
        context: &ToolContext,
    ) -> Result<ToolOutput, ToolError>;
}
```

规则：

- 重复注册返回 `RegistryError`，不静默覆盖；
- 未知工具返回 `ToolError { code: codes::UNKNOWN_TOOL, .. }`；
- 工具错误原样返回，不伪装成成功。

## 4. Context

`Context` 持有：

```rust
pub fn set_tool_registry(&mut self, registry: Arc<ToolRegistry>);
pub fn tool_registry(&self) -> Result<&Arc<ToolRegistry>, ContextError>;
pub fn set_tool_context(&mut self, context: ToolContext);
pub fn tool_context(&self) -> &ToolContext;
```

`LLM 节点和固定节点都通过 Context 取同一个 registry。

`Agent::run` 会把 `LlmRuntime.tools` 安装到 Context。

## 5. LLM 节点

LLM 请求使用 provider-neutral 的消息：

```rust
pub enum LlmRole { System, User, Assistant, Tool }

pub struct LlmToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

pub struct LlmMessage {
    pub role: LlmRole,
    pub content: String,
    pub tool_calls: Vec<LlmToolCall>,
    pub tool_call_id: Option<String>,
}

pub struct LlmRequest {
    pub model: ModelProfileRef,
    pub params: LlmParams,
    pub messages: Vec<LlmMessage>,
    pub tools: Vec<ToolDefinition>,
}

pub struct LlmResponse {
    pub message: LlmMessage,
    pub usage: Usage,
}
```

LLM 节点执行循环：

```text
system + user
  → model
  → 有 tool calls
      → 逐个调用 ToolRegistry
      → tool result 追加到 messages
      → model
  → 没有 tool calls
      → 当前 content 作为节点输出
```

规则：

- `LlmConfig.tools: Vec<String>` 声明本节点可见的工具名；
- 模型调用未启用的工具，返回 `tool.denied` 错误结果，不执行；
- 工具错误以固定 JSON 回给模型；
- 没有 max_steps；循环在模型不再返回 tool calls 时结束；
- 节点最终输出是 `Value::String(final_content)`。

## 6. 固定节点

```rust
let output = context
    .tool_registry()?
    .call("ds_clone", args, context.tool_context())
    .await?;
```

## 7. 错误输出

固定节点拿到 `Err(ToolError)`。

LLM 节点看到固定结构：

```json
{
  "tool": "git_clone",
  "error": {
    "code": 1000,
    "message": "repository not found"
  }
}
```

## 8. Foundry 适配

`foundry-agent-runtime::graph_tools` 提供：

```rust
pub fn adapt_registry(
    registry: &foundry_agent_runtime::ToolRegistry,
) -> Result<graph_agent::ToolRegistry, graph_agent::RegistryError>;
```

- 每个 Foundry `Tool` 包装成一个 graph-agent `Tool`；
- Foundry 的文本输出映射为 `ToolOutput::Text`；
- Foundry 错误映射到 graph-agent 的整型 code；
- 工具系统不包含暂停。

## 9. 暂停

工具系统不包含暂停。

“需要人类回复”不是工具语义，而是 Agent turn 的结束：智能体发一条 Issue 评论，然后正常退出本轮。人类回复作为新的输入消息触发下一轮。没有 checkpoint/resume。

## 10. 待做

- 工具超时和取消。
