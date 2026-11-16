use async_trait::async_trait;
use graph_agent::{
    Agent, Context, Graph, LlmConfig, LlmMessage, LlmNode, LlmParams, LlmRequest, LlmResponse,
    LlmRole, LlmRuntime, LlmToolCall, ModelError, ModelGateway, ModelProfileRef, NodeError,
    PromptTemplate, SkillError, SkillRegistry, Tool, ToolContext, ToolDefinition, ToolError,
    ToolOutput, ToolRegistry, TransitionIndex,
};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

struct ScriptedModel {
    responses: Mutex<VecDeque<LlmResponse>>,
    requests: Mutex<Vec<LlmRequest>>,
}

impl ScriptedModel {
    fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<LlmRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl ModelGateway for ScriptedModel {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, ModelError> {
        self.requests.lock().unwrap().push(request);
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| ModelError::Exhausted)
    }
}

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".to_owned(),
            description: "Echo text.".to_owned(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let text = arguments
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::new(1000, "missing text"))?;
        Ok(ToolOutput::Text(text.to_owned()))
    }
}

struct FailingTool;

#[async_trait]
impl Tool for FailingTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "fail".to_owned(),
            description: "Fail.".to_owned(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        Err(ToolError::new(2000, "boom"))
    }
}

struct SummarizeNode {
    system: PromptTemplate,
    user: PromptTemplate,
}

impl LlmNode for SummarizeNode {
    fn system(&self) -> &PromptTemplate {
        &self.system
    }

    fn user(&self) -> &PromptTemplate {
        &self.user
    }

    fn to(&self, _ctx: &Context) -> Result<TransitionIndex, NodeError> {
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

fn runtime(responses: Vec<LlmResponse>, tools: ToolRegistry) -> (Agent, Arc<ScriptedModel>) {
    let model = Arc::new(ScriptedModel::new(responses));
    let llm = Arc::new(LlmRuntime::new(
        model.clone(),
        Arc::new(tools),
        Arc::new(NoSkills),
    ));
    let graph = Graph::builder("tool-loop")
        .entry("summarize")
        .exit("done")
        .llm_node(
            "summarize",
            SummarizeNode {
                system: PromptTemplate::parse("system").unwrap(),
                user: PromptTemplate::parse("user").unwrap(),
            },
        )
        .edge("summarize", 0, "done")
        .build()
        .unwrap();
    (Agent::new("agent", graph, llm), model)
}

fn context() -> Context {
    let mut context = Context::new();
    context.set_llm_config(
        "summarize",
        LlmConfig {
            model: ModelProfileRef::new("test"),
            shared_system: PromptTemplate::parse("").unwrap(),
            tools: vec!["echo".to_owned()],
            params: LlmParams::default(),
        },
    );
    context
}

fn call(id: &str, name: &str, arguments: Value) -> LlmToolCall {
    LlmToolCall {
        id: id.to_owned(),
        name: name.to_owned(),
        arguments,
    }
}

#[tokio::test]
async fn executes_tool_calls_until_the_model_stops() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool)).unwrap();
    let (agent, model) = runtime(
        vec![
            LlmResponse::new(LlmMessage::assistant(
                "",
                vec![call("call-1", "echo", json!({"text": "hello"}))],
            )),
            LlmResponse::new(LlmMessage::assistant("done", vec![])),
        ],
        tools,
    );

    let output = agent.run(context(), json!(null)).await.unwrap();
    assert_eq!(output.output, json!("done"));

    let requests = model.requests();
    assert_eq!(requests.len(), 2);
    let tool_message = requests[1]
        .messages
        .iter()
        .find(|message| message.role == LlmRole::Tool)
        .expect("tool result");
    assert_eq!(tool_message.tool_call_id.as_deref(), Some("call-1"));
    assert_eq!(tool_message.content, "hello");
    assert_eq!(
        requests[0]
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["echo"]
    );
}

#[tokio::test]
async fn tool_errors_are_returned_to_the_model() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(FailingTool)).unwrap();
    let (agent, model) = runtime(
        vec![
            LlmResponse::new(LlmMessage::assistant(
                "",
                vec![call("call-1", "fail", json!({}))],
            )),
            LlmResponse::new(LlmMessage::assistant("recovered", vec![])),
        ],
        tools,
    );
    let mut context = context();
    context.set_llm_config(
        "summarize",
        LlmConfig {
            model: ModelProfileRef::new("test"),
            shared_system: PromptTemplate::parse("").unwrap(),
            tools: vec!["fail".to_owned()],
            params: LlmParams::default(),
        },
    );

    agent.run(context, json!(null)).await.unwrap();
    let requests = model.requests();
    let tool_message = requests[1]
        .messages
        .iter()
        .find(|message| message.role == LlmRole::Tool)
        .expect("tool result");
    let error: Value = serde_json::from_str(&tool_message.content).unwrap();
    assert_eq!(error["error"]["code"], 2000);
    assert_eq!(error["error"]["message"], "boom");
}

#[tokio::test]
async fn tools_not_enabled_for_the_node_are_denied() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool)).unwrap();
    tools.register(Arc::new(FailingTool)).unwrap();
    let (agent, model) = runtime(
        vec![
            LlmResponse::new(LlmMessage::assistant(
                "",
                vec![call("call-1", "fail", json!({}))],
            )),
            LlmResponse::new(LlmMessage::assistant("done", vec![])),
        ],
        tools,
    );

    agent.run(context(), json!(null)).await.unwrap();
    let requests = model.requests();
    let tool_message = requests[1]
        .messages
        .iter()
        .find(|message| message.role == LlmRole::Tool)
        .expect("tool result");
    let error: Value = serde_json::from_str(&tool_message.content).unwrap();
    assert_eq!(error["error"]["code"], graph_agent::codes::DENIED);
}
