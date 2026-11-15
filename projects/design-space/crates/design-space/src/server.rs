//! Standalone design-space server: project provisioning + multi-project sync.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::project::{self, Project};
use crate::remote::{FetchRequest, PushRequest, RefsResponse, ServerRepo};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub root: PathBuf,
    pub host: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub token: Option<String>,
}

#[derive(Clone)]
struct AppState {
    config: Arc<ServerConfig>,
}

#[derive(Debug, Serialize)]
struct ProjectView {
    name: String,
    slug: String,
    design_space: String,
}

impl From<&Project> for ProjectView {
    fn from(project: &Project) -> Self {
        Self {
            name: project.meta.name.clone(),
            slug: project.meta.slug.clone(),
            design_space: project.meta.design_space.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct CreateProject {
    name: String,
    /// Source git repository clients should clone. Optional.
    #[serde(default)]
    source: Option<String>,
}

type ApiError = (StatusCode, String);

fn bad(error: impl std::fmt::Display) -> ApiError {
    (StatusCode::BAD_REQUEST, error.to_string())
}

fn unauthorized() -> ApiError {
    (StatusCode::UNAUTHORIZED, "unauthorized".into())
}

fn check_auth(headers: &HeaderMap, token: &Option<String>) -> bool {
    let Some(expected) = token else {
        return true;
    };
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == expected)
}

fn open_repo(state: &AppState, slug: &str) -> Result<ServerRepo> {
    let project = project::open(&state.config.root, slug)?;
    ServerRepo::open(&project.dir)
}

fn default_host(config: &ServerConfig) -> String {
    config
        .host
        .clone()
        .or_else(|| std::env::var("DS_HOST").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "localhost".to_owned())
}

fn default_user(config: &ServerConfig) -> Option<String> {
    config
        .user
        .clone()
        .or_else(|| std::env::var("USER").ok())
        .filter(|user| !user.is_empty())
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> std::result::Result<axum::response::Response, ApiError> {
    if !check_auth(&headers, &state.config.token) {
        return Err(unauthorized());
    }
    let projects = project::list(&state.config.root).map_err(bad)?;
    let views: Vec<ProjectView> = projects.iter().map(ProjectView::from).collect();
    Ok(Json(views).into_response())
}

async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateProject>,
) -> std::result::Result<axum::response::Response, ApiError> {
    if !check_auth(&headers, &state.config.token) {
        return Err(unauthorized());
    }
    let project = project::create(
        &state.config.root,
        &body.name,
        body.source.as_deref(),
        &default_host(&state.config),
        default_user(&state.config).as_deref(),
        state.config.port,
    )
    .map_err(bad)?;
    Ok((StatusCode::CREATED, Json(ProjectView::from(&project))).into_response())
}

async fn project_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(slug): AxumPath<String>,
) -> std::result::Result<axum::response::Response, ApiError> {
    if !check_auth(&headers, &state.config.token) {
        return Err(unauthorized());
    }
    let project = project::open(&state.config.root, &slug).map_err(|error| match error {
        Error::NotFound(_) => (StatusCode::NOT_FOUND, error.to_string()),
        other => bad(other),
    })?;
    Ok(Json(ProjectView::from(&project)).into_response())
}

async fn refs(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(slug): AxumPath<String>,
) -> std::result::Result<axum::response::Response, ApiError> {
    if !check_auth(&headers, &state.config.token) {
        return Err(unauthorized());
    }
    let project = project::open(&state.config.root, &slug).map_err(bad)?;
    let repo = ServerRepo::open(&project.dir).map_err(bad)?;
    Ok(Json(RefsResponse {
        refs: repo.refs().map_err(bad)?,
        source: project.meta.source.clone(),
    })
    .into_response())
}

async fn fetch(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(slug): AxumPath<String>,
    Json(request): Json<FetchRequest>,
) -> std::result::Result<axum::response::Response, ApiError> {
    if !check_auth(&headers, &state.config.token) {
        return Err(unauthorized());
    }
    let repo = open_repo(&state, &slug).map_err(bad)?;
    let bytes = repo
        .fetch_pack(&request.want, request.have.as_deref())
        .map_err(bad)?;
    Ok((
        StatusCode::OK,
        [("content-type", "application/octet-stream")],
        bytes,
    )
        .into_response())
}

async fn pack(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(slug): AxumPath<String>,
    body: axum::body::Bytes,
) -> std::result::Result<axum::response::Response, ApiError> {
    if !check_auth(&headers, &state.config.token) {
        return Err(unauthorized());
    }
    let repo = open_repo(&state, &slug).map_err(bad)?;
    let checksum = repo.receive_pack(&body).map_err(bad)?;
    Ok(Json(serde_json::json!({ "checksum": checksum })).into_response())
}

async fn push(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(slug): AxumPath<String>,
    Json(request): Json<PushRequest>,
) -> std::result::Result<axum::response::Response, ApiError> {
    if !check_auth(&headers, &state.config.token) {
        return Err(unauthorized());
    }
    let repo = open_repo(&state, &slug).map_err(bad)?;
    let result = repo.push(&request).map_err(bad)?;
    let status = match result.status.as_str() {
        "ok" => StatusCode::OK,
        "conflict" => StatusCode::CONFLICT,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    Ok((status, Json(result)).into_response())
}

/// Run the multi-project HTTP server.
pub fn serve(config: ServerConfig, listen: &str) -> Result<()> {
    let state = AppState {
        config: Arc::new(config),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let app = Router::new()
            .route("/health", get(health))
            .route("/v1/projects", get(projects).post(create_project))
            .route("/v1/projects/{slug}", get(project_get))
            .route("/v1/projects/{slug}/v1/refs", get(refs))
            .route("/v1/projects/{slug}/v1/fetch", post(fetch))
            .route("/v1/projects/{slug}/v1/packs", post(pack))
            .route("/v1/projects/{slug}/v1/push", post(push))
            .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(listen)
            .await
            .map_err(Error::Io)?;
        axum::serve(listener, app).await.map_err(Error::Io)
    })
}

/// Print one project as key/value-free text (used by the server CLI).
pub fn describe(project: &Project) -> String {
    format!(
        "Project {}\n  Design space: {}\n  Server path:  {}",
        project.meta.name,
        project.meta.design_space,
        project.dir.display()
    )
}

#[allow(dead_code)]
fn _path(_: &Path) {}
