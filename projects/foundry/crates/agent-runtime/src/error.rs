//! Runtime errors.

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("scripted model has no responses left")]
    Exhausted,
    #[error("model backend error: {0}")]
    Backend(String),
    #[error("invalid model response: {0}")]
    InvalidResponse(String),
}

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
pub enum TrajectoryError {
    #[error("trajectory sink error: {0}")]
    Backend(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error("trajectory sink error: {0}")]
    Trajectory(String),
    #[error("run paused for human input; resume it instead")]
    UnexpectedPause,
}
