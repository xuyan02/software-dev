//! Authentication, session cookies and project context for the HTTP surface.

use crate::{ApiError, AppState};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use foundry_app::ServiceError;
use foundry_domain::{Account, Actor, Project, ProjectRole};
use foundry_web::ConsoleContext;
use time::OffsetDateTime;

pub const SESSION_COOKIE: &str = "foundry_session";
pub const PROJECT_COOKIE: &str = "foundry_project";
/// Short-lived ticket issued after the first-run initial login. It only
/// authorizes the administrator creation step of `/setup`.
pub const SETUP_COOKIE: &str = "foundry_setup";
pub const SETUP_TICKET_TTL_SECONDS: i64 = 600;

pub struct ApiSession {
    pub account: Account,
    pub project: Project,
    pub role: Option<ProjectRole>,
}

pub struct WebSession {
    pub account: Account,
    pub project: Project,
    pub role: Option<ProjectRole>,
    pub projects: Vec<Project>,
    pub context: ConsoleContext,
}

pub struct WebAccountSession {
    pub account: Account,
    pub projects: Vec<Project>,
    pub context: ConsoleContext,
}

impl ApiSession {
    pub fn actor(&self) -> Actor {
        self.account.actor()
    }

    pub fn can_contribute(&self) -> bool {
        can_contribute(&self.account, &self.project)
    }

    pub fn can_dispose(&self) -> bool {
        can_dispose(&self.account, &self.project)
    }

    pub fn can_manage_members(&self) -> bool {
        can_manage_members(&self.account, &self.project)
    }
}

impl WebSession {
    pub fn actor(&self) -> Actor {
        self.account.actor()
    }

    pub fn can_contribute(&self) -> bool {
        can_contribute(&self.account, &self.project)
    }

    pub fn can_dispose(&self) -> bool {
        can_dispose(&self.account, &self.project)
    }

    pub fn can_manage_members(&self) -> bool {
        can_manage_members(&self.account, &self.project)
    }
}

fn can_contribute(account: &Account, project: &Project) -> bool {
    account.is_admin()
        || project
            .role_of(account.id)
            .is_some_and(ProjectRole::can_contribute)
}

fn can_dispose(account: &Account, project: &Project) -> bool {
    account.is_admin()
        || project
            .role_of(account.id)
            .is_some_and(ProjectRole::can_dispose)
}

fn can_manage_members(account: &Account, project: &Project) -> bool {
    account.is_admin()
        || project
            .role_of(account.id)
            .is_some_and(ProjectRole::can_manage_members)
}

pub fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        let value = value.trim();
        if let Some(token) = value.strip_prefix("Bearer ") {
            return Some(token.trim().to_string());
        }
    }
    cookie_value(headers, SESSION_COOKIE)
}

pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    for cookie in cookies.split(';') {
        let Some((key, value)) = cookie.trim().split_once('=') else {
            continue;
        };
        if key == name {
            return Some(value.to_string());
        }
    }
    None
}

pub async fn authenticated(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<Account>, ServiceError> {
    let Some(token) = token_from_headers(headers) else {
        return Ok(None);
    };
    state.auth.authenticate(&token).await
}

pub async fn api_account(state: &AppState, headers: &HeaderMap) -> Result<Account, ApiError> {
    authenticated(state, headers)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "login required"))
}

pub async fn api_session(state: &AppState, headers: &HeaderMap) -> Result<ApiSession, ApiError> {
    let account = api_account(state, headers).await?;
    let projects = state.projects.list_visible(account.actor()).await?;
    let project = match project_header(headers) {
        Some(preferred) => find_project(&projects, &preferred).ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "project_not_found",
                "project does not exist or is not visible to this account",
            )
        })?,
        None => select_project(&projects, cookie_value(headers, PROJECT_COOKIE).as_deref())
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "project_required",
                    "no visible project; create one first",
                )
            })?,
    };
    let role = project.role_of(account.id);
    Ok(ApiSession {
        account,
        project,
        role,
    })
}

pub async fn web_session(state: &AppState, headers: &HeaderMap) -> Result<WebSession, Response> {
    let Some(account) = authenticated(state, headers)
        .await
        .map_err(|error| web_internal(error.to_string()))?
    else {
        return Err(Redirect::to("/login").into_response());
    };
    let projects = state
        .projects
        .list_visible(account.actor())
        .await
        .map_err(|error| web_internal(error.to_string()))?;
    let preferred = project_preference(headers);
    let Some(project) = select_project(&projects, preferred.as_deref()) else {
        return Err(Redirect::to("/projects").into_response());
    };
    let context = ConsoleContext::new(&account, &projects, Some(&project));
    let role = project.role_of(account.id);
    Ok(WebSession {
        account,
        project,
        role,
        projects,
        context,
    })
}

pub async fn web_account_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<WebAccountSession, Response> {
    let Some(account) = authenticated(state, headers)
        .await
        .map_err(|error| web_internal(error.to_string()))?
    else {
        return Err(Redirect::to("/login").into_response());
    };
    let projects = state
        .projects
        .list_visible(account.actor())
        .await
        .map_err(|error| web_internal(error.to_string()))?;
    let current = select_project(&projects, project_preference(headers).as_deref());
    let context = ConsoleContext::new(&account, &projects, current.as_ref());
    Ok(WebAccountSession {
        account,
        projects,
        context,
    })
}

fn project_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-foundry-project")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn project_preference(headers: &HeaderMap) -> Option<String> {
    project_header(headers).or_else(|| cookie_value(headers, PROJECT_COOKIE))
}

fn find_project(projects: &[Project], preferred: &str) -> Option<Project> {
    projects
        .iter()
        .find(|project| project.id.to_string() == preferred)
        .cloned()
}

fn select_project(projects: &[Project], preferred: Option<&str>) -> Option<Project> {
    if let Some(preferred) = preferred
        && let Some(project) = find_project(projects, preferred)
    {
        return Some(project);
    }
    projects
        .iter()
        .find(|project| !project.is_archived())
        .or_else(|| projects.first())
        .cloned()
}

pub fn session_cookie(token: &str, expires_at: OffsetDateTime) -> String {
    let max_age = (expires_at - OffsetDateTime::now_utc())
        .whole_seconds()
        .max(0);
    format!("{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}")
}

pub fn clear_session_cookie() -> String {
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

pub fn setup_cookie(token: &str) -> String {
    format!(
        "{SETUP_COOKIE}={token}; Path=/setup; HttpOnly; SameSite=Lax; Max-Age={SETUP_TICKET_TTL_SECONDS}"
    )
}

pub fn clear_setup_cookie() -> String {
    format!("{SETUP_COOKIE}=; Path=/setup; HttpOnly; SameSite=Lax; Max-Age=0")
}

pub fn project_cookie(project_id: &str) -> String {
    format!("{PROJECT_COOKIE}={project_id}; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000")
}

pub fn safe_next(next: &str) -> String {
    let next = next.trim();
    if next.starts_with('/')
        && !next.starts_with("//")
        && !next.contains('\\')
        && !next.chars().any(char::is_control)
    {
        next.to_string()
    } else {
        "/".to_string()
    }
}

pub fn web_forbidden(message: &str) -> Response {
    (StatusCode::FORBIDDEN, message.to_string()).into_response()
}

pub fn web_internal(message: String) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, message).into_response()
}

pub fn web_error(error: ServiceError) -> Response {
    match error {
        ServiceError::NotFound { .. } | ServiceError::NotFoundNamed { .. } => (
            StatusCode::NOT_FOUND,
            "没有找到该对象，或它不属于当前项目。",
        )
            .into_response(),
        ServiceError::Forbidden(message) | ServiceError::Unauthorized(message) => {
            (StatusCode::FORBIDDEN, message).into_response()
        }
        ServiceError::Validation(message) | ServiceError::InvalidTransition(message) => {
            (StatusCode::UNPROCESSABLE_ENTITY, message).into_response()
        }
        other @ ServiceError::Conflict { .. } => {
            (StatusCode::CONFLICT, other.to_string()).into_response()
        }
        other => web_internal(other.to_string()),
    }
}
