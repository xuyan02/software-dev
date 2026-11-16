use async_trait::async_trait;
use graph_agent::{
    Agent, Context, Graph, LlmConfig, LlmMessage, LlmNode, LlmParams, LlmResponse, LlmRole,
    LlmRuntime, ModelProfileRef, Node, NodeError, PromptTemplate, ScriptedModel, SkillError,
    SkillRegistry, Tool, ToolContext, ToolDefinition, ToolError, ToolOutput, ToolRegistry,
    TransitionIndex,
};
use serde_json::{Value, json};
use std::sync::Arc;

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
            .ok_or_else(|| ToolError::new(graph_agent::codes::INVALID_ARGUMENTS, "missing text"))?;
        Ok(ToolOutput::Text(text.to_owned()))
    }
}

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

    fn to(&self, _ctx: &Context) -> Result<TransitionIndex, NodeError> {
        Ok(TransitionIndex(0))
    }

    fn transition_count(&self) -> usize {
        1
    }
}

struct MarkerNode;

#[async_trait]
impl Node for MarkerNode {
    async fn run(&self, _input: Value, _ctx: &mut Context) -> Result<Value, NodeError> {
        Ok(Value::String("marker".to_owned()))
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

fn runtime(model: Arc<ScriptedModel>, tools: ToolRegistry) -> Arc<LlmRuntime> {
    Arc::new(LlmRuntime::new(model, Arc::new(tools), Arc::new(NoSkills)))
}

#[tokio::test]
async fn llm_records_input_and_tool_calls_but_not_its_output() {
    let model = Arc::new(ScriptedModel::new(vec![
        LlmResponse::tool_call("echo", json!({ "text": "hi" })),
        LlmResponse::final_answer("done"),
    ]));
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool)).unwrap();

    let graph = Graph::builder("history")
        .entry("agent")
        .exit("done")
        .llm_node("agent", TestLlm)
        .node("marker", MarkerNode)
        .edge("agent", 0, "marker")
        .edge("marker", 0, "done")
        .build()
        .unwrap();
    let agent = Agent::new("history", graph, runtime(Arc::clone(&model), tools));

    let mut context = Context::new();
    context.set_llm_config(
        "agent",
        LlmConfig {
            model: ModelProfileRef::new("test"),
            shared_system: PromptTemplate::literal(""),
            tools: vec!["echo".to_owned()],
            params: LlmParams::default(),
        },
    );

    let output = agent.run(context, json!("hello")).await.unwrap();
    assert_eq!(output.output, json!("marker"));

    let messages = output.context.history().messages();
    assert_eq!(messages[0].role, LlmRole::User);
    assert_eq!(messages[0].content, "hello");
    assert_eq!(messages[1].role, LlmRole::Assistant);
    assert_eq!(messages[1].tool_calls[0].name, "echo");
    assert_eq!(messages[2].role, LlmRole::Tool);
    assert_eq!(messages[2].content, "hi");
    assert_eq!(messages.last().unwrap().role, LlmRole::Assistant);
    assert_eq!(messages.last().unwrap().content, "marker");
    assert!(!messages.iter().any(|message| message.content == "done"));
}

#[tokio::test]
async fn fixed_nodes_can_record_tool_calls() {
    let mut context = Context::new();
    context
        .history_mut()
        .record_tool_call("echo", json!({ "text": "fixed" }), "fixed result");
    let messages = context.history().messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, LlmRole::Assistant);
    assert_eq!(messages[0].tool_calls[0].name, "echo");
    assert_eq!(messages[1].role, LlmRole::Tool);
    assert_eq!(messages[1].content, "fixed result");
}

#[tokio::test]
async fn history_is_compressed_before_the_request() {
    let model = Arc::new(ScriptedModel::new(vec![
        LlmResponse::final_answer("SUMMARY"),
        LlmResponse::final_answer("done"),
    ]));
    let graph = Graph::builder("compress")
        .entry("agent")
        .exit("done")
        .llm_node("agent", TestLlm)
        .edge("agent", 0, "done")
        .build()
        .unwrap();
    let agent = Agent::new(
        "compress",
        graph,
        runtime(Arc::clone(&model), ToolRegistry::new()),
    );

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

    let output = agent.run(context, json!("new")).await.unwrap();
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
    assert_eq!(messages.last().unwrap().content, "done");

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
