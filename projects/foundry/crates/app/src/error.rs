use uuid::Uuid;

use foundry_domain::Validation;

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("not found")]
    NotFound,
    #[error("version conflict: expected {expected}, current is {actual}")]
    Conflict { expected: u32, actual: u32 },
    #[error("duplicate value: {0}")]
    Duplicate(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("storage backend error: {0}")]
    Backend(String),
}

#[derive(Debug, thiserror::Error)]
pub enum EventError {
    #[error("event sink error: {0}")]
    Backend(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("{kind} {id} not found")]
    NotFound { kind: &'static str, id: Uuid },
    #[error("{kind} {id} not found")]
    NotFoundNamed { kind: &'static str, id: String },
    #[error("workflow {0} has no published version")]
    Unpublished(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("version conflict: expected {expected}, current is {actual}")]
    Conflict { expected: u32, actual: u32 },
    #[error("invalid transition: {0}")]
    InvalidTransition(String),
    #[error("workflow draft is invalid")]
    WorkflowInvalid(Validation),
    #[error("workflow definition exceeds the size limit")]
    WorkflowTooLarge,
    #[error("workflow {0} has no draft")]
    DraftNotFound(String),
    #[error("draft already exists: {0}")]
    DraftExists(String),
    #[error("requirements document is invalid")]
    RequirementsInvalid(Validation),
    #[error("requirements document exceeds the size limit")]
    RequirementsTooLarge,
    #[error("anchor document is invalid")]
    AnchorDocumentInvalid(Validation),
    #[error("anchor document exceeds the size limit")]
    AnchorDocumentTooLarge,
    #[error("draft revision conflict: expected {expected}, current is {current}")]
    DraftConflict {
        expected: u64,
        current: u64,
        current_updated_at: time::OffsetDateTime,
    },
    #[error(transparent)]
    Repository(#[from] RepoError),
    #[error(transparent)]
    Event(#[from] EventError),
    #[error("internal error: {0}")]
    Internal(String),
}

impl ServiceError {
    pub fn not_found(kind: &'static str, id: Uuid) -> Self {
        Self::NotFound { kind, id }
    }

    pub fn not_found_named(kind: &'static str, id: impl Into<String>) -> Self {
        Self::NotFoundNamed {
            kind,
            id: id.into(),
        }
    }
}
