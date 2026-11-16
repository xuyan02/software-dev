use async_trait::async_trait;
use graph_agent::{
    Agent, Context, ContextError, Graph, GraphError, LlmConfig, LlmMessage, LlmNode, LlmParams,
    LlmRequest, LlmResponse, LlmRole, LlmRuntime, ModelError, ModelGateway, ModelProfileRef,
    NodeError, PromptTemplate, SkillError, SkillRegistry, ToolRegistry, TransitionIndex, Usage,
};
use serde_json::{Value, json};
use std::sync::Arc;

struct TestModel;

#[async_trait]
impl ModelGateway for TestModel {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, ModelError> {
        let system = request
            .messages
            .iter()
            .find(|message| message.role == LlmRole::System)
            .map(|message| message.content.clone())
            .unwrap_or_default();
        let user = request
            .messages
            .iter()
            .find(|message| message.role == LlmRole::User)
            .map(|message| message.content.clone())
            .unwrap_or_default();
        Ok(LlmResponse {
            message: LlmMessage::assistant(
                json!({ "system": system, "user": user }).to_string(),
                vec![],
            ),
            usage: Usage::default(),
        })
    }
}

struct TestSkills;

impl SkillRegistry for TestSkills {
    fn prompt(&self, id: &str) -> Result<Option<String>, SkillError> {
        Ok(match id {
            "document" => Some("Document skill body".to_owned()),
            _ => None,
        })
    }
}

struct TestLlm {
    system: PromptTemplate,
    user: PromptTemplate,
    skills: Vec<String>,
}

impl LlmNode for TestLlm {
    fn system(&self) -> &PromptTemplate {
        &self.system
    }

    fn user(&self) -> &PromptTemplate {
        &self.user
    }

    fn skills(&self) -> &[String] {
        &self.skills
    }

    fn to(&self, _ctx: &Context) -> Result<TransitionIndex, NodeError> {
        Ok(TransitionIndex(0))
    }

    fn transition_count(&self) -> usize {
        1
    }
}

fn test_llm() -> TestLlm {
    TestLlm {
        system: PromptTemplate::parse("节点系统").unwrap(),
        user: PromptTemplate::parse("任务：{{task}}").unwrap(),
        skills: vec!["document".into()],
    }
}

fn runtime() -> Arc<LlmRuntime> {
    Arc::new(LlmRuntime::new(
        Arc::new(TestModel),
        Arc::new(ToolRegistry::new()),
        Arc::new(TestSkills),
    ))
}

fn llm_graph() -> Graph {
    Graph::builder("llm")
        .entry("extract")
        .exit("done")
        .llm_node("extract", test_llm())
        .edge("extract", 0, "done")
        .build()
        .expect("valid graph")
}

fn configure_context(ctx: &mut Context) {
    ctx.set_var("task", "login").unwrap();
    ctx.set_llm_config(
        "extract",
        LlmConfig {
            model: ModelProfileRef::new("test"),
            shared_system: PromptTemplate::parse("共享系统").unwrap(),
            tools: vec![],
            params: LlmParams::default(),
        },
    );
}

#[tokio::test]
async fn agent_installs_runtime_for_llm_nodes() {
    let agent = Agent::new("llm-agent", llm_graph(), runtime());
    let mut ctx = Context::new();
    configure_context(&mut ctx);

    let output = agent.run(ctx, json!(null)).await.expect("run");
    assert_eq!(output.exit().as_str(), "done");

    let result: Value =
        serde_json::from_str(output.output.as_str().expect("string output")).unwrap();
    assert_eq!(
        result["system"],
        "共享系统\n\n节点系统\n\n## Skills\n\n### document\n\nDocument skill body\n"
    );
    assert_eq!(result["user"], "任务：login");
}

#[tokio::test]
async fn graph_llm_node_requires_context_runtime() {
    let graph = llm_graph();
    let mut ctx = Context::new();
    configure_context(&mut ctx);

    let error = graph
        .run(&mut ctx, json!(null))
        .await
        .expect_err("run should fail");
    assert!(matches!(
        error,
        GraphError::Node(NodeError::Context(ContextError::MissingLlmRuntime))
    ));
}
