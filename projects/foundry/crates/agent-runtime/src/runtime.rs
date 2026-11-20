//! Foundry's agent turn, executed as a graph-agent LLM node.
//!
//! Foundry supplies the system/user prompt and Foundry tools. graph-agent owns
//! the model protocol, provider calls, tool loop and message history.

use crate::graph_tools::adapt_registry;
use crate::{
    ContextBundle, RunError, Shell, ToolRegistry, codesummary, daily_check, design_baseline,
};
use graph_agent::{
    Agent, Context, Graph, LlmConfig, LlmNode, LlmParams, LlmRuntime, ModelGateway,
    ModelProfileRef, NodeError, PromptTemplate, SkillError, SkillRegistry,
    ToolContext as GraphToolContext, TransitionIndex,
};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AgentRunRequest {
    pub agent_id: String,
    pub task_id: String,
    pub project_id: Option<String>,
    pub system_prompt: String,
    pub user_message: String,
    pub context: ContextBundle,
    pub task_input: Value,
    /// Workflow stage of the task being executed. Deterministic runtimes use it
    /// to select stage-specific behavior; the model never chooses the stage.
    pub stage: Option<String>,
}

impl AgentRunRequest {
    pub fn new(
        agent_id: impl Into<String>,
        task_id: impl Into<String>,
        system_prompt: impl Into<String>,
        user_message: impl Into<String>,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            task_id: task_id.into(),
            project_id: None,
            system_prompt: system_prompt.into(),
            user_message: user_message.into(),
            context: ContextBundle::default(),
            task_input: Value::Null,
            stage: None,
        }
    }

    pub fn with_project_id(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = Some(project_id.into());
        self
    }

    pub fn with_stage(mut self, stage: Option<String>) -> Self {
        self.stage = stage;
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    pub final_message: Option<String>,
}

pub struct AgentRuntime {
    model: Arc<dyn ModelGateway>,
    tools: Arc<ToolRegistry>,
    shell: Option<Arc<dyn Shell>>,
    mode: RuntimeMode,
}

enum RuntimeMode {
    Generic,
    CodeSummary,
    DailyCheck,
    DesignBaseline { state_dir: PathBuf },
}

impl AgentRuntime {
    pub fn new(model: Arc<dyn ModelGateway>, tools: Arc<ToolRegistry>) -> Self {
        Self {
            model,
            tools,
            shell: None,
            mode: RuntimeMode::Generic,
        }
    }

    pub fn new_codesummary(
        model: Arc<dyn ModelGateway>,
        tools: Arc<ToolRegistry>,
        shell: Arc<dyn Shell>,
    ) -> Self {
        Self {
            model,
            tools,
            shell: Some(shell),
            mode: RuntimeMode::CodeSummary,
        }
    }

    pub fn new_daily_check(model: Arc<dyn ModelGateway>, tools: Arc<ToolRegistry>) -> Self {
        Self {
            model,
            tools,
            shell: None,
            mode: RuntimeMode::DailyCheck,
        }
    }

    pub fn new_design_baseline(
        model: Arc<dyn ModelGateway>,
        tools: Arc<ToolRegistry>,
        shell: Arc<dyn Shell>,
        state_dir: PathBuf,
    ) -> Self {
        Self {
            model,
            tools,
            shell: Some(shell),
            mode: RuntimeMode::DesignBaseline { state_dir },
        }
    }

    pub async fn run(&self, request: AgentRunRequest) -> Result<RunResult, RunError> {
        match &self.mode {
            RuntimeMode::Generic => self.run_generic(request).await,
            RuntimeMode::CodeSummary => self.run_codesummary(request).await,
            RuntimeMode::DailyCheck => {
                let tools = adapt_registry(&self.tools)
                    .map_err(|error| RunError::Graph(format!("tool registry: {error}")))?;
                daily_check::run(Arc::clone(&self.model), Arc::new(tools), request).await
            }
            RuntimeMode::DesignBaseline { state_dir } => {
                let tools = adapt_registry(&self.tools)
                    .map_err(|error| RunError::Graph(format!("tool registry: {error}")))?;
                design_baseline::run(
                    Arc::clone(&self.model),
                    Arc::new(tools),
                    self.shell.clone(),
                    request,
                    state_dir.clone(),
                )
                .await
            }
        }
    }

    async fn run_codesummary(&self, request: AgentRunRequest) -> Result<RunResult, RunError> {
        let tools = adapt_registry(&self.tools)
            .map_err(|error| RunError::Graph(format!("tool registry: {error}")))?;
        codesummary::run(
            Arc::clone(&self.model),
            Arc::new(tools),
            self.shell.clone(),
            request,
        )
        .await
    }

    async fn run_generic(&self, request: AgentRunRequest) -> Result<RunResult, RunError> {
        let rendered_context = request.context.render();
        let user = if rendered_context.is_empty() {
            request.user_message
        } else {
            format!("{rendered_context}\n\n{}", request.user_message)
        };
        let node = FoundryLlmNode {
            system: PromptTemplate::literal(request.system_prompt),
            user: PromptTemplate::parse(
                "{{input}}\n\nReturn exactly one JSON object with the key \"message\" whose \
                 value is your complete final reply to the user. Do not add prose outside the JSON.",
            )
            .expect("valid generic user template"),
        };
        let graph = Graph::builder("foundry-agent")
            .entry("agent")
            .exit("done")
            .llm_node("agent", node)
            .edge("agent", 0, "done")
            .build()
            .map_err(|error| RunError::Graph(error.to_string()))?;
        let tools = adapt_registry(&self.tools)
            .map_err(|error| RunError::Graph(format!("tool registry: {error}")))?;
        let llm = Arc::new(LlmRuntime::new(
            Arc::clone(&self.model),
            Arc::new(tools),
            Arc::new(NoSkills),
        ));
        let agent = Agent::new("foundry-agent", graph, llm);

        let mut context = Context::new();
        context
            .set_var("task_id", request.task_id.clone())
            .map_err(|error| RunError::Graph(error.to_string()))?;
        if let Some(project_id) = &request.project_id {
            context
                .set_var("project_id", project_id.clone())
                .map_err(|error| RunError::Graph(error.to_string()))?;
        }
        context.set_llm_config(
            "agent",
            LlmConfig {
                model: ModelProfileRef::new(""),
                shared_system: PromptTemplate::literal(""),
                tools: self.tools.names(),
                params: LlmParams::default(),
            },
        );
        context.set_tool_context(GraphToolContext {
            run_id: request.task_id.clone(),
            agent_id: Some(request.agent_id),
            task_id: Some(request.task_id),
            project_id: request.project_id,
        });
        let output = agent
            .run(context, Value::String(user))
            .await
            .map_err(|error| RunError::Graph(error.to_string()))?;
        let final_message = output
            .output
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| output.output.as_str().map(str::to_owned))
            .unwrap_or_else(|| output.output.to_string());
        Ok(RunResult {
            final_message: Some(final_message),
        })
    }

    /// Compatibility helper for callers that used the old loop API.
    pub async fn run_to_completion(&self, request: AgentRunRequest) -> Result<RunResult, RunError> {
        self.run(request).await
    }
}

struct FoundryLlmNode {
    system: PromptTemplate,
    user: PromptTemplate,
}

impl LlmNode for FoundryLlmNode {
    fn system(&self) -> &PromptTemplate {
        &self.system
    }

    fn user(&self) -> &PromptTemplate {
        &self.user
    }

    fn to(&self, _output: &Value, _context: &Context) -> Result<TransitionIndex, NodeError> {
        Ok(TransitionIndex(0))
    }

    fn transition_count(&self) -> usize {
        1
    }
}

struct NoSkills;

impl SkillRegistry for NoSkills {
    fn prompt(&self, _id: &str) -> Result<Option<String>, SkillError> {
        Ok(None)
    }
}
