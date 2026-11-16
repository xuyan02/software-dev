//! Runtime errors.

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("tool failed: {0}")]
    Failed(String),
    #[error("unknown tool: {0}")]
    Unknown(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("agent graph error: {0}")]
    Graph(String),
}
