use crate::{Context, Graph, GraphError, LlmRuntime, NodeId};
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

    /// Run to completion using an owned context and return both the exit and the
    /// final context.
    pub async fn run(&self, mut context: Context) -> Result<AgentOutput, GraphError> {
        context.set_llm_runtime(self.llm.clone());
        let exit = self.graph.run(&mut context).await?;
        Ok(AgentOutput { exit, context })
    }

    /// Run against a borrowed context, useful when the caller owns the context.
    pub async fn run_mut(&self, context: &mut Context) -> Result<NodeId, GraphError> {
        context.set_llm_runtime(self.llm.clone());
        self.graph.run(context).await
    }
}

#[derive(Debug)]
pub struct AgentOutput {
    pub exit: NodeId,
    pub context: Context,
}

impl AgentOutput {
    pub fn exit(&self) -> &NodeId {
        &self.exit
    }

    pub fn into_context(self) -> Context {
        self.context
    }
}
