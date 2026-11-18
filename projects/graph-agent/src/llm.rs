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
/// The node provides its system prompt, its user prompt and transition logic.
/// The user prompt must describe the node's JSON output format and may use
/// `{{input}}` / `{{input.field}}` to render the previous node's JSON. The
/// rendered user prompt is recorded in History; raw input JSON is not appended.
/// The adapter parses the final reply as JSON and returns it as the node output.
pub trait LlmNode: Send + Sync {
    fn system(&self) -> &PromptTemplate;
    fn user(&self) -> &PromptTemplate;

    /// Skill names resolved through [`SkillRegistry`].
    fn skills(&self) -> &[String] {
        &[]
    }

    fn to(&self, output: &Value, ctx: &Context) -> Result<TransitionIndex, NodeError>;
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

        let shared_system = shared_system.render_with_input(ctx, &input)?;
        let node_system = self.inner.system().render_with_input(ctx, &input)?;
        let fixed_user = self.inner.user().render_with_input(ctx, &input)?;
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

        // The node's user prompt is fully rendered from context and input; the
        // previous node's JSON is not appended separately.
        let user_text = fixed_user.trim().to_owned();
        let fixed_tokens = estimate_tokens(&system) + estimate_tokens(&user_text);
        compress_history(&runtime, &model, &params, fixed_tokens, 0, ctx).await?;

        ctx.history_mut().push(LlmMessage::user(user_text));
        let mut messages = Vec::new();
        if !system.is_empty() {
            messages.push(LlmMessage::system(system));
        }
        messages.extend(ctx.history().messages().iter().cloned());

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
            let final_answer = message.tool_calls.is_empty();
            ctx.history_mut().push(message.clone());
            messages.push(message.clone());
            if final_answer {
                return parse_node_output(&message.content);
            }

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

    fn to(&self, output: &Value, ctx: &Context) -> Result<TransitionIndex, NodeError> {
        self.inner.to(output, ctx)
    }

    fn transition_count(&self) -> usize {
        self.inner.transition_count()
    }
}

fn parse_node_output(content: &str) -> Result<Value, NodeError> {
    let text = strip_json_fence(content.trim());
    if text.is_empty() {
        return Err(NodeError::failed("LLM node returned an empty response"));
    }
    if let Ok(value) = serde_json::from_str(text) {
        return Ok(value);
    }
    // Models occasionally wrap the final JSON object in prose. Keep the
    // strict parse above, then accept the last complete JSON object/array in
    // the response so a harmless explanation does not fail the whole run.
    if let Some(value) = last_json_value(text) {
        return Ok(value);
    }
    let error = serde_json::from_str::<Value>(text).expect_err("strict parse failed");
    Err(NodeError::failed(format!(
        "LLM node returned invalid JSON: {error}; response: {}",
        truncate_for_error(text)
    )))
}

fn last_json_value(text: &str) -> Option<Value> {
    let bytes = text.as_bytes();
    let mut last = None;
    for start in 0..bytes.len() {
        let opening = bytes[start];
        if opening != b'{' && opening != b'[' {
            continue;
        }
        let closing = if opening == b'{' { b'}' } else { b']' };
        let mut stack = vec![closing];
        let mut in_string = false;
        let mut escaped = false;
        for end in start + 1..bytes.len() {
            let byte = bytes[end];
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'{' => stack.push(b'}'),
                b'[' => stack.push(b']'),
                b'}' | b']' => {
                    if stack.pop() != Some(byte) {
                        break;
                    }
                    if stack.is_empty() {
                        if let Ok(value) = serde_json::from_str(&text[start..=end]) {
                            last = Some(value);
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    last
}

fn strip_json_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    rest.strip_suffix("```").map(str::trim_end).unwrap_or(text)
}

fn truncate_for_error(text: &str) -> String {
    const MAX_CHARS: usize = 500;
    if text.chars().count() <= MAX_CHARS {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(MAX_CHARS).collect();
    out.push('…');
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Agent, Graph, LlmConfig, LlmParams, PromptTemplate, ScriptedModel, ToolRegistry};
    use std::sync::Arc;

    struct TestLlm;

    impl LlmNode for TestLlm {
        fn system(&self) -> &PromptTemplate {
            static TEMPLATE: std::sync::OnceLock<PromptTemplate> = std::sync::OnceLock::new();
            TEMPLATE.get_or_init(|| PromptTemplate::literal(""))
        }

        fn user(&self) -> &PromptTemplate {
            static TEMPLATE: std::sync::OnceLock<PromptTemplate> = std::sync::OnceLock::new();
            TEMPLATE.get_or_init(|| PromptTemplate::literal(""))
        }

        fn to(&self, _output: &Value, _ctx: &Context) -> Result<TransitionIndex, NodeError> {
            Ok(TransitionIndex(0))
        }

        fn transition_count(&self) -> usize {
            1
        }
    }

    #[test]
    fn parses_json_after_prose() {
        let value = parse_node_output(
            "Cargo.toml 没有代码元素，无需修改。\n\n{\"ok\":true}",
        )
        .unwrap();
        assert_eq!(value, serde_json::json!({ "ok": true }));
    }

    struct NoSkills;

    impl SkillRegistry for NoSkills {
        fn prompt(&self, _id: &str) -> Result<Option<String>, SkillError> {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn history_is_compressed_before_the_request() {
        let model = Arc::new(ScriptedModel::new(vec![
            LlmResponse::final_answer("SUMMARY"),
            LlmResponse::final_answer(serde_json::json!({ "ok": true }).to_string()),
        ]));
        let llm = Arc::new(LlmRuntime::new(
            model.clone(),
            Arc::new(ToolRegistry::new()),
            Arc::new(NoSkills),
        ));
        let graph = Graph::builder("compress")
            .entry("agent")
            .exit("done")
            .llm_node("agent", TestLlm)
            .edge("agent", 0, "done")
            .build()
            .unwrap();
        let agent = Agent::new("compress", graph, llm);

        let mut context = Context::new();
        for index in 0..20 {
            context.history_mut().push(LlmMessage::user(format!(
                "old message {index} with enough text to count"
            )));
        }
        context.set_llm_config(
            "agent",
            LlmConfig {
                model: ModelProfileRef::new("test"),
                shared_system: PromptTemplate::literal(""),
                tools: Vec::new(),
                params: LlmParams {
                    history_token_budget: Some(40),
                    ..LlmParams::default()
                },
            },
        );

        let output = agent.run(context, serde_json::json!("new")).await.unwrap();
        let messages = output.context.history().messages();
        assert!(
            messages[0]
                .content
                .starts_with("[earlier conversation summary]")
        );
        assert!(messages[0].content.contains("SUMMARY"));
        assert!(
            !messages
                .iter()
                .any(|message| message.content.starts_with("old message 0 "))
        );
        assert_eq!(
            messages.last().unwrap().content,
            serde_json::json!({ "ok": true }).to_string()
        );

        let requests = model.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].messages[1].content.contains("old message 0"));
        assert!(requests[1].messages.iter().any(|message| {
            message
                .content
                .starts_with("[earlier conversation summary]")
        }));
        assert!(
            !requests[1]
                .messages
                .iter()
                .any(|message| message.content.starts_with("old message 0 "))
        );
    }
}
