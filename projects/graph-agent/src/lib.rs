//! A small, code-first graph runtime for deterministic node execution.
//!
//! A [`Graph`] is assembled from [`Node`] implementations and [`Edge`]
//! bindings. Node implementations decide which local [`TransitionIndex`] to
//! follow; the graph decides which node that index points to.
//!
//! Graph definitions are code, not serializable data. Conditions and node
//! behavior live in Rust, so a run must be replayed with the same code version.
//!
//! [`Graph::builder`] validates the assembly before returning a graph:
//! entry/exits, duplicate nodes, node names that collide with exits, duplicate
//! or out-of-range edges, missing targets, unreachable nodes/exits, and exits
//! with outgoing edges.
//!
//! Variables are addressed as `<name>` / `<name>.<field>` or
//! `nodes.<id>.output.<field>`. Local variables and external
//! [`VariableSource`] implementations are all resolved through [`Context`].
//! Prompt templates use the same paths inside `{{...}}`.

mod agent;
mod context;
mod graph;
mod id;
mod llm;
mod node;
mod template;
mod variable;

pub use agent::{Agent, AgentOutput};
pub use context::{Context, ContextError};
pub use graph::{Edge, Graph, GraphBuilder, GraphError};
pub use id::{NodeId, TransitionIndex};
pub(crate) use llm::LlmNodeAdapter;
pub use llm::{
    LlmConfig, LlmNode, LlmParams, LlmRequest, LlmResponse, LlmRuntime, ModelError, ModelGateway,
    ModelProfileRef, SkillError, SkillRegistry, ToolError, ToolRef, ToolRegistry, ToolSpec,
};
pub use node::{Node, NodeError};
pub use template::{PromptTemplate, TemplateError};
pub use variable::{MapSource, VariableError, VariablePath, VariableSource};
