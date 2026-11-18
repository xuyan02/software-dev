//! Credentials for remote agent processes.
//!
//! Agentd authenticates with its agent token and asks for the credentials of
//! one task. The control plane verifies that the account is an active actor on
//! that task, then mints a short-lived repository token.

use crate::github_app::{GitCredential, GitHubApp};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use foundry_adapter_postgres::PostgresStore;
use foundry_app::{AuthService, ProjectRepository, TaskRepository};
use foundry_domain::{AccountKind, TaskDAO};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct CredentialState {
    pub auth: Arc<AuthService<PostgresStore, PostgresStore>>,
    pub store: Arc<PostgresStore>,
    pub github: Option<Arc<GitHubApp>>,
    pub ds_token: Option<String>,
}

pub fn router(state: CredentialState) -> Router {
    Router::new()
        .route("/api/v1/agent/credentials", post(handle))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct CredentialRequest {
    task_id: Uuid,
}

async fn handle(
    State(state): State<CredentialState>,
    headers: HeaderMap,
    Json(request): Json<CredentialRequest>,
) -> Response {
    let Some(token) = bearer_token(&headers) else {
        return unauthorized("missing bearer token");
    };
    let account = match state.auth.authenticate(&token).await {
        Ok(Some(account)) => account,
        Ok(None) => return unauthorized("invalid or expired token"),
        Err(error) => return server_error(error.to_string()),
    };
    if account.kind != AccountKind::Agent {
        return unauthorized("only agent accounts may request task credentials");
    }

    let task: TaskDAO = match TaskRepository::get(state.store.as_ref(), request.task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => return not_found("task not found"),
        Err(error) => return server_error(error.to_string()),
    };
    let actors = match TaskRepository::actors(&*state.store, request.task_id).await {
        Ok(actors) => actors,
        Err(error) => return server_error(error.to_string()),
    };
    let active = actors
        .iter()
        .any(|actor| actor.account_id == account.id && actor.active);
    if !active {
        return forbidden("account is not an active actor on this task");
    }

    let project = match ProjectRepository::get(state.store.as_ref(), task.project_id).await {
        Ok(Some(project)) => project,
        Ok(None) => return not_found("project not found"),
        Err(error) => return server_error(error.to_string()),
    };

    let git = match &state.github {
        Some(github)
            if !project.git_repository.trim().is_empty()
                && project.git_repository.contains("github.com") =>
        {
            match github.token_for_repo(&project.git_repository).await {
                Ok(credential) => Some(credential),
                Err(error) => {
                    tracing::warn!(
                        task_id = %request.task_id,
                        project_id = %task.project_id,
                        error = %error,
                        "cannot mint GitHub credential"
                    );
                    return server_error(error);
                }
            }
        }
        _ => None,
    };

    let body = CredentialResponse {
        git,
        design_space: DesignSpaceCredential {
            url: project.design_space_url.clone(),
            token: state.ds_token.clone(),
        },
    };
    (StatusCode::OK, Json(body)).into_response()
}

#[derive(Debug, serde::Serialize)]
struct CredentialResponse {
    git: Option<GitCredential>,
    design_space: DesignSpaceCredential,
}

#[derive(Debug, serde::Serialize)]
struct DesignSpaceCredential {
    url: String,
    token: Option<String>,
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .map(|token| token.trim().to_owned())
}

fn unauthorized(message: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": message }))).into_response()
}

fn forbidden(message: &str) -> Response {
    (StatusCode::FORBIDDEN, Json(json!({ "error": message }))).into_response()
}

fn not_found(message: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": message }))).into_response()
}

fn server_error(message: String) -> Response {
    (StatusCode::BAD_GATEWAY, Json(json!({ "error": message }))).into_response()
}
