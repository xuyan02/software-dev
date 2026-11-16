use crate::{Context, Graph, GraphError, GraphOutput, LlmRuntime, NodeId};
use serde_json::Value;
use std::sync::Arc;

/// An agent is a named graph plus its runtime capabilities.
///
/// The graph owns the execution topology and nodes. The agent owns the
/// engine-level LLM capabilities and installs them into the context before a
/// run.
pub struct Agent {
    name: String,
    graph: Graph,
    llm: Arc<LlmRuntime>,
}

impl Agent {
    pub fn new(name: impl Into<String>, graph: Graph, llm: Arc<LlmRuntime>) -> Self {
        Self {
            name: name.into(),
            graph,
            llm,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn llm_runtime(&self) -> &Arc<LlmRuntime> {
        &self.llm
    }

    /// Run to completion using an owned context and return the exit, final
    /// output, and final context.
    pub async fn run(&self, mut context: Context, input: Value) -> Result<AgentOutput, GraphError> {
        context.set_llm_runtime(self.llm.clone());
        context.set_tool_registry(self.llm.tools.clone());
        let output = self.graph.run(&mut context, input).await?;
        Ok(AgentOutput {
            exit: output.exit,
            output: output.output,
            context,
        })
    }

    /// Run against a borrowed context, useful when the caller owns the context.
    pub async fn run_mut(
        &self,
        context: &mut Context,
        input: Value,
    ) -> Result<GraphOutput, GraphError> {
        context.set_llm_runtime(self.llm.clone());
        context.set_tool_registry(self.llm.tools.clone());
        self.graph.run(context, input).await
    }
}

#[derive(Debug)]
pub struct AgentOutput {
    pub exit: NodeId,
    pub output: Value,
    pub context: Context,
}

impl AgentOutput {
    pub fn exit(&self) -> &NodeId {
        &self.exit
    }

    pub fn output(&self) -> &Value {
        &self.output
    }

    pub fn into_context(self) -> Context {
        self.context
    }
}
