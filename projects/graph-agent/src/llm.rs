use crate::message::{LlmMessage, LlmRole, Usage};
use crate::tool::{ToolDefinition, ToolOutput, ToolRegistry, codes};
use crate::{Context, Node, NodeError, PromptTemplate, TransitionIndex};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static TOOL_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// An LLM-backed node.
///
/// The node only provides its own system/user templates and transition logic.
/// Model, shared system prompt, tools and parameters come from
/// [`Context::current_llm_config`].
pub trait LlmNode: Send + Sync {
    fn system(&self) -> &PromptTemplate;
    fn user(&self) -> &PromptTemplate;

    /// Skill names resolved through [`SkillRegistry`].
    fn skills(&self) -> &[String] {
        &[]
    }

    fn to(&self, ctx: &Context) -> Result<TransitionIndex, NodeError>;
    fn transition_count(&self) -> usize;
}

/// Engine-level LLM dependencies.
pub struct LlmRuntime {
    pub models: Arc<dyn ModelGateway>,
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<dyn SkillRegistry>,
}

impl LlmRuntime {
    pub fn new(
        models: Arc<dyn ModelGateway>,
        tools: Arc<ToolRegistry>,
        skills: Arc<dyn SkillRegistry>,
    ) -> Self {
        Self {
            models,
            tools,
            skills,
        }
    }
}

/// Per-node LLM configuration supplied through the context.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub model: ModelProfileRef,
    pub shared_system: PromptTemplate,
    pub tools: Vec<String>,
    pub params: LlmParams,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelProfileRef(String);

impl ModelProfileRef {
    pub fn new(model: impl Into<String>) -> Self {
        Self(model.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct LlmParams {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout: Option<Duration>,
    /// Compress shared history before the request when it exceeds this budget.
    pub history_token_budget: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub model: ModelProfileRef,
    pub params: LlmParams,
    pub messages: Vec<LlmMessage>,
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub message: LlmMessage,
    pub usage: Usage,
}

impl LlmResponse {
    pub fn new(message: LlmMessage) -> Self {
        Self {
            message,
            usage: Usage::default(),
        }
    }

    pub fn final_answer(content: impl Into<String>) -> Self {
        Self::new(LlmMessage::assistant(content, Vec::new()))
    }

    pub fn tool_call(name: impl Into<String>, arguments: serde_json::Value) -> Self {
        Self::new(LlmMessage::assistant(
            "",
            vec![crate::message::LlmToolCall {
                id: format!("call-{}", TOOL_CALL_COUNTER.fetch_add(1, Ordering::Relaxed)),
                name: name.into(),
                arguments,
            }],
        ))
    }

    pub fn tool_calls(tool_calls: Vec<crate::message::LlmToolCall>) -> Self {
        Self::new(LlmMessage::assistant("", tool_calls))
    }

    pub fn with_usage(mut self, usage: Usage) -> Self {
        self.usage = usage;
        self
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.message.content = content.into();
        self
    }
}

#[async_trait]
pub trait ModelGateway: Send + Sync {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, ModelError>;
}

/// External skill registry. Skill names in an LLM node are resolved here.
pub trait SkillRegistry: Send + Sync {
    fn prompt(&self, id: &str) -> Result<Option<String>, SkillError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("scripted model has no responses left")]
    Exhausted,
    #[error("model backend error: {0}")]
    Backend(String),
    #[error("invalid model response: {0}")]
    InvalidResponse(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("skill error: {0}")]
    Failed(String),
}

pub(crate) struct LlmNodeAdapter<N> {
    inner: N,
}

impl<N> LlmNodeAdapter<N> {
    pub(crate) fn new(inner: N) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<N: LlmNode + 'static> Node for LlmNodeAdapter<N> {
    async fn run(&self, input: Value, ctx: &mut Context) -> Result<Value, NodeError> {
        let runtime = ctx.llm_runtime()?.clone();
        let (shared_system, model, params, tools) = {
            let config = ctx.current_llm_config()?;
            (
                config.shared_system.clone(),
                config.model.clone(),
                config.params.clone(),
                config.tools.clone(),
            )
        };

        let shared_system = shared_system.render(ctx)?;
        let node_system = self.inner.system().render(ctx)?;
        let fixed_user = self.inner.user().render(ctx)?;
        let system = join_system(shared_system, node_system);
        let system = append_skills(system, runtime.skills.as_ref(), self.inner.skills())?;
        let tools = runtime
            .tools
            .definitions(&tools)
            .map_err(|error| NodeError::failed(error.to_string()))?;
        let allowed = tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<HashSet<_>>();

        let input_text = render_input(&input);
        let fixed_tokens = estimate_tokens(&system) + estimate_tokens(&fixed_user);
        compress_history(
            &runtime,
            &model,
            &params,
            fixed_tokens,
            estimate_tokens(&input_text),
            ctx,
        )
        .await?;
        let input_already = !input_text.is_empty()
            && ctx.history().messages().last().is_some_and(|message| {
                message.role == LlmRole::User && message.content == input_text
            });
        let mut input_recorded = input_text.is_empty() || input_already;

        let mut messages = Vec::new();
        if !system.is_empty() {
            messages.push(LlmMessage::system(system));
        }
        if !fixed_user.is_empty() {
            messages.push(LlmMessage::user(fixed_user));
        }
        messages.extend(ctx.history().messages().iter().cloned());
        if !input_recorded {
            messages.push(LlmMessage::user(input_text.clone()));
        }

        loop {
            let response = runtime
                .models
                .complete(LlmRequest {
                    model: model.clone(),
                    params: params.clone(),
                    messages: messages.clone(),
                    tools: tools.clone(),
                })
                .await
                .map_err(|error| NodeError::failed(error.to_string()))?;
            let message = response.message;
            if message.tool_calls.is_empty() {
                // The node does not write its output to History. The next node
                // or the graph framework decides what to record.
                return Ok(Value::String(message.content));
            }

            if !input_recorded {
                ctx.history_mut().push(LlmMessage::user(input_text.clone()));
                input_recorded = true;
            }
            ctx.history_mut().push(message.clone());
            messages.push(message.clone());
            let registry = ctx.tool_registry()?.clone();
            let tool_context = ctx.tool_context().clone();
            for call in message.tool_calls {
                let content = if !allowed.contains(&call.name) {
                    tool_error_json(
                        &call.name,
                        codes::DENIED,
                        "tool is not enabled for this node",
                    )
                } else {
                    match registry
                        .call(&call.name, call.arguments, &tool_context)
                        .await
                    {
                        Ok(ToolOutput::Text(text)) => text,
                        Ok(ToolOutput::Json(value)) => {
                            serde_json::to_string(&value).unwrap_or_else(|_| "null".to_owned())
                        }
                        Err(error) => tool_error_json(&call.name, error.code, &error.message),
                    }
                };
                let tool_message = LlmMessage::tool(call.id, content);
                ctx.history_mut().push(tool_message.clone());
                messages.push(tool_message);
            }
        }
    }

    fn to(&self, ctx: &Context) -> Result<TransitionIndex, NodeError> {
        self.inner.to(ctx)
    }

    fn transition_count(&self) -> usize {
        self.inner.transition_count()
    }
}

async fn compress_history(
    runtime: &LlmRuntime,
    model: &ModelProfileRef,
    params: &LlmParams,
    fixed_tokens: usize,
    input_tokens: usize,
    ctx: &mut Context,
) -> Result<(), NodeError> {
    let Some(budget) = params.history_token_budget else {
        return Ok(());
    };
    let history_tokens = estimate_history_tokens(ctx.history().messages());
    if fixed_tokens + input_tokens + history_tokens <= budget {
        return Ok(());
    }

    let keep_budget = (budget / 4).max(1);
    let keep_from = keep_start(ctx.history().messages(), keep_budget);
    if keep_from == 0 {
        return Ok(());
    }

    let older = &ctx.history().messages()[..keep_from];
    let transcript = render_transcript(older);
    let summary = match summarize_history(runtime, model, &transcript).await {
        Ok(summary) => summary,
        Err(_) => format!("[{} earlier messages omitted]", older.len()),
    };
    ctx.history_mut().replace_with_summary(keep_from, summary);
    Ok(())
}

async fn summarize_history(
    runtime: &LlmRuntime,
    model: &ModelProfileRef,
    transcript: &str,
) -> Result<String, ModelError> {
    let response = runtime
        .models
        .complete(LlmRequest {
            model: model.clone(),
            params: LlmParams::default(),
            messages: vec![
                LlmMessage::system(
                    "Compress the agent conversation. Keep facts, decisions, file paths, \
                     object names, completed work, and unresolved problems. Do not invent. \
                     Reply with the summary text only.",
                ),
                LlmMessage::user(transcript),
            ],
            tools: Vec::new(),
        })
        .await?;
    Ok(response.message.content)
}

fn estimate_history_tokens(messages: &[LlmMessage]) -> usize {
    messages
        .iter()
        .map(|message| {
            let tool_calls = serde_json::to_string(&message.tool_calls).unwrap_or_default();
            estimate_tokens(&message.content) + estimate_tokens(&tool_calls)
        })
        .sum()
}

fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / 4 + 1
}

fn keep_start(messages: &[LlmMessage], keep_budget: usize) -> usize {
    let mut tokens = 0;
    let mut start = messages.len();
    for (index, message) in messages.iter().enumerate().rev() {
        tokens += estimate_tokens(&message.content);
        if tokens > keep_budget {
            start = index + 1;
            break;
        }
        start = index;
    }
    while start < messages.len() && messages[start].role == LlmRole::Tool {
        start += 1;
    }
    start
}

fn render_transcript(messages: &[LlmMessage]) -> String {
    let mut out = String::new();
    for message in messages {
        let role = match message.role {
            LlmRole::System => "system",
            LlmRole::User => "user",
            LlmRole::Assistant => "assistant",
            LlmRole::Tool => "tool",
        };
        out.push('[');
        out.push_str(role);
        out.push_str("]\n");
        if !message.content.is_empty() {
            out.push_str(&message.content);
            out.push('\n');
        }
        if !message.tool_calls.is_empty() {
            out.push_str("tool calls: ");
            out.push_str(&serde_json::to_string(&message.tool_calls).unwrap_or_default());
            out.push('\n');
        }
    }
    out
}

fn render_input(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn tool_error_json(tool: &str, code: u32, message: &str) -> String {
    serde_json::json!({
        "tool": tool,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

fn join_system(shared: String, node: String) -> String {
    match (shared.is_empty(), node.is_empty()) {
        (true, true) => String::new(),
        (true, false) => node,
        (false, true) => shared,
        (false, false) => format!("{shared}\n\n{node}"),
    }
}

fn append_skills(
    mut system: String,
    registry: &dyn SkillRegistry,
    names: &[String],
) -> Result<String, NodeError> {
    let mut seen = HashSet::new();
    let mut skills = Vec::new();

    for name in names {
        if !seen.insert(name.as_str()) {
            continue;
        }

        let prompt = registry
            .prompt(name)
            .map_err(|error| NodeError::failed(error.to_string()))?
            .ok_or_else(|| NodeError::failed(format!("unknown skill: {name}")))?;
        skills.push((name, prompt));
    }

    if skills.is_empty() {
        return Ok(system);
    }

    if !system.is_empty() {
        system.push_str("\n\n");
    }
    system.push_str("## Skills\n");

    for (name, prompt) in skills {
        system.push_str("\n### ");
        system.push_str(name);
        system.push_str("\n\n");
        system.push_str(prompt.trim());
        system.push('\n');
    }

    Ok(system)
}
