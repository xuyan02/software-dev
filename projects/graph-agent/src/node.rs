use crate::{Context, ContextError, TemplateError, TransitionIndex};
use async_trait::async_trait;

/// One executable step in a [`crate::Graph`].
///
/// A node does not know its own graph name or the names of its targets. It
/// writes its output through [`Context::set_output`] and returns the local
/// transition index that the graph should follow.
#[async_trait]
pub trait Node: Send + Sync {
    async fn run(&self, ctx: &mut Context) -> Result<(), NodeError>;

    /// Select one of this node's local transition targets.
    fn to(&self, ctx: &Context) -> Result<TransitionIndex, NodeError>;

    /// Number of outgoing transition targets this node can select.
    fn transition_count(&self) -> usize;
}

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error(transparent)]
    Context(#[from] ContextError),

    #[error(transparent)]
    Template(#[from] TemplateError),

    #[error("{0}")]
    Failed(String),
}

impl NodeError {
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }
}
