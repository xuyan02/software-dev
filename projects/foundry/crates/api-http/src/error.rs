use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use foundry_app::{RepoError, ServiceError};
use serde_json::{Value, json};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub details: Option<Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn internal(error: impl std::fmt::Display) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            error.to_string(),
        )
    }
}

/// Maps a workflow use-case result to its wire contract.
///
/// Workflow writes are only forbidden for *visible* definitions, so a plain
/// `Forbidden` here always means `workflow_forbidden`.
pub fn workflow_error<T>(result: Result<T, ServiceError>) -> Result<T, ApiError> {
    result.map_err(|error| match error {
        ServiceError::Forbidden(message) => {
            ApiError::new(StatusCode::FORBIDDEN, "workflow_forbidden", message)
        }
        ServiceError::Validation(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", message)
        }
        ServiceError::WorkflowInvalid(validation) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "workflow_invalid",
            "workflow definition is invalid",
        )
        .with_details(json!({
            "errors": validation.errors,
            "warnings": validation.warnings,
        })),
        ServiceError::WorkflowTooLarge => ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "workflow_too_large",
            error.to_string(),
        ),
        ServiceError::DraftNotFound(_) => {
            ApiError::new(StatusCode::CONFLICT, "draft_not_found", error.to_string())
        }
        ServiceError::DraftConflict {
            expected,
            current,
            current_updated_at,
        } => ApiError::new(
            StatusCode::CONFLICT,
            "draft_conflict",
            format!("draft revision conflict: expected {expected}, current is {current}"),
        )
        .with_details(json!({
            "current_revision": current,
            "current_updated_at": rfc3339(current_updated_at),
        })),
        other => other.into(),
    })
}

fn rfc3339(at: time::OffsetDateTime) -> String {
    at.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        if let ServiceError::NotFoundNamed {
            kind: "workflow", ..
        } = &error
        {
            return Self::new(
                StatusCode::NOT_FOUND,
                "workflow_not_found",
                error.to_string(),
            );
        }
        match error {
            ServiceError::NotFound { .. } | ServiceError::NotFoundNamed { .. } => {
                Self::new(StatusCode::NOT_FOUND, "not_found", error.to_string())
            }
            ServiceError::Unpublished(_) => Self::new(
                StatusCode::CONFLICT,
                "workflow_unpublished",
                error.to_string(),
            ),
            ServiceError::Forbidden(_) => {
                Self::new(StatusCode::FORBIDDEN, "forbidden", error.to_string())
            }
            ServiceError::Unauthorized(_) => {
                Self::new(StatusCode::UNAUTHORIZED, "unauthorized", error.to_string())
            }
            ServiceError::Validation(_) => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                error.to_string(),
            ),
            ServiceError::Conflict { .. } => {
                Self::new(StatusCode::CONFLICT, "version_conflict", error.to_string())
            }
            ServiceError::InvalidTransition(_) => Self::new(
                StatusCode::CONFLICT,
                "invalid_transition",
                error.to_string(),
            ),
            ServiceError::Repository(RepoError::NotFound) => {
                Self::new(StatusCode::NOT_FOUND, "not_found", error.to_string())
            }
            ServiceError::Repository(RepoError::Conflict { .. }) => {
                Self::new(StatusCode::CONFLICT, "version_conflict", error.to_string())
            }
            ServiceError::Repository(RepoError::Duplicate(_)) => {
                Self::new(StatusCode::CONFLICT, "duplicate", error.to_string())
            }
            other => Self::internal(other),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut error = json!({
            "code": self.code,
            "message": self.message,
        });
        if let Some(details) = self.details
            && let (Some(object), Value::Object(details)) = (error.as_object_mut(), details)
        {
            object.extend(details);
        }
        (self.status, Json(json!({ "error": error }))).into_response()
    }
}

/// Maps requirements-document use-case results to their wire contract.
pub fn requirements_error<T>(result: Result<T, ServiceError>) -> Result<T, ApiError> {
    result.map_err(|error| match error {
        ServiceError::RequirementsInvalid(validation) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "requirements_invalid",
            "requirements document is invalid",
        )
        .with_details(json!({
            "errors": validation.errors,
            "warnings": validation.warnings,
        })),
        ServiceError::RequirementsTooLarge => ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "requirements_too_large",
            error.to_string(),
        ),
        ServiceError::DraftNotFound(_) => {
            ApiError::new(StatusCode::CONFLICT, "draft_not_found", error.to_string())
        }
        ServiceError::DraftConflict {
            expected,
            current,
            current_updated_at,
        } => ApiError::new(
            StatusCode::CONFLICT,
            "draft_conflict",
            format!("draft revision conflict: expected {expected}, current is {current}"),
        )
        .with_details(json!({
            "current_revision": current,
            "current_updated_at": rfc3339(current_updated_at),
        })),
        other => other.into(),
    })
}

/// Maps project-brief use-case results to their wire contract.
pub fn project_brief_error<T>(result: Result<T, ServiceError>) -> Result<T, ApiError> {
    result.map_err(|error| match error {
        ServiceError::ProjectBriefInvalid(validation) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "project_brief_invalid",
            "project brief is invalid",
        )
        .with_details(json!({
            "errors": validation.errors,
            "warnings": validation.warnings,
        })),
        ServiceError::ProjectBriefTooLarge => ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "project_brief_too_large",
            error.to_string(),
        ),
        ServiceError::DraftNotFound(_) => {
            ApiError::new(StatusCode::CONFLICT, "brief_not_found", error.to_string())
        }
        ServiceError::DraftConflict {
            expected,
            current,
            current_updated_at,
        } => ApiError::new(
            StatusCode::CONFLICT,
            "draft_conflict",
            format!("draft revision conflict: expected {expected}, current is {current}"),
        )
        .with_details(json!({
            "current_revision": current,
            "current_updated_at": rfc3339(current_updated_at),
        })),
        other => other.into(),
    })
}
