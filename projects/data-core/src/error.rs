#[derive(Debug, thiserror::Error)]
pub enum ValueError {
    #[error("invalid path: {0}")]
    InvalidPath(String),

    #[error("number is not finite")]
    NonFiniteNumber,
}
