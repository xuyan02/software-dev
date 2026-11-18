use async_trait::async_trait;
use graph_agent::{
    Agent, Context, Graph, LlmConfig, LlmNode, LlmParams, LlmResponse, LlmRole, LlmRuntime,
    ModelProfileRef, Node, NodeError, PromptTemplate, ScriptedModel, SkillError, SkillRegistry,
    Tool, ToolContext, ToolDefinition, ToolError, ToolOutput, ToolRegistry, TransitionIndex,
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
        TEMPLATE
            .get_or_init(|| PromptTemplate::parse("输入：{{input}}").expect("valid input template"))
    }

    fn to(&self, _output: &Value, _ctx: &Context) -> Result<TransitionIndex, NodeError> {
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

    fn to(&self, _output: &Value, _ctx: &Context) -> Result<TransitionIndex, NodeError> {
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
async fn llm_records_user_tool_calls_and_final_reply() {
    let model = Arc::new(ScriptedModel::new(vec![
        LlmResponse::tool_call("echo", json!({ "text": "hi" })),
        LlmResponse::final_answer(json!({ "summary": "done" }).to_string()),
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
    assert_eq!(messages[0].content, "输入：hello");
    assert_eq!(messages[1].role, LlmRole::Assistant);
    assert_eq!(messages[1].tool_calls[0].name, "echo");
    assert_eq!(messages[2].role, LlmRole::Tool);
    assert_eq!(messages[2].content, "hi");
    assert_eq!(messages[3].role, LlmRole::Assistant);
    assert_eq!(
        messages[3].content,
        json!({ "summary": "done" }).to_string()
    );
    assert_eq!(messages.len(), 4);
}
