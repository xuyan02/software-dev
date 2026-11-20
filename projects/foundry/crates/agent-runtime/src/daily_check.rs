//! Daily check dispatcher graph.
//!
//! The daily check dispatcher does no inspection. It creates the design
//! inspection child and reports the stage operation, so the whole run is
//! deterministic: no model call, no shell.

use crate::{AgentRunRequest, RunError, RunResult};
use async_trait::async_trait;
use graph_agent::{
    Agent, Context, Graph, LlmRuntime, ModelGateway, Node, NodeError, SkillRegistry, ToolRegistry,
    TransitionIndex,
};
use serde_json::{Value, json};
use std::sync::Arc;

const DISPATCH_OPERATION: &str = "dispatch:dispatched";
const FAIL_OPERATION: &str = "dispatch:failed";
const CHILD_WORKFLOW: &str = "design_inspection";
const CHILD_TITLE: &str = "设计检查";
const CHILD_DESCRIPTION: &str = "检查设计文档、系统/模块边界、接口和依赖，并把问题上报为 Issue。";

pub async fn run(
    model: Arc<dyn ModelGateway>,
    tools: Arc<ToolRegistry>,
    request: AgentRunRequest,
) -> Result<RunResult, RunError> {
    let graph = Graph::builder("daily-check")
        .entry("dispatch")
        .exit("done")
        .node("dispatch", DispatchNode)
        .edge("dispatch", 0, "done")
        .build()
        .map_err(|error| RunError::Graph(error.to_string()))?;
    let llm = Arc::new(LlmRuntime::new(model, tools, Arc::new(NoSkills)));
    let agent = Agent::new("daily-check", graph, llm);

    let mut context = Context::new();
    context
        .set_var("task_id", request.task_id.clone())
        .map_err(|error| RunError::Graph(error.to_string()))?;
    context
        .set_var("task_input", request.task_input.clone())
        .map_err(|error| RunError::Graph(error.to_string()))?;
    if let Some(project_id) = &request.project_id {
        context
            .set_var("project_id", project_id.clone())
            .map_err(|error| RunError::Graph(error.to_string()))?;
    }
    context.set_tool_context(graph_agent::ToolContext {
        run_id: request.task_id.clone(),
        agent_id: Some(request.agent_id.clone()),
        task_id: Some(request.task_id.clone()),
        project_id: request.project_id.clone(),
    });

    let output = agent
        .run(context, Value::Null)
        .await
        .map_err(|error| RunError::Graph(error.to_string()))?;
    let text = match &output.output {
        Value::String(text) => text.clone(),
        value => value.to_string(),
    };
    Ok(RunResult {
        final_message: Some(text),
    })
}

struct DispatchNode;

#[async_trait]
impl Node for DispatchNode {
    async fn run(&self, _input: Value, ctx: &mut Context) -> Result<Value, NodeError> {
        let task_id: String = ctx.var("task_id")?;
        let scope = ctx
            .var::<Value>("task_input")
            .ok()
            .and_then(|input| input.get("scope").cloned())
            .filter(|scope| !scope.is_null());

        let mut child_input = serde_json::Map::new();
        if let Some(scope) = scope {
            child_input.insert("scope".to_string(), scope);
        }
        let spawn_args = json!({
            "task_id": task_id,
            "sequential": true,
            "tasks": [{
                "workflow": CHILD_WORKFLOW,
                "title": CHILD_TITLE,
                "description": CHILD_DESCRIPTION,
                "input": Value::Object(child_input),
            }],
        });

        let registry = ctx
            .tool_registry()
            .map_err(|error| NodeError::failed(error.to_string()))?;
        let spawned = registry
            .call("task_spawn_children", spawn_args, ctx.tool_context())
            .await;

        let (operation, detail) = match spawned {
            Ok(_) => (DISPATCH_OPERATION, format!("{CHILD_TITLE}任务已创建")),
            Err(error) => (
                FAIL_OPERATION,
                format!("无法创建{CHILD_TITLE}子任务：{error}"),
            ),
        };
        let operation_args = json!({
            "task_id": task_id,
            "operation": operation,
            "idempotency_key": format!("{operation}:{task_id}"),
        });
        registry
            .call("task_operation", operation_args, ctx.tool_context())
            .await
            .map_err(|error| NodeError::failed(format!("cannot send {operation}: {error}")))?;
        Ok(json!({ "operation": operation, "detail": detail }))
    }

    fn to(&self, _output: &Value, _ctx: &Context) -> Result<TransitionIndex, NodeError> {
        Ok(TransitionIndex(0))
    }

    fn transition_count(&self) -> usize {
        1
    }
}

struct NoSkills;

impl SkillRegistry for NoSkills {
    fn prompt(&self, _id: &str) -> Result<Option<String>, graph_agent::SkillError> {
        Ok(None)
    }
}
