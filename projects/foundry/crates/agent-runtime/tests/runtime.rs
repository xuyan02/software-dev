use async_trait::async_trait;
use foundry_agent_runtime::{
    AgentRunRequest, AgentRuntime, EchoTool, Tool, ToolContext, ToolError, ToolOutput,
    ToolRegistry, ToolSpec,
};
use graph_agent::{LlmResponse, ScriptedModel};
use serde_json::{Value, json};
use std::sync::Arc;

struct FailingTool;

#[async_trait]
impl Tool for FailingTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fail".to_owned(),
            description: "Always fails.".to_owned(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        Err(ToolError::Failed("boom".to_owned()))
    }
}

fn runtime(responses: Vec<LlmResponse>, tools: ToolRegistry) -> (AgentRuntime, Arc<ScriptedModel>) {
    let model = Arc::new(ScriptedModel::new(responses));
    (AgentRuntime::new(model.clone(), Arc::new(tools)), model)
}

#[tokio::test]
async fn tool_call_then_final_answer() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool));
    let (runtime, model) = runtime(
        vec![
            LlmResponse::tool_call("echo", json!({ "text": "hello" })),
            LlmResponse::final_answer("done"),
        ],
        tools,
    );

    let result = runtime
        .run(AgentRunRequest::new("agent", "task", "system", "user"))
        .await
        .unwrap();
    assert_eq!(result.final_message.as_deref(), Some("done"));

    let requests = model.requests();
    assert_eq!(requests.len(), 2);
    let tool_message = requests[1]
        .messages
        .iter()
        .find(|message| message.role == graph_agent::LlmRole::Tool)
        .expect("tool result");
    assert_eq!(tool_message.content, "hello");
}

#[tokio::test]
async fn context_is_rendered_into_the_user_message() {
    let (runtime, model) = runtime(vec![LlmResponse::final_answer("ok")], ToolRegistry::new());
    let mut request = AgentRunRequest::new("agent", "task", "system", "user request");
    request
        .context
        .push("requirement", "req-1", "users must be able to search");

    runtime.run(request).await.unwrap();

    let requests = model.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].messages[0].content, "system");
    let user = &requests[0].messages[1].content;
    assert!(user.contains("[requirement] req-1"));
    assert!(user.ends_with("user request"));
}

#[tokio::test]
async fn tool_errors_are_fed_back_to_the_model() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(FailingTool));
    let (runtime, model) = runtime(
        vec![
            LlmResponse::tool_call("fail", json!({})),
            LlmResponse::final_answer("recovered"),
        ],
        tools,
    );

    let result = runtime
        .run(AgentRunRequest::new("agent", "task", "system", "user"))
        .await
        .unwrap();
    assert_eq!(result.final_message.as_deref(), Some("recovered"));

    let requests = model.requests();
    let tool_message = requests[1]
        .messages
        .iter()
        .find(|message| message.role == graph_agent::LlmRole::Tool)
        .expect("tool result");
    let error: Value = serde_json::from_str(&tool_message.content).unwrap();
    assert_eq!(error["error"]["message"], "boom");
}
