use crate::{Context, Node, NodeError, PromptTemplate, TransitionIndex};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

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
    pub tools: Arc<dyn ToolRegistry>,
    pub skills: Arc<dyn SkillRegistry>,
}

impl LlmRuntime {
    pub fn new(
        models: Arc<dyn ModelGateway>,
        tools: Arc<dyn ToolRegistry>,
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
    pub tools: Vec<ToolRef>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRef(String);

impl ToolRef {
    pub fn new(tool: impl Into<String>) -> Self {
        Self(tool.into())
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
}

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub model: ModelProfileRef,
    pub params: LlmParams,
    pub system: String,
    pub user: String,
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub output: Value,
}

#[async_trait]
pub trait ModelGateway: Send + Sync {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, ModelError>;
}

pub trait ToolRegistry: Send + Sync {
    fn spec(&self, tool: &ToolRef) -> Result<ToolSpec, ToolError>;
}

/// External skill registry. Skill names in an LLM node are resolved here.
pub trait SkillRegistry: Send + Sync {
    fn prompt(&self, id: &str) -> Result<Option<String>, SkillError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("model error: {0}")]
    Failed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("unknown tool: {0}")]
    UnknownTool(String),

    #[error("tool error: {0}")]
    Failed(String),
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
    async fn run(&self, ctx: &mut Context) -> Result<(), NodeError> {
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
        let user = self.inner.user().render(ctx)?;
        let system = join_system(shared_system, node_system);
        let system = append_skills(system, runtime.skills.as_ref(), self.inner.skills())?;

        let tools = tools
            .iter()
            .map(|tool| runtime.tools.spec(tool))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| NodeError::failed(error.to_string()))?;

        let response = runtime
            .models
            .complete(LlmRequest {
                model,
                params,
                system,
                user,
                tools,
            })
            .await
            .map_err(|error| NodeError::failed(error.to_string()))?;

        ctx.set_output(response.output)?;
        Ok(())
    }

    fn to(&self, ctx: &Context) -> Result<TransitionIndex, NodeError> {
        self.inner.to(ctx)
    }

    fn transition_count(&self) -> usize {
        self.inner.transition_count()
    }
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
