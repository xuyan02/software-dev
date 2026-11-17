use async_trait::async_trait;
use graph_agent::{
    Agent, Context, Graph, LlmRequest, LlmResponse, LlmRuntime, ModelError, ModelGateway, Node,
    NodeError, SkillError, SkillRegistry, ToolRegistry, TransitionIndex,
};
use serde_json::{Value, json};
use std::sync::Arc;

struct LoadNode;

#[async_trait]
impl Node for LoadNode {
    async fn run(&self, _input: Value, ctx: &mut Context) -> Result<Value, NodeError> {
        ctx.set_output("loaded")?;
        Ok(Value::String("loaded".to_owned()))
    }

    fn to(&self, _output: &Value, _ctx: &Context) -> Result<TransitionIndex, NodeError> {
        Ok(TransitionIndex(0))
    }

    fn transition_count(&self) -> usize {
        1
    }
}

struct NoopModel;

#[async_trait]
impl ModelGateway for NoopModel {
    async fn complete(&self, _request: LlmRequest) -> Result<LlmResponse, ModelError> {
        Err(ModelError::Backend("not used".into()))
    }
}

struct NoopSkills;

impl SkillRegistry for NoopSkills {
    fn prompt(&self, _id: &str) -> Result<Option<String>, SkillError> {
        Ok(None)
    }
}

fn runtime() -> Arc<LlmRuntime> {
    Arc::new(LlmRuntime::new(
        Arc::new(NoopModel),
        Arc::new(ToolRegistry::new()),
        Arc::new(NoopSkills),
    ))
}

fn graph() -> Graph {
    Graph::builder("agent-graph")
        .entry("load")
        .exit("done")
        .node("load", LoadNode)
        .edge("load", 0, "done")
        .build()
        .expect("valid graph")
}

#[tokio::test]
async fn agent_owns_name_graph_and_runtime() {
    let runtime = runtime();
    let agent = Agent::new("tester", graph(), runtime.clone());

    assert_eq!(agent.name(), "tester");
    assert_eq!(agent.graph().id(), "agent-graph");
    assert!(Arc::ptr_eq(agent.llm_runtime(), &runtime));
}

#[tokio::test]
async fn agent_run_returns_final_context() {
    let agent = Agent::new("tester", graph(), runtime());
    let context = Context::new();

    let output = agent.run(context, json!(null)).await.expect("run");
    assert_eq!(output.exit().as_str(), "done");
    assert_eq!(
        output.context.node_output::<String>("load").unwrap(),
        "loaded"
    );
}

#[tokio::test]
async fn agent_run_mut_uses_callers_context() {
    let agent = Agent::new("tester", graph(), runtime());
    let mut context = Context::new();

    let output = agent.run_mut(&mut context, json!(null)).await.expect("run");
    assert_eq!(output.exit.as_str(), "done");
    assert_eq!(context.node_output::<String>("load").unwrap(), "loaded");
}
